"""Builds humanoid-rigged creatures: the skeleton warrior, the stone golem and the Barrow King.

    art/blender.sh -P art/blender/build_creatures.py [-- --review]

They are body types on the shared humanoid skeleton, so the whole clip library, sockets and
gear work on them. The undead are Blender's CC0 anatomical skeleton: each bone is bound rigidly
to the matching rig bone after being laid along it. Golems are stone chunks, one per bone, on a
heavier rig. Run after build_races.py and before build_back.py and build_armour.py, which also
fit capes and plate to the undead.
"""

import json
import math
import random
import sys
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.noise import noise as perlin

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import cloth, export, races as rc, regions as rg, review, scene as sc, shapes as sh  # noqa: E402

OUT = sc.OUT / "humanoid"
CAT_PATH = sc.OUT / "catalog" / "humanoid.json"
HBM = sc.VENDOR / "blender-studio" / "human_base_meshes_bundle.blend"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []


def gl(v):
    return [round(v.x, 5), round(v.z, 5), round(-v.y, 5)]


def rel(p):
    return str(Path(p).relative_to(sc.OUT))


def heads(skel):
    return {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}


def strip_human(keep=("skeleton",)):
    """Leaves only the skeleton armature from a humanoid blend."""
    for o in list(bpy.data.objects):
        if o.name not in keep:
            bpy.data.objects.remove(o)


# --- The skeleton warrior --------------------------------------------------------------------

def anatomical_bones():
    """The CC0 anatomical skeleton: {name: world-space mesh object}, modifiers applied."""
    with bpy.data.libraries.load(str(HBM)) as (src, dst):
        dst.collections = ["Skeleton - Realistic"]
    col = bpy.data.collections["Skeleton - Realistic"]
    bpy.context.scene.collection.children.link(col)
    # Use the base level of any sculpt detail: the game needs thousands of triangles, not millions.
    for o in [o for o in col.all_objects if o.type == "MESH"]:
        for m in o.modifiers:
            if m.type == "MULTIRES":
                m.levels = 0
            elif m.type == "SUBSURF":
                m.levels = min(m.levels, 1)
    dg = bpy.context.evaluated_depsgraph_get()
    out = {}
    for o in [o for o in col.all_objects if o.type == "MESH"]:
        ev = o.evaluated_get(dg)
        me = bpy.data.meshes.new_from_object(ev)
        me.transform(o.matrix_world)
        ob = bpy.data.objects.new(o.name.replace("GEO-skeletion.", ""), me)
        bpy.context.scene.collection.objects.link(ob)
        out[ob.name] = ob
    for o in list(col.all_objects):
        bpy.data.objects.remove(o)
    bpy.data.collections.remove(col)
    return out


def endpoints(obs):
    """The two ends of a long bone (or several) along its principal axis."""
    pts = np.array([[v.co.x, v.co.y, v.co.z] for o in obs for v in o.data.vertices])
    c = pts.mean(axis=0)
    u, s, vt = np.linalg.svd(pts - c, full_matrices=False)
    d = vt[0]
    t = (pts - c) @ d
    return Vector(c + d * t.min()), Vector(c + d * t.max())


def segment_map(a0, a1, r0, r1, lateral):
    """An affine map laying segment a0->a1 along r0->r1 (lengths matched, thickness scaled)."""
    da, dr = (a1 - a0), (r1 - r0)
    q = da.normalized().rotation_difference(dr.normalized())
    R = q.to_matrix()
    axis = dr.normalized()
    along = dr.length / max(da.length, 1e-6)
    S = Matrix.Identity(3) * lateral + (along - lateral) * Matrix([[axis.x * axis.x, axis.x * axis.y, axis.x * axis.z],
                                                                  [axis.y * axis.x, axis.y * axis.y, axis.y * axis.z],
                                                                  [axis.z * axis.x, axis.z * axis.y, axis.z * axis.z]])
    lin = S @ R
    return lambda p: r0 + lin @ (p - a0)


