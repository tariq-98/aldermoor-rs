"""Fitting checks: which regions a piece really covers, and pushing cloth clear of the body."""

import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import regions as rg


def tree(objects, keep=None):
    """A world-space BVH of the evaluated meshes. `keep(obj, poly_index)` filters polygons."""
    verts, polys = [], []
    dg = bpy.context.evaluated_depsgraph_get()
    for ob in objects:
        ev = ob.evaluated_get(dg)
        me = ev.to_mesh()
        base = len(verts)
        verts += [ev.matrix_world @ v.co for v in me.vertices]
        for p in me.polygons:
            if keep is None or keep(ob, p.index):
                polys.append([base + i for i in p.vertices])
        ev.to_mesh_clear()
    return BVHTree.FromPolygons(verts, polys, all_triangles=False) if polys else None


def coverage(body, body_regions, pieces, reach_out=0.07, reach_in=0.012):
    """Area fraction of each body region hidden behind (or replaced by) the pieces.

    A body polygon counts as covered when a ray along its normal meets a piece within 7 cm, or
    a piece's surface lies just inside it (an outfit that brings its own hands or skin).
    """
    t = tree(pieces)
    mw = body.matrix_world
    nm = mw.to_3x3().inverted().transposed()
    total, covered = {}, {}
    for poly, r in zip(body.data.polygons, body_regions):
        c = mw @ poly.center
        n = (nm @ poly.normal).normalized()
        a = poly.area
        total[r] = total.get(r, 0.0) + a
        hit = t.ray_cast(c + n * 0.0005, n, reach_out)[0] or t.ray_cast(c + n * 0.0005, -n, reach_in)[0]
        if hit is not None:
            covered[r] = covered.get(r, 0.0) + a
    return {rg.REGIONS[r]: covered.get(r, 0.0) / total[r] for r in total}


def hides(cov, threshold=0.93):
    return sorted((r for r, f in cov.items() if f >= threshold), key=rg.INDEX.get)


def push_out(mesh, body_tree, margin=0.004, smooth=6, radius_steps=1):
    """Moves cloth vertices that sit inside (or within `margin` of) the body tree to `margin`
    outside it, then spreads the correction to neighbours so the cloth doesn't dent.

    Works on the rest pose in world space. Returns (vertices moved, largest move in metres).
    """
    me = mesh.data
    mw = mesh.matrix_world
    inv = mw.inverted()
    world = [mw @ v.co for v in me.vertices]
    delta = [Vector() for _ in world]
    moved = 0
    worst = 0.0
    for i, p in enumerate(world):
        loc, normal, _, dist = body_tree.find_nearest(p, 0.08)
        if loc is None:
            continue
        signed = (p - loc).dot(normal)
        if signed < margin and signed > -0.06:
            delta[i] = normal * (margin - signed)
            moved += 1
            worst = max(worst, margin - signed)
    if not moved:
        return 0, 0.0
    nbrs = [[] for _ in world]
    for e in me.edges:
        a, b = e.vertices
        nbrs[a].append(b)
        nbrs[b].append(a)
    # Spread: each pass takes, per vertex, the larger of its own correction and half its
    # neighbours' average, so the fix fades out over a few edge rings.
    for _ in range(smooth):
        nxt = []
        for i, d in enumerate(delta):
            if nbrs[i]:
                avg = sum((delta[j] for j in nbrs[i]), Vector()) / len(nbrs[i])
                nxt.append(d if d.length >= avg.length * 0.6 else avg * 0.6)
            else:
                nxt.append(d)
        delta = nxt
    for v, p, d in zip(me.vertices, world, delta):
        v.co = inv @ (p + d)
    me.update()
    return moved, worst
