"""Builds quadruped beasts with their own rig and animations: the grey wolf and the dire wolf.

    art/blender.sh -P art/blender/build_beasts.py [-- --review]

The body is a signed distance field (aldermoor_art/wolf.py: muscles, a carved mouth and eye
sockets, sculpted locks of fur) meshed at three levels of detail, each painted on its own: a
grizzled coat with markings, fur strands and a normal map. Bone-heat weights come from a clean
copy without the locks. Gaits are generated: paws follow stance-and-swing paths per leg, solved
with two-bone IK, with body bob, spine flex, head counter-motion and a swaying tail, then baked
to keyframes. Writes assets/creatures/<id>.glb (with .lod1 and .lod2 beside it) and
assets/catalog/creatures.json.
"""

import json
import math
import random
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Matrix, Quaternion, Vector
from mathutils.noise import noise as perlin

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import export, review, scene as sc, shapes as sh, wolf  # noqa: E402

OUT = sc.OUT / "creatures"
CAT_PATH = sc.OUT / "catalog" / "creatures.json"
# Painted maps are embedded in each glb, so the files themselves stay in the scratch folder.
TEX = sc.ROOT / "art" / "out" / "creatures" / "textures"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
FPS = sc.FPS

# The wolf faces -Y, stands on z = 0. Joint positions in metres.
J = {
    "root": (0, 0, 0),
    "hips": (0, 0.40, 0.72), "spine_1": (0, 0.18, 0.74), "spine_2": (0, -0.06, 0.76), "chest": (0, -0.28, 0.78),
    "neck_1": (0, -0.38, 0.80), "neck_2": (0, -0.49, 0.86), "head": (0, -0.56, 1.0), "head_end": (0, -0.68, 1.02),
    "jaw": (0, -0.6, 0.94), "jaw_end": (0, -0.77, 0.93),
    "tail_1": (0, 0.51, 0.73), "tail_2": (0, 0.61, 0.68), "tail_3": (0, 0.69, 0.585), "tail_4": (0, 0.755, 0.47), "tail_end": (0, 0.795, 0.33),
}
for sd, x in (("l", 1), ("r", -1)):
    J.update({
        f"ear_{sd}": (0.048 * x, -0.555, 1.072), f"ear_end_{sd}": (0.075 * x, -0.548, 1.17),
        f"scapula_{sd}": (0.065 * x, -0.24, 0.80), f"humerus_{sd}": (0.09 * x, -0.36, 0.60), f"radius_{sd}": (0.09 * x, -0.28, 0.40),
        f"pastern_{sd}": (0.08 * x, -0.292, 0.12), f"fpaw_{sd}": (0.08 * x, -0.31, 0.04), f"fpaw_end_{sd}": (0.08 * x, -0.38, 0.02),
        f"femur_{sd}": (0.085 * x, 0.42, 0.66), f"tibia_{sd}": (0.092 * x, 0.30, 0.44), f"hock_{sd}": (0.09 * x, 0.465, 0.19),
        f"hpaw_{sd}": (0.085 * x, 0.44, 0.045), f"hpaw_end_{sd}": (0.085 * x, 0.37, 0.02),
    })
J = {k: Vector(v) for k, v in J.items()}
# The head's joints above are in the head's own frame (wolf.head_matrix carries it onto the neck).
for _name in ("head", "head_end", "jaw", "jaw_end", "ear_l", "ear_end_l", "ear_r", "ear_end_r"):
    J[_name] = wolf.head_matrix() @ J[_name]

# Bone: (head joint, tail joint, parent).
BONES = [("root", "root", None), ("hips", "spine_1", "root"), ("spine_1", "spine_2", "hips"), ("spine_2", "chest", "spine_1"),
         ("chest", "neck_1", "spine_2"), ("neck_1", "neck_2", "chest"), ("neck_2", "head", "neck_1"), ("head", "head_end", "neck_2"),
         ("jaw", "jaw_end", "head"), ("tail_1", "tail_2", "hips"), ("tail_2", "tail_3", "tail_1"), ("tail_3", "tail_4", "tail_2"),
         ("tail_4", "tail_end", "tail_3")]
