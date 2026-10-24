"""The stone golem's stones and their paint.

Each stone is a signed distance field: an ellipsoid along its bone, cut by flat faces so it
reads as hewn, made irregular and lumpy by weathering and split by cracks. The chest has a
socket for the golem's heart and the head a heavy brow over two eye sockets. build_creatures.py
meshes the stones, binds each rigidly to its bone and joins them.

The paint is one set of maps over all the stones: stone with mottling, strata and grain, pale
lichen, moss on whatever faces up and in the hollows; runes on the chest, arms and thighs and
cracks running from the heart, cut into the normal map and glowing in the emissive map. All of
it is laid down from position and normal through fur.GBuffer. (Grooves a centimetre wide are
too fine to carve into the mesh itself.)
"""

import math
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

from . import fur, materials as mt, sdf, shapes as sh

GLOW = (0.25, 0.75, 1.0)
STEP = 0.005

# Rune glyphs: strokes in a unit cell (x across, y up), angular, like marks cut with a chisel.
GLYPHS = [
    [((-0.25, -0.5), (-0.25, 0.5)), ((-0.25, 0.05), (0.3, 0.5)), ((-0.25, -0.05), (0.3, -0.5))],
    [((0.0, -0.5), (0.0, 0.5)), ((-0.32, 0.18), (0.0, 0.5)), ((0.32, 0.18), (0.0, 0.5))],
    [((-0.25, -0.5), (-0.25, 0.5)), ((-0.25, 0.5), (0.28, 0.2)), ((0.28, 0.2), (-0.25, -0.08))],
    [((-0.32, -0.5), (0.32, 0.5)), ((0.32, -0.5), (-0.32, 0.5))],
    [((-0.28, -0.5), (-0.28, 0.5)), ((0.28, -0.5), (0.28, 0.5)), ((-0.28, 0.3), (0.28, -0.3))],
    [((0.0, -0.5), (0.0, 0.5)), ((-0.3, -0.1), (0.0, 0.2)), ((0.3, -0.1), (0.0, 0.2))],
    [((-0.3, 0.5), (0.3, 0.5)), ((0.3, 0.5), (-0.3, -0.5)), ((-0.3, -0.5), (0.3, -0.5))],
    [((0.0, -0.5), (0.0, 0.5)), ((-0.3, 0.5), (0.3, 0.1)), ((-0.3, 0.1), (0.3, -0.3))],
]


def stone(a, b, r, seed, flat=0.9, cuts=7, crack=1.0, lump=0.045, warp=0.1, depth=(0.6, 0.82)):
    """A hewn, weathered stone from a to b, about r thick. Returns (field, (centre, frame, radii))."""
    a, b = np.asarray(a, float), np.asarray(b, float)
    axis = b - a
    L = float(np.linalg.norm(axis))
    c = (a + b) / 2
    R = sdf.aim(axis)
    rad = np.array([r, L / 2, r * flat])
    rng = np.random.default_rng(seed)
    shape = sdf.Ellipsoid(c, rad, R)
    for _ in range(cuts):
        n = rng.normal(size=3)
        n /= np.linalg.norm(n)
        ext = math.sqrt(float((((R.T @ n) * rad) ** 2).sum()))
        shape = sdf.Intersect(shape, sdf.Plane(c + n * ext * rng.uniform(*depth), n), k=0.018 * r)
    s0, s1, s2 = (int(x) for x in rng.integers(1, 9999, size=3))

    def detail(p):
        bulge = warp * r * (2 * fur.fbm(p, 1.3 * r, s0, octaves=2) - 1)
        lumps = lump * r * (2 * fur.fbm(p, 0.4 * r, s1, octaves=3) - 1)
        v = np.abs(fur.fbm(p, 1.1 * r, s2, octaves=2) - 0.5)
        return bulge + lumps - 0.07 * r * crack * np.clip(1 - v / 0.022, 0, 1) ** 2

    return sdf.Displace(shape, detail, reach=(warp + lump) * r + 0.012), (c, R, rad)


