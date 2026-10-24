"""The dwarven beard: locks of hair grown from the face and draped over the chest.

Roots are scattered over the beard's ground on the body itself (cheeks, jaw, chin, upper lip and
the front of the neck, short of the lips and the eyes). Each lock grows downward from its root,
kept off the skin and the chest and drawn, at the chin, towards two braids; the moustache
sweeps out round the mouth first. The locks are flattened round cones joined into one surface
(aldermoor_art/sdf.py). The braids are plaited lobes with a bronze ring at the top and the tail
of each ends in a tuft.

The hair material is a tone-neutral detail map (mean 0.5, strands along the locks and their
occlusion), named `hair.*` so the catalogue's hair colours tint it like any other hair.
"""

import math
import random
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from . import fur, materials as mt, regions as rg, sdf, shapes as sh


class Face:
    """Landmarks found on the body: eye level, the nose tip, the mouth, the chin, the ears."""

    def __init__(self, body, eyes):
        mw = body.matrix_world
        pts = [eyes.matrix_world @ v.co for v in eyes.data.vertices]
        left = [p for p in pts if p.x > 0]
        right = [p for p in pts if p.x < 0]
        self.eye_l = sum(left, Vector()) / len(left)
        self.eye_r = sum(right, Vector()) / len(right)
        self.s = (self.eye_l.x - self.eye_r.x) / 0.064  # Scale against a human's eyes.
        self.ez = (self.eye_l.z + self.eye_r.z) / 2
        me = body.data
        regions = rg.read(body)
        face, ears, neck = rg.INDEX["face"], rg.INDEX["ears"], rg.INDEX["neck"]
        on = {face: set(), ears: set(), neck: set()}
        for poly, r in zip(me.polygons, regions):
            if r in on:
                on[r].update(poly.vertices)
        co = [mw @ v.co for v in me.vertices]
        fv = [co[i] for i in on[face]]
        s, ez = self.s, self.ez
        mid = [p for p in fv if abs(p.x) < 0.012 * s]
        self.nose = min((p for p in mid if ez - 0.065 * s < p.z < ez - 0.01 * s), key=lambda p: p.y)
        self.chin = min(mid, key=lambda p: p.z)
        front = [p for p in mid if self.chin.z < p.z < self.chin.z + 0.03 * s]
        self.chin_front = min(front, key=lambda p: p.y)
        self.mouth_z = self.nose.z + (self.chin.z - self.nose.z) * 0.42
        lips = [p for p in mid if abs(p.z - self.mouth_z) < 0.006 * s]
        self.mouth = Vector((0, min(p.y for p in lips), self.mouth_z))
        ear = [co[i] for i in on[ears]]
        self.ear_y = min(p.y for p in ear) if ear else self.eye_l.y + 0.08 * s
        self.tree = BVHTree.FromObject(body, bpy.context.evaluated_depsgraph_get())

    def top(self, ax):
        """The beard's upper edge: under the nose in the middle, rising to the sideburns."""
        s = self.s
        t = max(0.0, min(1.0, (ax - 0.02 * s) / (0.05 * s)))
        return self.ez - (0.047 - 0.03 * t) * s

    def in_beard(self, p, n, region):
        s = self.s
        ax = abs(p.x)
        if region == rg.INDEX["face"]:
            if p.z > self.top(ax) or p.y > self.ear_y - 0.004 * s:
                return 0
            # Leave the lips and the underside of the nose bare.
            if ax < 0.027 * s and abs(p.z - self.mouth_z) < 0.0095 * s and n.y < -0.3:
                return 0
            if ax < 0.022 * s and p.z > self.nose.z - 0.016 * s:
                return 0
            return 1
        if region == rg.INDEX["neck"]:
            return 1 if p.z > self.chin.z - 0.05 * s and n.y < 0.2 and p.y < self.chin.y + 0.05 * s else 0
        return 0

    def keep_off(self, p, clearance):
        """p pushed out of the body to at least `clearance` from its surface."""
        loc, nrm, _, dist = self.tree.find_nearest(p)
        if loc is None:
            return p
        inside = (p - loc).dot(nrm) < 0
        if inside or dist < clearance:
            return loc + nrm * clearance
        return p