def apply_map(ob, f):
    for v in ob.data.vertices:
        v.co = f(v.co)
    ob.data.update()


def rigid(ob, bone):
    vg = ob.vertex_groups.new(name=bone)
    vg.add(list(range(len(ob.data.vertices))), 1.0, "REPLACE")


def build_skeleton(cat, bt="skeleton", scale=1.0):
    bpy.ops.wm.open_mainfile(filepath=str(sc.ROOT / "art" / "out" / "humanoid-male.blend"))
    strip_human()
    skel = bpy.data.objects["skeleton"]
    if scale != 1.0:
        race = {k: (scale, scale) for k in ("legs", "spine", "neck", "clavicle", "upperarm", "lowerarm")}
        race.update(hands=scale, feet=scale, head=scale)
        A, nh = rc.transforms(skel, race)
        rc.rebuild_skeleton(skel, race, A, nh, 0.0)
    h = heads(skel)
    bones = anatomical_bones()
    # Global fit: height and hip centre.
    zs = [v.co.z for o in bones.values() for v in o.data.vertices]
    top, floor = max(zs), min(zs)
    rig_top = h["Head"].z + (0.24 * scale)
    s = (rig_top - 0.0) / (top - floor)
    hip = sum((v.co for v in bones["hip"].data.vertices), Vector()) / len(bones["hip"].data.vertices)
    target = h["pelvis"] + Vector((0, 0.01, 0))
    torso = lambda p: target + (p - Vector((hip.x, hip.y, hip.z))) * s
    side_of = lambda name: "l" if name.endswith(".L") else "r" if name.endswith(".R") else None
    # Long bones first: their anatomical joints, then the maps.
    maps = {}
    for sd, SD in (("l", "L"), ("r", "R")):
        hu0, hu1 = endpoints([bones[f"arm_humerus.{SD}"]])
        if hu0.z < hu1.z:
            hu0, hu1 = hu1, hu0
        f0, f1 = endpoints([bones[f"arm_radius.{SD}"], bones[f"arm_ulna.{SD}"]])
        if (f0 - hu1).length > (f1 - hu1).length:
            f0, f1 = f1, f0
        fe0, fe1 = endpoints([bones[f"leg_femur.{SD}"]])
        if fe0.z < fe1.z:
            fe0, fe1 = fe1, fe0
        ti0, ti1 = endpoints([bones[f"leg_tibula.{SD}"], bones[f"leg_fibula.{SD}"]])
        if ti0.z < ti1.z:
            ti0, ti1 = ti1, ti0
        maps[f"upperarm_{sd}"] = segment_map(hu0, hu1, h[f"upperarm_{sd}"], h[f"lowerarm_{sd}"], s)
        maps[f"lowerarm_{sd}"] = segment_map(f0, f1, h[f"lowerarm_{sd}"], h[f"hand_{sd}"], s)
        maps[f"thigh_{sd}"] = segment_map(fe0, fe1, h[f"thigh_{sd}"], h[f"calf_{sd}"], s)
        maps[f"calf_{sd}"] = segment_map(ti0, ti1, h[f"calf_{sd}"], h[f"foot_{sd}"], s)
        # Hand: from the wrist to the middle knuckle, then twisted so the palm faces as the rig's.
        hand_parts = [o for n, o in bones.items() if side_of(n) == sd and any(k in n for k in ("metacarpal", "hand_center", "proximal.thumb", "proximal.index", "proximal.middle", "proximal.ring", "proximal.little", "middle.index", "middle.middle", "middle.ring", "middle.little", "distal.thumb", "distal.index", "distal.middle", "distal.ring", "distal.little"))]
        knuckle = sum((v.co for v in bones[f"metacarpal.middle.{SD}"].data.vertices), Vector()) / len(bones[f"metacarpal.middle.{SD}"].data.vertices)
        maps[f"hand_{sd}"] = segment_map(f1, knuckle, h[f"hand_{sd}"], h[f"middle_01_{sd}"], s)
        bones_hand = hand_parts
        # Foot: from the ankle to the toe line.
        foot_parts = [o for n, o in bones.items() if side_of(n) == sd and any(k in n for k in ("foot_", "metatarsal", "proximal.1", "proximal.2", "proximal.3", "proximal.4", "proximal.5", "middle.2", "middle.3", "middle.4", "middle.5", "distal.1", "distal.2", "distal.3", "distal.4", "distal.5"))]
        toes = sum((v.co for o in foot_parts if "metatarsal" in o.name for v in o.data.vertices), Vector()) / max(1, sum(len(o.data.vertices) for o in foot_parts if "metatarsal" in o.name))
        maps[f"foot_{sd}"] = segment_map(ti1, toes, h[f"foot_{sd}"], h[f"ball_{sd}"], s)
        for o in bones_hand:
            apply_map(o, maps[f"hand_{sd}"])
            # The nearest finger bone of the rig, by the piece's centre.
            c = sum((v.co for v in o.data.vertices), Vector()) / len(o.data.vertices)
            cands = [b for b in h if b.endswith(f"_{sd}") and b.split("_")[0] in ("hand", "thumb", "index", "middle", "ring", "pinky") and "leaf" not in b]
            best = min(cands, key=lambda b: (h[b] - c).length)
            rigid(o, best)
        for o in foot_parts:
            apply_map(o, maps[f"foot_{sd}"])
            c = sum((v.co for v in o.data.vertices), Vector()) / len(o.data.vertices)
            rigid(o, f"ball_{sd}" if (c - h[f"ball_{sd}"]).length < (c - h[f"foot_{sd}"]).length * 0.7 else f"foot_{sd}")
        for name, bone in ((f"arm_humerus.{SD}", f"upperarm_{sd}"), (f"arm_radius.{SD}", f"lowerarm_{sd}"), (f"arm_ulna.{SD}", f"lowerarm_{sd}"),
                           (f"leg_femur.{SD}", f"thigh_{sd}"), (f"leg_tibula.{SD}", f"calf_{sd}"), (f"leg_fibula.{SD}", f"calf_{sd}"),
                           (f"leg_patella.{SD}", f"calf_{sd}")):
            apply_map(bones[name], maps[bone])
            rigid(bones[name], bone)
    # Everything else follows the torso fit and the nearest spine-chain bone.
    spine = ["pelvis", "spine_01", "spine_02", "spine_03", "neck_01", "Head"]
    done = {o.name for o in bones.values() if o.vertex_groups}
    for name, o in bones.items():
        if name in done:
            continue
        apply_map(o, torso)
        c = sum((v.co for v in o.data.vertices), Vector()) / len(o.data.vertices)
        if "skull" in name:
            bone = "Head"
        elif "cervical" in name:
            bone = "neck_01"
        elif "clavicle" in name or "scapula" in name:
            bone = f"clavicle_{side_of(name)}"
        elif name in ("hip", "sacral", "coccygeal"):
            bone = "pelvis"
        else:
            bone = min(spine[:-1], key=lambda b: abs(h[b].z - c.z) if b != "pelvis" else abs(h[b].z - c.z) + 0.05)
        if name == "ripcage":
            # The ribcage flexes between the mid and upper spine.
            vg2, vg3 = o.vertex_groups.new(name="spine_02"), o.vertex_groups.new(name="spine_03")
            z2, z3 = h["spine_02"].z, h["spine_03"].z + 0.08
            for v in o.data.vertices:
                t = max(0.0, min(1.0, (v.co.z - z2) / (z3 - z2)))
                vg2.add([v.index], 1 - t, "REPLACE")
                vg3.add([v.index], t, "REPLACE")
        else:
            rigid(o, bone)
    # One mesh, aged bone, and a cold glow in the sockets.
    objs = list(bones.values())
    body = sh.join(objs, "body")
    tris = sum(len(p.vertices) - 2 for p in body.data.polygons)
    if tris > 30000:
        dec = body.modifiers.new("decimate", "DECIMATE")
        dec.ratio = 28000 / tris
        sc.activate(body)
        bpy.ops.object.modifier_apply(modifier=dec.name)
    body.data.materials.clear()
    body.data.materials.append(sh.material("bone"))
    for p in body.data.polygons:
        p.use_smooth = True
    zs = [v.co.z for v in body.data.vertices]
    skull_pts = [v.co for v in body.data.vertices if v.co.z > h["Head"].z + 0.03]
    sk_front = min(p.y for p in skull_pts)
    sk_z = h["Head"].z + 0.24 * scale * 0.42
    eyes = []
    for sx in (-1, 1):
        e = sh.obj("eye", sh.lathe([(0.0, -0.012), (0.011, -0.006), (0.012, 0.0), (0.011, 0.006), (0.0, 0.012)], 12), "crystal_blue")
        e.scale = (scale, scale, scale)
        e.location = (sx * 0.031 * scale, sk_front + 0.03 * scale, sk_z)
        rigid(e, "Head")
        eyes.append(e)
    glow = sh.join(eyes, "eyes")
    occlusion_colours(body, base=(0.86, 0.80, 0.66), dark=(0.32, 0.27, 0.2))
    sc.bind(body, skel)
    sc.bind(glow, skel)
    rg.tag(body, skel)
    return skel, body, glow


