"""Foxfolk: a muzzle, fox ears and a tail on the race-scaled human body, with fur, whiskers, a
nose, claws and fox eyes.

Everything here works on the rest pose in world space, after races.deform has scaled the body:
- `muzzle` is a spatial field: it pushes the lower face forward into a snout and narrows it. The
  same field moves beards, brows and hoods, so they keep their fit.
- The human ears are replaced by skull; fox ears stand on top of the head.
- The tail hangs from six new bones rooted on the pelvis, for spring physics.
- `paint` writes the fur (aldermoor_art/fur.py): a tone-neutral detail map with strands and
  clumps running along the fur's flow (the skin tint colours it, as for every body), a matching
  normal map, and a markings mask (R the light fur, G the dark, including the lips and eye rims).
- `details` grows the rest: tufts (cheek ruff, throat, inner ears, chest, tail), whiskers, the
  nose and claws. Tufts, whiskers and claws use the fur material with UVs into the body's own
  texture, so they take the coat's colours, the markings' dark for whiskers and claws, and hide
  with the body's regions.
- `dress` gives outfits that bring their own bare arms the same fur and claws.
"""

import math
import random
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import fur, materials as mt, regions as rg, scene as sc

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
        self.mouth = []
        self.eye_rims = []
        self.ears = {}

    def find_openings(self, body):
        """The mouth slit and the eye openings: the open edges of the face (after the muzzle)."""
        me = body.data
        mw = body.matrix_world
        regions = rg.read(body)
        face = rg.INDEX["face"]
        bm = bmesh.new()
        bm.from_mesh(me)
        bm.faces.ensure_lookup_table()
        mouth, rims = [], []
        for e in bm.edges:
            if not e.is_boundary or regions[e.link_faces[0].index] != face:
                continue
            for v in e.verts:
                p = mw @ v.co
                if min((p - self.eye_l).length, (p - self.eye_r).length) < 0.032:
                    rims.append(p)
                elif p.z < self.eye_z - 0.03 and abs(p.x) < 0.06 and p.y < self.head.y - 0.03:
                    mouth.append(p)
        bm.free()
        self.mouth = mouth
        self.eye_rims = rims
        if mouth:
            xs = [p.x for p in mouth]
            self.mouth_corners = [max(mouth, key=lambda p: p.x), min(mouth, key=lambda p: p.x)]


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
    # Rings may run either way round; make every face point out.
    bmesh.ops.recalc_face_normals(bm, faces=faces)
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

    face.ears[side] = (base, up, front, height)
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

def flow_rules(face):
    """Which way the fur grows: back from the nose over the head, down the neck and body, along
    the limbs to the fingers and toes, and along the tail."""
    nose = face.nose

    def rule(name, head, tail, p):
        if name == "Head":
            d = p - nose
            d.z -= 0.35 * d.length
            return d
        if name.startswith(("upperarm", "lowerarm", "hand", "index", "middle", "ring", "pinky", "thumb",
                             "thigh", "calf", "foot", "ball", "tail_")):
            return tail - head
        if name.startswith(("neck", "spine", "pelvis", "clavicle")):
            return Vector((0, 0.25, -1))
        return None

    return rule


def flow(body, skel, face):
    out = fur.bone_flow(body, skel, flow_rules(face))
    # The ears grow from base to tip.
    part = np.zeros(len(body.data.vertices))
    body.data.attributes["fox_part"].data.foreach_get("value", part)
    mw = body.matrix_world
    for v in body.data.vertices:
        if part[v.index] in (PART_EAR_FRONT, PART_EAR_BACK):
            side = 1 if (mw @ v.co).x > 0 else -1
            up = face.ears[side][1]
            n = (mw.to_3x3() @ v.normal).normalized()
            f = up - n * up.dot(n)
            if f.length > 1e-6:
                out[v.index] = f.normalized()
    return out


def _segment_distance(pos, a, b):
    ab = b - a
    t = np.clip(((pos - a) @ ab) / max(ab @ ab, 1e-12), 0, 1)
    return np.linalg.norm(pos - (a + t[:, None] * ab), axis=1)