def braid_path(face, x, length, step=0.01):
    """A braid's centre line: down the front of the chest from below the chin."""
    s = face.s
    z0 = face.chin.z - 0.115 * s
    hit = face.tree.ray_cast(Vector((x * s, -2.0, z0)), Vector((0, 1, 0)))
    y0 = (hit[0].y if hit[0] is not None else face.chin_front.y) - 0.03 * s
    p = Vector((x * s, y0, z0))
    pts = [p]
    for _ in range(int(length * s / step)):
        p = face.keep_off(p + Vector((0, 0, -step * s)), 0.024 * s)
        pts.append(p)
    return pts


def grow(face, root, n, length, seed, target=None, sweep=None):
    """A lock's centre line from a root: out from the skin, then down, kept off the body.
    `target` draws it in (to a braid); `sweep` sends it sideways first (a moustache)."""
    rnd = random.Random(seed)
    s = face.s
    step = 0.008 * s
    p = root + n * 0.004 * s
    side = math.copysign(1.0, root.x) if abs(root.x) > 0.004 * s else rnd.choice((-1, 1))
    d = (n * 0.55 + Vector((-side * 0.12, -0.05, -1.0))).normalized()
    if sweep is not None:
        d = (Vector((side, -0.15, -0.35)) + n * 0.3).normalized()
    pts = [p]
    steps = max(2, int(length / step))
    for i in range(steps):
        t = i / steps
        want = Vector((rnd.uniform(-0.15, 0.15), 0, -1.0))
        if sweep is not None and t < 0.45:
            want = Vector((side * 1.2, -0.2, -0.5))
        if target is not None:
            want = want * (1 - t) + (target - p).normalized() * (0.4 + 1.2 * t)
        d = (d * 0.62 + want.normalized() * 0.38).normalized()
        clearance = (0.009 + 0.01 * min(1.0, i * step / (0.06 * s))) * s
        p = face.keep_off(p + d * step, clearance)
        pts.append(p)
    return pts


def lock_shapes(face, lines, r0):
    """Flattened round cones along each lock, thinning to a soft point."""
    out = []
    for pts, radius in zip(lines, r0):
        n = len(pts)
        for i in range(n - 1):
            a, b = pts[i], pts[i + 1]
            loc, nrm, _, _ = face.tree.find_nearest((a + b) / 2)
            up = nrm if nrm is not None else Vector((0, -1, 0))
            ra = radius * (1 - 0.8 * (i / (n - 1)) ** 1.4) + 0.0008
            rb = radius * (1 - 0.8 * ((i + 1) / (n - 1)) ** 1.4) + 0.0008
            out.append(sdf.FlatCone(np.array(a), np.array(b), ra, rb, np.array(up), flat=0.52))
    return out


def plait(path, s, lobes_per_m=62.0):
    """A braid along a path: lobes leaning left and right in turn round a slim core."""
    out = []
    length = sum((b - a).length for a, b in zip(path, path[1:]))
    count = max(3, int(length * lobes_per_m / s))
    for k in range(count):
        t = (k + 0.5) / count * (len(path) - 1)
        i = min(int(t), len(path) - 2)
        c = path[i].lerp(path[i + 1], t - i)
        tan = (path[i + 1] - path[i]).normalized()
        lean = 1 if k % 2 == 0 else -1
        side = Vector((1, 0, 0))
        axis = (tan + side * lean * 0.55).normalized()
        R = sdf.aim(np.array(axis), up=(0, -1, 0))
        out.append(sdf.Ellipsoid(np.array(c + side * lean * 0.0045 * s), np.array((0.0105, 0.0125, 0.0082)) * s, R))
    core = [np.array(p) for p in path]
    out.append(sdf.chain(core, [0.0075 * s] * len(core)))
    return out


