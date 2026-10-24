"""Body regions: what a piece of equipment hides on the layers beneath it.

Every skinned piece (the body, base clothes, armour) carries a per-corner `_REGION` attribute,
exported as a glTF vertex attribute. A piece lists the regions it covers, and the renderer drops
triangles in those regions from every piece on a lower layer, so skin and inner layers can't poke
through. Regions follow the skeleton, so the same rule works on every body, race and outfit.
"""

import bpy
from mathutils import Vector

REGIONS = [
    "scalp", "face", "ears", "neck",
    "chest", "belly", "hips",
    "upperarm_hi", "upperarm_lo", "forearm_hi", "forearm_lo", "hand",
    "thigh_hi", "thigh_lo", "shin_hi", "shin_lo", "foot",
    "tail",
]
INDEX = {name: i for i, name in enumerate(REGIONS)}

# Limb bones split along the segment from their head to the next joint's head. The shin splits
# where trousers end and shoes begin, so the two together replace the lower leg exactly.
LIMBS = {
    "upperarm": ("lowerarm", "upperarm_hi", "upperarm_lo", 0.5),
    "lowerarm": ("hand", "forearm_hi", "forearm_lo", 0.5),
    "thigh": ("calf", "thigh_hi", "thigh_lo", 0.5),
    "calf": ("foot", "shin_hi", "shin_lo", 0.28),
}
TORSO = {
    "neck_01": "neck",
    "spine_03": "chest", "clavicle_l": "chest", "clavicle_r": "chest",
    "spine_01": "belly", "spine_02": "belly",
    "pelvis": "hips", "root": "hips",
}
FINGERS = ("thumb", "index", "middle", "ring", "pinky")


def _bone_region(bone):
    if bone in TORSO:
        return TORSO[bone]
    base = bone.rsplit("_", 1)[0] if bone.endswith(("_l", "_r")) else bone
    if base == "hand" or base.split("_")[0] in FINGERS:
        return "hand"
    if base in ("foot", "ball", "ball_leaf"):
        return "foot"
    if base.startswith("tail"):
        return "tail"
    return None


def islands(mesh):
    """Polygon index -> loose-part id."""
    import bmesh

    bm = bmesh.new()
    bm.from_mesh(mesh.data)
    bm.faces.ensure_lookup_table()
    part = {}
    pid = 0
    for f in bm.faces:
        if f.index in part:
            continue
        stack = [f]
        while stack:
            x = stack.pop()
            if x.index in part:
                continue
            part[x.index] = pid
            for e in x.edges:
                stack += [g for g in e.link_faces if g.index not in part]
        pid += 1
    neighbours = {f.index: {g.index for e in f.edges for g in e.link_faces if g.index != f.index} for f in bm.faces}
    bm.free()
    return part, neighbours


