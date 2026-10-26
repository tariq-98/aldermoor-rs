"""Builds the humanoid clip library: the Universal Animation Library's clips plus our own.

    art/blender.sh -P art/blender/build_clips.py [-- --review] [-- --only Bow_,Staff_]

Writes assets/humanoid/anims.glb and the `animations` entry of assets/catalog/humanoid.json.

Our clips are authored in code (aldermoor_art/authoring.py) on the male body's skeleton: each pose
places the items, and the hands follow their sockets, so on that body every grip is exact. Other
bodies keep their grips with the IK curves each clip carries (docs/ASSETS.md, "Clip curves and
events"). Every skeleton shares the library's rest frames, so local rotations carry over as they
are; only the pelvis translation is rescaled to the library's leg length.
"""

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import authoring as au, export, rig, scene as sc  # noqa: E402
from aldermoor_art.holds import BOW_DRAW, NOCK_Z, OFF_HAND as OFF  # noqa: E402

V = sc.VENDOR / "quaternius"
OUT = sc.OUT / "humanoid"
REVIEW = sc.ROOT / "art" / "out" / "review" / "clips"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
ONLY = next((a.split("=", 1)[1].split(",") for a in ARGS if a.startswith("--only=")), None)

UP = Vector((0, 0, 1))
FWD = Vector((0, -1, 0))   # The way the character faces.
LEFT = Vector((1, 0, 0))


def gl(v):
    return [round(v.x, 5), round(v.z, 5), round(-v.y, 5)]


# --- The vendor clips -----------------------------------------------------------------------

def clip_tags(name):
    n = name.lower()
    tags = []
    for key, words in {
        "locomotion": ("walk", "jog", "sprint", "crouch", "swim", "jump", "roll", "slide", "climb", "ninja"),
        "combat": ("sword", "punch", "melee", "shield", "hit", "death", "knockback", "spell", "throw", "pistol",
                   "bow", "staff", "spear", "greatsword", "draw_", "sheathe_"),
        "work": ("farm", "chopping", "fixing", "chest", "pickup", "push", "interact", "consume", "carry"),
        "social": ("talking", "wave", "yes", "idle_no", "dance", "fold", "sitting", "lay", "rail"),
        "undead": ("zombie",),
    }.items():
        if any(w in n for w in words):
            tags.append(key)
    return tags or ["idle"]


# When the vendor clips' blows land (`hit`) and their spells or thrown weapons leave (`cast`), in
# seconds, so the game can line a clip's strike up with the moment the simulation resolves it.
# Measured by art/blender/measure_strikes.py: a slash lands at the swinging hand's fastest; a
# punch or a shield bash at full stretch. Combos land several times.
VENDOR_EVENTS = {
    "Sword_Regular_A": [("hit", 0.23)],
    "Sword_Regular_B": [("hit", 0.25)],
    "Sword_Regular_C": [("hit", 0.65)],
    "Sword_Regular_Combo": [("hit", 0.23), ("hit", 0.72), ("hit", 1.55)],
    "Sword_Heavy_Combo": [("hit", 0.38), ("hit", 1.17), ("hit", 1.82), ("hit", 2.6)],
    "Sword_Attack": [("hit", 0.38)],
    "Sword_Dash": [("hit", 0.32)],
    "Melee_Hook": [("hit", 0.25)],
    "Punch_Jab": [("hit", 0.2)],
    "Punch_Cross": [("hit", 0.3)],
    "Shield_OneShot": [("hit", 0.6)],
    "Zombie_Scratch": [("hit", 0.58)],
    "Spell_Simple_Shoot": [("cast", 0.2)],
    "OverhandThrow": [("cast", 0.37)],
}

# Where the vendor's traversal clips cover a move's path (#213), as `travel` curves: the share of
# the path covered by each moment, so the game plays them by how far the body has come. Measured
# from the clips' poses: the roll dives at 0.1 s, its hands touch down at 0.43 and its feet plant
# at 0.87, before it stands; the dashes lunge from 0.03; the climb and the ninja jump travel
# throughout. What follows the last key plays after the move, as its recovery.
VENDOR_CURVES = {
    "Roll": {"travel": [[0.1, 0], [0.43, 0.5], [0.87, 1]]},
    "ClimbUp_1m": {"travel": [[0, 0], [0.667, 1]]},
    "NinjaJump_Start": {"travel": [[0, 0], [0.967, 1]]},
    "Shield_Dash": {"travel": [[0.03, 0], [0.27, 1]]},
    "Sword_Dash": {"travel": [[0.03, 0], [0.25, 1]]},
}

# Single swings cut out of longer vendor clips: name: (source, start, end or None, events).
# The heavy combo's last blow is a clean one-handed overhead on its own.
CUTS = {
    "Sword_Overhead": ("Sword_Heavy_Combo", 2.3, None, [("hit", 0.3)]),
}


def cut(src, name, start, end=None):
    """A new clip from part of another, beginning exactly at `start` seconds (the pose there is
    keyed) and running to `end` or the source's end. The export slides it to zero."""
    act = src.copy()
    act.name = name
    act.use_fake_user = True
    first, last = act.frame_range
    f0 = first + start * sc.FPS
    f1 = first + end * sc.FPS if end is not None else last
    for fc in fcurves_of(act):
        for f in (f0, f1):
            fc.keyframe_points.insert(f, fc.evaluate(f), options={"FAST"})
        for kp in reversed(list(fc.keyframe_points)):
            if kp.co.x < f0 - 1e-3 or kp.co.x > f1 + 1e-3:
                fc.keyframe_points.remove(kp, fast=True)
        fc.update()
    return act, round((f1 - f0) / sc.FPS, 3)


def fcurves_of(action):
    if getattr(action, "layers", None) and action.layers:
        bag = action.layers[0].strips[0].channelbags
        return bag[0].fcurves if len(bag) else []
    return action.fcurves


def vendor_library():
    """Imports both Universal Animation Library packs onto one armature, `humanoid`."""
    first = sc.import_gltf(V / "anims/UAL1_Standard.glb")
    arm = sc.armature_of(first)
    sc.remove(sc.meshes_of(first))
    second = sc.import_gltf(V / "anims/UAL2_Standard.glb")
    sc.remove([o for o in second])
    arm.name = "humanoid"
    arm.data.name = "humanoid"
    clips = []
    for action in sorted(bpy.data.actions, key=lambda a: a.name):
        if action.name.startswith("A_TPose"):
            bpy.data.actions.remove(action)
            continue
        fcs = fcurves_of(action)
        # Only rotations travel between skeletons; the pelvis also keeps its translation.
        for fc in [fc for fc in fcs if fc.data_path.endswith(".scale")
                   or (fc.data_path.endswith(".location") and '"pelvis"' not in fc.data_path)]:
            fcs.remove(fc)
        start, end = action.frame_range
        action.use_fake_user = True
        tags = clip_tags(action.name)
        entry = {
            "name": action.name,
            "seconds": round((end - start) / sc.FPS, 3),
            "loop": "loop" in action.name.lower(),
            "group": tags[0],
            "tags": tags,
        }
        if action.name in VENDOR_CURVES:
            entry["curves"] = VENDOR_CURVES[action.name]
        if action.name in VENDOR_EVENTS:
            entry["events"] = [{"t": t, "name": kind} for kind, t in VENDOR_EVENTS[action.name]]
        clips.append(entry)
    for name, (source, start, end, events) in CUTS.items():
        _, seconds = cut(bpy.data.actions[source], name, start, end)
        tags = clip_tags(source)
        clips.append({"name": name, "seconds": seconds, "loop": False, "group": tags[0], "tags": tags,
                      "source": "cut", "events": [{"t": t, "name": kind} for kind, t in events]})
    return arm, clips


# --- Authoring context ----------------------------------------------------------------------

# The hunting bow's string rests this far behind its grip (weapons._bow).
BOW_STRING_Y = 0.0435
# The right corner of the jaw on the male's rest pose: where the string comes to at full draw.
ANCHOR = Vector((-0.032, -0.088, 1.575))


class Ctx:
    def __init__(self, arm, sockets):
        self.arm = arm
        self.p = au.Pose(arm, sockets)

    def base(self, clip, at=0.0):
        self.p.from_clip(bpy.data.actions[clip], at)


def grip(side, origin, up, palm):
    """A hand socket's frame from the item's up and the way the palm faces (world)."""
    y = Vector(up).normalized()
    pl = Vector(palm)
    pl = (pl - y * pl.dot(y)).normalized()
    x = pl if side == "r" else -pl
    z = x.cross(y)
    m = Matrix((x, y, z)).transposed().to_4x4()
    m.translation = Vector(origin)
    return m


def along(m, s, turn=0.0):
    """The frame `s` metres along an item's up axis, turned about it."""
    out = m @ Matrix.Translation((0, s, 0))
    if turn:
        out = out @ Matrix.Rotation(turn, 4, "Y")
    return out