def occlusion_colours(ob, base, dark, rays=10, reach=0.06):
    """Bakes a cheap ambient-occlusion tint into the vertex colours, used by the material."""
    from mathutils.bvhtree import BVHTree
    me = ob.data
    tree = BVHTree.FromObject(ob, bpy.context.evaluated_depsgraph_get())
    rnd = random.Random(7)
    dirs = []
    while len(dirs) < rays:
        d = Vector((rnd.uniform(-1, 1), rnd.uniform(-1, 1), rnd.uniform(-1, 1)))
        if 0.1 < d.length <= 1:
            dirs.append(d.normalized())
    occ = []
    for v in me.vertices:
        n = v.normal
        hits, tot = 0, 0
        for d in dirs:
            if d.dot(n) < 0:
                d = -d
            tot += 1
            if tree.ray_cast(v.co + n * 0.002, d, reach)[0] is not None:
                hits += 1
        occ.append(hits / max(tot, 1))
    attr = me.color_attributes.new("Col", "BYTE_COLOR", "POINT")
    for i, o in enumerate(occ):
        k = min(1.0, o * 1.4)
        c = [base[j] * (1 - k) + dark[j] * k for j in range(3)]
        attr.data[i].color = (*c, 1.0)
    me.color_attributes.active_color = attr
    m = me.materials[0]
    nt = m.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Col"
    nt.links.new(vc.outputs["Color"], bsdf.inputs["Base Color"])