def _points_distance(pos, pts, chunk=200000):
    """Distance from each position to the nearest of a few points."""
    pts = np.array([tuple(p) for p in pts])
    out = np.empty(len(pos))
    for k in range(0, len(pos), chunk):
        d = np.linalg.norm(pos[k:k + chunk, None, :] - pts[None, :, :], axis=2)
        out[k:k + chunk] = d.min(1)
    return out


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
    on_face = is_body & (region == R["face"])
    # The face and throat: light below a line from the outer eye corners down to the nose tip,
    # on the front of the head and neck. Only position decides, so region seams don't show.
    front = ramp(head[1] + 0.012, head[1] - 0.028, y)
    along = ramp(head[1] - 0.04, nose[1], y)
    line = (face.eye_z - 0.032) * (1 - along) + (nose[2] - 0.012) * along
    chest_top = skel_heads["neck_01"][2]
    face_light = ramp(line + 0.007, line - 0.007, z) * front * ramp(chest_top - 0.03, chest_top + 0.03, z)
    light = np.where(trunk, np.maximum(light, face_light), light)
    # Dark tear marks: from each eye's inner corner down the side of the muzzle.
    for eye in (face.eye_l, face.eye_r):
        a = np.array((eye.x * 0.55, eye.y - 0.004, eye.z - 0.012))
        b = np.array((eye.x * 0.35 + 0.0, (eye.y + nose[1]) / 2, (eye.z + nose[2]) / 2 - 0.022))
        ab = b - a
        tt = np.clip(((pos - a) @ ab) / (ab @ ab), 0, 1)
        d = np.linalg.norm(pos - (a + tt[:, None] * ab), axis=1)
        width = 0.0055 * (1 - 0.6 * tt)
        dark = np.where(on_face, np.maximum(dark, ramp(width + 0.003, width, d) * 0.85), dark)
    # The nose.
    dn = np.linalg.norm(pos - nose, axis=1)
    dark = np.where(on_face, np.maximum(dark, ramp(0.021, 0.013, dn)), dark)
    # Dark lips: along the mouth slit, and on back from each corner along the jaw.
    near = on_face & (np.abs(z - (face.chin.z + face.nose.z) / 2) < 0.05)
    if face.mouth and near.any():
        idx = np.nonzero(near)[0]
        d = _points_distance(pos[idx], face.mouth)
        for c in getattr(face, "mouth_corners", []):
            a = np.array(tuple(c))
            side = 1.0 if a[0] > 0 else -1.0
            b = a + np.array((side * 0.012, 0.03, 0.006))
            d = np.minimum(d, _segment_distance(pos[idx], a, b) + 0.0006)
        dark[idx] = np.maximum(dark[idx], ramp(0.0032, 0.0016, d))
    # Dark rims round the eyes.
    if face.eye_rims:
        near = on_face & (np.minimum(np.linalg.norm(pos - np.array(face.eye_l), axis=1),
                                     np.linalg.norm(pos - np.array(face.eye_r), axis=1)) < 0.045)
        idx = np.nonzero(near)[0]
        if len(idx):
            d = _points_distance(pos[idx], face.eye_rims)
            dark[idx] = np.maximum(dark[idx], ramp(0.0034, 0.0014, d) * 0.9)
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