def stance(c, base="Idle_Loop", at=0.0, turn=0.0, width=0.0, stagger=0.0, drop=0.0, toe_out=0.0, shift=(0, 0)):
    """A base pose with the feet set for the action.

    The body turns by `turn` (radians, + is left); feet go `width` apart across the body and
    `stagger` along it (the left foot forward), and the hips drop by `drop`.
    """
    p = c.p
    c.base(base, at)
    if not (turn or width or stagger or drop or any(shift)):
        p.plant()
        return
    ankle = p.planted["l"][0].z
    pelvis = p.head("pelvis")
    side = Vector((math.cos(turn), math.sin(turn), 0))       # The body's left, once turned.
    ahead = Vector((math.sin(turn), -math.cos(turn), 0))     # Its front.
    mid = Vector((pelvis.x + shift[0], pelvis.y + shift[1], ankle))
    feet = {
        "l": (mid + side * width / 2 + ahead * stagger / 2, turn + toe_out),
        "r": (mid - side * width / 2 - ahead * stagger / 2, turn - toe_out),
    }
    p.hips(offset=(shift[0], shift[1], -drop), turn=turn)
    p.plant(feet)


def finish(c):
    return c.p.snapshot()


# --- Clips ----------------------------------------------------------------------------------

CLIPS = []


def clip(name, seconds, loop=False, tags=(), upper=False):
    def deco(fn):
        CLIPS.append(dict(name=name, seconds=seconds, loop=loop, tags=list(tags), upper=upper, fn=fn))
        return fn
    return deco


# Archery. The bow is in the left hand; the right hand draws. At full draw the string sits on
# the jaw and the bow arm points down the arrow.

def archer(c, at=0.0, sway=0.0):
    """The aiming body: side-on, chest turned further, head back to the target."""
    stance(c, "Idle_Loop", at, turn=-0.85, width=0.36, drop=0.03)
    p = c.p
    p.spine(twist=-0.45, lean=0.06 + sway * 0.01, bend=0.02)
    p.look_at(p.head("Head") + FWD * 10 + UP * 0.05)
    p.look(roll=0.08)


def full_draw(c, at=0.0, sway=0.0, release=0.0):
    """Full draw (release > 0 opens the hand and lets it fly back past the ear)."""
    archer(c, at, sway)
    p = c.p
    nock = p.carry("Head", ANCHOR) + UP * 0.004 * sway
    bow_up = Vector((-0.12, 0.0, 1.0)).normalized()
    bow_grip = nock + FWD * (BOW_STRING_Y + BOW_DRAW) - bow_up * NOCK_Z
    err, _ = p.hold("l", grip("l", bow_grip, bow_up, palm=-LEFT), elbow=(0.9, 0.3, -0.4))
    hand = nock + (Vector((-0.07, 0.15, 0.03)) * release)
    p.hold("r", grip("r", hand, UP, palm=LEFT + FWD * 0.15 * release), elbow=(-0.5, 1.0, 0.15))
    p.fingers("l", per={"index": (0.95, 1.1, 0.7)}, curl=(1.2, 1.45, 0.9), thumb=(0.45, 0.6, 0.45))
    if release:
        p.fingers("r", curl=(0.25, 0.35, 0.25), thumb=(0.2, 0.2, 0.1), spread=0.15)
    else:
        p.fingers("r", per={"pinky": (1.3, 1.5, 1.0)}, curl=(0.35, 1.35, 0.7), thumb=(0.35, 0.3, 0.2))
    return err, (bow_grip - p.carry("Head", ANCHOR)).length


def bow_low(c, at=0.0):
    """Bow at the ready, low on the left, string towards the body."""
    stance(c, "Idle_Loop", at, turn=-0.3)
    p = c.p
    p.spine(twist=-0.1)
    p.look(yaw=0.32)
    g = p.carry("pelvis", Vector((0.22, -0.2, 0.95)))
    up = Vector((-0.12, -0.42, 0.9))
    p.hold("l", grip("l", g, up, palm=Vector((-0.9, 0.3, 0.0))), elbow=(0.5, 0.6, -0.6))
    p.fingers("l", curl=(1.2, 1.45, 0.9), thumb=(0.45, 0.6, 0.45))
    p.relaxed("r")


def bow_nock(c, at=0.0):
    """Bow up and the arrow on the string, before the pull."""
    archer(c, at)
    p = c.p
    nock = p.carry("Head", ANCHOR)
    bow_up = Vector((-0.12, 0.0, 1.0)).normalized()
    bow_grip = nock + FWD * (BOW_STRING_Y + BOW_DRAW) - bow_up * NOCK_Z
    p.hold("l", grip("l", bow_grip, bow_up, palm=-LEFT), elbow=(0.9, 0.3, -0.4))
    hand = bow_grip + Vector((0, BOW_STRING_Y, NOCK_Z)) + bow_up * 0.0
    p.hold("r", grip("r", hand, UP, palm=LEFT), elbow=(-0.6, 0.6, -0.3))
    p.fingers("l", per={"index": (0.95, 1.1, 0.7)}, curl=(1.2, 1.45, 0.9), thumb=(0.45, 0.6, 0.45))
    p.fingers("r", per={"pinky": (1.3, 1.5, 1.0)}, curl=(0.35, 1.3, 0.7), thumb=(0.35, 0.3, 0.2))


def bow_quiver(c, at=0.0):
    """The right hand up at the quiver over the right shoulder, the bow coming up."""
    stance(c, "Idle_Loop", at, turn=-0.6, width=0.3, drop=0.015)
    p = c.p
    p.spine(twist=-0.25, lean=0.04)
    p.look(yaw=0.6, pitch=0.05)
    p.shrug("r", up=0.2)
    top = p.socket("socket_quiver") @ Vector((0, 0.5, -0.064))  # The nocks standing out of the quiver.
    p.hold("r", grip("r", top + Vector((-0.02, -0.03, 0.05)), Vector((0.3, -0.2, 1.0)), palm=Vector((0.6, 0.6, 0))),
           elbow=(-0.3, -0.6, 0.8))
    p.fingers("r", curl=(0.6, 0.9, 0.5), thumb=(0.5, 0.5, 0.3))
    g = p.carry("spine_03", Vector((0.2, -0.42, 1.25)))
    up = Vector((-0.15, -0.35, 0.92))
    p.hold("l", grip("l", g, up, palm=-LEFT), elbow=(0.8, 0.4, -0.5))
    p.fingers("l", curl=(1.2, 1.45, 0.9), thumb=(0.45, 0.6, 0.45))


@clip("Bow_Idle_Loop", 2.4, loop=True, tags=["combat", "bow"])
def bow_idle_loop(c):
    keys = []
    for t, at in ((0.0, 0.0), (1.2, 0.5), (2.4, 0.0)):
        bow_low(c, at)
        keys.append((t, finish(c)))
    return dict(keys=keys)


@clip("Bow_Aim_Loop", 1.6, loop=True, tags=["combat", "bow"])
def bow_aim_loop(c):
    keys = []
    for t, at, sway in ((0.0, 0.0, 0.0), (0.8, 0.3, 1.0), (1.6, 0.0, 0.0)):
        err, d = full_draw(c, at, sway)
        keys.append((t, finish(c)))
    print(f"CLIP Bow_Aim_Loop bow-hand error {err * 100:.1f} cm")
    return dict(keys=keys, curves={"string": [[0, 1]], "arrow": [[0, 1]]})


def draw_keys(c, t0=0.0, speed=1.0):
    keys = []
    bow_low(c)
    keys.append((t0, finish(c)))
    bow_quiver(c, 0.05)
    keys.append((t0 + 0.16 * speed, finish(c)))
    bow_nock(c, 0.1)
    keys.append((t0 + 0.32 * speed, finish(c)))
    full_draw(c, 0.15)
    keys.append((t0 + 0.55 * speed, finish(c)))
    return keys


@clip("Bow_Draw", 0.65, tags=["combat", "bow"])
def bow_draw(c):
    keys = draw_keys(c)
    full_draw(c, 0.2, 0.3)
    keys.append((0.65, finish(c)))
    return dict(keys=keys, curves={"string": [[0.3, 0], [0.32, 1]], "arrow": [[0.3, 0], [0.3, 1]]})


@clip("Bow_Release", 0.6, tags=["combat", "bow"])
def bow_release(c):
    keys = []
    full_draw(c)
    keys.append((0.0, finish(c)))
    full_draw(c, 0.05, release=1.0)
    keys.append((0.07, finish(c)))
    full_draw(c, 0.1, release=1.15)
    keys.append((0.25, finish(c)))
    bow_low(c, 0.2)
    keys.append((0.6, finish(c)))
    return dict(keys=keys, curves={"string": [[0, 1], [0.0, 0]], "arrow": [[0, 0]]},
                events=[{"t": 0.0, "name": "release"}])


@clip("Bow_Shoot", 1.2, tags=["combat", "bow"])
def bow_shoot(c):
    keys = draw_keys(c, 0.0, 0.95)
    full_draw(c, 0.2, 0.4)
    keys.append((0.7, finish(c)))
    full_draw(c, 0.22, release=1.0)
    keys.append((0.78, finish(c)))
    full_draw(c, 0.25, release=1.15)
    keys.append((0.92, finish(c)))
    bow_low(c, 0.3)
    keys.append((1.2, finish(c)))
    return dict(keys=keys, curves={"string": [[0.29, 0], [0.31, 1], [0.72, 1], [0.72, 0]],
                                   "arrow": [[0.29, 0], [0.29, 1], [0.72, 1], [0.72, 0]]},
                events=[{"t": 0.72, "name": "release"}])


