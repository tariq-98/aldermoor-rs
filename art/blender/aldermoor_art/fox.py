"""Foxfolk: a muzzle, fox ears and a tail on the race-scaled human body, with fur painted in.

Everything here works on the rest pose in world space, after races.deform has scaled the body:
- `muzzle` is a spatial field: it pushes the lower face forward into a snout and narrows it. The
  same field moves beards, brows and hoods, so they keep their fit.
- The human ears fold into the skull (and become scalp); fox ears stand on top of the head.
- The tail hangs from six new bones rooted on the pelvis, for spring physics.
- `paint` rasterises every triangle in UV space and writes a tone-neutral fur detail map (the
  skin tint colours it, as for every body) and a markings mask: R for the light fur, G the dark.
  The body's own UVs shrink to make room for the ears and tail.
"""

import math
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import materials as mt, regions as rg, scene as sc

TAIL = [f"tail_{i:02d}" for i in range(1, 7)]
UV_SHRINK = 0.84  # The body's islands move into [0, 0.84]^2; ears and tail take the strips.
EAR_UV = {1: ((0.01, 0.855), (0.30, 0.995)), -1: ((0.31, 0.855), (0.60, 0.995))}
TAIL_UV = ((0.855, 0.01), (0.995, 0.99))
PART_BODY, PART_EAR_FRONT, PART_EAR_BACK, PART_TAIL = 0.0, 1.0, 2.0, 3.0


def ramp(a, b, x):
    """0 at a, 1 at b (either order), smoothstepped; works on floats and arrays."""
    t = np.clip((np.asarray(x, dtype=np.float64) - a) / (b - a), 0.0, 1.0)
    out = t * t * (3 - 2 * t)
    return float(out) if np.ndim(out) == 0 else out


class Face:
    """Landmarks of the head, measured on the (race-scaled) body and eyes."""

    def __init__(self, skel, body, eyes):
        mw = eyes.matrix_world
        pts = [mw @ v.co for v in eyes.data.vertices]
        left = [p for p in pts if p.x > 0]
        right = [p for p in pts if p.x < 0]
        self.eye_l = sum(left, Vector()) / len(left)
        self.eye_r = sum(right, Vector()) / len(right)
        self.eye_z = (self.eye_l.z + self.eye_r.z) / 2
        self.eye_front = min(p.y for p in pts)
        self.head = skel.matrix_world @ skel.data.bones["Head"].head_local
        bw = body.matrix_world
        regions = rg.read(body)
        verts = {name: set() for name in ("face", "scalp")}
        for poly, r in zip(body.data.polygons, regions):
            name = rg.REGIONS[r]
            if name in verts:
                verts[name].update(poly.vertices)
        face = [bw @ body.data.vertices[i].co for i in verts["face"]]
        scalp = [bw @ body.data.vertices[i].co for i in verts["scalp"]]
        middle = [p for p in face if abs(p.x) < 0.012]
        self.nose = min(middle, key=lambda p: p.y).copy()
        self.chin = min(middle, key=lambda p: p.z).copy()
        self.top_z = max(p.z for p in scalp + face)
        self.half_width = max(abs(p.x) for p in scalp)


# --- The muzzle -----------------------------------------------------------------------------

def muzzle_point(face, p, length):
    """Where the muzzle field moves a point (or None if it doesn't)."""
    wz = ramp(face.eye_z - 0.004, face.eye_z - 0.038, p.z)
    if wz <= 0.0:
        return None
    # The jaw comes forward a little less than the nose; nothing far below the chin.
    wz *= 1.0 - 0.25 * ramp(face.nose.z - 0.03, face.chin.z, p.z)
    wz *= ramp(face.chin.z - 0.035, face.chin.z - 0.005, p.z)
    wx = 1.0 - ramp(0.008, 0.058, abs(p.x))
    wy = ramp(face.head.y - 0.02, face.head.y - 0.055, p.y)
    w = wz * wx * wy
    if w <= 1e-4:
        return None
    return Vector((p.x * (1.0 - 0.35 * w), p.y - length * w, p.z - 0.012 * w))