def paint(body, face, skel, out_dir, stem, size=2048):
    """Writes `<stem>_fur.jpg` (detail), `<stem>_fur_normal.jpg` and `<stem>_markings.png`, and
    gives the body one fur material (tinted like skin)."""
    me = body.data
    part = np.zeros(len(me.vertices))
    tval = np.zeros(len(me.vertices))
    me.attributes["fox_part"].data.foreach_get("value", part)
    me.attributes["fox_t"].data.foreach_get("value", tval)
    face.find_openings(body)
    flows = flow(body, skel, face)
    gb = fur.GBuffer(body, size, flow=flows, attrs={"part": part, "t": tval})
    regions = np.array(rg.read(body))[gb.poly]
    heads = {b.name: np.array(skel.matrix_world @ b.head_local) for b in skel.data.bones}
    light, dark = markings(face, gb.pos, regions, np.round(gb.attrs["part"]), gb.attrs["t"], heads)
    strands, clumps = fur.fur_fields(gb, seed=7)
    ao = fur.bake_ao(body, size)[gb.iy, gb.ix]
    ao = np.clip(ao, 0, 1) ** 0.8
    height = np.clip(0.5 + 0.2 * clumps + 0.13 * strands, 0, 1)
    detail = (1 + 0.17 * clumps + 0.09 * strands) * (0.62 + 0.38 * ao)
    detail *= 0.5 / detail.mean()
    out_dir = Path(out_dir)
    det = fur.bleed(gb.image(np.repeat(detail[:, None], 3, 1), fill=0.5), gb.mask, 10)
    # Stored as sRGB like the other skin detail maps: mean 0.5 after decoding.
    fur_img = fur.save_rgb(mt._linear_to_srgb(np.clip(det, 0, 1)), out_dir / f"{stem}_fur.jpg", "sRGB")
    nrm_img = fur.save_rgb(fur.normal_map(gb, height, 0.00045), out_dir / f"{stem}_fur_normal.jpg", "Non-Color")
    marks = np.zeros((len(light), 3))
    marks[:, 0] = light
    marks[:, 1] = dark
    fur.save_rgb(fur.bleed(gb.image(marks), gb.mask, 10), out_dir / f"{stem}_markings.png", "Non-Color")
    mat = bpy.data.materials.new(f"skin.{stem}")
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = fur_img
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    tn = nt.nodes.new("ShaderNodeTexImage")
    tn.image = nrm_img
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(tn.outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    bsdf.inputs["Roughness"].default_value = 0.86
    mat["tint"] = "skin"
    body.data.materials.clear()
    body.data.materials.append(mat)
    for p in body.data.polygons:
        p.material_index = 0
    # A texel deep inside the dark fur (the gloves), for parts that wear the coat's dark:
    # whiskers and claws. Away from island edges, so filtering never reaches other colours.
    solid = gb.image(np.where((dark > 0.99) & (light < 0.01), 1.0, 0.0))
    for _ in range(6):
        solid = np.minimum.reduce([solid, np.roll(solid, 1, 0), np.roll(solid, -1, 0), np.roll(solid, 1, 1), np.roll(solid, -1, 1)])
    ys, xs = np.nonzero(solid > 0.5)
    k = len(ys) // 2
    body["dark_uv"] = ((xs[k] + 0.5) / size, (ys[k] + 0.5) / size)
    for name in ("fox_part", "fox_t"):
        body.data.attributes.remove(body.data.attributes[name])
    return f"{stem}_fur.jpg", f"{stem}_markings.png"


# --- Details: tufts, whiskers, nose and claws ------------------------------------------------------

def _tube(bm, pts, radii, sides=4):
    rings = []
    side = None
    for i, (p, r) in enumerate(zip(pts, radii)):
        tan = (pts[min(i + 1, len(pts) - 1)] - pts[max(i - 1, 0)]).normalized()
        if side is None:
            side = tan.orthogonal().normalized()
        side = (side - tan * side.dot(tan)).normalized()
        up = tan.cross(side).normalized()
        if r <= 0:
            rings.append([bm.verts.new(p)])
            continue
        rings.append([bm.verts.new(p + (side * math.cos(a) + up * math.sin(a)) * r)
                      for a in (2 * math.pi * k / sides for k in range(sides))])
    faces = []
    for r0, r1 in zip(rings, rings[1:]):
        m = len(r0)
        for i in range(m):
            j = (i + 1) % m
            if len(r1) == 1:
                faces.append(bm.faces.new((r0[i], r0[j], r1[0])))
            else:
                faces.append(bm.faces.new((r0[i], r0[j], r1[j], r1[i])))
    if len(rings[0]) > 2:
        faces.append(bm.faces.new(list(reversed(rings[0]))))
    return rings, faces


def _finish(bm, name, material, uv, region, weights, lod_max):
    """A mesh object from a bmesh: one UV everywhere, one region, rigid skin weights."""
    uv_layer = bm.loops.layers.uv.new("UVMap")
    reg_layer = bm.loops.layers.float.new("_REGION")
    for f in bm.faces:
        f.smooth = True
        for loop in f.loops:
            loop[uv_layer].uv = uv
            loop[reg_layer] = float(rg.INDEX[region])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(material)
    for vi, wd in weights(me):
        for bone, w in wd.items():
            vg = ob.vertex_groups.get(bone) or ob.vertex_groups.new(name=bone)
            vg.add([vi], w, "REPLACE")
    ob["lod_max"] = lod_max
    ob["lod_decimate"] = False
    return ob


def whiskers(face, body, material, uv):
    """Whiskers on the muzzle pads, two over each eye and one on each cheek."""
    tree = BVHTree.FromObject(body, bpy.context.evaluated_depsgraph_get())
    eye_mid = (face.eye_l + face.eye_r) / 2
    nose = face.nose
    back = (eye_mid - nose)
    back.z = 0
    back.normalize()
    rng = random.Random(5)
    bm = bmesh.new()

    def surface_point(start, direction):
        hit = tree.ray_cast(start, direction.normalized(), 0.2)
        return hit[0], hit[1]

    for side in (1, -1):
        out = Vector((side, 0, 0))
        # Pads: on the muzzle sides, a third of the way from the nose to the eyes.
        for row, dz in enumerate((0.007, 0.0, -0.007)):
            for col, ds in enumerate((-0.009, 0.0, 0.009)):
                c = nose.lerp(eye_mid, 0.36) + back * ds
                c.z = nose.z - 0.014 + dz
                p, n = surface_point(Vector((0, c.y, c.z)), out)
                if p is None:
                    continue
                length = (0.12 - 0.012 * row) * rng.uniform(0.9, 1.1)
                d = (out * 0.85 + back * 0.5 + Vector((0, 0, 0.12 - 0.12 * row))).normalized()
                droop = Vector((0, 0, -1)) * 0.25 + back * 0.15
                pts = [p - n * 0.002 + (d + droop * (s * s)) * (length * s) for s in (0, 0.2, 0.4, 0.6, 0.8, 1.0)]
                _tube(bm, pts, [0.0008, 0.0007, 0.00055, 0.0004, 0.00025, 0.0], sides=4)
        # Over each eye: two longer hairs sweeping up and back.
        eye = face.eye_l if side > 0 else face.eye_r
        for k, (dy, h) in enumerate(((-0.004, 0.016), (0.006, 0.019))):
            # From in front of the brow, back into it.
            p, n = surface_point(Vector((eye.x * 0.85, eye.y + dy - 0.05, eye.z + h)), Vector((0, 1, 0)))
            if p is None:
                continue
            d = (out * 0.5 + back * 0.6 + Vector((0, 0, 0.75))).normalized()
            pts = [p - n * 0.002 + (d + back * 0.3 * s * s) * (0.055 * s) for s in (0, 0.25, 0.5, 0.75, 1.0)]
            _tube(bm, pts, [0.0006, 0.0005, 0.00038, 0.00022, 0.0], sides=4)
        # One on each cheek, behind the mouth corner.
        c = nose.lerp(eye_mid, 0.75)
        c.z = (face.eye_z + nose.z) / 2 - 0.02
        p, n = surface_point(Vector((0, c.y, c.z)), out)
        if p is not None:
            d = (out * 0.8 + back * 0.6 - Vector((0, 0, 0.1))).normalized()
            pts = [p - n * 0.002 + d * (0.06 * s) for s in (0, 0.25, 0.5, 0.75, 1.0)]
            _tube(bm, pts, [0.0006, 0.0005, 0.00038, 0.00022, 0.0], sides=4)
    return _finish(bm, "whiskers", material, uv, "face", lambda me: [(v.index, {"Head": 1.0}) for v in me.vertices], 0)


def nose_tip(face, body):
    """A glossy nose pad over the muzzle's tip, with two nostrils."""
    eye_mid = (face.eye_l + face.eye_r) / 2
    fwd = (face.nose - eye_mid)
    fwd.normalize()
    side = Vector((1, 0, 0))
    up = side.cross(fwd).normalized()
    if up.z < 0:
        up = -up
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=16, v_segments=10, radius=1.0)
    for v in bm.verts:
        q = v.co
        # Wider than tall, flattened at the front, with two nostril dents low on the front.
        x, y, z = q.x * 0.0125, q.y * 0.0095, q.z * 0.0085
        fwd_k = max(0.0, q.y)
        y *= 1.0 - 0.25 * fwd_k ** 2
        for sx in (-1, 1):
            d = (Vector((q.x, q.y, q.z)) - Vector((sx * 0.45, 0.85, -0.25))).length
            if d < 0.45:
                y -= 0.004 * (1 - d / 0.45) ** 2
        v.co = face.nose + side * x + fwd * (y - 0.003) + up * (z + 0.001)
    mat = bpy.data.materials.get("fox_nose") or bpy.data.materials.new("fox_nose")
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (0.018, 0.014, 0.013, 1)
    bsdf.inputs["Roughness"].default_value = 0.32
    return _finish(bm, "nose", mat, (0.5, 0.5), "face", lambda me: [(v.index, {"Head": 1.0}) for v in me.vertices], 1)