# Staves: held 60% of the way up (weapons._regrip), planted beside the right foot at rest.

def staff_planted(c, at=0.0):
    stance(c, "Idle_Loop", at)
    p = c.p
    foot = p.carry("pelvis", Vector((-0.33, -0.2, 0.0)))
    foot.z = 0.0
    g = p.carry("spine_01", Vector((-0.27, -0.16, 1.03)))
    up = (g - foot).normalized()
    p.hold("r", grip("r", foot + up * 1.075, up, palm=LEFT), elbow=(-0.4, 0.5, -0.8))
    p.fist("r")
    p.relaxed("l")


@clip("Staff_Idle_Loop", 2.4, loop=True, tags=["combat", "staff"])
def staff_idle_loop(c):
    keys = []
    for t, at in ((0.0, 0.0), (1.2, 0.5), (2.4, 0.0)):
        staff_planted(c, at)
        keys.append((t, finish(c)))
    return dict(keys=keys)


def staff_windup(c, at=0.0):
    stance(c, "Idle_Loop", at, turn=-0.2, width=0.3, stagger=0.12, drop=0.03)
    p = c.p
    p.spine(twist=-0.3, lean=-0.03, bend=-0.08)
    p.look(yaw=0.45)
    g = p.carry("spine_03", Vector((-0.3, 0.02, 1.4)))
    up = Vector((0.2, 0.55, 0.8))
    p.hold("r", grip("r", g, up, palm=LEFT + FWD * 0.3), elbow=(-0.5, 0.2, -0.8))
    p.fist("r")
    p.hand_dir("l", fingers=Vector((-0.6, -0.3, 0.6)), palm=Vector((-0.8, 0.2, 0.0)),
               wrist=p.carry("spine_03", Vector((0.0, -0.22, 1.33))), elbow=(0.8, 0.4, -0.6))
    p.fingers("l", curl=(0.5, 0.7, 0.4), thumb=(0.3, 0.3, 0.2), spread=0.1)


def staff_release(c, at=0.0, follow=0.0):
    stance(c, "Idle_Loop", at, turn=0.05, width=0.3, stagger=0.2, drop=0.05, shift=(0, -0.03))
    p = c.p
    p.spine(twist=0.15 + 0.05 * follow, bend=0.14 + 0.03 * follow)
    p.look(yaw=-0.12, pitch=-0.05)
    g = p.carry("spine_03", Vector((-0.22, -0.42 - 0.03 * follow, 1.33)))
    up = Vector((0.08, -0.72, 0.69))
    p.hold("r", grip("r", g, up, palm=LEFT), elbow=(-0.6, 0.4, -0.7))
    p.fist("r")
    p.hand_dir("l", fingers=Vector((0.05, -0.2, 1.0)), palm=FWD,
               wrist=p.carry("spine_03", Vector((0.16, -0.5 - 0.02 * follow, 1.38))), elbow=(0.7, 0.2, -0.6))
    p.fingers("l", curl=(0.12, 0.18, 0.1), thumb=(0.05, 0.1, 0.05), spread=0.2)


@clip("Staff_Cast", 1.0, tags=["combat", "staff"])
def staff_cast(c):
    keys = []
    staff_planted(c)
    keys.append((0.0, finish(c)))
    staff_windup(c, 0.1)
    keys.append((0.28, finish(c)))
    staff_release(c, 0.15)
    keys.append((0.42, finish(c)))
    staff_release(c, 0.2, follow=1.0)
    keys.append((0.6, finish(c)))
    staff_planted(c, 0.3)
    keys.append((1.0, finish(c)))
    return dict(keys=keys, events=[{"t": 0.42, "name": "cast"}])


def staff_raised(c, at=0.0, sway=0.0):
    stance(c, "Idle_Loop", at, width=0.32)
    p = c.p
    p.spine(lean=0.02 * sway, bend=-0.05)
    p.look(pitch=-0.12)
    g = p.carry("spine_03", Vector((-0.24, -0.26, 1.72 + 0.01 * sway)))
    up = Vector((-0.05, -0.1, 1.0))
    p.hold("r", grip("r", g, up, palm=LEFT), elbow=(-0.8, 0.3, -0.4))
    p.fist("r")
    p.hand_dir("l", fingers=Vector((0.1, -0.5, 0.86)), palm=Vector((0.0, -0.6, -0.8)),
               wrist=p.carry("spine_03", Vector((0.2, -0.42, 1.38 + 0.01 * sway))), elbow=(0.7, 0.2, -0.7))
    p.fingers("l", curl=(0.2, 0.3, 0.2), thumb=(0.1, 0.1, 0.05), spread=0.2)


@clip("Staff_Channel_Loop", 2.0, loop=True, tags=["combat", "staff"])
def staff_channel(c):
    keys = []
    for t, at, sway in ((0.0, 0.0, 0.0), (1.0, 0.5, 1.0), (2.0, 0.0, 0.0)):
        staff_raised(c, at, sway)
        keys.append((t, finish(c)))
    return dict(keys=keys)


def staff_two_hands(c, at, hilt, up, palm, turn=0.0, bend=0.0, drop=0.0, look=0.0):
    stance(c, "Idle_Loop", at, width=0.36, stagger=0.12, drop=drop)
    p = c.p
    p.spine(twist=turn, bend=bend)
    p.look(pitch=look)
    g = p.carry("spine_03", hilt)
    m = grip("r", g, up, palm)
    p.hold("r", m, elbow=(-0.7, 0.3, -0.6))
    p.hold("l", along(m, OFF["staff"][0], OFF["staff"][1]), elbow=(0.7, 0.3, -0.6))
    p.fist("r")
    p.fist("l")


@clip("Staff_Slam", 1.1, tags=["combat", "staff"])
def staff_slam(c):
    keys = []
    staff_planted(c)
    keys.append((0.0, finish(c)))
    staff_two_hands(c, 0.1, Vector((-0.12, -0.3, 1.55)), Vector((0.05, -0.25, 1.0)), LEFT, bend=-0.1, look=-0.1)
    keys.append((0.35, finish(c)))
    staff_two_hands(c, 0.15, Vector((-0.08, -0.4, 1.22)), Vector((0.02, 0.15, 1.0)), LEFT, bend=0.28, drop=0.12, look=0.25)
    keys.append((0.52, finish(c)))
    staff_two_hands(c, 0.2, Vector((-0.08, -0.41, 1.2)), Vector((0.02, 0.15, 1.0)), LEFT, bend=0.3, drop=0.13, look=0.25)
    keys.append((0.7, finish(c)))
    staff_planted(c, 0.4)
    keys.append((1.1, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0.12, 0], [0.3, 1], [0.75, 1], [0.95, 0]]},
                events=[{"t": 0.52, "name": "cast"}])


# Spears: held at the middle, right hand behind, left hand forward, point at the throat.

def spear_pose(c, at=0.0, reach=0.0, pull=0.0, aim=Vector((0.08, -1.0, 0.18)), turn=-0.45, lunge=0.0, sweep=0.0):
    stance(c, "Idle_Loop", at, turn=turn, width=0.3, stagger=0.42 + 0.2 * lunge, drop=0.07 + 0.06 * lunge,
           shift=(0, -0.18 * lunge))
    p = c.p
    p.spine(twist=0.25 + 0.15 * reach + sweep, bend=0.06 + 0.12 * lunge)
    p.look(yaw=-turn - 0.25 - 0.15 * reach - sweep, pitch=-0.05)
    # The rear hand drives the shaft forward and in towards the middle, through the front hand.
    hilt = p.carry("pelvis", Vector((-0.2 + 0.11 * reach + 0.16 * max(sweep, 0.0) + 0.14 * max(-sweep, 0.0),
                                     0.06 + 0.12 * pull - 0.38 * reach,
                                     1.04 + 0.1 * reach)))
    up = Vector(aim).normalized()
    m = grip("r", hilt, up, palm=Vector((0.8, 0, -0.6)))
    p.hold("r", m, elbow=(-0.6, 0.6, -0.5))
    # The front hand stays by the chest and the shaft runs through it.
    lo, hi = OFF["spear"][2]
    front = p.carry("spine_02", Vector((-0.06, -0.4, 1.12)))
    want = max(lo, min(hi, (front - m.translation).dot(up)))
    # Slide back towards the shoulder if that's out of reach.
    shoulder = p.head("upperarm_l")
    nearest = max(lo, min(hi, (shoulder - m.translation).dot(up)))
    s_off = want
    for k in range(21):
        s_off = want + (nearest - want) * k / 20
        if (along(m, s_off).translation - shoulder).length < 0.54:
            break
    err, tw = p.hold("l", along(m, s_off, OFF["spear"][1]), elbow=(0.4, 0.2, -0.9))
    if err > 0.01:
        sh = p.head("upperarm_l")
        d = sh - m.translation
        closest = d - up * d.dot(up)
        print(f"WARN spear off hand misses by {err * 100:.1f} cm (twist {math.degrees(tw):.0f}); s {s_off:.2f}, "
              f"shoulder to shaft {closest.length:.2f} at s {d.dot(up):.2f}, to grip {(sh - along(m, s_off).translation).length:.2f}")
    p.fist("r")
    p.fist("l")