def classify(mesh, arm, body=False):
    """Region index for each polygon of a mesh in its rest pose (world space).

    For a Quaternius body, the face is its own loose part (the front mask), and the ears grow
    out of the widest points of the two scalp parts.
    """
    heads = {b.name: arm.matrix_world @ b.head_local for b in arm.data.bones}
    head = heads["Head"]
    groups = {g.index: g.name for g in mesh.vertex_groups}
    mw = mesh.matrix_world
    verts = mesh.data.vertices
    world = [mw @ v.co for v in verts]
    polys = mesh.data.polygons
    centres = [sum((world[vi] for vi in p.vertices), Vector()) / len(p.vertices) for p in polys]
    dominant = []
    for poly in polys:
        weights = {}
        for vi in poly.vertices:
            for g in verts[vi].groups:
                n = groups.get(g.group)
                if n:
                    weights[n] = weights.get(n, 0.0) + g.weight
        dominant.append(max(weights.items(), key=lambda kv: kv[1])[0] if weights else None)
    out = [None] * len(polys)
    for i, bone in enumerate(dominant):
        if bone is None:
            out[i] = "chest"
        elif bone != "Head":
            base = bone.rsplit("_", 1)[0] if bone.endswith(("_l", "_r")) else bone
            side = bone[-1] if bone.endswith(("_l", "_r")) else None
            if base in LIMBS and side:
                nxt, hi, lo, split = LIMBS[base]
                a, b = heads[bone], heads[f"{nxt}_{side}"]
                t = (centres[i] - a).dot(b - a) / max((b - a).length_squared, 1e-9)
                out[i] = hi if t < split else lo
            else:
                out[i] = _bone_region(bone) or "chest"
    head_faces = [i for i, b in enumerate(dominant) if b == "Head"]
    if head_faces:
        part, neighbours = islands(mesh) if body else ({}, {})
        hx = max(abs(centres[i].x - head.x) for i in head_faces)
        if body:
            # The face mask: the Head-only part reaching furthest forward (-Y).
            parts = {}
            for i in head_faces:
                parts.setdefault(part[i], []).append(i)
            front = min(parts, key=lambda k: min(centres[i].y for i in parts[k]) - 0.001 * len(parts[k]))
            face = set(parts[front])
            # Small Head-only parts (the mouth) belong to the face too.
            size = {}
            for i, k in part.items():
                size[k] = size.get(k, 0) + 1
            for k, fs in parts.items():
                if k != front and len(fs) == size[k] and len(fs) < 0.15 * len(parts[front]):
                    face |= set(fs)
        else:
            hz_top = max(centres[i].z for i in head_faces)
            hy_front = min(centres[i].y for i in head_faces)
            face = {i for i in head_faces
                    if centres[i].y < hy_front + 0.55 * (head.y - hy_front) + 0.03
                    and centres[i].z < head.z + 0.72 * (hz_top - head.z)}
        rest = [i for i in head_faces if i not in face]
        ears = {i for i in rest if abs(centres[i].x - head.x) > 0.9 * hx}
        if ears and body:
            # Ear faces are small; the scalp's long triangles around the ear stay scalp.
            areas = sorted(polys[i].area for i in ears)
            limit = areas[len(areas) // 2] * 2.5
            lo = Vector((min(centres[i].y for i in ears), min(centres[i].z for i in ears)))
            hi = Vector((max(centres[i].y for i in ears), max(centres[i].z for i in ears)))
            frontier = list(ears)
            while frontier:
                i = frontier.pop()
                for j in neighbours[i]:
                    c = centres[j]
                    if (j in ears or j in face or dominant[j] != "Head" or abs(c.x - head.x) < 0.70 * hx
                            or polys[j].area > limit
                            or not lo.x - 0.012 < c.y < hi.x + 0.012 or not lo.y - 0.012 < c.z < hi.y + 0.012):
                        continue
                    ears.add(j)
                    frontier.append(j)
        for i in head_faces:
            out[i] = "face" if i in face else "ears" if i in ears else "scalp"
    return [INDEX[r] for r in out]


def write(mesh, regions):
    """Stores polygon regions as the per-corner float attribute `_REGION`."""
    me = mesh.data
    if "_REGION" in me.attributes:
        me.attributes.remove(me.attributes["_REGION"])
    attr = me.attributes.new("_REGION", "FLOAT", "CORNER")
    values = [0.0] * len(me.loops)
    for poly, r in zip(me.polygons, regions):
        for li in poly.loop_indices:
            values[li] = float(r)
    attr.data.foreach_set("value", values)


def read(mesh):
    me = mesh.data
    attr = me.attributes["_REGION"]
    return [int(attr.data[poly.loop_start].value) for poly in me.polygons]


def tag(mesh, arm, body=False):
    regions = classify(mesh, arm, body)
    write(mesh, regions)
    return regions


# A distinct colour per region for the debug render.
PALETTE = [
    (0.95, 0.85, 0.2), (1.0, 0.55, 0.6), (0.9, 0.3, 0.9), (0.55, 0.35, 0.2),
    (0.2, 0.5, 1.0), (0.1, 0.8, 0.8), (0.4, 0.2, 0.7),
    (1.0, 0.3, 0.2), (1.0, 0.65, 0.2), (0.3, 0.85, 0.3), (0.1, 0.5, 0.15), (0.95, 0.95, 0.95),
    (0.6, 0.6, 1.0), (0.2, 0.2, 0.6), (0.9, 0.5, 0.1), (0.55, 0.25, 0.05), (0.5, 0.5, 0.5),
    (0.1, 0.1, 0.1),
]


def debug_material(mesh, regions):
    """Replaces the mesh's materials with one flat colour per region (for review renders)."""
    me = mesh.data
    me.materials.clear()
    for i, col in enumerate(PALETTE):
        m = bpy.data.materials.get(f"region_{REGIONS[i]}") or bpy.data.materials.new(f"region_{REGIONS[i]}")
        m.use_nodes = True
        bsdf = m.node_tree.nodes["Principled BSDF"]
        bsdf.inputs["Base Color"].default_value = (*col, 1)
        bsdf.inputs["Roughness"].default_value = 0.8
        me.materials.append(m)
    for poly, r in zip(me.polygons, regions):
        poly.material_index = r