def head(c, s=1.0, seed=99):
    """The head: a squat block with a heavy brow over two eye sockets. Returns (field, eye centres)."""
    c = np.asarray(c, float)
    block, _ = stone(c + np.array((0, 0, -0.1)) * s, c + np.array((0, 0, 0.12)) * s, 0.13 * s, seed, flat=1.05, cuts=3, crack=0.6)
    brow = sdf.Ellipsoid(c + np.array((0, -0.085, 0.06)) * s, np.array((0.13, 0.05, 0.035)) * s)
    shape = sdf.Union([block, brow], k=0.03 * s)
    eyes = [c + np.array((x * 0.052, -0.118, 0.025)) * s for x in (1, -1)]
    sockets = sdf.Union([sdf.Ellipsoid(e, np.array((0.026, 0.03, 0.017)) * s) for e in eyes])
    return sdf.Subtract(shape, sockets, k=0.006 * s), eyes


def surface_at(shape, centre, direction, far=1.5):
    """Where a ray from inside the shape leaves it, and the outward normal there."""
    p = sdf.march(shape, centre, direction, far)
    return p, sdf.gradient(shape, p)


def runes(shape, centre, direction, up, glyphs, size, spacing=1.35):
    """A column of glyphs on a stone where a ray from `centre` along `direction` leaves it,
    reading down along `up`: strokes as runs of points on the surface, and the groove radius."""
    p, n = surface_at(shape, centre, direction)
    v = np.asarray(up, float)
    v = v - n * (v @ n)
    v /= np.linalg.norm(v)
    u = np.cross(v, n)
    strokes = []
    for k, g in enumerate(glyphs):
        off = ((len(glyphs) - 1) / 2 - k) * spacing * size
        for (x0, y0), (x1, y1) in GLYPHS[g % len(GLYPHS)]:
            pts = np.array([p + u * (x0 + (x1 - x0) * t) * size * 0.8 + v * ((y0 + (y1 - y0) * t) * size + off)
                            for t in np.linspace(0, 1, 5)])
            strokes.append(sdf.project(shape, pts, iters=5))
    return strokes, size * 0.075


def heart(shape, centre, direction, up, size, seed, cracks=7):
    """The golem's heart: a socket where a ray from `centre` leaves the stone, and cracks wandering
    out from it. Returns (carved field, the socket's centre and normal, crack strokes, width)."""
    p, n = surface_at(shape, centre, direction)
    v = np.asarray(up, float)
    v = v - n * (v @ n)
    v /= np.linalg.norm(v)
    u = np.cross(v, n)
    rng = np.random.default_rng(seed)
    socket = sdf.Ellipsoid(p - n * size * 0.25, np.array((size * 0.8, size * 0.8, size * 1.1)), np.stack([u, v, n], axis=1))
    strokes = []
    for k in range(cracks):
        ang = 2 * math.pi * k / cracks + rng.uniform(-0.25, 0.25)
        d = u * math.cos(ang) + v * math.sin(ang)
        pts = [p + d * size * 0.75]
        for _ in range(7):
            ang += rng.uniform(-0.45, 0.45)
            d = u * math.cos(ang) + v * math.sin(ang)
            pts.append(pts[-1] + d * size * rng.uniform(0.28, 0.4))
        strokes.append(sdf.project(shape, np.array(pts), iters=5))
    return sdf.Subtract(shape, socket, k=size * 0.12), (p, n), strokes, size * 0.06


def crystal(c, n, size, name="heart"):
    """A glowing crystal: a stretched octahedron set into the heart's socket."""
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=1, radius=1.0)
    for v in bm.verts:
        v.co = Vector((v.co.x * 0.62, v.co.y * 0.62, v.co.z * 1.0)) * size
    rot = Vector((0, 0, 1)).rotation_difference(Vector((0, 0, 1)).lerp(Vector(n), 0.2).normalized())
    bmesh.ops.transform(bm, matrix=Matrix.Translation(Vector(c)) @ rot.to_matrix().to_4x4(), verts=bm.verts)
    return sh.obj(name, bm, "rune", smooth=False)