# --- The stone golem ---------------------------------------------------------------------------

GOLEM = {
    "label": "Stone golem",
    "legs": (0.95, 1.9), "spine": (1.2, 1.9), "neck": (0.7, 1.8), "clavicle": (1.75, 1.7),
    "upperarm": (1.35, 2.1), "lowerarm": (1.4, 2.3), "hands": 2.0, "feet": 1.85, "head": 1.0,
}


def rock(name, centre, axis, length, radius, seed, mat="stone"):
    """A faceted stone, roughly an ellipsoid along `axis`."""
    rnd = random.Random(seed)
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=2, radius=1.0)
    off = Vector((rnd.random() * 10, rnd.random() * 10, rnd.random() * 10))
    for v in bm.verts:
        n = v.co.normalized()
        d = 1.0 + 0.22 * perlin(n * 1.6 + off) + 0.08 * perlin(n * 4.0 + off)
        v.co = Vector((n.x * radius * d, n.y * length / 2 * d, n.z * radius * d * 0.92))
    # Flatten a few facets so it reads as cut stone.
    for _ in range(3):
        plane = Vector((rnd.uniform(-1, 1), rnd.uniform(-1, 1), rnd.uniform(-1, 1))).normalized()
        lim = 0.78 * min(radius, length / 2)
        for v in bm.verts:
            k = v.co.dot(plane)
            if k > lim:
                v.co -= plane * (k - lim)
    q = Vector((0, 1, 0)).rotation_difference(axis.normalized())
    bmesh.ops.transform(bm, matrix=Matrix.Translation(centre) @ q.to_matrix().to_4x4(), verts=bm.verts)
    ob = sh.obj(name, bm, mat, smooth=False)
    # Moss on the upward faces.
    ob.data.materials.append(sh.material("moss"))
    for p in ob.data.polygons:
        if p.normal.z > 0.62:
            p.material_index = 1
    return ob