def muzzle(face, meshes, length=0.085):
    """Moves every mesh's vertices by the muzzle field; returns the new nose tip."""
    for ob in meshes:
        mw = ob.matrix_world
        inv = mw.inverted()
        moved = False
        for v in ob.data.vertices:
            q = muzzle_point(face, mw @ v.co, length)
            if q is not None:
                v.co = inv @ q
                moved = True
        if moved:
            ob.data.update()
    face.nose = muzzle_point(face, face.nose, length) or face.nose
    face.chin = muzzle_point(face, face.chin, length) or face.chin
    return face.nose


# Where the patches over the old ears sit in UV space once join_parts has shrunk the body's UVs.
EAR_PATCH_UV = {1: ((0.62, 0.86), (0.70, 0.94)), -1: ((0.72, 0.86), (0.80, 0.94))}


def fold_ears(body):
    """Replaces the human ears with patches of skull, tagged scalp.

    Around each ear the side of the head is close to a plane: fit one to the surface nearby
    (leaving the ear out), delete the faces that stand out from it, and fill the hole from its
    rim, which lies on the skull.
    """
    me = body.data
    mw = body.matrix_world
    inv = mw.inverted()
    regions = rg.read(body)
    ears = rg.INDEX["ears"]
    head = {rg.INDEX[n] for n in ("scalp", "face", "neck")}
    world = np.array([tuple(mw @ v.co) for v in me.vertices])
    tagged = sorted({i for p, r in zip(me.polygons, regions) if r == ears for i in p.vertices})
    head_verts = sorted({i for p, r in zip(me.polygons, regions) if r in head for i in p.vertices})
    plans = []
    for side in (1, -1):
        ear = [i for i in tagged if world[i][0] * side > 0]
        if not ear:
            continue
        centre = world[ear].mean(0)
        lo = world[ear].min(0) - 0.006
        hi = world[ear].max(0) + 0.006
        near = [i for i in head_verts if world[i][0] * side > 0 and np.linalg.norm(world[i] - centre) < 0.06
                and not np.all((world[i] >= lo) & (world[i] <= hi))]
        pts = world[near]
        origin = pts.mean(0)
        _, _, vt = np.linalg.svd(pts - origin)
        n = vt[2]
        if n[0] * side < 0:
            n = -n
        box = {i for i in range(len(world)) if np.all((world[i] >= lo) & (world[i] <= hi)) and world[i][0] * side > 0}
        doomed = set()
        for p, r in zip(me.polygons, regions):
            if not all(i in box for i in p.vertices):
                continue
            c = world[list(p.vertices)].mean(0)
            if r == ears or (c - origin) @ n > 0.004:
                doomed.add(p.index)
        plans.append((side, origin, n, box, doomed))
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    bm.verts.ensure_lookup_table()
    reg_layer = bm.loops.layers.float.get("_REGION")
    uv_layer = bm.loops.layers.uv.get("UVMap")
    # Element references now, before any deletion renumbers them.
    plans = [(side, origin, n, [bm.verts[i] for i in box], [bm.faces[i] for i in doomed])
             for side, origin, n, box, doomed in plans]
    for side, origin, n, box, faces in plans:
        doomed = set(faces)
        rim = {e for f in faces for e in f.edges if any(lf not in doomed for lf in e.link_faces)}
        rim_verts = [v for v in {v for e in rim for v in e.verts}]
        bmesh.ops.delete(bm, geom=faces, context="FACES_ONLY")
        nv = Vector(n)
        # A fan from the middle of the rim, in order around it (the mesh splits vertices along
        # UV seams, so the hole's edge isn't one clean loop to fill).
        rim_verts = [v for v in rim_verts if v.is_valid and v.link_faces]
        a = Vector((0, 0, 1)).cross(nv).normalized()
        b = nv.cross(a).normalized()
        world_rim = [mw @ v.co for v in rim_verts]
        c = sum(world_rim, Vector()) / len(world_rim)
        order = sorted(range(len(rim_verts)), key=lambda i: math.atan2((world_rim[i] - c) @ b, (world_rim[i] - c) @ a))
        ring = []
        for i in order:
            if ring and (world_rim[i] - mw @ ring[-1].co).length < 1e-5:
                continue
            ring.append(rim_verts[i])
        deform = bm.verts.layers.deform.active
        hub = bm.verts.new(inv @ (c + nv * 0.004))
        if deform is not None:
            for k, w in ring[0][deform].items():
                hub[deform][k] = w
        new_faces = []
        for v1, v2 in zip(ring, ring[1:] + ring[:1]):
            try:
                new_faces.append(bm.faces.new((hub, v1, v2)))
            except ValueError:
                pass  # The face already exists.
        # The patch's UVs: its points projected on the plane, fitted into a reserved rectangle
        # (divided by the shrink join_parts will apply).
        a = Vector((0, 0, 1)).cross(nv).normalized()
        b = nv.cross(a).normalized()
        pts = [(mw @ v.co) for f in new_faces for v in f.verts]
        if not pts:
            continue
        us = [p @ a for p in pts]
        vs = [p @ b for p in pts]
        (u0, v0), (u1, v1) = EAR_PATCH_UV[side]
        for f in new_faces:
            f.smooth = True
            for loop in f.loops:
                p = mw @ loop.vert.co
                fu = (p @ a - min(us)) / max(max(us) - min(us), 1e-6)
                fv = (p @ b - min(vs)) / max(max(vs) - min(vs), 1e-6)
                loop[uv_layer].uv = ((u0 + (u1 - u0) * fu) / UV_SHRINK, (v0 + (v1 - v0) * fv) / UV_SHRINK)
                loop[reg_layer] = float(rg.INDEX["scalp"])
        # Face out of the head, like the skull around them.
        for f in new_faces:
            f.normal_update()
            if (mw.to_3x3() @ f.normal) @ nv < 0:
                f.normal_flip()
    # Whatever is left of the ear regions becomes scalp.
    for f in bm.faces:
        for loop in f.loops:
            if int(round(loop[reg_layer])) == ears:
                loop[reg_layer] = float(rg.INDEX["scalp"])
    bm.to_mesh(me)
    bm.free()
    me.update()