FINGERS = ("thumb", "index", "middle", "ring", "pinky")


def claws(skel, material, uv, region="hand", name="claws"):
    """A curved claw from each fingertip, rigid on the last finger bone."""
    bm = bmesh.new()
    owners = []
    heads = {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}
    for side in ("l", "r"):
        w, idx, pk, mid = (heads[f"{n}_{side}"] for n in ("hand", "index_01", "pinky_01", "middle_01"))
        palm = (mid - w).cross(idx - pk).normalized()
        thumb = heads[f"thumb_02_{side}"]
        if palm.dot(thumb - (idx + pk) / 2) < 0:
            palm = -palm
        # The palm faces where the thumb hangs; the nail is on the other side.
        for f in FINGERS:
            tip_bone = f"{f}_03_{side}"
            leaf = f"{f}_04_leaf_{side}"
            if tip_bone not in heads or leaf not in heads:
                continue
            a, b = heads[tip_bone], heads[leaf]
            along = (b - a).normalized()
            dorsal = -(palm - along * palm.dot(along)).normalized()
            size = 0.85 if f in ("pinky", "thumb") else 1.0
            base = b - along * 0.004 + dorsal * 0.0035
            length = 0.013 * size
            pts = [base + (along + -dorsal * 0.45 * s * s) * (length * s) for s in (0, 0.33, 0.66, 1.0)]
            rings, _ = _tube(bm, pts, [0.0034 * size, 0.0026 * size, 0.0014 * size, 0.0], sides=6)
            owners.append((tip_bone, [v for r in rings for v in r]))
    bm.verts.index_update()
    owner_of = {v.index: bone for bone, vs in owners for v in vs}
    return _finish(bm, name, material, uv, region, lambda me: [(vi, {bone: 1.0}) for vi, bone in owner_of.items()], 1)


