"""Plate armour built from each body's own silhouette.

A plate is lofted from rings of rays cast out from a bone axis to the outermost layer underneath
(skin or clothing, by region). The ring radii are smoothed into clean, convex steel and kept a
set clearance outside that layer, so the same code fits every body type, race and build. Weights
come from the bones the plate covers. Cloth-hugging parts (glove fingers) copy the body's own
weights instead.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from . import regions as rg, scene as sc, shapes as sh


class Layer:
    """Ray queries against the outermost surface of the chosen regions (body plus clothing)."""

    def __init__(self, objects, regions, side=0):
        """side: +1 keeps only the character's left (+X), -1 only the right, 0 both."""
        keep = {rg.INDEX[r] for r in regions}
        dg = bpy.context.evaluated_depsgraph_get()
        verts, polys = [], []
        for ob in objects:
            ev = ob.evaluated_get(dg)
            me = ev.to_mesh()
            base = len(verts)
            verts += [ev.matrix_world @ v.co for v in me.vertices]
            attr = me.attributes.get("_REGION")
            if attr is None:
                ev.to_mesh_clear()
                continue
            vals = [0.0] * len(me.loops)
            attr.data.foreach_get("value", vals)
            mw = ev.matrix_world
            for p in me.polygons:
                if int(round(vals[p.loop_start])) not in keep:
                    continue
                if side and (mw @ p.center).x * side < 0.01:
                    continue
                polys.append([base + i for i in p.vertices])
            ev.to_mesh_clear()
        self.tree = BVHTree.FromPolygons(verts, polys)

    def outer(self, origin, direction, reach=0.45):
        """Distance to the last surface along a ray from inside (the outermost layer)."""
        d = direction.normalized()
        hit = self.tree.ray_cast(origin, d, reach)
        last = None
        while hit[0] is not None:
            last = (hit[0] - origin).length
            hit = self.tree.ray_cast(hit[0] + d * 1e-4, d, reach)
        return last


def _smooth_ring(r, window=3, closed=True):
    n = len(r)
    out = []
    for i in range(n):
        acc, w = 0.0, 0.0
        for k in range(-window, window + 1):
            j = i + k
            if closed:
                j %= n
            elif j < 0 or j >= n:
                continue
            g = math.exp(-(k / max(window, 1)) ** 2 * 1.5)
            acc += r[j] * g
            w += g
        out.append(acc / w)
    return out


def radial_grid(layer, centre_fn, frame_fn, levels, angles, clearance, smooth=(3, 2), passes=4, fallback=0.1):
    """Rings of radii around an axis: levels are axis parameters, angles in radians.

    centre_fn(level) gives the axis point; frame_fn(level) gives (u, v) unit vectors spanning
    the ring plane. Returns rows of points, smoothed and kept `clearance` outside the layer.
    """
    raw = []
    for lv in levels:
        c = centre_fn(lv)
        u, v = frame_fn(lv)
        row = []
        for a in angles:
            d = u * math.cos(a) + v * math.sin(a)
            r = layer.outer(c, d)
            row.append(r)
        raw.append(row)
    # Fill misses from neighbours (or the fallback radius).
    for row in raw:
        known = [x for x in row if x is not None]
        for i, x in enumerate(row):
            if x is None:
                row[i] = max(known) if known else fallback
    need = [[x + clearance for x in row] for row in raw]
    closed = abs((angles[-1] - angles[0]) - 2 * math.pi * (len(angles) - 1) / len(angles)) < 1e-6
    r = [row[:] for row in need]
    for _ in range(passes):
        r = [_smooth_ring(row, smooth[0], closed) for row in r]
        # Along the axis.
        cols = list(zip(*r))
        cols = [_smooth_ring(list(col), smooth[1], closed=False) for col in cols]
        r = [list(x) for x in zip(*cols)]
        r = [[max(a, b) for a, b in zip(ra, rb)] for ra, rb in zip(r, need)]
    pts = []
    for lv, row in zip(levels, r):
        c = centre_fn(lv)
        u, v = frame_fn(lv)
        pts.append([c + (u * math.cos(a) + v * math.sin(a)) * rad for a, rad in zip(angles, row)])
    return pts