@clip("Spear_Idle_Loop", 2.0, loop=True, tags=["combat", "spear"])
def spear_idle(c):
    keys = []
    for t, at in ((0.0, 0.0), (1.0, 0.5), (2.0, 0.0)):
        spear_pose(c, at)
        keys.append((t, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]})


@clip("Spear_Thrust", 0.75, tags=["combat", "spear"])
def spear_thrust(c):
    keys = []
    spear_pose(c)
    keys.append((0.0, finish(c)))
    spear_pose(c, 0.05, pull=1.0, turn=-0.55)
    keys.append((0.18, finish(c)))
    spear_pose(c, 0.1, reach=1.0, lunge=1.0, turn=-0.35, aim=Vector((0.04, -1.0, 0.1)))
    keys.append((0.32, finish(c)))
    spear_pose(c, 0.12, reach=0.9, lunge=1.0, turn=-0.35, aim=Vector((0.04, -1.0, 0.1)))
    keys.append((0.45, finish(c)))
    spear_pose(c, 0.3)
    keys.append((0.75, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]}, events=[{"t": 0.3, "name": "hit"}])


@clip("Spear_Sweep", 0.9, tags=["combat", "spear"])
def spear_sweep(c):
    keys = []
    spear_pose(c)
    keys.append((0.0, finish(c)))
    spear_pose(c, 0.05, pull=0.5, sweep=-0.55, aim=Vector((-0.75, -0.65, 0.1)))
    keys.append((0.25, finish(c)))
    spear_pose(c, 0.1, reach=0.4, sweep=0.0, aim=Vector((0.0, -1.0, 0.05)), lunge=0.4)
    keys.append((0.4, finish(c)))
    spear_pose(c, 0.15, reach=0.2, sweep=0.55, aim=Vector((0.8, -0.55, 0.0)), lunge=0.3)
    keys.append((0.52, finish(c)))
    spear_pose(c, 0.3)
    keys.append((0.9, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]}, events=[{"t": 0.4, "name": "hit"}])


# Greatswords: two hands on the long grip, right hand at the guard.

def great_pose(c, at, hilt, up, palm=Vector((0.6, 0.0, 0.8)), turn=-0.25, twist=0.0, bend=0.05, drop=0.08,
               lunge=0.0, look=0.0):
    stance(c, "Idle_Loop", at, turn=turn, width=0.36, stagger=0.36 + 0.16 * lunge, drop=drop + 0.04 * lunge,
           shift=(0, -0.12 * lunge))
    p = c.p
    p.spine(twist=twist, bend=bend)
    p.look(yaw=-turn - twist, pitch=look)
    g = p.carry("spine_03", hilt)
    m = grip("r", g, Vector(up), Vector(palm))
    p.hold("r", m, elbow=(-0.6, 0.2, -0.8))
    p.hold("l", along(m, OFF["greatsword"][0], OFF["greatsword"][1]), elbow=(0.6, 0.2, -0.8))
    p.fist("r")
    p.fist("l")


def great_guard(c, at=0.0, sway=0.0):
    great_pose(c, at, Vector((-0.04, -0.36, 1.06 + 0.01 * sway)), Vector((0.12, -0.8, 0.58)))


@clip("GreatSword_Idle_Loop", 2.2, loop=True, tags=["combat", "greatsword"])
def great_idle(c):
    keys = []
    for t, at, sway in ((0.0, 0.0, 0.0), (1.1, 0.5, 1.0), (2.2, 0.0, 0.0)):
        great_guard(c, at, sway)
        keys.append((t, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]})


@clip("GreatSword_Slash", 1.05, tags=["combat", "greatsword"])
def great_slash(c):
    keys = []
    great_guard(c)
    keys.append((0.0, finish(c)))
    great_pose(c, 0.05, Vector((-0.24, -0.06, 1.55)), Vector((-0.25, 0.75, 0.62)), palm=Vector((0.3, -0.5, 0.8)),
               twist=-0.45, bend=-0.05, look=-0.05)
    keys.append((0.32, finish(c)))
    great_pose(c, 0.1, Vector((-0.06, -0.45, 1.38)), Vector((-0.55, -0.75, 0.35)), palm=Vector((0.5, 0.0, 0.85)),
               twist=-0.1, bend=0.08, lunge=0.6)
    keys.append((0.43, finish(c)))
    great_pose(c, 0.15, Vector((0.18, -0.38, 1.02)), Vector((0.6, -0.5, -0.62)), palm=Vector((0.4, 0.4, 0.8)),
               twist=0.35, bend=0.25, lunge=1.0, look=0.15)
    keys.append((0.55, finish(c)))
    great_pose(c, 0.2, Vector((0.2, -0.33, 0.98)), Vector((0.55, -0.45, -0.7)), palm=Vector((0.4, 0.4, 0.8)),
               twist=0.38, bend=0.27, lunge=1.0, look=0.15)
    keys.append((0.7, finish(c)))
    great_guard(c, 0.35)
    keys.append((1.05, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]}, events=[{"t": 0.48, "name": "hit"}])


@clip("GreatSword_Swing", 1.1, tags=["combat", "greatsword"])
def great_swing(c):
    keys = []
    great_guard(c)
    keys.append((0.0, finish(c)))
    great_pose(c, 0.05, Vector((-0.3, -0.05, 1.2)), Vector((-0.55, 0.82, 0.15)), palm=Vector((0.0, 0.0, 1.0)),
               twist=-0.6, bend=0.0)
    keys.append((0.36, finish(c)))
    great_pose(c, 0.1, Vector((-0.05, -0.45, 1.16)), Vector((-0.2, -0.98, 0.05)), palm=Vector((0.0, 0.0, 1.0)),
               twist=-0.05, bend=0.08, lunge=0.6)
    keys.append((0.5, finish(c)))
    great_pose(c, 0.15, Vector((0.24, -0.25, 1.14)), Vector((0.85, 0.5, 0.05)), palm=Vector((0.0, 0.0, 1.0)),
               twist=0.55, bend=0.1, lunge=0.8)
    keys.append((0.62, finish(c)))
    great_pose(c, 0.2, Vector((0.26, -0.2, 1.12)), Vector((0.8, 0.58, 0.0)), palm=Vector((0.0, 0.0, 1.0)),
               twist=0.6, bend=0.1, lunge=0.8)
    keys.append((0.76, finish(c)))
    great_guard(c, 0.35)
    keys.append((1.1, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]}, events=[{"t": 0.5, "name": "hit"}])


@clip("GreatSword_Overhead", 1.2, tags=["combat", "greatsword"])
def great_overhead(c):
    keys = []
    great_guard(c)
    keys.append((0.0, finish(c)))
    great_pose(c, 0.05, Vector((-0.06, -0.12, 1.86)), Vector((-0.05, 0.6, 0.8)), palm=Vector((0.2, -0.9, 0.3)),
               bend=-0.12, look=-0.15)
    keys.append((0.42, finish(c)))
    great_pose(c, 0.1, Vector((-0.04, -0.5, 1.5)), Vector((-0.02, -0.8, 0.6)), palm=Vector((0.6, 0.0, 0.8)),
               bend=0.1, lunge=0.6)
    keys.append((0.52, finish(c)))
    great_pose(c, 0.15, Vector((-0.02, -0.48, 1.0)), Vector((0.0, -0.75, -0.66)), palm=Vector((0.6, 0.4, 0.7)),
               bend=0.35, lunge=1.0, look=0.25, drop=0.14)
    keys.append((0.62, finish(c)))
    great_pose(c, 0.2, Vector((-0.02, -0.46, 0.98)), Vector((0.0, -0.72, -0.69)), palm=Vector((0.6, 0.4, 0.7)),
               bend=0.36, lunge=1.0, look=0.25, drop=0.14)
    keys.append((0.8, finish(c)))
    great_guard(c, 0.35)
    keys.append((1.2, finish(c)))
    return dict(keys=keys, curves={"off_hand": [[0, 1]]}, events=[{"t": 0.6, "name": "hit"}])


# Drawing and sheathing. The hand meets the stowed item where its shaft or grip passes (`reach`
# pulls it there on other bodies), takes it at `grab`, and brings it round to a ready pose; the
# drawn idle takes over from there. Sheathing reverses it, letting go at `stow`.

def stowed_frame(c, socket, s=0.0):
    """Where the hand holds an item stowed on `socket`, `s` metres up its axis."""
    return along(c.p.socket(socket), s)


def reach_pose(c, at, socket, s, elbow, look=(0.0, 0.0), twist=0.0, side="r", shrug=0.0):
    stance(c, "Idle_Loop", at)
    p = c.p
    p.spine(twist=twist)
    p.look(yaw=look[0], pitch=look[1])
    if shrug:
        p.shrug(side, up=shrug)
    err, tw = p.hold(side, stowed_frame(c, socket, s), elbow=elbow)
    p.fist(side, 0.9)
    return err


