"""Builds quadruped beasts with their own rig and animations: the grey wolf and the dire wolf.

    art/blender.sh -P art/blender/build_beasts.py [-- --review]

The body is an implicit surface (metaball capsules along the skeleton), remeshed into one clean
manifold mesh, given fur ruffs and shaded with vertex colours. The rig is weighted
automatically. Gaits are generated: paws follow stance-and-swing paths per leg, solved with
two-bone IK, with body bob, spine flex, head counter-motion and a swaying tail, then baked to
keyframes. Writes assets/creatures/<id>.glb and assets/catalog/creatures.json.
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
from aldermoor_art import review, scene as sc, shapes as sh  # noqa: E402

OUT = sc.OUT / "creatures"
CAT_PATH = sc.OUT / "catalog" / "creatures.json"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
FPS = sc.FPS

# The wolf faces -Y, stands on z = 0. Joint positions in metres.
J = {
    "root": (0, 0, 0),
    "hips": (0, 0.40, 0.74), "spine_1": (0, 0.18, 0.76), "spine_2": (0, -0.06, 0.79), "chest": (0, -0.28, 0.81),
    "neck_1": (0, -0.40, 0.86), "neck_2": (0, -0.50, 0.95), "head": (0, -0.58, 1.01), "head_end": (0, -0.66, 1.03),
    "jaw": (0, -0.61, 0.95), "jaw_end": (0, -0.8, 0.925),
    "tail_1": (0, 0.50, 0.75), "tail_2": (0, 0.62, 0.70), "tail_3": (0, 0.72, 0.60), "tail_4": (0, 0.80, 0.49), "tail_end": (0, 0.86, 0.38),
}
for sd, x in (("l", 1), ("r", -1)):
    J.update({
        f"ear_{sd}": (0.055 * x, -0.57, 1.08), f"ear_end_{sd}": (0.075 * x, -0.555, 1.18),
        f"scapula_{sd}": (0.09 * x, -0.25, 0.83), f"humerus_{sd}": (0.12 * x, -0.33, 0.62), f"radius_{sd}": (0.125 * x, -0.29, 0.38),
        f"pastern_{sd}": (0.125 * x, -0.30, 0.11), f"fpaw_{sd}": (0.125 * x, -0.33, 0.04), f"fpaw_end_{sd}": (0.125 * x, -0.40, 0.02),
        f"femur_{sd}": (0.11 * x, 0.42, 0.70), f"tibia_{sd}": (0.12 * x, 0.32, 0.45), f"hock_{sd}": (0.12 * x, 0.47, 0.20),
        f"hpaw_{sd}": (0.12 * x, 0.44, 0.045), f"hpaw_end_{sd}": (0.12 * x, 0.37, 0.02),
    })
J = {k: Vector(v) for k, v in J.items()}

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


def capsule(mb, a, b, r):
    el = mb.elements.new(type="CAPSULE")
    d = b - a
    el.co = (a + b) / 2
    el.size_x = max(d.length / 2, 1e-4)
    el.radius = r
    el.rotation = Vector((1, 0, 0)).rotation_difference(d.normalized() if d.length > 1e-6 else Vector((1, 0, 0)))
    el.stiffness = 2.0
    return el


def ball(mb, c, r, kind="BALL", size=(1, 1, 1), rot=None):
    el = mb.elements.new(type=kind)
    el.co = c
    el.radius = r
    if kind == "ELLIPSOID":
        el.size_x, el.size_y, el.size_z = size
    if rot is not None:
        el.rotation = rot
    el.stiffness = 2.0
    return el


def _ring(c, w, h, n=20, drop=0.0):
    """An egg-shaped cross-section in the XZ plane around c: w wide, h deep, fuller below."""
    pts = []
    for i in range(n):
        a = 2 * math.pi * i / n
        x = math.cos(a) * w / 2
        z = math.sin(a) * h / 2
        if z < 0:
            x *= 1.0 - drop * (-z / (h / 2)) ** 2
        pts.append(c + Vector((x, 0, z)))
    return pts


def _tube(points, radii, n=12):
    """A tube through points; ring frames are carried along (parallel transport), so no twist."""
    secs = []
    side = None
    for i, (p, r) in enumerate(zip(points, radii)):
        tan = (points[min(i + 1, len(points) - 1)] - points[max(i - 1, 0)]).normalized()
        if side is None:
            side = tan.cross(Vector((1, 0, 0)))
            if side.length < 1e-3:
                side = tan.cross(Vector((0, 1, 0)))
        side = (side - tan * side.dot(tan)).normalized()
        up = side.cross(tan).normalized()
        secs.append(sh.ring(p, side, up, r, r, n))
    return sh.loft(secs)


def wolf_body(scale, bulk=1.0):
    """The body as lofted torso, neck, head, legs and tail, unioned by a voxel remesh."""
    s = scale
    B = bulk
    parts = []
    # Torso along the spine from rump to shoulders: (y, back height, width, depth).
    prof = [(0.50, 0.70, 0.12, 0.16), (0.42, 0.74, 0.20, 0.25), (0.30, 0.755, 0.19, 0.24), (0.16, 0.765, 0.165, 0.22),
            (0.02, 0.785, 0.19, 0.27), (-0.12, 0.81, 0.235, 0.35), (-0.24, 0.83, 0.25, 0.38), (-0.34, 0.835, 0.23, 0.34),
            (-0.42, 0.84, 0.18, 0.25)]
    secs = []
    for y, back, w, d in prof:
        c = Vector((0, y, back - d / 2)) * s
        secs.append(_ring(c, w * s * B, d * s * B, 24, drop=0.25))
    torso = sh.loft([[Vector((0, (prof[0][0] + 0.03) * s, (prof[0][1] - 0.08) * s))]] + secs + [[Vector((0, (prof[-1][0] - 0.04) * s, (prof[-1][1] - 0.12) * s))]])
    parts.append(sh.obj("torso", torso))
    # Neck with a ruff, rising to the head.
    neck_pts = [Vector((0, -0.34, 0.76)), Vector((0, -0.44, 0.84)), Vector((0, -0.52, 0.93)), Vector((0, -0.57, 0.99))]
    parts.append(sh.obj("neck", _tube([p * s for p in neck_pts], [r * s * B for r in (0.13, 0.12, 0.095, 0.08)], 16)))
    # Head: a rounded skull and a tapering muzzle.
    skull = sh.lathe([(0.0, -0.075), (0.05, -0.065), (0.075, -0.02), (0.075, 0.03), (0.05, 0.07), (0.0, 0.08)], 16)
    sh.transform(skull, Matrix.Translation(Vector((0, -0.6, 1.0)) * s) @ Matrix.Rotation(math.radians(90), 4, "X") @ Matrix.Diagonal((1.0 * s * B, 0.9 * s, 1.15 * s, 1.0)))
    parts.append(sh.obj("skull", skull))
    muzzle_pts = [Vector((0, -0.62, 0.985)), Vector((0, -0.7, 0.965)), Vector((0, -0.78, 0.952)), Vector((0, -0.845, 0.95))]
    parts.append(sh.obj("muzzle", _tube([p * s for p in muzzle_pts], [r * s * B for r in (0.06, 0.047, 0.036, 0.026)], 14)))
    # Legs.
    for sd in ("l", "r"):
        f = [J[f"scapula_{sd}"] + Vector((0, 0.02, -0.05)), J[f"humerus_{sd}"], J[f"radius_{sd}"], J[f"pastern_{sd}"], J[f"fpaw_{sd}"]]
        parts.append(sh.obj("fleg", _tube([p * s for p in f], [r * s * B for r in (0.08, 0.068, 0.043, 0.033, 0.031)], 12)))
        h = [J[f"femur_{sd}"] + Vector((0, 0.0, 0.02)), J[f"tibia_{sd}"] + Vector((0, 0.01, 0.0)), J[f"hock_{sd}"], J[f"hpaw_{sd}"]]
        parts.append(sh.obj("hleg", _tube([p * s for p in h], [r * s * B for r in (0.1, 0.065, 0.036, 0.031)], 12)))
        for pw in (J[f"fpaw_{sd}"], J[f"hpaw_{sd}"]):
            paw = sh.lathe([(0.0, -0.025), (0.032, -0.02), (0.036, 0.0), (0.026, 0.02), (0.0, 0.026)], 12)
            sh.transform(paw, Matrix.Translation(pw * s + Vector((0, -0.025, -0.012)) * s) @ Matrix.Diagonal((s * B, 1.35 * s, 0.75 * s, 1.0)))
            parts.append(sh.obj("paw", paw))
    # A bushy tail, thickest in the middle.
    tail = [J["tail_1"], J["tail_2"], J["tail_3"], J["tail_4"], J["tail_end"]]
    parts.append(sh.obj("tail", _tube([p * s for p in tail], [r * s * B for r in (0.045, 0.07, 0.075, 0.06, 0.02)], 14)))
    body = sh.join(parts, "body")
    body.data.remesh_voxel_size = 0.0105 * scale
    sc.activate(body)
    bpy.ops.object.voxel_remesh()
    # Blend the creases where legs and neck meet the torso, keeping the volume.
    lap = body.modifiers.new("blend", "LAPLACIANSMOOTH")
    lap.lambda_factor = 0.6
    lap.iterations = 12
    lap.use_volume_preserve = True
    bpy.ops.object.modifier_apply(modifier=lap.name)
    return body


def lower_jaw(scale, bulk=1.0):
    """The lower jaw as its own piece, so the mouth can open without tearing the muzzle."""
    s, B = scale, bulk
    pts = [Vector((0, -0.62, 0.935)), Vector((0, -0.7, 0.922)), Vector((0, -0.78, 0.917)), Vector((0, -0.825, 0.92))]
    jaw = sh.obj("jaw", _tube([p * s for p in pts], [r * s * B for r in (0.038, 0.03, 0.023, 0.012)], 12))
    jaw.data.materials.clear()
    vg = jaw.vertex_groups.new(name="jaw")
    vg.add(list(range(len(jaw.data.vertices))), 1.0, "REPLACE")
    return jaw


def fur(body, scale, seed):
    """Spiky ruffs on the neck, back and tail; smooth fur elsewhere."""
    rnd = random.Random(seed)
    off = Vector((rnd.random() * 9, rnd.random() * 9, rnd.random() * 9))
    me = body.data
    for v in me.vertices:
        p = v.co / scale
        n = v.normal
        ruff = max(0.0, 1 - ((p.y + 0.42) / 0.16) ** 2) * (1 if p.z > 0.72 else 0.5)     # Neck mane.
        back = max(0.0, n.z) * (1 if p.z > 0.8 else 0) * 0.5                                 # Along the spine.
        tail = max(0.0, (p.y - 0.55) / 0.3) * (1 if p.z < 0.8 else 0)                       # The tail.
        cheek = max(0.0, 1 - ((p - Vector((0, -0.6, 0.97))).length / 0.09) ** 2) * (1 if abs(p.x) > 0.04 else 0)
        mask = min(1.0, ruff + back + tail + cheek * 0.8)
        if mask <= 0:
            continue
        k = perlin(p * 38 + off)
        spike = max(0.0, k) ** 1.5
        v.co += n * (0.022 * scale * mask * spike)
    me.update()


def ears(skel, scale):
    parts = []
    for sd, x in (("l", 1), ("r", -1)):
        base = J[f"ear_{sd}"] * scale
        tip = J[f"ear_end_{sd}"] * scale
        bm = bmesh.new()
        ring = []
        for i in range(6):
            a = 2 * math.pi * i / 6
            ring.append(bm.verts.new(base + Vector((math.cos(a) * 0.035, math.sin(a) * 0.016 + 0.006, 0)) * scale))
        t = bm.verts.new(tip)
        for i in range(6):
            bm.faces.new((ring[i], ring[(i + 1) % 6], t))
        bm.faces.new(list(reversed(ring)))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        ob = sh.obj(f"ear_{sd}", bm, None, smooth=True)
        parts.append(ob)
    return parts


def paint(body, palette):
    """Vertex colours: darker saddle, pale belly and legs, dark muzzle tip and ear tips."""
    me = body.data
    attr = me.color_attributes.new("Col", "BYTE_COLOR", "POINT")
    s = palette["scale"]
    for v in me.vertices:
        p = v.co / s
        n = v.normal
        top = max(0.0, n.z) ** 0.7 * (1 if p.z > 0.62 else 0)
        under = max(0.0, -n.z) * 1.2 + (1 if p.z < 0.42 else 0) * 0.8
        c = Vector(palette["coat"])
        c = c.lerp(Vector(palette["saddle"]), min(1.0, top * 1.2))
        c = c.lerp(Vector(palette["belly"]), min(1.0, under))
        if p.y < -0.78:
            c = c.lerp(Vector(palette["nose"]), min(1.0, (-0.78 - p.y) / 0.06))
        if p.z > 1.12:
            c = c.lerp(Vector(palette["saddle"]), 0.8)
        attr.data[v.index].color = (*c, 1.0)
    me.color_attributes.active_color = attr


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


def coat_material(name, rough=0.85):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Col"
    nt.links.new(vc.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Sheen Weight"].default_value = 0.0
    return m


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
        for pb in self.skel.pose.bones:
            pb.matrix_basis.identity()
            pb.rotation_mode = "QUATERNION"
        bpy.context.view_layer.update()

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
        P.rotate("jaw", X, -0.55 * bite)
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
        pb = P.skel.pose.bones["root"]
        pb.rotation_quaternion = Quaternion((0, 1, 0), -1.45 * roll)
        pb.location = Vector((0.0, 0.0, 0.0))
        bpy.context.view_layer.update()
        P.move_root(Vector((0, 0, -0.38 * k * scale)))
        bent = {leg: p + Vector((0, 0.06 if leg[0] == "f" else -0.04, 0.22 * k)) * scale for leg, p in rest.items()}
        legs(P, scale, bent, crouch=k)
        P.rotate("neck_1", X, -0.5 * k)
        P.rotate("jaw", X, -0.2 * k)
        P.rotate("tail_1", X, 0.4 * k)

    clip(P, scale, "Death", 1.6, death)

    def jump(P, t, f):
        crouch = ease(t / 0.35) * (1 - ease((t - 0.35) / 0.1))
        up = math.sin(math.pi * max(0.0, min(1.0, (t - 0.4) / 0.5)))
        P.move_root(Vector((0, 0, (-0.1 * crouch + 0.35 * up) * scale)))
        paws = {leg: p + Vector((0, 0, 0.35 * up + (0.12 * up if leg[0] == "h" else 0.2 * up))) * scale for leg, p in rest.items()}
        legs(P, scale, paws, crouch)
        P.rotate("neck_1", X, 0.2 * up)
        P.rotate("tail_1", X, -0.3 * up)

    clip(P, scale, "Jump", 1.0, jump)

    def howl(P, t, f):
        k = ease(t / 0.25) * (1 - ease((t - 0.8) / 0.2))
        legs(P, scale, rest)
        P.rotate("neck_1", X, 0.75 * k)
        P.rotate("neck_2", X, 0.35 * k)
        P.rotate("jaw", X, -0.35 * k * (0.8 + 0.2 * math.sin(t * 40)))
        for sd in ("l", "r"):
            P.rotate(f"ear_{sd}", X, 0.15 * k)

    clip(P, scale, "Howl", 2.6, howl)
    P.reset()
    skel.animation_data.action = None


BEASTS = [
    dict(id="wolf.grey", label="Grey wolf", scale=1.0, bulk=1.0, seed=3,
         palette=dict(coat=(0.15, 0.14, 0.125), saddle=(0.045, 0.04, 0.036), belly=(0.36, 0.33, 0.29), nose=(0.02, 0.016, 0.014)),
         eyes="crystal_gold", speeds={"walk": 1.6, "run": 6.4}),
    dict(id="wolf.dire", label="Dire wolf", scale=1.28, bulk=1.12, seed=11,
         palette=dict(coat=(0.05, 0.045, 0.042), saddle=(0.016, 0.014, 0.013), belly=(0.12, 0.105, 0.095), nose=(0.012, 0.01, 0.01)),
         eyes="crystal_red", speeds={"walk": 1.8, "run": 7.0}),
]


def build(beast):
    sc.reset()
    sh.PALETTE.setdefault("crystal_gold", ((1.0, 0.7, 0.15), 0.0, 0.1))
    sh.PALETTE.setdefault("crystal_red", ((1.0, 0.15, 0.08), 0.0, 0.1))
    sh.GLOW.update(crystal_gold=2.5, crystal_red=3.5)
    s = beast["scale"]
    skel = make_rig(s)
    body = wolf_body(s, beast["bulk"])
    fur(body, s, beast["seed"])
    # Keep it light: a few thousand triangles.
    tris = sum(len(p.vertices) - 2 for p in body.data.polygons)
    if tris > 16000:
        dec = body.modifiers.new("dec", "DECIMATE")
        dec.ratio = 16000 / tris
        sc.activate(body)
        bpy.ops.object.modifier_apply(modifier=dec.name)
    sm = body.modifiers.new("smooth", "SMOOTH")
    sm.factor, sm.iterations = 0.5, 2
    sc.activate(body)
    bpy.ops.object.modifier_apply(modifier=sm.name)
    for p in body.data.polygons:
        p.use_smooth = True
    pal = dict(beast["palette"], scale=s)
    extras = ears(skel, s)
    body = sh.join([body] + extras, "body")
    paint(body, pal)
    body.data.materials.clear()
    body.data.materials.append(coat_material(f"coat.{beast['id']}"))
    # Weights from the nearest bone segments, then eyes and nose as rigid parts of the head.
    segment_weights(body, skel, s)
    jaw = lower_jaw(s, beast["bulk"])
    body = sh.join([body, jaw], "body")
    for name in [c.name for c in body.data.color_attributes]:
        body.data.color_attributes.remove(body.data.color_attributes[name])
    paint(body, pal)
    sc.bind(body, skel)
    eyes = []
    for sd, x in (("l", 1), ("r", -1)):
        e = sh.obj("eye", sh.lathe([(0.0, -0.012), (0.011, -0.004), (0.011, 0.004), (0.0, 0.012)], 10), beast["eyes"])
        e.scale = (s, s, s)
        e.location = Vector((0.042 * x, -0.66, 1.025)) * s
        e.rotation_euler = (math.radians(90), 0, 0)
        eyes.append(e)
    nose = sh.obj("nose", sh.lathe([(0.0, -0.016), (0.018, -0.006), (0.016, 0.01), (0.0, 0.016)], 10), "leather_dark")
    nose.scale = (s, s, s)
    nose.location = Vector((0, -0.84, 0.965)) * s
    extras = sh.join(eyes + [nose], "face")
    vg = extras.vertex_groups.new(name="head")
    vg.add(list(range(len(extras.data.vertices))), 1.0, "REPLACE")
    sc.bind(extras, skel)
    animate(skel, s)
    path = OUT / f"{beast['id']}.glb"
    path.parent.mkdir(parents=True, exist_ok=True)
    for o in bpy.context.view_layer.objects:
        o.select_set(o in (skel, body, extras))
    bpy.context.view_layer.objects.active = skel
    bpy.ops.export_scene.gltf(filepath=str(path), export_format="GLB", use_selection=True, export_animations=True,
                              export_animation_mode="ACTIONS", export_force_sampling=True, export_skins=True,
                              export_yup=True, export_apply=False, export_vertex_color="ACTIVE", export_all_vertex_colors=False)
    entry = {
        "id": beast["id"], "label": beast["label"], "family": "beast", "file": str(path.relative_to(sc.OUT)),
        "height": round(max((body.matrix_world @ v.co).z for v in body.data.vertices), 3),
        "triangles": sum(len(p.vertices) - 2 for p in body.data.polygons),
        "clips": {"idle": "Idle_Loop", "walk": "Walk_Loop", "run": "Run_Loop", "attack": "Attack", "hit": "Hit",
                  "death": "Death", "jump": "Jump", "special": "Howl"},
        "speeds": beast["speeds"], "sim_body": "wolf",
    }
    if "--review" in ARGS:
        review_beast(beast, skel, [body, extras])
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


main()