def glow_eyes(eyes, s=1.0):
    parts = []
    for e in eyes:
        bm = bmesh.new()
        bmesh.ops.create_uvsphere(bm, u_segments=10, v_segments=8, radius=0.017 * s)
        bmesh.ops.transform(bm, matrix=Matrix.Translation(Vector(e)) @ Matrix.Diagonal((1.15, 0.8, 0.75, 1.0)), verts=bm.verts)
        parts.append(sh.obj("eye", bm, "rune", smooth=True))
    return parts


# --- Paint --------------------------------------------------------------------------------------

def _glow(P, segments, reach):
    """Brightness at points P from glowing grooves [(a, b, radius, intensity)]: full inside the
    groove, with a faint halo round it."""
    cell = reach * 2
    keys = np.floor(P / cell).astype(np.int64)

    def hash3(x, y, z):
        return (x * 73856093) ^ (y * 19349663) ^ (z * 83492791)

    h = hash3(keys[:, 0], keys[:, 1], keys[:, 2])
    order = np.argsort(h, kind="stable")
    hs = h[order]
    out = np.zeros(len(P))
    for a, b, w, inten in segments:
        lo = np.floor((np.minimum(a, b) - reach) / cell).astype(np.int64)
        hi = np.floor((np.maximum(a, b) + reach) / cell).astype(np.int64)
        idx = []
        for x in range(lo[0], hi[0] + 1):
            for y in range(lo[1], hi[1] + 1):
                for z in range(lo[2], hi[2] + 1):
                    k = hash3(np.int64(x), np.int64(y), np.int64(z))
                    l, r = np.searchsorted(hs, k, "left"), np.searchsorted(hs, k, "right")
                    if r > l:
                        idx.append(order[l:r])
        if not idx:
            continue
        idx = np.unique(np.concatenate(idx))
        q = P[idx]
        ab = b - a
        t = np.clip((q - a) @ ab / max(ab @ ab, 1e-12), 0, 1)
        d = np.linalg.norm(q - (a + t[:, None] * ab), axis=1)
        g = np.clip((w * 1.15 - d) / (w * 0.35), 0, 1) + 0.3 * np.exp(-((d - w) / (1.6 * w)) ** 2) * (d > w)
        out[idx] = np.maximum(out[idx], inten * g)
    return np.clip(out, 0, 1)


def unwrap(ob):
    for o in bpy.context.selected_objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(66), island_margin=0.004, area_weight=0.0)
    bpy.ops.uv.pack_islands(rotate=True, margin=0.004)
    bpy.ops.object.mode_set(mode="OBJECT")