for sd in ("l", "r"):
    BONES += [(f"ear_{sd}", f"ear_end_{sd}", "head"), (f"scapula_{sd}", f"humerus_{sd}", "chest"), (f"humerus_{sd}", f"radius_{sd}", f"scapula_{sd}"),
              (f"radius_{sd}", f"pastern_{sd}", f"humerus_{sd}"), (f"pastern_{sd}", f"fpaw_{sd}", f"radius_{sd}"),
              (f"fpaw_{sd}", f"fpaw_end_{sd}", f"pastern_{sd}"), (f"femur_{sd}", f"tibia_{sd}", "hips"), (f"tibia_{sd}", f"hock_{sd}", f"femur_{sd}"),
              (f"hock_{sd}", f"hpaw_{sd}", f"tibia_{sd}"), (f"hpaw_{sd}", f"hpaw_end_{sd}", f"hock_{sd}")]


def make_rig(scale):
    arm = bpy.data.armatures.new("wolf")
    ob = bpy.data.objects.new("skeleton", arm)
    bpy.context.scene.collection.objects.link(ob)
    sc.activate(ob, "EDIT")
    ebs = arm.edit_bones
    for name, a, b, in [(n, h, t) for n, h, t, _ in [(x[0], x[0], x[1], x[2]) for x in BONES]]:
        eb = ebs.new(name)
        eb.head = J[a] * scale
        tail = J[b] * scale
        if name == "root":
            tail = Vector((0, -0.2, 0)) * scale
        eb.tail = tail
    for name, _, parent in BONES:
        if parent:
            ebs[name].parent = ebs[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    return ob


def segment_weights(body, skel, scale, falloff=0.035):
    """Weights each vertex to its nearest bone segments, blended across joints.

    Tube-shaped limbs skin cleanly this way. Bones are matched by region first (a front-left
    vertex never takes a hind-right bone), and the upper muzzle stays on the head; only the
    lower jaw follows the jaw bone.
    """
    bones = [b for b in skel.data.bones if b.name != "root"]
    segs = {b.name: (skel.matrix_world @ b.head_local, skel.matrix_world @ b.tail_local) for b in bones}

    def dist(p, a, b):
        ab = b - a
        t = max(0.0, min(1.0, (p - a).dot(ab) / max(ab.length_squared, 1e-12)))
        return (p - (a + ab * t)).length, t

    mw = body.matrix_world
    groups = {}
    jaw_line = J["jaw"].z * scale - 0.01 * scale
    for v in body.data.vertices:
        p = mw @ v.co
        q = p / scale
        cands = []
        for name, (a, b) in segs.items():
            side = name[-1] if name[-2:] in ("_l", "_r") else None
            if side == "l" and q.x < -0.02:
                continue
            if side == "r" and q.x > 0.02:
                continue
            if name.startswith(("scapula", "humerus", "radius", "pastern", "fpaw")) and q.y > 0.05:
                continue
            if name.startswith(("femur", "tibia", "hock", "hpaw")) and q.y < 0.15:
                continue
            if name.startswith("ear") and q.z < 1.04:
                continue
            if name == "jaw":
                continue
            d, t = dist(p, a, b)
            cands.append((d, name))
        cands.sort()
        best = cands[:3]
        d0 = best[0][0]
        wd = {}
        for d, name in best:
            w = math.exp(-((d - d0) / (falloff * scale)) ** 2)
            if w > 0.02:
                wd[name] = w
        total = sum(wd.values())
        for name, w in wd.items():
            groups.setdefault(name, []).append((v.index, w / total))
    for name, items in groups.items():
        vg = body.vertex_groups.get(name) or body.vertex_groups.new(name=name)
        for i, w in items:
            vg.add([i], w, "REPLACE")


# --- Animation -----------------------------------------------------------------------------

def two_bone(a, l1, l2, target, pole):
    """Joint position for a two-segment chain from a to target, bending towards pole."""
    d = target - a
    dist = min(d.length, (l1 + l2) * 0.999)
    dn = d.normalized()
    cos_a = (l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist)
    cos_a = max(-1.0, min(1.0, cos_a))
    side = (pole - a)
    side = (side - dn * side.dot(dn)).normalized()
    return a + dn * (l1 * cos_a) + side * (l1 * math.sqrt(max(0.0, 1 - cos_a * cos_a)))


class Poser:
    """Sets bones by aiming them at world-space joint positions, parents first."""

    def __init__(self, skel):
        self.skel = skel
        self.rest = {b.name: b.matrix_local.copy() for b in skel.data.bones}
        self.len = {b.name: b.length for b in skel.data.bones}

    def reset(self):
        """The rest pose, with the mouth closed (it is modelled a little open)."""
        for pb in self.skel.pose.bones:
            pb.matrix_basis.identity()
            pb.rotation_mode = "QUATERNION"
        bpy.context.view_layer.update()
        self.rotate("jaw", Vector((1, 0, 0)), -wolf.JAW_CLOSE)

    def head(self, name):
        return self.skel.pose.bones[name].head.copy()

    def tail(self, name):
        return self.skel.pose.bones[name].tail.copy()

    def aim(self, name, target, twist_up=None):
        pb = self.skel.pose.bones[name]
        bpy.context.view_layer.update()
        h = pb.head.copy()
        cur = (pb.tail - pb.head).normalized()
        want = (target - h).normalized()
        q = cur.rotation_difference(want)
        m = Matrix.Translation(h) @ q.to_matrix().to_4x4() @ Matrix.Translation(-h) @ pb.matrix
        pb.matrix = m
        bpy.context.view_layer.update()

    def rotate(self, name, axis, angle):
        """Rotates a bone about a world axis through its head."""
        pb = self.skel.pose.bones[name]
        bpy.context.view_layer.update()
        h = pb.head.copy()
        q = Quaternion(axis, angle)
        pb.matrix = Matrix.Translation(h) @ q.to_matrix().to_4x4() @ Matrix.Translation(-h) @ pb.matrix
        bpy.context.view_layer.update()

    def move_root(self, offset):
        pb = self.skel.pose.bones["hips"]
        pb.location = pb.bone.matrix_local.to_3x3().inverted() @ offset
        bpy.context.view_layer.update()

    def key(self, frame):
        for pb in self.skel.pose.bones:
            pb.keyframe_insert("rotation_quaternion", frame=frame)
        self.skel.pose.bones["hips"].keyframe_insert("location", frame=frame)
        self.skel.pose.bones["root"].keyframe_insert("location", frame=frame)
        self.skel.pose.bones["root"].keyframe_insert("rotation_quaternion", frame=frame)


def legs(P, scale, paws, crouch=0.0):
    """Solves all four legs for paw targets {leg: world point}."""
    s = scale
    for sd in ("l", "r"):
        # Front: humerus + radius by IK to the wrist; the pastern drops to the paw.
        shoulder = P.head(f"humerus_{sd}")
        paw = paws[f"f{sd}"]
        wrist = paw + Vector((0, 0.03, 0.07)) * s
        l1, l2 = P.len[f"humerus_{sd}"], P.len[f"radius_{sd}"]
        elbow = two_bone(shoulder, l1, l2, wrist, shoulder + Vector((0, 0.5, -0.3)) * s)
        P.aim(f"humerus_{sd}", elbow)
        P.aim(f"radius_{sd}", wrist)
        P.aim(f"pastern_{sd}", paw)
        P.aim(f"fpaw_{sd}", paw + Vector((0, -0.07, -0.02)) * s)
        # Hind: femur + tibia by IK to the hock; the metatarsus drops to the paw.
        hip = P.head(f"femur_{sd}")
        paw = paws[f"h{sd}"]
        hock = paw + Vector((0, 0.03, 0.155 - crouch * 0.05)) * s
        l1, l2 = P.len[f"femur_{sd}"], P.len[f"tibia_{sd}"]
        knee = two_bone(hip, l1, l2, hock, hip + Vector((0, -0.6, -0.2)) * s)
        P.aim(f"femur_{sd}", knee)
        P.aim(f"tibia_{sd}", hock)
        P.aim(f"hock_{sd}", paw)
        P.aim(f"hpaw_{sd}", paw + Vector((0, -0.07, -0.02)) * s)


def rest_paws(scale):
    return {f"f{sd}": J[f"fpaw_{sd}"] * scale for sd in ("l", "r")} | {f"h{sd}": J[f"hpaw_{sd}"] * scale for sd in ("l", "r")}


def gait(P, scale, name, seconds, stride, lift, phases, duty, bob, flex=0.0, head_bob=0.03, tail_amp=0.25, ears_back=0.0):
    act = bpy.data.actions.new(name)
    P.skel.animation_data.action = act
    n = round(seconds * FPS)
    base = rest_paws(scale)
    for f in range(n + 1):
        t = f / n
        P.reset()
        # Body bob and spine flex.
        P.move_root(Vector((0, 0, -bob * scale * (0.5 + 0.5 * math.cos(4 * math.pi * t)))))
        if flex:
            P.rotate("spine_1", Vector((1, 0, 0)), flex * math.sin(2 * math.pi * t))
            P.rotate("chest", Vector((1, 0, 0)), -flex * 0.7 * math.sin(2 * math.pi * t))
        paws = {}
        for leg, ph in phases.items():
            u = (t + ph) % 1.0
            if u < duty:
                # Stance: planted, sliding back under the body (forward is -Y).
                k = u / duty
                dy, dz = -stride / 2 + stride * k, 0.0
            else:
                # Swing: lifted and carried forward again.
                k = (u - duty) / (1 - duty)
                dy, dz = stride / 2 - stride * k, lift * math.sin(math.pi * k)
            paws[leg] = base[leg] + Vector((0, dy, dz)) * scale
        legs(P, scale, paws)
        # Head counter-motion, tail sway, ears.
        P.rotate("neck_1", Vector((1, 0, 0)), head_bob * math.cos(4 * math.pi * t))
        P.rotate("tail_1", Vector((0, 0, 1)), tail_amp * 0.6 * math.sin(2 * math.pi * t))
        P.rotate("tail_2", Vector((0, 0, 1)), tail_amp * math.sin(2 * math.pi * t - 0.6))
        if ears_back:
            for sd in ("l", "r"):
                P.rotate(f"ear_{sd}", Vector((1, 0, 0)), -ears_back)
        P.key(f)
    act.use_fake_user = True
    return act


def clip(P, scale, name, seconds, fn):
    """A clip from fn(P, t) called per frame on a reset pose."""
    act = bpy.data.actions.new(name)
    P.skel.animation_data.action = act
    n = round(seconds * FPS)
    for f in range(n + 1):
        P.reset()
        fn(P, f / n, f)
        P.key(f)
    act.use_fake_user = True
    return act


def ease(x):
    x = max(0.0, min(1.0, x))
    return x * x * (3 - 2 * x)


def animate(skel, scale):
    skel.animation_data_create()
    P = Poser(skel)
    rest = rest_paws(scale)
    X = Vector((1, 0, 0))

    def idle(P, t, f):
        P.move_root(Vector((0, 0, -0.006 * scale * (0.5 + 0.5 * math.sin(2 * math.pi * t * 2)))))
        legs(P, scale, rest)
        P.rotate("neck_1", Vector((0, 0, 1)), 0.18 * math.sin(2 * math.pi * t))
        P.rotate("neck_2", X, 0.05 * math.sin(4 * math.pi * t))
        P.rotate("tail_1", Vector((0, 0, 1)), 0.12 * math.sin(2 * math.pi * t))
        P.rotate("tail_2", Vector((0, 0, 1)), 0.18 * math.sin(2 * math.pi * t - 0.5))
        if 0.6 < t < 0.7:
            P.rotate("ear_l", X, -0.25 * math.sin(math.pi * (t - 0.6) / 0.1))

    clip(P, scale, "Idle_Loop", 3.0, idle)
    gait(P, scale, "Walk_Loop", 1.1, stride=0.42, lift=0.07, phases={"hl": 0.0, "fl": 0.25, "hr": 0.5, "fr": 0.75},
         duty=0.65, bob=0.012, head_bob=0.04)
    gait(P, scale, "Run_Loop", 0.5, stride=0.95, lift=0.13, phases={"hl": 0.0, "hr": 0.08, "fl": 0.5, "fr": 0.58},
         duty=0.36, bob=0.03, flex=0.13, head_bob=0.06, tail_amp=0.15, ears_back=0.35)

    def attack(P, t, f):
        crouch = ease(t / 0.3) * (1 - ease((t - 0.3) / 0.15))
        lunge = ease((t - 0.3) / 0.15) * (1 - ease((t - 0.6) / 0.4))
        P.move_root(Vector((0, -0.28 * lunge * scale, (-0.07 * crouch + 0.03 * lunge) * scale)))
        paws = dict(rest)
        for sd in ("l", "r"):
            paws[f"f{sd}"] = rest[f"f{sd}"] + Vector((0, -0.22 * lunge, 0.12 * lunge * (1 - lunge) * 4)) * scale
        legs(P, scale, paws, crouch)
        P.rotate("neck_1", X, -0.25 * crouch + 0.3 * lunge)
        P.rotate("head", X, 0.2 * lunge)
        bite = math.sin(math.pi * max(0.0, min(1.0, (t - 0.32) / 0.25)))
        P.rotate("jaw", X, 0.5 * bite)
        for sd in ("l", "r"):
            P.rotate(f"ear_{sd}", X, -0.5 * max(crouch, lunge))
        P.rotate("tail_1", X, 0.3 * crouch)

    clip(P, scale, "Attack", 0.9, attack)

    def hit(P, t, f):
        k = math.sin(math.pi * t)
        P.move_root(Vector((0, 0.05 * k * scale, 0)))
        legs(P, scale, rest)
        P.rotate("chest", Vector((0, 0, 1)), 0.25 * k)
        P.rotate("neck_1", X, 0.3 * k)
        for sd in ("l", "r"):
            P.rotate(f"ear_{sd}", X, -0.6 * k)

    clip(P, scale, "Hit", 0.35, hit)

    def death(P, t, f):
        k = ease(t / 0.7)
        roll = ease((t - 0.25) / 0.6)
        # The legs buckle and fold as the body sinks, all in the standing frame...
        P.move_root(Vector((0, 0, -0.2 * k * scale)))
        bent = {leg: p + Vector((0, 0.06 if leg[0] == "f" else -0.04, 0.2 * k)) * scale for leg, p in rest.items()}
        legs(P, scale, bent, crouch=k)
        P.rotate("neck_1", X, -0.5 * k)
        P.rotate("jaw", X, 0.15 * k)
        P.rotate("tail_1", X, 0.4 * k)
        # ...then the whole wolf rolls onto its side about the ground, lifted so its flank rests on it.
        th = 1.45 * roll
        lift = max(0.0, 0.15 - (0.62 - 0.2 * k) * math.cos(th)) * roll
        pb = P.skel.pose.bones["root"]
        pb.matrix = Matrix.Translation((0, 0, lift * scale)) @ Quaternion((0, 1, 0), -th).to_matrix().to_4x4() @ pb.bone.matrix_local
        bpy.context.view_layer.update()

    clip(P, scale, "Death", 1.6, death)

    def jump(P, t, f):
        crouch = ease(t / 0.35) * (1 - ease((t - 0.35) / 0.1))
        up = math.sin(math.pi * max(0.0, min(1.0, (t - 0.4) / 0.5)))
        P.move_root(Vector((0, 0, (-0.1 * crouch + 0.35 * up) * scale)))
        paws = {leg: p + Vector((0, 0, 0.35 * up + (0.12 * up if leg[0] == "h" else 0.2 * up))) * scale for leg, p in rest.items()}
        legs(P, scale, paws, crouch)
        P.rotate("neck_1", X, -0.15 * up)
        P.rotate("tail_1", X, -0.3 * up)

    clip(P, scale, "Jump", 1.0, jump)

    def howl(P, t, f):
        k = ease(t / 0.25) * (1 - ease((t - 0.8) / 0.2))
        legs(P, scale, rest)
        P.rotate("neck_1", X, -0.7 * k)
        P.rotate("neck_2", X, -0.35 * k)
        P.rotate("head", X, -0.2 * k)
        P.rotate("jaw", X, 0.3 * k * (0.85 + 0.15 * math.sin(t * 9)))
        for sd in ("l", "r"):
            P.rotate(f"ear_{sd}", X, 0.15 * k)

    clip(P, scale, "Howl", 2.6, howl)
    P.reset()
    skel.animation_data.action = None


BEASTS = [
    dict(id="wolf.grey", label="Grey wolf", scale=1.0, bulk=1.0, seed=3,
         palette=dict(coat=(0.12, 0.105, 0.085), saddle=(0.028, 0.025, 0.023), tawny=(0.2, 0.125, 0.065),
                      belly=(0.42, 0.38, 0.31), nose=(0.012, 0.01, 0.009)),
         iris=(0.85, 0.52, 0.1), speeds={"walk": 1.6, "run": 6.4}),
    dict(id="wolf.dire", label="Dire wolf", scale=1.28, bulk=1.12, seed=11,
         palette=dict(coat=(0.035, 0.032, 0.03), saddle=(0.011, 0.01, 0.0095), tawny=(0.05, 0.04, 0.032),
                      belly=(0.1, 0.092, 0.082), nose=(0.006, 0.005, 0.005)),
         iris=(0.9, 0.22, 0.06), speeds={"walk": 1.8, "run": 7.0}),
]


def auto_weights(body, skel, scale):
    """Bone-heat weights, the way Blender's automatic weights spread them over the surface; the
    root bone deforms nothing and the jaw moves only the separate lower jaw. Vertices heat
    leaves unweighted fall back to their nearest bone segments."""
    root = skel.data.bones["root"]
    root.use_deform = False
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    body.select_set(True)
    skel.select_set(True)
    bpy.context.view_layer.objects.active = skel
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    root.use_deform = True
    # The jaw and the ears move only their own parts, which are weighted separately.
    head = body.vertex_groups["head"]
    for name in ("jaw", "ear_l", "ear_r"):
        vg = body.vertex_groups.get(name)
        if not vg:
            continue
        for v in body.data.vertices:
            for g in v.groups:
                if g.group == vg.index and g.weight > 0:
                    head.add([v.index], g.weight, "ADD")
        body.vertex_groups.remove(vg)
    loose = [v.index for v in body.data.vertices if not any(g.weight > 1e-4 for g in v.groups)]
    if loose:
        print("heat left", len(loose), "vertices unweighted; using bone segments for them")
        tmp = bpy.data.objects.new("tmp", body.data.copy())
        bpy.context.scene.collection.objects.link(tmp)
        tmp.vertex_groups.clear()
        segment_weights(tmp, skel, scale)
        names = {g.index: g.name for g in tmp.vertex_groups}
        for i in loose:
            for g in tmp.data.vertices[i].groups:
                name = names[g.group]
                dst = body.vertex_groups.get(name) or body.vertex_groups.new(name=name)
                dst.add([i], g.weight, "REPLACE")
        bpy.data.objects.remove(tmp)


def transfer_weights(src, dst):
    """Copies vertex weights from src onto dst's surface (interpolated from the nearest face)."""
    for name in [g.name for g in src.vertex_groups]:
        if name not in dst.vertex_groups:
            dst.vertex_groups.new(name=name)
    mod = dst.modifiers.new("weights", "DATA_TRANSFER")
    mod.object = src
    mod.use_vert_data = True
    mod.data_types_verts = {"VGROUP_WEIGHTS"}
    mod.vert_mapping = "POLYINTERP_NEAREST"
    mod.layers_vgroup_select_src = "ALL"
    mod.layers_vgroup_select_dst = "NAME"
    sc.activate(dst)
    bpy.ops.object.modifier_apply(modifier=mod.name)


def jaw_weights(body, scale):
    """The lower jaw: whatever lies under the mouth's slit, wholly from the corners forward."""
    inv = wolf.head_matrix().inverted()
    jaw = body.vertex_groups.get("jaw") or body.vertex_groups.new(name="jaw")
    for v in body.data.vertices:
        q = inv @ (v.co / scale)
        if q.y > -0.59 or q.z > wolf.slit_z(abs(q.x)):
            continue
        w = ease((-0.59 - q.y) / 0.05)
        if w <= 0:
            continue
        for g in v.groups:
            g.weight *= 1 - w
        jaw.add([v.index], w, "REPLACE")


def ear_weights(ear, skel, side):
    """The ear bends from its base: head weight there, ear weight up the rest."""
    b = skel.data.bones[f"ear_{side}"]
    h, t = skel.matrix_world @ b.head_local, skel.matrix_world @ b.tail_local
    ax = t - h
    ge = ear.vertex_groups.new(name=f"ear_{side}")
    gh = ear.vertex_groups.new(name="head")
    for v in ear.data.vertices:
        u = (v.co - h).dot(ax) / ax.length_squared
        w = max(0.0, min(1.0, (u + 0.05) / 0.3))
        w = w * w * (3 - 2 * w)
        if w > 0:
            ge.add([v.index], w, "REPLACE")
        if w < 1:
            gh.add([v.index], 1 - w, "REPLACE")


# Levels of detail: (grid step, triangles, coat texture size). Each is meshed from the field and
# painted on its own; they share the skeleton and its clips.
LEVELS = ((0.0035, 26000, 2048), (0.006, 11000, 1024), (0.01, 4200, 512))


def make(beast):
    """The rig; the body at each level of detail, weighted, bound and painted; and the extras
    (ear fur, eyes, nose, teeth, claws). No clips yet."""
    sc.reset()
    s = beast["scale"]
    B = beast["bulk"]
    skel = make_rig(s)
    base, full, eye = wolf.anatomy(B, beast["seed"])
    eye = (Vector(eye[0]) * s, Vector(eye[1]), Vector(eye[2]), Vector(eye[3]))
    # Bone heat over a clean surface without the locks, carried onto each furred level; then the
    # lower jaw under the mouth's slit, and the ears bending from their bases.
    ref = wolf.mesh_body(base, s, 0.007, 9000, "body_ref")
    auto_weights(ref, skel, s)
    levels = []
    for n, (step, target, size) in enumerate(LEVELS):
        name = "body" if n == 0 else f"body_lod{n}"
        body = wolf.mesh_body(full, s, step, target, name)
        transfer_weights(ref, body)
        jaw_weights(body, s)
        parts = [body]
        for ear, side in zip(wolf.ears(J, s, B), ("l", "r")):
            ear_weights(ear, skel, side)
            parts.append(ear)
        body = sh.join(parts, name)
        for p in body.data.polygons:
            p.use_smooth = True
        sc.bind(body, skel)
        stem = beast["id"].replace(".", "_") + (f"_lod{n}" if n else "")
        wolf.paint(body, skel, J, s, beast["palette"], eye, TEX, stem, size=size, seed=beast["seed"])
        levels.append(body)
    bpy.data.objects.remove(ref)
    coat = wolf.ear_fur(levels[0], skel, s, seed=beast["seed"])
    eyes = wolf.eyes(eye, s, TEX, beast["id"].replace(".", "_"), iris=beast["iris"])
    nose = wolf.nose(s, B)
    upper, lower = wolf.teeth(s, B)
    front, hind = wolf.claws(s, B)
    rigid = [(nose, "head"), (upper, "head"), (lower, "jaw")] + [(e, "head") for e in eyes]
    for ob, bone in rigid:
        g = ob.vertex_groups.new(name=bone)
        g.add(list(range(len(ob.data.vertices))), 1.0, "REPLACE")
    # Each claw follows its own paw.
    for ob, bone in ((front, "fpaw"), (hind, "hpaw")):
        for side in ("l", "r"):
            g = ob.vertex_groups.new(name=f"{bone}_{side}")
            g.add([v.index for v in ob.data.vertices if (v.co.x > 0) == (side == "l")], 1.0, "REPLACE")
    # How far down each extra goes: ear fur only at full detail, teeth and claws to LOD 1.
    coat["lod_max"] = 0
    for ob in (upper, lower, front, hind):
        ob["lod_max"] = 1
    for ob in [nose] + eyes:
        ob["lod_max"] = 2
    extras = [coat, nose, upper, lower, front, hind] + eyes
    for ob in extras:
        sc.bind(ob, skel)
    return skel, levels, extras


def build(beast):
    s = beast["scale"]
    skel, levels, extras = make(beast)
    animate(skel, s)
    path = OUT / f"{beast['id']}.glb"
    path.parent.mkdir(parents=True, exist_ok=True)
    for n, body in enumerate(levels):
        objs = [skel, body] + [e for e in extras if e["lod_max"] >= n]
        for o in bpy.context.view_layer.objects:
            o.select_set(o in objs)
        bpy.context.view_layer.objects.active = skel
        bpy.ops.export_scene.gltf(filepath=str(export.lod_path(path, n) if n else path), export_format="GLB",
                                  use_selection=True, export_animations=True, export_animation_mode="ACTIONS",
                                  export_force_sampling=True, export_skins=True, export_yup=True, export_apply=False,
                                  export_vertex_color="MATERIAL")
    meshes = [levels[0]] + extras
    entry = {
        "id": beast["id"], "label": beast["label"], "family": "beast", "file": str(path.relative_to(sc.OUT)),
        "height": round(max((levels[0].matrix_world @ v.co).z for v in levels[0].data.vertices), 3),
        "triangles": sum(len(p.vertices) - 2 for m in meshes for p in m.data.polygons),
        "clips": {"idle": "Idle_Loop", "walk": "Walk_Loop", "run": "Run_Loop", "attack": "Attack", "hit": "Hit",
                  "death": "Death", "jump": "Jump", "special": "Howl"},
        "speeds": beast["speeds"], "sim_body": "wolf",
    }
    if "--review" in ARGS:
        for b in levels[1:]:
            b.hide_render = True
        review_beast(beast, skel, meshes)
    return entry


def review_beast(beast, skel, objs):
    out = sc.ROOT / "art" / "out" / "review" / "beasts"
    files = []
    for act, frame in (("Idle_Loop", 0), ("Walk_Loop", 8), ("Run_Loop", 5), ("Attack", 13), ("Death", 47), ("Howl", 40)):
        skel.animation_data.action = bpy.data.actions[act]
        bpy.context.scene.frame_set(frame)
        f = out / f"{beast['id']}-{act}.png"
        review.render(f, objs, yaw=70 if act != "Howl" else 40, size=(700, 520), pad=1.15)
        files.append(f)
    review.montage(out / f"{beast['id']}.png", files, 3)


def main():
    entries = [build(b) for b in BEASTS]
    CAT_PATH.write_text(json.dumps({"version": 1, "creatures": entries}, indent=1))
    print("WROTE", CAT_PATH, [(e["id"], e["triangles"]) for e in entries])


if __name__ == "__main__":
    main()