def details(face, skel, body):
    """Tufts, whiskers, the nose and claws, as separate objects bound to the skeleton."""
    mat = body.data.materials[0]
    dark_uv = tuple(body["dark_uv"])
    surface = fur.Surface(body)
    rule = flow_rules(face)
    heads = {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}
    R = rg.INDEX
    eye_mid = (face.eye_l + face.eye_r) / 2

    def flow_at(p, n):
        idx = surface.nearest(p)[1]
        wd = surface.weights_at(idx, surface.bary(idx, p))
        acc = Vector()
        for bone, w in wd.items():
            b = skel.data.bones.get(bone)
            if b is None:
                continue
            d = rule(bone, skel.matrix_world @ b.head_local, skel.matrix_world @ b.tail_local, Vector(p))
            if d is not None:
                acc += Vector(d).normalized() * w
        return acc if acc.length > 1e-6 else Vector((0, 0, -1))

    objs = []
    # The cheek ruff: a band behind and below the eyes, flaring back, out and down, longest at
    # the angle of the jaw.
    jaw_z = (face.chin.z + face.eye_z) / 2 - 0.01

    def cheeks(p, n, r):
        if r not in (R["face"], R["neck"]) or abs(p.x) < 0.04 or n.x * p.x < 0.3 * abs(p.x):
            return 0
        if not (eye_mid.y - 0.004 < p.y < eye_mid.y + 0.075):
            return 0
        return 1 if jaw_z - 0.035 < p.z < face.eye_z - 0.006 else 0

    def cheek_dir(p, n):
        side = 1 if p.x > 0 else -1
        return Vector((side * 0.55, 0.85, -0.55))

    def cheek_len(p):
        k = min(1.0, max(0.0, (p.y - eye_mid.y) / 0.07)) * 0.6 + min(1.0, max(0.0, (face.eye_z - p.z) / 0.05)) * 0.4
        return 0.03 + 0.032 * k

    roots = fur.scatter(surface, cheeks, 80, 0.0065, seed=11)
    objs.append(fur.tufts(surface, roots, cheek_dir, length=cheek_len, width=0.016, lift=0.55, droop=0.3, spread=0.3,
                          seed=11, name="fur_cheeks"))
    # The throat, under the jaw.
    neck_y = heads["neck_01"].y
    def throat(p, n, r):
        if r not in (R["neck"], R["face"]):
            return 0
        return 1 if (p.y < neck_y - 0.035 and heads["neck_01"].z + 0.01 < p.z < face.chin.z + 0.005 and n.z < 0.3) else 0
    roots = fur.scatter(surface, throat, 14, 0.01, seed=12)
    objs.append(fur.tufts(surface, roots, lambda p, n: Vector((0, 0.3, -1)), length=0.032, width=0.017, lift=0.35, droop=0.2,
                          seed=12, name="fur_throat"))
    # Inside the ears: light fur rising from the base.
    for side in (1, -1):
        base, up, front, height = face.ears[side]
        def ear(p, n, r, base=base, up=up, front=front, height=height):
            if r != R["ears"]:
                return 0
            d = p - base
            t = d.dot(up) / height
            lateral = (d - up * d.dot(up) - front * d.dot(front)).length
            # The middle of the cup, low down, where the fur stays inside the ear.
            return 1 if (0.06 < t < 0.3 and n.dot(front) > 0.35 and lateral < 0.012 * (1 - t)) else 0
        roots = fur.scatter(surface, ear, 7, 0.006, seed=13 + side)
        objs.append(fur.tufts(surface, roots, lambda p, n, up=up, front=front: up + front * 0.25, length=0.022,
                              width=0.009, lift=0.3, droop=0.12, spread=0.15, seed=13 + side, name="fur_ear"))
    # The chest bib (hidden under shirts with the chest).
    def chest(p, n, r):
        return 1 if (r == R["chest"] and abs(p.x) < 0.06 and p.y < heads["spine_03"].y - 0.05 and n.y < -0.4) else 0
    roots = fur.scatter(surface, chest, 14, 0.012, seed=15)
    objs.append(fur.tufts(surface, roots, lambda p, n: Vector((0, 0.15, -1)), length=0.028, width=0.016, lift=0.3, droop=0.15,
                          seed=15, name="fur_chest"))
    # The tail: fluff all along it, pointing to the tip.
    def tail(p, n, r):
        return 1 if r == R["tail"] else 0
    roots = fur.scatter(surface, tail, 80, 0.016, seed=16)
    objs.append(fur.tufts(surface, roots, flow_at, length=0.055, width=0.024, lift=0.45, droop=0.2, spread=0.5,
                          seed=16, name="fur_tail"))
    tufts = sc.alive([o for o in objs if o is not None and len(o.data.polygons)])
    for o in tufts:
        o.data.materials.append(mat)
    fur_ob = fur_join(tufts, "fur")
    fur_ob["lod_max"] = 1
    whisk = whiskers(face, body, mat, dark_uv)
    nose = nose_tip(face, body)
    claw = claws(skel, mat, dark_uv)
    out = [fur_ob, whisk, nose, claw]
    for o in out:
        sc.bind(o, skel)
    return out