def ready_pose(c, at, hilt, up, palm, elbow=(-0.6, 0.3, -0.7)):
    stance(c, "Idle_Loop", at)
    p = c.p
    g = p.carry("spine_03", hilt)
    p.hold("r", grip("r", g, up, palm), elbow=elbow)
    p.fist("r")


def lift_pose(c, at, socket, s, pull, elbow, twist=0.0, look=(0.0, 0.0), shrug=0.0):
    """The item drawn `pull` metres back along its own axis (out of a sheath or off the back)."""
    stance(c, "Idle_Loop", at)
    p = c.p
    p.spine(twist=twist)
    p.look(yaw=look[0], pitch=look[1])
    if shrug:
        p.shrug("r", up=shrug)
    m = stowed_frame(c, socket, s) @ Matrix.Translation((0, -pull, 0))
    p.hold("r", m, elbow=elbow)
    p.fist("r")


def hand_rest(c, at=0.0):
    stance(c, "Idle_Loop", at)


# The ready pose the drawn idles take over from: the item upright in front of the right shoulder.
READY = dict(hilt=Vector((-0.22, -0.3, 1.12)), up=Vector((0.05, -0.25, 1.0)), palm=LEFT)
# Halfway: the pole brought over the shoulder upright, before it comes down to the ready.
OVER_POLE = dict(hilt=Vector((-0.24, -0.16, 1.55)), up=Vector((-0.2, -0.1, 1.0)), palm=Vector((0.7, 0.7, 0)),
                 elbow=(-0.5, -0.3, -0.8))
READY_SWORD = dict(hilt=Vector((-0.2, -0.36, 1.05)), up=Vector((0.12, -0.85, 0.5)), palm=Vector((0.4, 0, 0.9)))


def draw_from(c, socket, s, reach_elbow, lift, ready, twist=0.0, look=(0.0, 0.0), shrug=0.0, over=None):
    keys = []
    hand_rest(c)
    keys.append((0.0, finish(c)))
    reach_pose(c, 0.05, socket, s, reach_elbow, look=look, twist=twist, shrug=shrug)
    keys.append((0.24, finish(c)))
    reach_pose(c, 0.07, socket, s, reach_elbow, look=look, twist=twist, shrug=shrug)
    keys.append((0.28, finish(c)))
    lift_pose(c, 0.1, socket, s, lift, reach_elbow, twist=twist * 0.6, look=look, shrug=shrug * 0.6)
    keys.append((0.4, finish(c)))
    if over:
        ready_pose(c, 0.12, **over)
        keys.append((0.5, finish(c)))
    ready_pose(c, 0.15, **ready)
    keys.append((0.65, finish(c)))
    return keys


def sheathe_to(c, socket, s, reach_elbow, lift, ready, twist=0.0, look=(0.0, 0.0), shrug=0.0, over=None):
    keys = []
    ready_pose(c, 0.0, **ready)
    keys.append((0.0, finish(c)))
    if over:
        ready_pose(c, 0.03, **over)
        keys.append((0.14, finish(c)))
    lift_pose(c, 0.05, socket, s, lift, reach_elbow, twist=twist * 0.6, look=look, shrug=shrug * 0.6)
    keys.append((0.26, finish(c)))
    reach_pose(c, 0.1, socket, s, reach_elbow, look=look, twist=twist, shrug=shrug)
    keys.append((0.42, finish(c)))
    reach_pose(c, 0.12, socket, s, reach_elbow, look=look, twist=twist, shrug=shrug)
    keys.append((0.48, finish(c)))
    hand_rest(c, 0.2)
    keys.append((0.75, finish(c)))
    return keys


DRAW_CURVES = {"reach": [[0.08, 0], [0.24, 1], [0.28, 1], [0.34, 0]]}
SHEATHE_CURVES = {"reach": [[0.3, 0], [0.42, 1], [0.48, 1], [0.58, 0]]}


@clip("Draw_Hip", 0.65, tags=["combat", "draw"], upper=True)
def draw_hip(c):
    keys = draw_from(c, "socket_hip_l", 0.0, (-0.2, 0.6, -0.8), 0.32, READY_SWORD, twist=0.25, look=(0.25, 0.3))
    return dict(keys=keys, curves=DRAW_CURVES, events=[{"t": 0.28, "name": "grab"}])


@clip("Sheathe_Hip", 0.75, tags=["combat", "draw"], upper=True)
def sheathe_hip(c):
    keys = sheathe_to(c, "socket_hip_l", 0.0, (-0.2, 0.6, -0.8), 0.32, READY_SWORD, twist=0.25, look=(0.25, 0.3))
    return dict(keys=keys, curves=SHEATHE_CURVES, events=[{"t": 0.48, "name": "stow"}])


@clip("Draw_Side", 0.6, tags=["combat", "draw"], upper=True)
def draw_side(c):
    keys = draw_from(c, "socket_hip_r", 0.0, (-0.5, 0.6, -0.6), 0.2, READY_SWORD, twist=-0.1, look=(-0.2, 0.3))
    return dict(keys=keys, curves=DRAW_CURVES, events=[{"t": 0.28, "name": "grab"}])


@clip("Sheathe_Side", 0.75, tags=["combat", "draw"], upper=True)
def sheathe_side(c):
    keys = sheathe_to(c, "socket_hip_r", 0.0, (-0.5, 0.6, -0.6), 0.2, READY_SWORD, twist=-0.1, look=(-0.2, 0.3))
    return dict(keys=keys, curves=SHEATHE_CURVES, events=[{"t": 0.48, "name": "stow"}])


@clip("Draw_Back", 0.65, tags=["combat", "draw"], upper=True)
def draw_back(c):
    keys = draw_from(c, "socket_back_1", 0.0, (0.0, -0.4, 1.0), 0.45, READY_SWORD, twist=0.1, look=(-0.3, -0.1), shrug=0.25)
    return dict(keys=keys, curves=DRAW_CURVES, events=[{"t": 0.28, "name": "grab"}])


@clip("Sheathe_Back", 0.75, tags=["combat", "draw"], upper=True)
def sheathe_back(c):
    keys = sheathe_to(c, "socket_back_1", 0.0, (0.0, -0.4, 1.0), 0.45, READY_SWORD, twist=0.1, look=(-0.3, -0.1), shrug=0.25)
    return dict(keys=keys, curves=SHEATHE_CURVES, events=[{"t": 0.48, "name": "stow"}])


POLE_REACH = 0.33  # How far up the stowed pole's axis the hand takes it, behind the shoulder.


@clip("Draw_Pole", 0.65, tags=["combat", "draw"], upper=True)
def draw_pole(c):
    keys = draw_from(c, "socket_back_pole", POLE_REACH, (0.0, -0.4, 1.0), -0.35, READY, twist=0.1, look=(-0.3, -0.1), shrug=0.25,
                     over=OVER_POLE)
    return dict(keys=keys, curves=DRAW_CURVES, events=[{"t": 0.28, "name": "grab"}])


@clip("Sheathe_Pole", 0.75, tags=["combat", "draw"], upper=True)
def sheathe_pole(c):
    keys = sheathe_to(c, "socket_back_pole", POLE_REACH, (0.0, -0.4, 1.0), -0.35, READY, twist=0.1, look=(-0.3, -0.1), shrug=0.25,
                      over=OVER_POLE)
    return dict(keys=keys, curves=SHEATHE_CURVES, events=[{"t": 0.48, "name": "stow"}])


# Climbing (docs/MOVEMENT.md, "Climbing"; #245-#247). The sim hangs a body with its feet HANG_DROP
# below the hold and its middle HANG_OUT out from the wall, facing it, so these clips put the hands
# over an edge HANG_DROP up and HANG_OUT ahead of the root, and the feet on the wall below. Moves
# onto, off and between holds follow the sim's own carries (sim::climbing::carry, and the climb's
# up-then-over), so a hand on the edge stays on it while the body travels: each key is planned
# against the edge, where it is in the world, and turned into the moving root's frame.

HANG_DROP = 1.9     # sim::climbing::HANG_DROP
HANG_OUT = 0.4      # the body's radius
EDGE = Vector((0, -HANG_OUT, HANG_DROP))
SHIMMY_SPEED = 1.2  # sim::climbing::SHIMMY_SPEED
SHIMMY_STEP = 0.4   # seconds per step of each hand; the loop is one step
DROP_IN = 0.4       # how far in from the edge a drop to hang is planned to start


def ease(k):
    return 1 - (1 - k) * (1 - k)


def smooth(k):
    k = min(max(k, 0.0), 1.0)
    return k * k * (3 - 2 * k)


def edge_grip(c, side, edge, x, lift=0.0):
    """A hand over the edge at `edge` (the point on it in front of the root), `x` across: wrist
    just back from the lip, fingers over it. `lift` raises the hand off the edge as it moves."""
    sign = 1 if side == "l" else -1
    wrist = Vector((x, edge.y + 0.07 + 0.08 * min(lift / 0.08, 1.0), edge.z - 0.03 + lift))
    c.p.hand_dir(side, fingers=Vector((0.12 * sign, -0.75, -0.62)), palm=Vector((0.0, 0.3, -1.0)), wrist=wrist,
                 elbow=(0.6 * sign, 0.45, -0.45))
    c.p.fingers(side, curl=(1.0, 1.3, 0.9), thumb=(0.45, 0.55, 0.4))