def build_golem(cat):
    bpy.ops.wm.open_mainfile(filepath=str(sc.ROOT / "art" / "out" / "humanoid-male.blend"))
    body_h = bpy.data.objects["body"]
    body_h.name = "human_reference"
    skel = bpy.data.objects["skeleton"]
    # Size each stone from the human body before the rig changes.
    A, nh = rc.transforms(skel, GOLEM)
    rc.deform(body_h, A, 0.0)
    dz = rc.ground_offset([body_h])
    for v in body_h.data.vertices:
        v.co.z -= dz
    body_h.data.update()
    rc.rebuild_skeleton(skel, GOLEM, A, nh, dz)
    strip_human(keep=("skeleton", "human_reference"))
    h = heads(skel)
    groups = {g.index: g.name for g in body_h.vertex_groups}
    pts_by = {}
    for v in body_h.data.vertices:
        if v.groups:
            g = max(v.groups, key=lambda g: g.weight)
            pts_by.setdefault(groups[g.group], []).append(body_h.matrix_world @ v.co)
    nxt = {"pelvis": "spine_01", "spine_01": "spine_02", "spine_02": "spine_03", "spine_03": "neck_01", "neck_01": "Head"}
    for sd in ("l", "r"):
        nxt.update({f"clavicle_{sd}": f"upperarm_{sd}", f"upperarm_{sd}": f"lowerarm_{sd}", f"lowerarm_{sd}": f"hand_{sd}",
                    f"hand_{sd}": f"middle_02_{sd}", f"thigh_{sd}": f"calf_{sd}", f"calf_{sd}": f"foot_{sd}", f"foot_{sd}": f"ball_leaf_{sd}"})
    stones = []
    for i, (bone, child) in enumerate(nxt.items()):
        a, b = h[bone], h[child]
        pts = pts_by.get(bone, [])
        seg = b - a
        if pts:
            ds = sorted((p - (a + seg * max(0.0, min(1.0, (p - a).dot(seg) / seg.length_squared)))).length for p in pts)
            r = ds[int(len(ds) * 0.75)]
        else:
            r = 0.08
        length = seg.length * (1.5 if bone.startswith(("spine", "pelvis")) else 1.35)
        r *= 1.15 if bone.startswith(("spine", "pelvis")) else 1.08
        if bone.startswith(("thigh", "calf")):
            r = max(r * 1.2, 0.16 if bone.startswith("thigh") else 0.14)
        c = a + seg * 0.5
        if bone == "spine_03":
            r *= 1.18
        st = rock(f"stone_{bone}", c, seg, length, max(r, 0.05), seed=i * 7 + 3)
        rigid(st, bone)
        stones.append(st)
    # Joint stones bridge the elbows, knees and shoulders.
    for k, (bone, at, rad) in enumerate([(f"{b}_{sd}", j, r) for sd in ("l", "r") for b, j, r in
                                         (("lowerarm", "lowerarm", 0.15), ("calf", "calf", 0.16), ("upperarm", "upperarm", 0.21))]):
        side = bone[-1]
        jt = h[f"{at}_{side}"]
        st = rock(f"joint_{bone}", jt + Vector((0, 0.0, 0.03 if "upperarm" in bone else 0.0)), Vector((0, 0, 1)), rad * 1.7, rad, seed=200 + k)
        rigid(st, bone)
        stones.append(st)
    # The head: a small brow-heavy stone with a glowing slit.
    hd = rock("stone_head", h["Head"] + Vector((0, -0.02, 0.13)), Vector((0, 0, 1)), 0.26, 0.13, seed=99)
    rigid(hd, "Head")
    stones.append(hd)
    slit = sh.obj("slit", sh.ribbon([h["Head"] + Vector((x, -0.13, 0.15)) for x in (-0.07, 0.0, 0.07)], [Vector((0, -1, 0))] * 3, 0.018, 0.004), "rune")
    rigid(slit, "Head")
    # A glowing core in the chest, and runes down the arms.
    core = sh.obj("core", sh.lathe([(0.0, -0.07), (0.06, -0.03), (0.065, 0.0), (0.06, 0.03), (0.0, 0.07)], 8), "rune")
    core.location = h["spine_03"] + Vector((0, -0.24, 0.02))
    rigid(core, "spine_03")
    parts = stones + [slit, core]
    for sd in ("l", "r"):
        a, b = h[f"upperarm_{sd}"], h[f"lowerarm_{sd}"]
        line = [a.lerp(b, t) + Vector((0, -0.17, 0.12)) for t in (0.25, 0.5, 0.75)]
        rune = sh.obj("rune", sh.ribbon(line, [Vector((0, -0.6, 0.8))] * 3, 0.02, 0.004), "rune")
        rigid(rune, f"upperarm_{sd}")
        parts.append(rune)
    body = sh.join(parts, "body")
    bpy.data.objects.remove(body_h)
    sc.bind(body, skel)
    rg.tag(body, skel)
    return skel, body