def fur_join(objects, name):
    if len(objects) == 1:
        objects[0].name = name
        return objects[0]
    for o in bpy.context.selected_objects:
        o.select_set(False)
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.join()
    ob = bpy.context.view_layer.objects.active
    ob.name = name
    ob.data.name = name
    return ob


# --- Outfits and eyes ------------------------------------------------------------------------------

def dress(outfits, body, skel):
    """Bare skin that outfits bring (Quaternius tops come with their own forearms and hands)
    takes the body's fur: its UVs are moved onto the body's texture and it wears the body's fur
    material. Outfits with hands get claws. Returns the new claw objects (tagged with their piece)."""
    mat = body.data.materials[0]
    dark_uv = tuple(body["dark_uv"])
    surface = fur.Surface(body)
    added = []
    for ob in outfits:
        slots = [k for k, s in enumerate(ob.material_slots) if s.material and s.material.name.startswith("skin")]
        if not slots:
            continue
        me = ob.data
        uv = me.uv_layers.active.data
        mw = ob.matrix_world
        regions = rg.read(ob) if "_REGION" in me.attributes else None
        has_hand = False
        for p in me.polygons:
            if p.material_index not in slots:
                continue
            if regions and regions[p.index] == rg.INDEX["hand"]:
                has_hand = True
            # The whole face maps through the body triangle under its centre (extrapolated
            # linearly), so it never straddles a seam in the body's UVs.
            _, idx = surface.nearest(mw @ p.center)
            for li in p.loop_indices:
                q = mw @ me.vertices[me.loops[li].vertex_index].co
                uv[li].uv = surface.uv_at(idx, surface.bary(idx, q, clamp=False))
        for k in slots:
            ob.material_slots[k].material = mat
        if has_hand:
            c = claws(skel, mat, dark_uv, name=f"{ob['piece']}.claws")
            c["piece"] = ob["piece"]
            sc.bind(c, skel)
            added.append(c)
    return added