def place_hips(c, target, tilt=0.0, turn=0.0):
    c.p.hips(offset=Vector(target) - c.p.head("pelvis"), tilt=tilt, turn=turn)


def lean_to(c, shoulders):
    """Bends the spine forward until the shoulders' middle is over `shoulders` (y, z), roughly."""
    p = c.p
    mid = (p.head("upperarm_l") + p.head("upperarm_r")) / 2
    pel = p.head("pelvis")
    want = math.atan2(-(shoulders[0] - pel.y), shoulders[1] - pel.z)
    have = math.atan2(-(mid.y - pel.y), mid.z - pel.z)
    p.spine(bend=want - have)


def foot_on_wall(c, side, ankle, knee_out=0.3, pitch=-1.05):
    """A foot braced on the wall in front: ball on the wall, toes up, knee bent out a little."""
    sign = 1 if side == "l" else -1
    rot = Matrix.Rotation(pitch, 3, "X") @ c.p.rest[f"foot_{side}"].to_3x3()
    c.p.leg(side, Vector(ankle), knee=(knee_out * sign, -1.0, 0.05), foot_rot=rot)


def hang(c, edge=EDGE, hands=(0.22, -0.22), lifts=(0.0, 0.0), feet=((0.17, 0.42), (-0.15, 0.47)), hips=(0.0, 0.06, 0.95),
         sway=0.0, at=0.0):
    """The braced hang: hands over the edge, feet on the wall below, hips out from it. `feet` are
    (across, height) from the root; `sway` shifts the hips toward the left hand (+) or the right."""
    p = c.p
    c.base("Idle_Loop", at)
    hx, hy, hz = hips
    place_hips(c, (hx + 0.08 * sway, hy, hz - 0.02 * abs(sway)), tilt=0.15, turn=-0.12 * sway)
    p.spine(bend=0.45, lean=-0.12 * sway)
    for side, x, lift, (fx, fz) in (("l", hands[0], lifts[0], feet[0]), ("r", hands[1], lifts[1], feet[1])):
        edge_grip(c, side, edge, x, lift)
        foot_on_wall(c, side, (fx, edge.y + 0.13, fz))
    p.look(pitch=-0.25, yaw=0.15 * sway)


@clip("Hang_Idle_Loop", 2.4, loop=True, tags=["locomotion", "climb"])
def hang_idle_loop(c):
    keys = []
    for t, breath in ((0.0, 0.0), (0.6, 0.5), (1.2, 1.0), (1.8, 0.5), (2.4, 0.0)):
        hang(c, hips=(0.0, 0.06 - 0.01 * breath, 0.95 - 0.015 * breath), at=t / 2.4)
        keys.append((t, finish(c)))
    return dict(keys=keys)


def shimmy(c, t):
    """Shimmying right (to -X) at the sim's speed: each step the leading hand reaches, the
    trailing hand follows it, and the feet step after them. Hands and feet are planned along the
    edge, where they grip, and turned into the root's frame as the root slides along."""
    travel = SHIMMY_SPEED * SHIMMY_STEP

    def limb(base, start, dur, lift):
        k = smooth((t - start) / dur)
        return base + travel * k - SHIMMY_SPEED * t, lift * math.sin(math.pi * k)

    lead, lead_lift = limb(0.22, 0.0, 0.16, 0.06)
    trail, trail_lift = limb(-0.22, 0.2, 0.16, 0.06)
    lead_foot, lead_up = limb(0.15, 0.08, 0.18, 0.05)
    trail_foot, trail_up = limb(-0.15, 0.26, 0.14, 0.05)
    sway = -math.sin(2 * math.pi * t / SHIMMY_STEP) * 0.6
    # Along the edge to the right is -X in the clip.
    hang(c, hands=(-trail, -lead), lifts=(trail_lift, lead_lift),
         feet=((-trail_foot, 0.45 + trail_up), (-lead_foot, 0.47 + lead_up)), sway=sway)


@clip("Hang_Shimmy_R_Loop", SHIMMY_STEP, loop=True, tags=["locomotion", "climb"])
def hang_shimmy_r(c):
    keys = []
    for t in (0.0, 0.08, 0.16, 0.24, 0.32, SHIMMY_STEP):
        shimmy(c, t)
        keys.append((t, finish(c)))
    return dict(keys=keys)


def climb_up_root(k):
    """Where the sim has carried the root, `k` through the climb from a hang onto the top: up
    first (the whole HANG_DROP by 0.62), then in over the edge (HANG_OUT plus the 0.5 m in to
    stand on the top)."""
    return Vector((0, -(HANG_OUT + 0.5) * k * k, HANG_DROP * ease(min(1.6 * k, 1.0))))


def climb_up(c, k):
    p = c.p
    e = EDGE - climb_up_root(k)
    if k == 0.0:
        hang(c)
        return
    if k >= 1.0:
        stance(c)
        return
    c.base("Idle_Loop", 0.3 * k)
    if k < 0.2:
        # Pulling: chest up to the hands, elbows down by the ribs, feet driving on the wall.
        place_hips(c, (0, -0.06, e.z - 0.62), tilt=0.1)
        lean_to(c, (-0.2, e.z - 0.12))
        for side, x in (("l", 0.2), ("r", -0.2)):
            edge_grip(c, side, e, x)
        foot_on_wall(c, "l", (0.15, e.y + 0.13, e.z - 1.05))
        foot_on_wall(c, "r", (-0.15, e.y + 0.13, e.z - 0.8))
        p.look(pitch=-0.15)
    elif k < 0.38:
        # Pressing down over the top, chest past the edge, the right knee coming up to it.
        place_hips(c, (0, e.y + 0.25, e.z - 0.08), tilt=0.25)
        lean_to(c, (e.y - 0.08, e.z + 0.36))
        for side, x in (("l", 0.24), ("r", -0.24)):
            sign = 1 if side == "l" else -1
            p.hand_dir(side, fingers=Vector((0.15 * sign, -1.0, -0.15)), palm=-UP, wrist=Vector((x, e.y - 0.14, e.z + 0.05)),
                       elbow=(0.6 * sign, 0.6, 0.3))
            p.fingers(side, curl=(0.3, 0.4, 0.3), thumb=(0.2, 0.2, 0.1), spread=0.1)
        p.leg("r", Vector((-0.14, e.y + 0.12, e.z - 0.22)), knee=(-0.2, -1.0, 0.8),
              foot_rot=Matrix.Rotation(-0.6, 3, "X") @ p.rest["foot_r"].to_3x3())
        foot_on_wall(c, "l", (0.15, e.y + 0.14, e.z - 0.85))
        p.look(pitch=0.05)
    else:
        # The right foot on the top, rising over it; the left leg coming up behind.
        s = (k - 0.38) / 0.62
        place_hips(c, (0, e.y - 0.05 - 0.2 * s, e.z + 0.55 + 0.37 * s), tilt=0.3 - 0.3 * s)
        lean_to(c, (e.y - 0.25 - 0.15 * s, e.z + 1.0 + 0.42 * s))
        p.leg("r", Vector((-0.13, e.y - 0.2, e.z + 0.1)), knee=(-0.1, -1.0, 0.4), foot_rot=p.rest["foot_r"].to_3x3())
        p.leg("l", Vector((0.13, e.y + 0.08 - 0.3 * s, e.z + 0.12 + 0.15 * (1 - s))), knee=(0.1, -1.0, 0.3),
              foot_rot=p.rest["foot_l"].to_3x3())
        for side in ("l", "r"):
            sign = 1 if side == "l" else -1
            p.arm_to(side, p.carry("pelvis", Vector((0.3 * sign, -0.12 - 0.1 * (1 - s), 0.95 + 0.12 * (1 - s)))),
                     elbow=(0.3 * sign, 0.6, -0.6))
            p.relaxed(side)
        p.look(pitch=0.05)


@clip("Hang_ClimbUp", 0.8, tags=["locomotion", "climb"])
def hang_climb_up(c):
    keys = []
    for k in (0.0, 0.15, 0.3, 0.45, 0.6, 0.8, 1.0):
        climb_up(c, k)
        keys.append((0.8 * k, finish(c)))
    return dict(keys=keys, events=[{"t": 0.24, "name": "push"}, {"t": 0.4, "name": "land"}])


def drop_root(k):
    """Where the sim has carried the root, `k` through lowering over an edge: back over the edge
    first, then down, from DROP_IN in from it to the hang."""
    return Vector((0, (DROP_IN + HANG_OUT) * ease(min(1.6 * k, 1.0)), -HANG_DROP * k * k))


