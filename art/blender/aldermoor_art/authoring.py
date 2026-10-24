"""Authoring humanoid clips in code: key poses from targets, with IK limbs and finger grips.

A clip is a list of key poses at times. Each pose starts from a frame of a library clip, so legs
and breathing stay natural, then turns the spine and head, plants the feet, places items in the
hands (the hand follows its socket, so a staff or bow ends up exactly where the pose puts it),
and curls the fingers. Poses are computed with no action on the armature, so nothing re-evaluates
them, then keyed as quaternions; Blender's Bezier keys carry the motion between them.

Limbs bend like hinges: the upper bone's bend axis points the joint the way the rig's elbows and
knees fold, and the lower bone turns only about the hinge, then the forearm takes most of the
hand's twist (there are no twist bones). Positions are in the armature's space: +Z up, the
character facing -Y, its left at +X.
"""

import math

import bpy
from mathutils import Matrix, Quaternion, Vector

FINGERS = ("index", "middle", "ring", "pinky")
MIRROR = Matrix.Diagonal((-1.0, 1.0, 1.0))


def frame(up, front, origin):
    """An item's placement as a socket matrix: its up along local Y, its front along local Z."""
    y = Vector(up).normalized()
    f = Vector(front)
    z = (f - y * f.dot(y)).normalized()
    x = y.cross(z)
    m = Matrix((x, y, z)).transposed().to_4x4()
    m.translation = Vector(origin)
    return m


def two_bone(a, l1, l2, target, pole):
    """The middle joint of a two-bone chain from a to target, bending towards pole."""
    d = target - a
    dist = max(1e-4, min(d.length, (l1 + l2) * 0.9995))
    dn = d.normalized()
    cos_a = max(-1.0, min(1.0, (l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist)))
    side = pole - a
    side = side - dn * side.dot(dn)
    side = side.normalized() if side.length > 1e-6 else Vector((0, -1, 0))
    return a + dn * (l1 * cos_a) + side * (l1 * math.sqrt(max(0.0, 1 - cos_a * cos_a)))


def mirror_name(name):
    if name.endswith("_l"):
        return name[:-2] + "_r"
    if name.endswith("_r"):
        return name[:-2] + "_l"
    return name


def _bone_order(arm):
    out = []

    def visit(b):
        out.append(b.name)
        for c in b.children:
            visit(c)

    for b in arm.data.bones:
        if b.parent is None:
            visit(b)
    return out