# --- New parts ------------------------------------------------------------------------------

def _part(name, rings, uv_rect, weights, region, part_of, cap_tip=True):
    """A mesh from rings of points (the last may be one point) with UVs in a rectangle.

    `weights(j)` gives the vertex groups for ring j; `part_of(j, i)` the (part, t) values the
    painter reads.
    """
    bm = bmesh.new()
    uv_layer = bm.loops.layers.uv.new("UVMap")
    n = len(rings[0])
    verts = [[bm.verts.new(p) for p in ring] for ring in rings]
    (u0, v0), (u1, v1) = uv_rect
    rows = len(rings) - 1
    faces = []
    for j in range(rows):
        a, b = verts[j], verts[j + 1]
        for i in range(n):
            i2 = (i + 1) % n
            if len(b) == 1:
                f = bm.faces.new((a[i], a[i2], b[0]))
                uvs = [(i, j), (i + 1, j), (i + 0.5, j + 1)]
            else:
                f = bm.faces.new((a[i], a[i2], b[i2], b[i]))
                uvs = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]
            for loop, (ui, vj) in zip(f.loops, uvs):
                loop[uv_layer].uv = (u0 + (u1 - u0) * ui / n, v0 + (v1 - v0) * vj / rows)
            faces.append(f)
    if cap_tip is False:
        pass
    bm.normal_update()
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    for p in me.polygons:
        p.use_smooth = True
    # Vertex groups and the painter's attributes, ring by ring.
    flat = [(j, i) for j, ring in enumerate(rings) for i in range(len(ring))]
    for k, (j, i) in enumerate(flat):
        for bone, w in weights(j).items():
            vg = ob.vertex_groups.get(bone) or ob.vertex_groups.new(name=bone)
            vg.add([k], w, "REPLACE")
    part = me.attributes.new("fox_part", "FLOAT", "POINT")
    tval = me.attributes.new("fox_t", "FLOAT", "POINT")
    for k, (j, i) in enumerate(flat):
        pv, tv = part_of(j, i)
        part.data[k].value = pv
        tval.data[k].value = tv
    rg.write(ob, [rg.INDEX[region]] * len(me.polygons))
    return ob