def paint(body, glowing, out_dir, stem, size=2048, seed=4, base=(0.16, 0.152, 0.14), moss_amount=1.0, ground=None,
          lichen_amount=1.0, streaks=0.0):
    """UVs and one material for all the stones: colour, a normal map and, if anything glows,
    an emissive map. Scenery rocks use it too, with no glow and darker, damper stone near
    `ground` (a height).

    `glowing` lists grooves to light: [(a, b, radius, intensity)] in the body's rest pose.
    """
    unwrap(body)
    m = bpy.data.materials.new(f"stone.{stem}")
    m.use_nodes = True
    body.data.materials.clear()
    body.data.materials.append(m)
    gb = fur.GBuffer(body, size)
    q, N = gb.pos, gb.nrm
    ao = np.clip(fur.bake_ao(body, size, samples=24, distance=0.12)[gb.iy, gb.ix], 0, 1)
    mott = fur.fbm(q, 0.3, seed, octaves=3) - 0.5
    warm = fur.fbm(q, 0.55, seed + 1, octaves=2) - 0.5
    strata = np.sin(q @ np.array((0.25, 0.1, 0.96)) / 0.03 + 5 * (fur.fbm(q, 0.2, seed + 2, octaves=2) - 0.5))
    grain = fur.value_noise(q, 0.0035, seed + 3) - 0.5
    grit = fur.value_noise(q, 0.0012, seed + 4) - 0.5
    col = np.array(base)[None, :] * (1 + 0.5 * mott + 0.08 * strata + 0.25 * grain + 0.14 * grit)[:, None]
    col *= 1 + warm[:, None] * np.array((0.16, 0.02, -0.14))[None, :]
    # Lichen: pale crusts on the sides and tops.
    lichen = fur.ramp(0.62 + 0.08 * (1 - lichen_amount), 0.68 + 0.08 * (1 - lichen_amount), fur.fbm(q, 0.04, seed + 5, octaves=3))
    lichen = lichen * fur.ramp(-0.4, 0.2, N[:, 2]) * min(1.0, lichen_amount * 1.5)
    col += (np.array((0.36, 0.35, 0.24))[None, :] * (1 + 0.2 * grain)[:, None] - col) * (lichen * 0.85)[:, None]
    # Rain streaks: darker runs down the steeper faces.
    if streaks:
        run = fur.fbm(q * np.array((1.0, 1.0, 0.12)), 0.035, seed + 11, octaves=3)
        col *= (1 - streaks * 0.3 * fur.ramp(0.52, 0.68, run) * fur.ramp(0.8, 0.35, np.abs(N[:, 2])))[:, None]
    # Moss: on what faces up and in the hollows, broken up.
    up = fur.ramp(0.2, 0.75, N[:, 2] + 0.4 * (fur.fbm(q, 0.07, seed + 6, octaves=3) - 0.5))
    hollow = fur.ramp(0.72, 0.42, ao)
    moss = np.clip(np.maximum(up, hollow * 0.7) * fur.ramp(0.36, 0.56, fur.fbm(q, 0.11, seed + 7, octaves=3)), 0, 1) * moss_amount
    tint = fur.fbm(q, 0.05, seed + 8, octaves=2)
    mcol = np.array((0.05, 0.1, 0.02))[None, :] * (1 - tint[:, None]) + np.array((0.12, 0.15, 0.035))[None, :] * tint[:, None]
    mcol = mcol * (1 + 0.6 * grit)[:, None]
    col += (mcol - col) * moss[:, None]
    col *= (0.45 + 0.55 * ao)[:, None]
    if ground is not None:
        col *= (1 - 0.4 * fur.ramp(ground + 0.25, ground, q[:, 2] + 0.06 * (fur.fbm(q, 0.1, seed + 10, octaves=2) - 0.5)))[:, None]
    # The runes and cracks: dark grooves that glow.
    glow = _glow(q, glowing, max(w for _, _, w, _ in glowing) * 3) if glowing else np.zeros(len(q))
    col *= (1 - 0.85 * np.clip(glow * 1.5, 0, 1))[:, None]
    emit = np.array(GLOW)[None, :] * glow[:, None]
    # The grooves are cut into the normal map: a rounded channel along each stroke.
    groove = _glow(q, [(a, b, w, 1.0) for a, b, w, _ in glowing], max(w for _, _, w, _ in glowing) * 3) if glowing else 0
    height = np.clip(0.5 + 0.18 * grain + 0.1 * grit + moss * 0.25 * (fur.value_noise(q, 0.002, seed + 9) - 0.5)
                     - 0.45 * np.clip(groove, 0, 1), 0, 1)
    out_dir = Path(out_dir)
    col_img = fur.save_rgb(mt._linear_to_srgb(np.clip(fur.bleed(gb.image(col), gb.mask, 10), 0, 1)), out_dir / f"{stem}_color.jpg", "sRGB")
    nrm_img = fur.save_rgb(fur.normal_map(gb, height, 0.004), out_dir / f"{stem}_normal.jpg", "Non-Color")
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
    if glowing:
        emi_img = fur.save_rgb(mt._linear_to_srgb(np.clip(fur.bleed(gb.image(emit), gb.mask, 10), 0, 1)), out_dir / f"{stem}_emissive.jpg", "sRGB")
        te = nt.nodes.new("ShaderNodeTexImage")
        te.image = emi_img
        nt.links.new(te.outputs["Color"], bsdf.inputs["Emission Color"])
        bsdf.inputs["Emission Strength"].default_value = 4.0
    bsdf.inputs["Roughness"].default_value = 0.88
    return m