def fox_eyes(eyes, out_dir):
    """Amber eyes with slit pupils, painted over the human eye texture's layout."""
    out = Path(out_dir) / "T_Eye_Fox.jpg"
    mat = eyes.material_slots[0].material.copy()
    mat.name = "MI_Eyes_Fox"
    eyes.material_slots[0].material = mat
    node = next(n for n in mat.node_tree.nodes if n.type == "TEX_IMAGE" and "Normal" not in n.image.name)
    src = mt._pixels(node.image)
    h, w, _ = src.shape
    size = 512
    # Upsample the sclera and lids (nearest is fine; they're soft), then paint the iris.
    ys = (np.arange(size) + 0.5) / size * h
    xs = (np.arange(size) + 0.5) / size * w
    base = src[np.clip(ys.astype(int), 0, h - 1)][:, np.clip(xs.astype(int), 0, w - 1)][..., :3]
    v, u = np.mgrid[0:size, 0:size]
    u = (u + 0.5) / size - 0.5
    v = (v + 0.5) / size - 0.5
    r = np.sqrt(u * u + v * v)
    ang = np.arctan2(v, u)
    iris_r = 0.15
    rng = np.random.default_rng(3)
    fibres = np.zeros_like(r)
    for k in range(6):
        fibres += np.sin(ang * rng.integers(20, 60) + rng.uniform(0, 6.28)) * rng.uniform(0.3, 1.0)
    fibres = fibres / 6
    t = np.clip(r / iris_r, 0, 1)
    inner = np.array([0.95, 0.62, 0.12])
    outer = np.array([0.55, 0.22, 0.04])
    iris = inner[None, None, :] * (1 - t[..., None]) ** 0.7 + outer[None, None, :] * (1 - (1 - t[..., None]) ** 0.7)
    iris *= (1 + 0.12 * fibres)[..., None]
    ring = np.clip((r - iris_r * 0.86) / (iris_r * 0.14), 0, 1)
    iris *= (1 - 0.75 * ring)[..., None]
    # The slit: tall and narrow, soft-edged.
    slit = ((u / 0.018) ** 2 + (v / 0.105) ** 2)
    pupil = np.clip((1.15 - slit) / 0.3, 0, 1)
    iris *= (1 - 0.96 * pupil)[..., None]
    a = np.clip((iris_r - r) / 0.006, 0, 1)
    col = base * (1 - a[..., None]) + mt._linear_to_srgb(np.clip(iris, 0, 1)) * a[..., None]
    # A highlight, upper left.
    hl = np.exp(-(((u + 0.035) / 0.016) ** 2 + ((v - 0.04) / 0.016) ** 2))
    col = col * (1 - hl[..., None]) + hl[..., None] * 0.97
    rgba = np.ones((size, size, 4), np.float32)
    rgba[..., :3] = np.clip(col, 0, 1)
    img = mt._save(rgba, out, "sRGB")
    node.image = img
    return out.name