def fox_ear(face, side, height=0.125, width=0.074, depth=0.042):
    """A tall ear on top of the head, cupped towards the front."""
    base = Vector((side * face.half_width * 0.52, face.head.y + 0.014, face.top_z - 0.03))
    up = Vector((side * 0.42, 0.12, 1.0)).normalized()
    front = Vector((0, -1, 0))
    front = (front - up * front.dot(up)).normalized()
    out = up.cross(front).normalized() * (-side)
    if out.x * side < 0:
        out = -out
    rings = []
    n, rows = 14, 12
    for j in range(rows + 1):
        t = j / rows
        c = base + up * (height * t) + front * (0.006 * math.sin(math.pi * t))
        if j == rows:
            rings.append([c])
            break
        w = width / 2 * (1 - t) ** 0.85
        d = depth / 2 * (1 - t) ** 0.7
        ring = []
        for i in range(n):
            a = 2 * math.pi * i / n
            x = math.cos(a) * w
            back = math.sin(a) * d
            if back < 0:
                back *= -0.35  # The front is cupped: pulled behind the middle.
            ring.append(c + out * x - front * back)
        rings.append(ring)

    def part_of(j, i):
        t = j / rows
        a = 2 * math.pi * i / n
        return (PART_EAR_FRONT if math.sin(a) < 0 else PART_EAR_BACK), t

    return _part(f"ear_{'l' if side > 0 else 'r'}", rings, EAR_UV[side], lambda j: {"Head": 1.0}, "ears", part_of)


def tail_points(skel, body, length=0.72):
    """The tail's joint positions: from the base of the spine, back and down, the tip turning up."""
    pelvis = skel.matrix_world @ skel.data.bones["pelvis"].head_local
    tree = BVHTree.FromObject(body, bpy.context.evaluated_depsgraph_get())
    start = pelvis + Vector((0, 0, -0.06))
    hit = tree.ray_cast(start, Vector((0, 1, 0)), 1.0)
    root = (hit[0] if hit[0] is not None else start + Vector((0, 0.12, 0))) - Vector((0, 0.02, 0))
    seg = length / len(TAIL)
    pts = [root]
    for k in range(len(TAIL)):
        a = k / (len(TAIL) - 2)
        if k < len(TAIL) - 1:
            d = Vector((0, 0.82, -0.57)).lerp(Vector((0, 0.16, -0.99)), min(a, 1.0)).normalized()
        else:
            d = Vector((0, 0.5, -0.86)).normalized()
        pts.append(pts[-1] + d * seg)
    return pts


def add_tail_bones(skel, pts):
    sc.activate(skel, "EDIT")
    ebs = skel.data.edit_bones
    parent = ebs["pelvis"]
    inv = skel.matrix_world.inverted()
    for name, a, b in zip(TAIL, pts, pts[1:]):
        eb = ebs.get(name) or ebs.new(name)
        eb.head = inv @ a
        eb.tail = inv @ b
        eb.roll = 0.0
        eb.parent = parent
        eb.use_connect = False
        eb.use_deform = True
        parent = eb
    bpy.ops.object.mode_set(mode="OBJECT")


def _spline(pts, s):
    """Catmull-Rom through pts at s in 0..1."""
    n = len(pts) - 1
    x = min(max(s, 0.0), 1.0) * n
    k = min(int(x), n - 1)
    f = x - k
    p0 = pts[max(k - 1, 0)]
    p1, p2 = pts[k], pts[k + 1]
    p3 = pts[min(k + 2, n)]
    f2, f3 = f * f, f * f * f
    return 0.5 * ((2 * p1) + (-p0 + p2) * f + (2 * p0 - 5 * p1 + 4 * p2 - p3) * f2 + (-p0 + 3 * p1 - 3 * p2 + p3) * f3)