def crown(skel, s, body=None):
    h = heads(skel)
    c = h["Head"] + Vector((0, 0.005, 0.2 * s))
    if body is not None:
        # Rest the band just below the crown of the skull, sized to it.
        mw = body.matrix_world
        skull = [mw @ v.co for v in body.data.vertices if (mw @ v.co).z > h["Head"].z and abs((mw @ v.co).x - h["Head"].x) < 0.15 * s]
        top = max(p.z for p in skull)
        ring = [p for p in skull if abs(p.z - (top - 0.035 * s)) < 0.01 * s]
        cx = sum((p for p in ring), Vector()) / max(1, len(ring))
        rad = max(((p - cx) * Vector((1, 1, 0))).length for p in ring) if ring else 0.07 * s
        c = Vector((cx.x, cx.y, top - 0.035 * s))
        s = rad / 0.085 * 1.04
    ring = []
    n = 9
    pts = []
    for i in range(n * 2 + 1):
        a = 2 * math.pi * i / (n * 2)
        r = 0.085 * s
        z = c.z + (0.045 * s if i % 2 == 0 else 0.012 * s)
        pts.append(Vector((c.x + math.cos(a) * r, c.y + math.sin(a) * r, z)))
    base = [Vector((c.x + math.cos(2 * math.pi * i / 36) * 0.085 * s, c.y + math.sin(2 * math.pi * i / 36) * 0.085 * s, c.z - 0.01 * s)) for i in range(37)]
    band = sh.obj("crown_band", sh.ribbon(base, [(p - c).normalized() for p in base], 0.03 * s, 0.004 * s), "gold")
    spikes = sh.obj("crown_points", sh.ribbon(pts, [(p - c).normalized() for p in pts], 0.012 * s, 0.004 * s), "gold")
    gems = []
    for i in range(0, n * 2, 2):
        a = 2 * math.pi * i / (n * 2)
        gm = sh.obj("gem", sh.lathe([(0.0, -0.008), (0.008, 0.0), (0.0, 0.008)], 6), "crystal_violet")
        gm.scale = (s, s, s)
        gm.location = (c.x + math.cos(a) * 0.088 * s, c.y + math.sin(a) * 0.088 * s, c.z + 0.002 * s)
        gems.append(gm)
    ob = sh.join([band, spikes] + gems, "crown")
    rigid(ob, "Head")
    sc.bind(ob, skel)
    return ob