def grid_mesh(name, rows, closed, mat, flip=False):
    bm = bmesh.new()
    vg = [[bm.verts.new(p) for p in row] for row in rows]
    n = len(rows[0])
    for j in range(len(rows) - 1):
        for i in range(n if closed else n - 1):
            k = (i + 1) % n
            f = (vg[j][i], vg[j][k], vg[j + 1][k], vg[j + 1][i])
            bm.faces.new(tuple(reversed(f)) if flip else f)
    uv = bm.loops.layers.uv.new("UVMap")
    pos = {vg[j][i]: (i, j) for j in range(len(rows)) for i in range(n)}
    for f in bm.faces:
        for loop in f.loops:
            i, j = pos[loop.vert]
            loop[uv].uv = (i / max(n - 1, 1), j / max(len(rows) - 1, 1))
    ob = sh.obj(name, bm, mat)
    return ob, vg


def plate(ob, thickness=0.004, centre=None):
    """Gives a sheet thickness inwards, so its outside keeps the clearance, with closed edges."""
    me = ob.data
    c = centre if centre is not None else sum((v.co for v in me.vertices), Vector()) / len(me.vertices)
    if sum((p.center - c).dot(p.normal) for p in me.polygons) < 0:
        me.flip_normals()
    mod = ob.modifiers.new("plate", "SOLIDIFY")
    mod.thickness = thickness
    mod.offset = -1.0
    mod.use_rim = True
    mod.use_quality_normals = True
    sc.activate(ob)
    bpy.ops.object.modifier_apply(modifier=mod.name)
    return ob


def boundary_loops(rows, closed):
    """The open edges of a grid as point lists: first row, last row and, if open, the two sides."""
    loops = [rows[0], rows[-1]]
    if not closed:
        loops += [[r[0] for r in rows], [r[-1] for r in rows]]
    return loops


def rim_tubes(name, loops, closed_rows, radius, mat):
    out = []
    for k, loop in enumerate(loops):
        pts = list(loop) + ([loop[0]] if closed_rows and k < 2 else [])
        out.append(sh.obj(f"{name}_rim", sh.sweep(pts, lambda t: radius, n=8, up=Vector((0, 0, 1))), mat))
    return out


def assign(ob, weight_fn):
    """weight_fn(world point) -> {bone: weight}."""
    mw = ob.matrix_world
    for v in ob.data.vertices:
        wd = weight_fn(mw @ v.co)
        s = sum(wd.values()) or 1.0
        for bone, w in wd.items():
            if w / s <= 1e-4:
                continue
            vg = ob.vertex_groups.get(bone) or ob.vertex_groups.new(name=bone)
            vg.add([v.index], w / s, "REPLACE")


def chain_weights(heads, chain, point):
    """Blends between consecutive bones of a chain by height (for torsos) or along it."""
    pts = [heads[b] for b in chain]
    best = None
    for i in range(len(pts) - 1):
        a, b = pts[i], pts[i + 1]
        ab = b - a
        t = max(0.0, min(1.0, (point - a).dot(ab) / max(ab.length_squared, 1e-9)))
        d = (point - (a + ab * t)).length
        if best is None or d < best[0]:
            best = (d, i, t)
    _, i, t = best
    w = {chain[i]: 1.0 - t * 0.5, chain[i + 1]: t * 0.5}
    if t > 0.5 and i + 2 < len(chain):
        w = {chain[i]: 1.0 - t, chain[i + 1]: t}
    return w


def rivets(points_normals, radius, mat, name="rivet"):
    out = []
    for p, n in points_normals:
        bm = sh.lathe([(0, radius * 0.75), (radius * 0.75, radius * 0.6), (radius, 0.0), (radius * 0.9, -radius * 0.4), (0, -radius * 0.4)], 8)
        sh.transform(bm, Matrix.Translation(p) @ n.to_track_quat("Z", "Y").to_matrix().to_4x4())
        out.append(sh.obj(name, bm, mat))
    return out