def tail_mesh(pts):
    """A bushy tail around the bone chain, with a pointed tip."""
    rows, n = 34, 16
    centres = [_spline(pts, j / rows) for j in range(rows + 1)]
    # Parallel-transported frames along the curve.
    t0 = (centres[1] - centres[0]).normalized()
    side = Vector((1, 0, 0))
    side = (side - t0 * side.dot(t0)).normalized()
    frames = []
    prev_t = t0
    for j in range(rows + 1):
        tj = (centres[min(j + 1, rows)] - centres[max(j - 1, 0)]).normalized()
        side = (side - tj * side.dot(tj)).normalized()
        frames.append((side.copy(), tj.cross(side).normalized()))
        prev_t = tj
    rings = []
    for j in range(rows + 1):
        t = j / rows
        if j == rows:
            rings.append([centres[j]])
            break
        r = 0.022 + 0.064 * math.sin(math.pi * min(1.0, t / 0.93) ** 0.8)
        r *= 1.0 - 0.75 * ramp(0.86, 0.99, t)
        sx, sy = frames[j]
        ring = []
        for i in range(n):
            a = 2 * math.pi * i / n
            fluff = 1.0 + 0.07 * math.sin(5 * a + 11 * t) * math.sin(3 * a - 7 * t)
            ring.append(centres[j] + (sx * math.cos(a) + sy * math.sin(a)) * r * fluff)
        rings.append(ring)
    seg_len = [(b - a).length for a, b in zip(pts, pts[1:])]
    total = sum(seg_len)
    arc = [0.0]
    for a, b in zip(centres, centres[1:]):
        arc.append(arc[-1] + (b - a).length)

    def weights(j):
        s = arc[j] / arc[-1] * total
        k, acc = 0, 0.0
        while k < len(seg_len) - 1 and acc + seg_len[k] < s:
            acc += seg_len[k]
            k += 1
        f = (s - acc) / seg_len[k]
        w = {TAIL[k]: 1.0}
        nxt = ramp(0.6, 1.0, f) * 0.5
        if nxt > 0 and k + 1 < len(TAIL):
            w[TAIL[k + 1]] = nxt
            w[TAIL[k]] -= nxt
        prv = (1.0 - ramp(0.0, 0.4, f)) * 0.5
        if prv > 0:
            name = TAIL[k - 1] if k > 0 else "pelvis"
            w[name] = prv
            w[TAIL[k]] -= prv
        return w

    return _part("tail", rings, TAIL_UV, weights, "tail", lambda j, i: (PART_TAIL, j / rows))


def join_parts(body, parts):
    """Shrinks the body's UVs to make room, then joins the parts into the body."""
    uv = body.data.uv_layers["UVMap"]
    co = np.empty(len(uv.data) * 2, dtype=np.float32)
    uv.data.foreach_get("uv", co)
    uv.data.foreach_set("uv", co * UV_SHRINK)
    for name in ("fox_part", "fox_t"):
        if name not in body.data.attributes:
            body.data.attributes.new(name, "FLOAT", "POINT")
    sc.activate(body)
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()
    return body


# --- Fur --------------------------------------------------------------------------------------

def _noise(p, scale, seed):
    """Smooth pseudo-noise from summed sines (cheap and seamless across UV islands)."""
    rng = np.random.default_rng(seed)
    acc = np.zeros(p.shape[0])
    for _ in range(4):
        d = rng.normal(size=3)
        d /= np.linalg.norm(d)
        acc += np.sin(p @ d * scale * rng.uniform(0.7, 1.4) + rng.uniform(0, 6.28))
    return acc / 4