def drop(c, k):
    """Planned against the edge (the top at z = 0, its lip DROP_IN behind the start)."""
    p = c.p
    root = drop_root(k)
    lip = Vector((0, DROP_IN, 0))

    def rel(v):
        return Vector(v) - root

    e = rel(lip)
    if k == 0.0:
        stance(c)
        p.look(pitch=0.1)
        return
    if k >= 1.0:
        hang(c)
        return
    c.base("Idle_Loop", 0.2)
    if k < 0.3:
        # Squatting at the edge, back to the drop, hands going down to it beside the feet.
        place_hips(c, rel((0, DROP_IN - 0.18, 0.42)), tilt=0.6)
        lean_to(c, rel((0, DROP_IN - 0.42, 0.88)).yz)
        for side, x in (("l", 0.24), ("r", -0.24)):
            sign = 1 if side == "l" else -1
            p.hand_dir(side, fingers=Vector((0.1 * sign, 0.65, -0.75)), palm=Vector((0, 0.3, -1)), wrist=Vector((x, e.y - 0.1, e.z + 0.06)),
                       elbow=(0.7 * sign, -0.3, 0.2))
            p.fingers(side, curl=(1.0, 1.3, 0.9), thumb=(0.45, 0.55, 0.4))
        for side, x in (("l", 0.16), ("r", -0.16)):
            p.leg(side, rel((x, DROP_IN - 0.2, 0.1)), knee=(0.45 * (1 if side == "l" else -1), -1.0, 0.4),
                  foot_rot=p.rest[f"foot_{side}"].to_3x3())
        p.look(pitch=0.5)
    else:
        # Swinging down the wall, hands on the edge, feet finding the wall below.
        s = smooth((k - 0.3) / 0.7)
        pelvis = -0.25 - 0.7 * s
        place_hips(c, rel((0, DROP_IN + 0.36 + 0.1 * (1 - s), pelvis)), tilt=0.15)
        lean_to(c, rel((0, DROP_IN + 0.2, pelvis + 0.5 - 0.02 * s)).yz)
        for side, x in (("l", 0.22), ("r", -0.22)):
            edge_grip(c, side, e, x)
        foot_on_wall(c, "l", rel((0.17, DROP_IN + 0.27, pelvis - 0.53 + 0.05 * (1 - s))))
        foot_on_wall(c, "r", rel((-0.15, DROP_IN + 0.27, pelvis - 0.48 + 0.05 * (1 - s))))
        p.look(pitch=0.35 * (1 - s) - 0.2 * s)


@clip("Hang_Drop", 0.6, tags=["locomotion", "climb"])
def hang_drop(c):
    keys = []
    for k in (0.0, 0.2, 0.4, 0.6, 0.8, 1.0):
        drop(c, k)
        keys.append((0.6 * k, finish(c)))
    return dict(keys=keys, events=[{"t": 0.15, "name": "hold"}])


@clip("Hang_Catch", 0.2, tags=["locomotion", "climb"])
def hang_catch(c):
    """Caught in the air: arms straight up on the edge and the legs hanging, swinging in to the
    braced hang."""
    p = c.p
    c.base("Jump_Loop", 0.5)
    place_hips(c, (0, -0.25, 0.72), tilt=-0.05)
    lean_to(c, (-0.3, 1.25))
    for side, x in (("l", 0.24), ("r", -0.24)):
        edge_grip(c, side, EDGE, x)
    for side, x in (("l", 0.12), ("r", -0.12)):
        p.leg(side, Vector((x, -0.12, 0.08)), knee=(0, -1, 0.1), foot_rot=Matrix.Rotation(0.5, 3, "X") @ p.rest[f"foot_{side}"].to_3x3())
    p.look(pitch=-0.3)
    keys = [(0.0, finish(c))]
    hang(c)
    keys.append((0.2, finish(c)))
    return dict(keys=keys, events=[{"t": 0.0, "name": "hold"}])


def climb_between(c, k, rise):
    """Hand over hand to a hold `rise` metres up (or down, if negative), on the sim's carry: up
    before across when rising, across before down when dropping."""
    p = c.p
    up = rise > 0
    y = ease(min(1.6 * k, 1.0)) if up else k * k
    root = Vector((0, 0, rise * y))
    old, new = EDGE - root, EDGE + Vector((0, 0, rise)) - root
    # The body rises (or sinks) smoothly from hang to hang, whatever the root does.
    body = Vector((0, 0, rise * smooth(k))) - root
    lead, trail = ("r", "l") if up else ("l", "r")
    lead_k = smooth(k / 0.5)                  # the leading hand moves first
    trail_k = smooth((k - 0.45) / 0.5)
    c.base("Idle_Loop", 0.25 * k)
    place_hips(c, Vector((0, 0.06, 0.95)) + body, tilt=0.15)
    p.spine(bend=0.45)
    for side, kk in ((lead, lead_k), (trail, trail_k)):
        x = 0.22 if side == "l" else -0.22
        e = old.lerp(new, kk)
        edge_grip(c, side, e, x, lift=0.08 * math.sin(math.pi * kk))
    feet_k = smooth((k - (0.3 if up else 0.0)) / 0.6)
    for side, x, z in (("l", 0.17, 0.42), ("r", -0.15, 0.47)):
        foot_on_wall(c, side, Vector((x, EDGE.y + 0.13, z + rise * feet_k)) - root)
    p.look(pitch=-0.25 - 0.15 * (1 if up else -1) * math.sin(math.pi * k))


@clip("Hang_Climb_Up", 0.4, tags=["locomotion", "climb"])
def hang_climb_up_hold(c):
    keys = []
    for k in (0.0, 0.25, 0.5, 0.75, 1.0):
        climb_between(c, k, 0.8)
        keys.append((0.4 * k, finish(c)))
    return dict(keys=keys, events=[{"t": 0.2, "name": "hold"}])


@clip("Hang_Climb_Down", 0.4, tags=["locomotion", "climb"])
def hang_climb_down_hold(c):
    keys = []
    for k in (0.0, 0.25, 0.5, 0.75, 1.0):
        climb_between(c, k, -0.8)
        keys.append((0.4 * k, finish(c)))
    return dict(keys=keys, events=[{"t": 0.2, "name": "hold"}])


HOP = 0.7  # A hop to the next hold along: the gap (up to sim::climbing::HOLD_GAP) and the hands' spacing.


@clip("Hang_Hop_R", 0.3, tags=["locomotion", "climb"])
def hang_hop_r(c):
    """Across a gap to the next hold on the right (-X): the leading hand flies over, the body
    swings after it, and the trailing hand follows."""
    keys = []
    for k in (0.0, 0.3, 0.55, 0.8, 1.0):
        root = Vector((-HOP * ease(k), 0, 0))
        lead_k, trail_k = smooth(k / 0.55), smooth((k - 0.3) / 0.55)
        lead = -0.22 - HOP * lead_k - root.x
        trail = 0.22 - HOP * trail_k - root.x
        hang(c, hands=(trail, lead), lifts=(0.1 * math.sin(math.pi * trail_k), 0.12 * math.sin(math.pi * lead_k)),
             feet=((0.17 - HOP * smooth((k - 0.35) / 0.65) - root.x, 0.42), (-0.15 - HOP * smooth((k - 0.2) / 0.6) - root.x, 0.47)),
             sway=-0.8 * math.sin(math.pi * k))
        keys.append((0.3 * k, finish(c)))
    return dict(keys=keys, events=[{"t": 0.16, "name": "hold"}])


# Robes: the arms held a little out, so hands clear a skirt.

@clip("Idle_Robe_Loop", 2.5, loop=True, tags=["idle"])
def idle_robe(c):
    keys = []
    for t, at in ((0.0, 0.0), (0.625, 0.25), (1.25, 0.5), (1.875, 0.75), (2.5, 0.0)):
        stance(c, "Idle_Loop", at)
        p = c.p
        for side, sign in (("l", 1), ("r", -1)):
            p.rotate(f"upperarm_{side}", (0, 1, 0), -0.16 * sign)
            p.rotate(f"lowerarm_{side}", (0, 1, 0), -0.06 * sign)
        keys.append((t, finish(c)))
    return dict(keys=keys)


# Mirrored clips: the left hand's draws.
MIRRORS = {
    "Draw_Hip_L": "Draw_Hip",
    "Sheathe_Hip_L": "Sheathe_Hip",
    "Draw_Bow": "Draw_Pole",
    "Sheathe_Bow": "Sheathe_Pole",
    "Hang_Shimmy_L_Loop": "Hang_Shimmy_R_Loop",
    "Hang_Hop_L": "Hang_Hop_R",
}


# --- Building ---------------------------------------------------------------------------------

def bake_onto(lib, name, keys, scale):
    """Keys snapshots taken on the authoring skeleton onto the library armature."""
    out = []
    for t, snap in keys:
        snap = dict(snap)
        q, loc = snap["pelvis"]
        snap["pelvis"] = (q, loc * scale)
        out.append((t, snap))
    return au.bake(lib, name, out, sc.FPS)