def finish_body_type(cat, bt, label, race, skel, objs, extra=None):
    """Exports a creature body and records it in the catalogue."""
    path = export.piece(OUT / bt / "body.gltf", [skel] + objs)
    bpy.ops.wm.save_as_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{bt}.blend"))
    body = bpy.data.objects["body"]
    entry = {
        "label": label, "race": race, "frame": "male", "file": rel(path),
        "pelvis": gl(skel.data.bones["pelvis"].head_local),
        "height": round(max((body.matrix_world @ v.co).z for v in body.data.vertices), 3),
        "skin_reference": None,
        "sockets": sorted(b.name for b in skel.data.bones if b.name.startswith("socket_")),
        "colliders": cloth.colliders(skel, body),
        "gear_scale": 1.0, "creature": True,
    }
    entry.update(extra or {})
    cat["body_types"][bt] = entry
    return entry


def main():
    cat = json.loads(CAT_PATH.read_text())
    skel, body, eyes = build_skeleton(cat)
    finish_body_type(cat, "skeleton", "Skeleton warrior", "undead", skel, [body, eyes])
    skel, body, eyes = build_skeleton(cat, "barrow_king", scale=1.3)
    c = crown(skel, 1.3, body)
    finish_body_type(cat, "barrow_king", "The Barrow King", "undead", skel, [body, eyes, c],
                     {"gear_scale": 1.3, "boss": True})
    skel, body = build_golem(cat)
    finish_body_type(cat, "golem", "Stone golem", "construct", skel, [body], {"gear_scale": 1.8, "dress": False})
    cat.setdefault("races", {})
    cat["races"]["undead"] = {"label": "Undead", "frames": {"male": "skeleton", "boss": "barrow_king"}, "npc_only": True, "creature": True}
    cat["races"]["construct"] = {"label": "Construct", "frames": {"male": "golem"}, "npc_only": True, "creature": True}
    for bt in ("skeleton", "barrow_king", "golem"):
        cat.setdefault("defaults", {})[bt] = {"pieces": [], "hair": None, "brows": None, "beard": None, "tone": "warm", "hair_colour": "black"}
    CAT_PATH.write_text(json.dumps(cat, indent=1))
    print("WROTE", CAT_PATH)
    if "--review" in ARGS:
        from aldermoor_art import looks
        looks.CAT = looks.catalog()
        files = []
        for bt, clip, frame in (("skeleton", "Sword_Idle", 12), ("skeleton", "Zombie_Walk_Fwd_Loop", 10), ("barrow_king", "Sword_Idle", 12),
                                ("golem", "Idle_Loop", 20), ("golem", "Punch_Cross", 10)):
            _, objs = looks.assemble(dict(body=bt, pieces=[], hair=None, brows=None, beard=None, tone="warm", hair_colour="black", clip=clip, frame=frame))
            for yaw in (25, 180):
                f = sc.ROOT / "art" / "out" / "review" / "creatures" / f"{bt}-{clip}-{yaw}.png"
                review.render(f, objs, yaw=yaw, size=(560, 760))
                files.append(f)
        review.montage(sc.ROOT / "art" / "out" / "review" / "creatures" / "creatures.png", files, 4)


main()