def markings(face, pos, region, part, t, skel_heads):
    """Light (R) and dark (G) fur amounts per sample; positions in world space (rest pose)."""
    n = pos.shape[0]
    light = np.zeros(n)
    dark = np.zeros(n)
    x, y, z = pos[:, 0], pos[:, 1], pos[:, 2]
    head = np.array(face.head)
    nose = np.array(face.nose)
    R = rg.INDEX
    is_body = part < 0.5
    limbs = np.isin(region, [R[n] for n in ("upperarm_hi", "upperarm_lo", "forearm_hi", "forearm_lo", "hand",
                                            "thigh_hi", "thigh_lo", "shin_hi", "shin_lo", "foot")])
    trunk = is_body & ~limbs
    # The face and throat: light below a line from the outer eye corners down to the nose tip,
    # on the front of the head and neck. Only position decides, so region seams don't show.
    front = ramp(head[1] + 0.012, head[1] - 0.028, y)
    along = ramp(head[1] - 0.04, nose[1], y)
    line = (face.eye_z - 0.032) * (1 - along) + (nose[2] - 0.012) * along
    chest_top = skel_heads["neck_01"][2]
    face_light = ramp(line + 0.007, line - 0.007, z) * front * ramp(chest_top - 0.03, chest_top + 0.03, z)
    on_head = np.isin(region, [R["face"], R["neck"], R["scalp"]]) & is_body
    light = np.where(trunk, np.maximum(light, face_light), light)
    # Dark tear marks: from each eye's inner corner down the side of the muzzle.
    for eye in (face.eye_l, face.eye_r):
        a = np.array((eye.x * 0.55, eye.y - 0.004, eye.z - 0.012))
        b = np.array((eye.x * 0.35 + 0.0, (eye.y + nose[1]) / 2, (eye.z + nose[2]) / 2 - 0.022))
        ab = b - a
        tt = np.clip(((pos - a) @ ab) / (ab @ ab), 0, 1)
        d = np.linalg.norm(pos - (a + tt[:, None] * ab), axis=1)
        width = 0.0055 * (1 - 0.6 * tt)
        dark = np.where(on_head & (region == R["face"]), np.maximum(dark, ramp(width + 0.003, width, d) * 0.85), dark)
    # The nose.
    dn = np.linalg.norm(pos - nose, axis=1)
    dark = np.where(is_body & (region == R["face"]), np.maximum(dark, ramp(0.021, 0.013, dn)), dark)
    # A bib down the chest, narrowing and fading towards the belly.
    spine_y = skel_heads["spine_03"][1]
    belly_z = skel_heads["spine_01"][2]
    width = 0.024 + 0.056 * ramp(belly_z, chest_top + 0.02, z)
    bib = ramp(width + 0.02, width, np.abs(x)) * ramp(spine_y - 0.02, spine_y - 0.06, y)
    bib *= ramp(belly_z - 0.03, belly_z + 0.07, z) * ramp(chest_top + 0.08, chest_top + 0.02, z)
    light = np.where(trunk, np.maximum(light, bib), light)
    # Dark socks and gloves, fading up the shins and forearms.
    knee_z = skel_heads["calf_l"][2] * 0.4 + skel_heads["foot_l"][2] * 0.6
    legs = np.isin(region, [R[n] for n in ("thigh_lo", "shin_hi", "shin_lo", "foot")])
    socks = np.where(legs, ramp(knee_z + 0.05, knee_z - 0.03, z), 0.0)
    wrist = np.array([skel_heads["hand_l"], skel_heads["hand_r"]])
    elbow = np.array([skel_heads["lowerarm_l"], skel_heads["lowerarm_r"]])
    side = (x > 0)
    w_pos = np.where(side[:, None], wrist[0], wrist[1])
    e_pos = np.where(side[:, None], elbow[0], elbow[1])
    along_arm = np.einsum("ij,ij->i", pos - e_pos, w_pos - e_pos) / np.einsum("ij,ij->i", w_pos - e_pos, w_pos - e_pos)
    gloves = np.where(region == R["hand"], 1.0, np.where(np.isin(region, [R["forearm_lo"], R["forearm_hi"]]), ramp(0.45, 0.85, along_arm), 0.0))
    dark = np.where(is_body, np.maximum(dark, np.maximum(socks, gloves)), dark)
    # Ears: light inside, dark behind and at the tips.
    ear_f = np.abs(part - PART_EAR_FRONT) < 0.5
    ear_b = np.abs(part - PART_EAR_BACK) < 0.5
    light = np.where(ear_f, ramp(0.08, 0.22, t) * (1 - ramp(0.7, 0.8, t)), light)
    dark = np.where(ear_b, ramp(0.2, 0.4, t), dark)
    dark = np.where(ear_f | ear_b, np.maximum(dark, ramp(0.72, 0.84, t)), dark)
    # Tail: a white tip.
    tail = np.abs(part - PART_TAIL) < 0.5
    light = np.where(tail, ramp(0.8, 0.88, t), light)
    return np.clip(light, 0, 1), np.clip(dark, 0, 1)


def fur_detail(pos, part):
    """Tone-neutral fur (mean 0.5): broad soft variation and fine streaks running down the body."""
    broad = _noise(pos, 18.0, 3)
    streak = _noise(pos * np.array([1.0, 1.0, 0.22]), 160.0, 7)
    fine = _noise(pos, 420.0, 11)
    return 0.5 + 0.045 * broad + 0.04 * streak + 0.02 * fine


def _bleed(img, filled, passes=6):
    """Spreads painted texels into the empty ones next to them (no dark seams in the mips)."""
    for _ in range(passes):
        grow = np.zeros_like(filled)
        acc = np.zeros_like(img)
        cnt = np.zeros(filled.shape)
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            sf = np.roll(filled, (dy, dx), (0, 1))
            si = np.roll(img, (dy, dx), (0, 1))
            acc += si * sf[..., None]
            cnt += sf
        add = (~filled) & (cnt > 0)
        img[add] = acc[add] / cnt[add][:, None]
        filled = filled | add
    return img