class Pose:
    """Poses one armature in place (no action assigned) and snapshots the result."""

    def __init__(self, arm, sockets):
        self.arm = arm
        self.pb = arm.pose.bones
        for pb in self.pb:
            pb.rotation_mode = "QUATERNION"
        self.order = _bone_order(arm)
        self.rest = {b.name: b.matrix_local.copy() for b in arm.data.bones}
        # Sockets as offsets from their parent bones (they need not exist on this armature).
        self.sockets = {n: (parent, self.rest[parent].inverted() @ m) for n, (parent, m) in sockets.items()}
        bones = arm.data.bones
        self.length = {}
        self.bend = {}
        for upper, lower, end, towards in (("upperarm_l", "lowerarm_l", "hand_l", (0, -1, 0)),
                                           ("upperarm_r", "lowerarm_r", "hand_r", (0, -1, 0)),
                                           ("thigh_l", "calf_l", "foot_l", (0, 1, 0)),
                                           ("thigh_r", "calf_r", "foot_r", (0, 1, 0))):
            self.length[upper] = (bones[lower].head_local - bones[upper].head_local).length
            self.length[lower] = (bones[end].head_local - bones[lower].head_local).length
            # Which local axis the joint folds towards: elbows fold forward, knees back.
            rot = self.rest[upper].to_3x3()
            best = max((("X", 1), ("X", -1), ("Z", 1), ("Z", -1)),
                       key=lambda a: (rot.col[0 if a[0] == "X" else 2] * a[1]).dot(Vector(towards)))
            self.bend[upper] = best
        self.flex = {}
        for side in ("l", "r"):
            palm = self._rest_palm(side)
            # The thumb folds across the palm towards the middle finger, not along the palm's
            # normal like the fingers do.
            across = self.rest[f"middle_02_{side}"].translation + palm * 0.03
            for f in FINGERS + ("thumb",):
                for j in ("01", "02", "03"):
                    name = f"{f}_{j}_{side}"
                    if name not in bones:
                        continue
                    r = self.rest[name].to_3x3()
                    y = r.col[1].normalized()
                    if f == "thumb":
                        axis = y.cross((across - self.rest[name].translation).normalized()).normalized()
                    else:
                        axis = y.cross(palm).normalized()
                    self.flex[name] = (r.transposed() @ axis).normalized()
        self.planted = None
        self.base_rotations = None

    # --- Basics ---------------------------------------------------------------------------

    def update(self):
        bpy.context.view_layer.update()

    def m(self, name):
        return self.pb[name].matrix.copy()

    def head(self, name):
        return self.pb[name].matrix.translation.copy()

    def set_rot(self, name, rot):
        """Sets a bone's armature-space rotation, keeping its head where it is."""
        pb = self.pb[name]
        mat = rot.to_3x3().normalized().to_4x4()
        mat.translation = pb.matrix.translation.copy()
        pb.matrix = mat
        self.update()

    def rotate(self, name, axis, angle):
        """Turns a bone about a world axis through its head (children follow)."""
        pb = self.pb[name]
        h = pb.matrix.translation.copy()
        q = Quaternion(Vector(axis).normalized(), angle)
        pb.matrix = Matrix.Translation(h) @ q.to_matrix().to_4x4() @ Matrix.Translation(-h) @ pb.matrix
        self.update()

    def carry(self, bone, point):
        """A point given in rest-pose coordinates, carried along by the bone's current motion."""
        return (self.m(bone) @ self.rest[bone].inverted()) @ Vector(point)

    def carry_dir(self, bone, d):
        r = self.m(bone).to_3x3() @ self.rest[bone].to_3x3().transposed()
        return (r @ Vector(d)).normalized()

    def socket(self, name):
        parent, local = self.sockets[name]
        return self.m(parent) @ local

    # --- Whole poses ----------------------------------------------------------------------

    def rest_pose(self):
        if self.arm.animation_data:
            self.arm.animation_data.action = None
        for pb in self.pb:
            pb.matrix_basis = Matrix()
        self.update()
        self.planted = None

    def from_clip(self, action, at=0.0):
        """Starts from a library clip's pose, `at` (0..1) of the way through it."""
        a = self.arm
        a.animation_data_create()
        a.animation_data.action = action
        if hasattr(a.animation_data, "action_slot") and action.slots:
            a.animation_data.action_slot = action.slots[0]
        start, end = action.frame_range
        f = start + (end - start) * at
        bpy.context.scene.frame_set(int(math.floor(f)), subframe=f - math.floor(f))
        bases = {pb.name: pb.matrix_basis.copy() for pb in self.pb}
        a.animation_data.action = None
        for pb in self.pb:
            pb.matrix_basis = bases[pb.name]
        self.update()
        self.planted = {s: (self.head(f"foot_{s}"), self.m(f"foot_{s}").to_3x3()) for s in ("l", "r")}

    def snapshot(self):
        return {pb.name: (pb.rotation_quaternion.copy(), pb.location.copy()) for pb in self.pb}

    def apply(self, snap):
        for pb in self.pb:
            q, loc = snap[pb.name]
            pb.rotation_quaternion = q
            pb.location = loc
            pb.scale = (1, 1, 1)
        self.update()

    # --- Body -----------------------------------------------------------------------------

    def spine(self, bend=0.0, twist=0.0, lean=0.0, share=(0.25, 0.35, 0.4)):
        """Bends forward (+), twists left (+, about Z) and leans left (+) across the spine."""
        for bone, k in zip(("spine_01", "spine_02", "spine_03"), share):
            if twist:
                self.rotate(bone, (0, 0, 1), twist * k)
            if bend:
                self.rotate(bone, self.carry_dir(bone, (1, 0, 0)), bend * k)
            if lean:
                self.rotate(bone, self.carry_dir(bone, (0, 1, 0)), lean * k)

    def look(self, pitch=0.0, yaw=0.0, roll=0.0):
        """Neck and head: down (+pitch), left (+yaw) and tilted left (+roll)."""
        for bone, k in (("neck_01", 0.4), ("Head", 0.6)):
            if yaw:
                self.rotate(bone, (0, 0, 1), yaw * k)
            if pitch:
                self.rotate(bone, self.carry_dir(bone, (1, 0, 0)), pitch * k)
            if roll:
                self.rotate(bone, self.carry_dir(bone, (0, 1, 0)), -roll * k)

    def look_at(self, target, share=1.0):
        """Turns the head (and a little of the neck) to face a world point."""
        h = self.head("Head")
        want = (Vector(target) - h).normalized()
        cur = self.carry_dir("Head", (0, -1, 0))
        q = cur.rotation_difference(want)
        axis, ang = q.to_axis_angle()
        for bone, k in (("neck_01", 0.35), ("Head", 0.65)):
            self.rotate(bone, axis, ang * k * share)

    def hips(self, offset=(0, 0, 0), turn=0.0, tilt=0.0):
        """Moves the pelvis (world offset), turns it about Z and tilts it forward (+)."""
        pb = self.pb["pelvis"]
        if turn:
            self.rotate("pelvis", (0, 0, 1), turn)
        if tilt:
            self.rotate("pelvis", self.carry_dir("pelvis", (1, 0, 0)), tilt)
        if any(offset):
            mat = pb.matrix.copy()
            mat.translation += Vector(offset)
            pb.matrix = mat
            self.update()

    def shrug(self, side, up=0.0, forward=0.0):
        """Raises (+up) and brings forward (+forward) a clavicle, in radians."""
        name = f"clavicle_{side}"
        sign = 1 if side == "l" else -1
        if up:
            self.rotate(name, (0, 1, 0), -up * sign)
        if forward:
            self.rotate(name, (0, 0, 1), forward * sign)

    # --- Limbs ----------------------------------------------------------------------------

    def _hinge_frame(self, upper, d1, b):
        axis, sign = self.bend[upper]
        y = d1.normalized()
        b = (b - y * b.dot(y)).normalized() * sign
        if axis == "Z":
            z = b
            x = y.cross(z)
        else:
            x = b
            z = x.cross(y)
        return Matrix((x, y, z)).transposed()

    def limb(self, upper, lower, end, target, pole):
        """Two-bone IK as a hinge: the joint folds towards `pole` (a world direction)."""
        s = self.head(upper)
        l1, l2 = self.length[upper], self.length[lower]
        target = Vector(target)
        joint = two_bone(s, l1, l2, target, s + Vector(pole))
        d1 = (joint - s).normalized()
        d2 = (target - joint).normalized()
        b = d2 - d1 * d2.dot(d1)
        if b.length < 1e-3:
            b = Vector(pole) - d1 * Vector(pole).dot(d1)
        r_up = self._hinge_frame(upper, d1, b)
        self.set_rot(upper, r_up)
        self.set_rot(lower, d1.rotation_difference(d2).to_matrix() @ r_up)
        return (self.head(end) - target).length

    def leg(self, side, ankle, knee=(0, -1, 0.15), foot_rot=None):
        """Places an ankle; the knee points along `knee`. The foot keeps `foot_rot` (world)."""
        rot = foot_rot if foot_rot is not None else self.m(f"foot_{side}").to_3x3()
        self.limb(f"thigh_{side}", f"calf_{side}", f"foot_{side}", Vector(ankle), Vector(knee))
        self.set_rot(f"foot_{side}", rot)

    def plant(self, feet=None, knees=None):
        """Puts the feet back where the base clip had them (or at given (position, yaw) pairs)."""
        for s in ("l", "r"):
            pos, rot = self.planted[s]
            if feet and s in feet:
                p, yaw = feet[s]
                pos = Vector(p)
                rot = Matrix.Rotation(yaw, 3, "Z") @ self.rest[f"foot_{s}"].to_3x3()
            knee = (knees or {}).get(s)
            if knee is None:
                # Knees over the toes.
                toe = rot @ self.rest[f"foot_{s}"].to_3x3().transposed() @ (self.rest[f"ball_{s}"].translation - self.rest[f"foot_{s}"].translation)
                knee = Vector((toe.x, toe.y, 0)).normalized() + Vector((0, 0, 0.3))
            self.leg(s, pos, knee, rot)

    def arm_to(self, side, wrist, elbow=(0, 0.3, -1)):
        """Places a wrist; the elbow points along `elbow` (a world direction)."""
        return self.limb(f"upperarm_{side}", f"lowerarm_{side}", f"hand_{side}", Vector(wrist), Vector(elbow))

    def _twist(self, side, hand_rot, share):
        """Gives the forearm `share` of the hand's twist about the forearm."""
        lower, hand = f"lowerarm_{side}", f"hand_{side}"
        rf = self.m(lower).to_3x3()
        rel_rest = self.rest[lower].to_3x3().transposed() @ self.rest[hand].to_3x3()
        d = rf.transposed() @ hand_rot @ rel_rest.transposed()
        q = d.to_quaternion()
        ang = 2 * math.atan2(q.y, q.w)
        ang = (ang + math.pi) % (2 * math.pi) - math.pi
        axis = rf.col[1].normalized()
        self.set_rot(lower, Matrix.Rotation(ang * share, 3, axis) @ rf)
        return ang

    def hold(self, side, socket_target, elbow=(0, 0.3, -1), twist_share=0.65):
        """Puts a hand where its socket lands on `socket_target` (a 4x4 from `frame`)."""
        local = self.sockets[f"socket_hand_{side}"][1]
        want = socket_target @ local.inverted()
        err = self.arm_to(side, want.translation, elbow)
        rot = want.to_3x3().normalized()
        tw = self._twist(side, rot, twist_share)
        self.set_rot(f"hand_{side}", rot)
        return err, tw

    def hand_dir(self, side, fingers, palm, elbow=None, wrist=None, twist_share=0.65):
        """Points a hand's fingers along `fingers` with its palm towards `palm` (world)."""
        name = f"hand_{side}"
        y = Vector(fingers).normalized()
        rest = self.rest[name].to_3x3()
        # The palm in the hand's rest frame, from the finger and thumb joints.
        palm_local = rest.transposed() @ self._rest_palm(side)
        pl = Vector(palm)
        pl = (pl - y * pl.dot(y)).normalized()
        # Find the rotation taking local Y to y and the local palm to pl.
        ly = Vector((0, 1, 0))
        lp = (palm_local - ly * palm_local.dot(ly)).normalized()
        a = Matrix((ly, lp, ly.cross(lp))).transposed()
        b = Matrix((y, pl, y.cross(pl))).transposed()
        rot = b @ a.transposed()
        if wrist is not None:
            self.arm_to(side, wrist, elbow if elbow is not None else (0, 0.3, -1))
        self._twist(side, rot, twist_share)
        self.set_rot(name, rot)

    def _rest_palm(self, side):
        h = lambda n: self.rest[n].translation
        w, idx, pk, mid, th = (h(f"{n}_{side}") for n in ("hand", "index_01", "pinky_01", "middle_01", "thumb_02"))
        n = (mid - w).cross(idx - pk).normalized()
        if n.dot(th - (idx + pk) / 2) < 0:
            n = -n
        return n

    def fingers(self, side, curl=(0.9, 1.2, 0.8), thumb=(0.3, 0.4, 0.3), spread=0.0, per=None):
        """Sets every finger joint from rest: `curl` per joint (01, 02, 03), optionally per finger."""
        for i, f in enumerate(FINGERS):
            c = (per or {}).get(f, curl)
            for j, a in zip(("01", "02", "03"), c):
                name = f"{f}_{j}_{side}"
                q = Quaternion(self.flex[name], a)
                if j == "01" and spread:
                    # Fan the fingers about the palm's normal.
                    q = Quaternion(self.flex[name].cross(Vector((0, 1, 0))).normalized(), spread * (i - 1.5) / 1.5) @ q
                self.pb[name].rotation_quaternion = q
        for j, a in zip(("01", "02", "03"), thumb):
            name = f"thumb_{j}_{side}"
            self.pb[name].rotation_quaternion = Quaternion(self.flex[name], a)
        self.update()

    def fist(self, side, tight=1.0):
        self.fingers(side, curl=(1.25 * tight, 1.45 * tight, 0.9 * tight), thumb=(0.35, 0.55 * tight, 0.5 * tight))

    def relaxed(self, side):
        self.fingers(side, curl=(0.25, 0.45, 0.3), thumb=(0.1, 0.15, 0.1), per={"pinky": (0.4, 0.6, 0.4)})