def main():
    sc.reset()
    lib, clips = vendor_library()
    objs = sc.import_gltf(sc.OUT / "humanoid/male/body.gltf")
    male = sc.armature_of(objs)
    body = next(o for o in sc.meshes_of(objs) if o.name.startswith("body"))
    sockets = rig.socket_frames(male, body)
    c = Ctx(male, sockets)
    scale = lib.data.bones["pelvis"].head_local.z / male.data.bones["pelvis"].head_local.z
    authored = []
    snaps = {}
    for spec in CLIPS:
        if ONLY and not any(spec["name"].startswith(o) for o in ONLY):
            continue
        out = spec["fn"](c)
        keys = out["keys"]
        if spec["loop"]:
            assert abs(keys[-1][0] - spec["seconds"]) < 1e-6, spec["name"]
        snaps[spec["name"]] = keys
        bake_onto(lib, spec["name"], keys, scale)
        authored.append((spec, out))
        print("CLIP", spec["name"], len(keys), "keys")
    for name, src in MIRRORS.items():
        if src not in snaps:
            continue
        spec, out = next((s, o) for s, o in authored if s["name"] == src)
        keys = [(t, au.mirror_snapshot(c.p, snap)) for t, snap in snaps[src]]
        bake_onto(lib, name, keys, scale)
        authored.append((dict(spec, name=name), out))
        print("CLIP", name, "mirrors", src)
    authored_names = {s["name"] for s, _ in authored}
    clips = [cl for cl in clips if cl["name"] not in authored_names]
    for spec, out in authored:
        tags = spec["tags"] or clip_tags(spec["name"])
        entry = {"name": spec["name"], "seconds": spec["seconds"], "loop": spec["loop"], "group": tags[0], "tags": tags,
                 "source": "authored"}
        if spec["upper"]:
            entry["upper_body"] = True
        if out.get("curves"):
            entry["curves"] = out["curves"]
        if out.get("events"):
            entry["events"] = out["events"]
        clips.append(entry)
    clips.sort(key=lambda cl: cl["name"])
    sc.remove(objs)
    path = export.animations(OUT / "anims.glb", lib)
    cat_path = sc.OUT / "catalog" / "humanoid.json"
    cat = json.loads(cat_path.read_text())
    cat["animations"] = {"file": str(path.relative_to(sc.OUT)), "pelvis": gl(lib.data.bones["pelvis"].head_local),
                         "fps": sc.FPS, "clips": clips}
    cat_path.write_text(json.dumps(cat, indent=1))
    print("WROTE", path, len(clips), "clips,", len(authored), "authored")
    return [cl for cl in clips if cl.get("source") == "authored"]


# --- Review -----------------------------------------------------------------------------------

# Items per clip prefix: held in hand, stowed, or moving between the two (draw/sheathe).
REVIEW_ITEMS = [
    ("Bow_", {"bow.hunting": "held", "quiver.leather": "stowed"}),
    ("Staff_", {"staff.oak": "held"}),
    ("Spear_", {"spear.ash": "held"}),
    ("GreatSword_", {"sword.great": "held"}),
    ("Draw_Hip_L", {"dagger.hunting": "ready"}),
    ("Sheathe_Hip_L", {"dagger.hunting": "stow"}),
    ("Draw_Hip", {"sword.arming": "ready"}),
    ("Sheathe_Hip", {"sword.arming": "stow"}),
    ("Draw_Side", {"wand.yew": "ready"}),
    ("Sheathe_Side", {"wand.yew": "stow"}),
    ("Draw_Back", {"sword.great": "ready"}),
    ("Sheathe_Back", {"sword.great": "stow"}),
    ("Draw_Pole", {"staff.oak": "ready"}),
    ("Sheathe_Pole", {"staff.oak": "stow"}),
    ("Draw_Bow", {"bow.hunting": "ready", "quiver.leather": "stowed"}),
    ("Sheathe_Bow", {"bow.hunting": "stow", "quiver.leather": "stowed"}),
    ("Idle_Robe", {}),
]
C = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))  # Blender to glTF axes.


def curve(keys, t):
    """A clip curve's value at t: linear between keys, held outside, the later value at a step."""
    if not keys:
        return 0.0
    if t < keys[0][0]:
        return keys[0][1]
    for (t0, v0), (t1, v1) in zip(keys, keys[1:]):
        if t0 <= t < t1:
            return v0 + (v1 - v0) * (t - t0) / (t1 - t0)
    return keys[-1][1]


def smooth(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def review(entries):
    from aldermoor_art import looks, review as rv
    gear = {e["id"]: e for e in json.loads((sc.OUT / "catalog" / "gear.json").read_text())["items"]}
    REVIEW.mkdir(parents=True, exist_ok=True)
    offsets = looks.socket_offsets("male")
    sheets = []
    for entry in entries:
        name = entry["name"]
        items = next((v for k, v in REVIEW_ITEMS if name.startswith(k)), {})
        pieces = ["robe.mage"] if name.startswith("Idle_Robe") else ["ranger.top", "ranger.trousers", "ranger.boots"]
        look = dict(body="male", pieces=pieces, hair="hair.simple_parted", brows="brows.strong", beard="beard.full",
                    tone="warm", hair_colour="chestnut", clip=name, frame=0)
        skel, objs = looks.assemble(look)
        loaded = []
        for iid, state in items.items():
            e = gear[iid]
            obs = [o for o in sc.import_gltf(sc.OUT / e["file"]) if o.type == "MESH"]
            loaded.append((e, state, obs, [o.matrix_world.copy() for o in obs]))
            if e.get("sheath"):
                sh_obs = [o for o in sc.import_gltf(sc.OUT / e["sheath"]) if o.type == "MESH"]
                loaded.append((dict(e, string=None), "sheath", sh_obs, [o.matrix_world.copy() for o in sh_obs]))
        curves = entry.get("curves", {})
        events = {ev["name"]: ev["t"] for ev in entry.get("events", [])}
        n = round(entry["seconds"] * sc.FPS)
        frames = sorted({0, *[round(n * k / 4) for k in (1, 2, 3)], n, *[round(v * sc.FPS) for v in events.values()]})
        times = [f / sc.FPS for f in frames]
        focus_t = min(times, key=lambda x: abs(x - (min(events.values()) if events else n / 2 / sc.FPS)))
        files = []
        for t in times:
            bpy.context.scene.frame_set(int(round(t * sc.FPS)))
            bpy.context.view_layer.update()
            shown = list(objs)
            for e, state, obs, base in loaded:
                held = looks.socket_world(skel, offsets, e["held"]) if e.get("held") else None
                stowed = looks.socket_world(skel, offsets, e["stowed"]["default"])
                if state in ("held",):
                    m = held
                elif state in ("stowed", "sheath"):
                    m = stowed
                else:
                    key = "grab" if state == "ready" else "stow"
                    tk = events[key]
                    # Where along the stowed item the hand is when it takes or leaves it.
                    bpy.context.scene.frame_set(int(round(tk * sc.FPS)))
                    bpy.context.view_layer.update()
                    h = looks.socket_world(skel, offsets, e["held"]).translation
                    st = looks.socket_world(skel, offsets, e["stowed"]["default"])
                    lo, hi = e.get("grab_range", [0.0, 0.0])
                    s0 = max(lo, min(hi, (h - st.translation).dot(st.col[1].to_3d().normalized())))
                    bpy.context.scene.frame_set(int(round(t * sc.FPS)))
                    bpy.context.view_layer.update()
                    held = looks.socket_world(skel, offsets, e["held"])
                    if state == "ready":
                        k = 1 - smooth((t - tk) / max(entry["seconds"] - tk, 1e-3))
                        m = stowed if t < tk else held @ Matrix.Translation((0, -s0 * k, 0))
                    else:
                        k = smooth(t / max(tk, 1e-3))
                        m = held @ Matrix.Translation((0, -s0 * k, 0)) if t < tk else stowed
                for o, b in zip(obs, base):
                    o.matrix_world = m @ C @ b
                if e.get("string"):
                    hand = looks.socket_world(skel, offsets, "socket_hand_r").translation
                    for o in obs:
                        keys = o.data.shape_keys.key_blocks if o.data.shape_keys else None
                        if not keys:
                            continue
                        local = o.matrix_world.inverted() @ hand
                        rest, pull = Vector((0, 0.0435, NOCK_Z)), Vector((0, BOW_DRAW, 0))
                        w = max(0.0, min(1.0, (local - rest).dot(pull) / pull.length_squared))
                        keys["draw"].value = w * curve(curves.get("string", []), t)
                        keys["arrow"].value = curve(curves.get("arrow", []), t)
                shown += obs
            bpy.context.view_layer.update()
            for yaw in (35, 115):
                f = REVIEW / f"{name}-{int(round(t * sc.FPS)):02d}-{yaw}.png"
                rv.render(f, shown, yaw=yaw, size=(380, 520), pad=1.12)
                files.append(f)
            if abs(t - focus_t) < 1e-6:
                chest = skel.matrix_world @ skel.pose.bones["spine_03"].head + Vector((0, 0, 0.12))
                for yaw in (20, 70):
                    rv.render(REVIEW / f"{name}-close-{yaw}.png", shown, yaw=yaw, size=(520, 520), focus=(chest, 0.42))
        sheet = REVIEW / f"{name}.png"
        rv.montage(sheet, files, len(times) * 2 if len(times) <= 3 else 6)
        for f in files:
            f.unlink()
        sheets.append(sheet)
        print("REVIEW", sheet)
    return sheets


entries = main()
if "--review" in ARGS:
    review(entries)