def paint(body, face, skel, out_dir, stem, size=2048):
    """Writes `<stem>_fur.jpg` (detail) and `<stem>_markings.png`, and gives the body a fur material."""
    me = body.data
    mw = body.matrix_world
    me.calc_loop_triangles()
    uv = np.empty(len(me.loops) * 2, dtype=np.float32)
    me.uv_layers["UVMap"].data.foreach_get("uv", uv)
    uv = uv.reshape(-1, 2)
    co = np.empty(len(me.vertices) * 3, dtype=np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    m = np.array(mw)
    world = co @ m[:3, :3].T + m[:3, 3]
    part = np.zeros(len(me.vertices), dtype=np.float32)
    tval = np.zeros(len(me.vertices), dtype=np.float32)
    me.attributes["fox_part"].data.foreach_get("value", part)
    me.attributes["fox_t"].data.foreach_get("value", tval)
    regions = np.array(rg.read(body))
    heads = {b.name: np.array(skel.matrix_world @ b.head_local) for b in skel.data.bones}
    detail = np.zeros((size, size, 3), dtype=np.float32)
    marks = np.zeros((size, size, 3), dtype=np.float32)
    filled = np.zeros((size, size), dtype=bool)
    for tri in me.loop_triangles:
        li = list(tri.loops)
        vi = list(tri.vertices)
        t_uv = uv[li] * size
        lo = np.floor(t_uv.min(0)).astype(int)
        hi = np.ceil(t_uv.max(0)).astype(int)
        lo = np.clip(lo, 0, size - 1)
        hi = np.clip(hi, 0, size - 1)
        xs, ys = np.meshgrid(np.arange(lo[0], hi[0] + 1), np.arange(lo[1], hi[1] + 1))
        px = np.stack([xs.ravel() + 0.5, ys.ravel() + 0.5], 1)
        a, b, c = t_uv
        v0, v1 = b - a, c - a
        den = v0[0] * v1[1] - v1[0] * v0[1]
        if abs(den) < 1e-12:
            continue
        d = px - a
        w1 = (d[:, 0] * v1[1] - v1[0] * d[:, 1]) / den
        w2 = (v0[0] * d[:, 1] - d[:, 0] * v0[1]) / den
        w0 = 1 - w1 - w2
        inside = (w0 >= -1e-3) & (w1 >= -1e-3) & (w2 >= -1e-3)
        if not inside.any():
            continue
        bc = np.stack([w0, w1, w2], 1)[inside]
        p = bc @ world[vi]
        pt = bc @ part[vi]
        tt = bc @ tval[vi]
        reg = np.full(p.shape[0], regions[tri.polygon_index])
        L, D = markings(face, p, reg, np.round(pt), tt, heads)
        f = fur_detail(p, pt)
        ix = xs.ravel()[inside]
        iy = ys.ravel()[inside]
        detail[iy, ix] = f[:, None]
        marks[iy, ix, 0] = L
        marks[iy, ix, 1] = D
        filled[iy, ix] = True
    detail = _bleed(detail, filled.copy(), 8)
    marks = _bleed(marks, filled.copy(), 8)
    detail[~filled] = 0.5
    rgba = np.concatenate([detail, np.ones((size, size, 1), np.float32)], -1)
    # The detail is stored as sRGB like the other skin detail maps: mean 0.5 after decoding.
    srgb = mt._linear_to_srgb(np.clip(rgba[..., :3], 0, 1))
    out_dir = Path(out_dir)
    fur_img = mt._save(np.concatenate([srgb, rgba[..., 3:]], -1).astype(np.float32), out_dir / f"{stem}_fur.jpg", "sRGB")
    mrgba = np.concatenate([marks, np.ones((size, size, 1), np.float32)], -1)
    mt._save(mrgba, out_dir / f"{stem}_markings.png", "Non-Color")
    # One fur material for the whole body: tinted like skin, rough, no skin normal map.
    mat = bpy.data.materials.new(f"skin.{stem}")
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = fur_img
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.88
    mat["tint"] = "skin"
    body.data.materials.clear()
    body.data.materials.append(mat)
    for p in body.data.polygons:
        p.material_index = 0
    for name in ("fox_part", "fox_t"):
        body.data.attributes.remove(body.data.attributes[name])
    return f"{stem}_fur.jpg", f"{stem}_markings.png"