def ring(c, axis, s, name="beard_ring"):
    """A bronze ring round a braid."""
    bm = bmesh.new()
    rings = []
    seg, around = 20, 8
    R, r = 0.0125 * s, 0.0032 * s
    for i in range(seg):
        a = 2 * math.pi * i / seg
        row = []
        for j in range(around):
            b = 2 * math.pi * j / around
            row.append(bm.verts.new(Vector(((R + r * math.cos(b)) * math.cos(a), (R + r * math.cos(b)) * math.sin(a), r * math.sin(b) * 1.6))))
        rings.append(row)
    for i in range(seg):
        for j in range(around):
            bm.faces.new((rings[i][j], rings[(i + 1) % seg][j], rings[(i + 1) % seg][(j + 1) % around], rings[i][(j + 1) % around]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    rot = Vector((0, 0, 1)).rotation_difference(Vector(axis).normalized())
    bmesh.ops.transform(bm, matrix=Matrix.Translation(c) @ rot.to_matrix().to_4x4(), verts=bm.verts)
    return sh.obj(name, bm, "brass", smooth=True)


def build(body, eyes, seed=7):
    """The beard's field, the lock directions (for painting) and the rings' positions."""
    face = Face(body, eyes)
    s = face.s
    surface = fur.Surface(body)
    roots = fur.scatter(surface, face.in_beard, 190, 0.0085 * s, seed)
    paths = {x: braid_path(face, x, 0.12) for x in (0.021, -0.021)}
    rnd = random.Random(seed)
    lines, radii = [], []
    for k, (p, idx, bc) in enumerate(roots):
        n = surface.normal_at(idx, bc)
        ax = abs(p.x)
        if p.z > face.mouth_z - 0.004 * s and ax < 0.04 * s:
            # The moustache: out over the corners of the mouth, then down into the beard.
            pts = grow(face, p, n, rnd.uniform(0.07, 0.095) * s, seed + k, sweep=True)
            r = 0.0062 * s
        elif ax < 0.045 * s and p.z < face.mouth_z:
            # The chin: long, drawn into the braids.
            braid = paths[0.021 if p.x > 0 else -0.021]
            pts = grow(face, p, n, rnd.uniform(0.15, 0.19) * s, seed + k, target=braid[2])
            r = 0.0072 * s
        elif p.z > face.ez - 0.05 * s:
            pts = grow(face, p, n, rnd.uniform(0.05, 0.08) * s, seed + k)  # Sideburns.
            r = 0.0058 * s
        else:
            pts = grow(face, p, n, rnd.uniform(0.11, 0.17) * s, seed + k)  # Cheeks, jaw and neck.
            r = 0.0068 * s
        lines.append(pts)
        radii.append(r)
    parts = lock_shapes(face, lines, radii)
    rings = []
    for x, path in paths.items():
        parts += plait(path, s)
        tip = path[-1]
        tan = (path[-1] - path[-2]).normalized()
        tuft = []
        for j in range(6):
            a = 2 * math.pi * j / 6
            spread = Vector((math.cos(a) * 0.35, math.sin(a) * 0.2, -1.0)).normalized()
            tuft.append([tip + tan * 0.004 * s + spread * (0.005 + 0.0075 * t) * s * 4 for t in range(4)])
        parts += lock_shapes(face, tuft, [0.0055 * s] * len(tuft))
        lines += tuft
        rings.append((path[1], (path[2] - path[0])))
        rings.append((path[int(len(path) * 0.72)], (path[int(len(path) * 0.72) + 1] - path[int(len(path) * 0.72) - 1])))
    return sdf.Union(parts, k=0.0022 * s), lines, rings, face


def flow_from(lines):
    """A per-point direction field from the locks' centre lines (nearest segment)."""
    from mathutils.kdtree import KDTree
    segs = [(a, b) for pts in lines for a, b in zip(pts, pts[1:])]
    tree = KDTree(len(segs))
    for i, (a, b) in enumerate(segs):
        tree.insert((a + b) / 2, i)
    tree.balance()

    def at(p):
        _, i, _ = tree.find(p)
        a, b = segs[i]
        return (b - a).normalized()

    return at


def paint(beard, lines, out_dir, stem, reference, size=1024, seed=3):
    """UVs and the tone-neutral hair material: strands along the locks, clumps and occlusion."""
    for o in bpy.context.selected_objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = beard
    beard.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(66), island_margin=0.004, area_weight=0.0)
    bpy.ops.uv.pack_islands(rotate=True, margin=0.004)
    bpy.ops.object.mode_set(mode="OBJECT")
    m = bpy.data.materials.new(f"hair.{stem}")
    m.use_nodes = True
    beard.data.materials.clear()
    beard.data.materials.append(m)
    flow = flow_from(lines)
    mw = beard.matrix_world
    flows = np.array([flow(mw @ v.co) for v in beard.data.vertices])
    gb = fur.GBuffer(beard, size, flow=flows)
    strands, clumps = fur.fur_fields(gb, strand=0.0009, clump=0.006, strand_len=0.016, clump_len=0.03, seed=seed)
    ao = np.clip(fur.bake_ao(beard, size, samples=24, distance=0.03)[gb.iy, gb.ix], 0, 1)
    detail = (1 + 0.28 * clumps + 0.24 * strands) * (0.45 + 0.55 * ao)
    detail = detail / max(detail.mean(), 1e-6) * 0.5
    rgb = np.repeat(np.clip(detail, 0, 1)[:, None], 3, axis=1)
    img = fur.bleed(gb.image(rgb, fill=0.5), gb.mask, 8)
    out_dir = Path(out_dir)
    col_img = fur.save_rgb(mt._linear_to_srgb(np.clip(img, 0, 1)), out_dir / f"{stem}_detail.jpg", "sRGB")
    height = np.clip(0.5 + 0.25 * clumps + 0.2 * strands, 0, 1)
    nrm_img = fur.save_rgb(fur.normal_map(gb, height, 0.0008), out_dir / f"{stem}_normal.jpg", "Non-Color")
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = col_img
    nt.links.new(t.outputs["Color"], bsdf.inputs["Base Color"])
    tn = nt.nodes.new("ShaderNodeTexImage")
    tn.image = nrm_img
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(tn.outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    bsdf.inputs["Roughness"].default_value = 0.62
    m["tint"] = "hair"
    m["reference"] = list(reference)
    return m


def mesh(field, face, target=9000):
    """The beard's field as a mesh of about `target` triangles."""
    ob = sdf.mesh(field, 0.0022 * face.s, "beard.dwarven", relax_iters=3)
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    if tris > target:
        dec = ob.modifiers.new("decimate", "DECIMATE")
        dec.ratio = target / tris
        for o in bpy.context.selected_objects:
            o.select_set(False)
        bpy.context.view_layer.objects.active = ob
        bpy.ops.object.modifier_apply(modifier=dec.name)
    for p in ob.data.polygons:
        p.use_smooth = True
    return ob


def weights(ob, skel, face):
    """The beard follows the head; its lower reaches rest partly on the chest."""
    s = face.s
    gh = ob.vertex_groups.get("Head") or ob.vertex_groups.new(name="Head")
    gc = ob.vertex_groups.get("spine_03") or ob.vertex_groups.new(name="spine_03")
    for v in ob.data.vertices:
        z = (ob.matrix_world @ v.co).z
        t = max(0.0, min(1.0, (face.chin.z - 0.02 * s - z) / (0.17 * s)))
        t = t * t * (3 - 2 * t) * 0.45
        gh.add([v.index], 1 - t, "REPLACE")
        if t > 0:
            gc.add([v.index], t, "REPLACE")