# --- Clips --------------------------------------------------------------------------------

def bake(arm, name, keys, fps=30):
    """Writes key poses [(seconds, snapshot)] as a new action, keeping quaternions continuous."""
    act = bpy.data.actions.new(name)
    arm.animation_data_create()
    arm.animation_data.action = act
    if hasattr(arm.animation_data, "action_slot") and act.slots:
        arm.animation_data.action_slot = act.slots[0]
    prev = {}
    for t, snap in sorted(keys, key=lambda k: k[0]):
        f = round(t * fps)
        for pb in arm.pose.bones:
            q, loc = snap[pb.name]
            q = q.copy()
            if pb.name in prev and prev[pb.name].dot(q) < 0:
                q.negate()
            prev[pb.name] = q
            pb.rotation_quaternion = q
            pb.keyframe_insert("rotation_quaternion", frame=f)
            if pb.name == "pelvis":
                pb.location = loc
                pb.keyframe_insert("location", frame=f)
    act.use_fake_user = True
    arm.animation_data.action = None
    return act


def mirror_snapshot(pose, snap):
    """The same pose reflected left to right (bones swap sides)."""
    pose.apply(snap)
    mats = {pb.name: pb.matrix.copy() for pb in pose.pb}
    rest = pose.rest
    for name in pose.order:
        src = mirror_name(name)
        if src not in mats:
            continue
        m = mats[src]
        r_src = rest[src].to_3x3()
        r_dst = rest[name].to_3x3()
        rot = MIRROR @ m.to_3x3() @ MIRROR
        rot = rot @ (MIRROR @ r_src @ MIRROR).transposed() @ r_dst
        out = rot.normalized().to_4x4()
        out.translation = MIRROR @ m.translation
        pose.pb[name].matrix = out
        pose.update()
    return pose.snapshot()
