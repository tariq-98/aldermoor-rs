"""Cloth garments fitted to each body: robes, tunics, coats, aprons and hats.

Like plate (armour.py), a garment is lofted from rings of rays cast out to the outermost layer
it covers, but it keeps closer to the body (cloth clearance, not steel) and flares where cloth
falls free: skirts, coat tails and bell sleeves. Weights come from the body under each vertex.
Skirts blend from the pelvis into the thighs, so a stride pulls the hem along instead of tearing
it. Colours are painted into a texture laid out along the garment's own rows and columns
(u around, v along), so hems, cuffs and collars get trim bands, and a dye mask marks the main
cloth (red) and the trim (green).
"""

import math
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import armour as ar, materials as mt, scene as sc, tiles

TORSO = ["chest", "belly", "hips", "neck"]


def heads(skel):
    return {b.name: skel.matrix_world @ b.head_local for b in skel.data.bones}


def torso_centre(layer, p):
    f = layer.outer(p, Vector((0, -1, 0))) or 0.15
    b = layer.outer(p, Vector((0, 1, 0))) or 0.05
    l = layer.outer(p, Vector((1, 0, 0))) or 0.15
    r = layer.outer(p, Vector((-1, 0, 0))) or 0.15
    return p + Vector(((l - r) / 2, (b - f) / 2, 0))


def spine_at(h, z):
    chain = [h["pelvis"], h["spine_01"], h["spine_02"], h["spine_03"], h["neck_01"]]
    if z <= chain[0].z:
        return Vector((chain[0].x, chain[0].y, z))
    for a, b in zip(chain, chain[1:]):
        if a.z <= z <= b.z:
            return a.lerp(b, (z - a.z) / max(b.z - a.z, 1e-6))
    return chain[-1]


class BodyWeights:
    """Skin weights of the nearest body surface point, for cloth that follows the body."""

    def __init__(self, body):
        self.body = body
        self.world = [body.matrix_world @ v.co for v in body.data.vertices]
        self.tree = BVHTree.FromPolygons(self.world, [list(p.vertices) for p in body.data.polygons])
        self.groups = {g.index: g.name for g in body.vertex_groups}

    def at(self, p, limit=4):
        loc, _, face, _ = self.tree.find_nearest(p)
        if loc is None:
            return {"spine_02": 1.0}
        acc = {}
        for vi in self.body.data.polygons[face].vertices:
            w = 1.0 / (1e-4 + (self.world[vi] - loc).length)
            for g in self.body.data.vertices[vi].groups:
                n = self.groups.get(g.group)
                if n:
                    acc[n] = acc.get(n, 0.0) + g.weight * w
        top = sorted(acc.items(), key=lambda kv: -kv[1])[:limit]
        s = sum(w for _, w in top) or 1.0
        return {n: w / s for n, w in top}


def assign(ob, weights):
    for i, wd in enumerate(weights):
        for bone, w in wd.items():
            if w <= 1e-4:
                continue
            vg = ob.vertex_groups.get(bone) or ob.vertex_groups.new(name=bone)
            vg.add([i], w, "REPLACE")


def body_grid(layer, h, z_top, z_bottom, rows, cols, clearance, flare_from=None, flare=0.0, hem_wave=0.0, smooth=(3, 2), open_front=0.0):
    """Rings around the torso and, below the hips, around both legs together (a skirt).

    Returns (rows of points, the z of each row). `flare` widens the rings below `flare_from`
    towards the hem; `open_front` (radians) leaves a gap at the front, for coats.
    """
    levels = [z_top + (z_bottom - z_top) * (j / (rows - 1)) for j in range(rows)]
    if open_front:
        # An open grid whose gap is centred on the front (90 degrees is -Y).
        span = 2 * math.pi - open_front
        angs = [math.radians(90) + open_front / 2 + span * i / (cols - 1) for i in range(cols)]
    else:
        # Start at the back (270 degrees), so the seam and texture join sit behind.
        angs = [2 * math.pi * i / cols + math.radians(270) for i in range(cols)]
    grid = ar.radial_grid(layer, lambda z: torso_centre(layer, spine_at(h, z)), lambda z: (Vector((1, 0, 0)), Vector((0, -1, 0))),
                          levels, angs, clearance, smooth=smooth, passes=3)
    if flare and flare_from is not None:
        for j, z in enumerate(levels):
            if z < flare_from:
                t = (flare_from - z) / max(flare_from - z_bottom, 1e-6)
                c = torso_centre(layer, spine_at(h, flare_from))
                c.z = z
                k = 1 + flare * t ** 1.3
                grid[j] = [Vector((c.x + (p.x - c.x) * k, c.y + (p.y - c.y) * k, p.z)) for p in grid[j]]
    if hem_wave:
        for i in range(len(grid[-1])):
            p = grid[-1][i]
            grid[-1][i] = p + Vector((0, 0, hem_wave * (0.5 + 0.5 * math.cos(i * 2 * math.pi * 7 / len(grid[-1])))))
    return grid, levels, (not open_front)


def sleeve_grid(layer, h, side, t0, t1, rows, clearance, bell=0.0):
    """A sleeve around one arm, from the shoulder (t0 along the upper arm) to t1 on the forearm."""
    s = "l" if side > 0 else "r"
    sh_, el, wr = h[f"upperarm_{s}"], h[f"lowerarm_{s}"], h[f"hand_{s}"]
    pts = []
    for j in range(rows):
        t = t0 + (t1 - t0) * j / (rows - 1)
        pts.append(sh_.lerp(el, t) if t <= 1 else el.lerp(wr, t - 1))
    cols = 20
    angs = [2 * math.pi * i / cols for i in range(cols)]

    def frame(k):
        j = min(range(rows), key=lambda r: abs((t0 + (t1 - t0) * r / (rows - 1)) - k))
        a = pts[max(j - 1, 0)]
        b = pts[min(j + 1, rows - 1)]
        axis = (b - a).normalized()
        up = Vector((0, 0, 1))
        up = (up - axis * up.dot(axis)).normalized()
        return up, axis.cross(up)

    levels = [t0 + (t1 - t0) * j / (rows - 1) for j in range(rows)]
    grid = ar.radial_grid(layer, lambda k: (sh_.lerp(el, k) if k <= 1 else el.lerp(wr, k - 1)), frame, levels, angs, clearance,
                          smooth=(3, 1), passes=3)
    if bell:
        for j, k in enumerate(levels):
            t = max(0.0, (k - (t1 - 0.45)) / 0.45)
            c = sh_.lerp(el, k) if k <= 1 else el.lerp(wr, k - 1)
            f = 1 + bell * t ** 1.6
            grid[j] = [c + (p - c) * f for p in grid[j]]
    return grid


def skirt_weights(h, p, centre_z):
    """Pelvis at the waist, easing into each thigh (by side) and calf further down."""
    side = "l" if p.x > 0 else "r"
    hip = h[f"thigh_{side}"]
    knee = h[f"calf_{side}"]
    if p.z >= hip.z:
        return None
    t = min(1.0, (hip.z - p.z) / max(hip.z - knee.z, 1e-6))
    lateral = min(1.0, abs(p.x) / 0.12)
    to_leg = t * (0.25 + 0.55 * lateral)
    w = {"pelvis": 1.0 - to_leg, f"thigh_{side}": to_leg}
    if p.z < knee.z:
        u = min(1.0, (knee.z - p.z) / max(knee.z - h[f"foot_{side}"].z, 1e-6))
        w[f"calf_{side}"] = w.pop(f"thigh_{side}") * u * 0.6
        w[f"thigh_{side}"] = to_leg * (1 - u * 0.6)
    return w


# --- Cloth textures --------------------------------------------------------------------------

def drape(size, seed, folds=1.0, hang_top=0.7):
    """Fold heights (-1..1) over a garment's UVs (u round it, v from hem to collar, rows bottom
    first): broad folds and finer ones, wandering a little, deepest at the hem and gone by
    `hang_top`."""
    rng = np.random.default_rng(seed)
    v = np.linspace(0, 1, size)[:, None] * np.ones((1, size))
    u = np.linspace(0, 1, size)[None, :] * np.ones((size, 1))
    wander = tiles.noise(size, 2, seed + 11) - 0.5
    h = np.zeros((size, size))
    for n, amp in ((6, 1.0), (11, 0.55), (19, 0.3)):
        h += amp * np.sin(2 * np.pi * u * n + rng.uniform(0, 2 * np.pi) + 2.2 * wander * (n / 6) ** 0.5)
    hang = np.clip(1 - v / hang_top, 0, 1) ** 1.1
    return h / 1.85 * hang * folds


def cloth_texture(path, size, base, trim, bands, weave=0.06, tatter=False, seed=1, ubands=(), folds=1.0, hang_top=0.7):
    """Paints base cloth with trim bands at given v ranges (and u ranges), plus a soft weave and
    the shading of hanging folds.

    Writes `<path>.jpg` (colour), `<path>_dye.png` (R = main cloth, G = trim) and
    `<path>_normal.png` (the weave and the folds; cloth_material picks it up). Colours are
    linear RGB; `bands` and `ubands` are (lo, hi) pairs in 0..1.
    """
    rng = np.random.default_rng(seed)
    v = np.linspace(0, 1, size)[:, None] * np.ones((1, size))
    u = np.linspace(0, 1, size)[None, :] * np.ones((size, 1))
    fold = drape(size, seed, folds, hang_top)
    trim_mask = np.zeros((size, size))
    for v0, v1 in bands:
        trim_mask = np.maximum(trim_mask, ((v >= v0) & (v <= v1)).astype(float))
    for u0, u1 in ubands:
        trim_mask = np.maximum(trim_mask, ((u >= u0) & (u <= u1)).astype(float))
    # A thin dark line along each band's edges, like stitching.
    edge = np.zeros((size, size))
    for v0, v1 in bands:
        for e in (v0, v1):
            if 0 < e < 1:
                edge = np.maximum(edge, np.exp(-((v - e) * size / 2.5) ** 2))
    for u0, u1 in ubands:
        for e in (u0, u1):
            if 0 < e < 1:
                edge = np.maximum(edge, np.exp(-((u - e) * size / 2.5) ** 2))
    noise = rng.normal(0, 1, (size // 8, size // 8))
    noise = np.kron(noise, np.ones((8, 8)))[:size, :size]
    threads = 0.5 + 0.5 * np.sin(u * size * 0.9) * np.sin(v * size * 0.9)
    shade = (1 + weave * (threads - 0.5) * 2 + 0.03 * noise) * (1 + 0.14 * fold)
    col = np.empty((size, size, 3))
    for k in range(3):
        col[..., k] = (base[k] * (1 - trim_mask) + trim[k] * trim_mask) * shade * (1 - 0.35 * edge)
    alpha = np.ones((size, size))
    srgb = mt._linear_to_srgb(np.clip(col, 0, 1))
    px = np.concatenate([srgb, alpha[..., None]], axis=-1).astype(np.float32)
    colour = mt._save(px, Path(f"{path}.jpg"), "sRGB")
    dye = np.zeros((size, size, 4), dtype=np.float32)
    dye[..., 0] = 1 - trim_mask
    dye[..., 1] = trim_mask
    dye[..., 3] = 1
    mt._save(dye, Path(f"{path}_dye.png"), "Non-Color")
    # The normal map: rows here run bottom first, the way tiles.normals' are flipped.
    height = 0.5 + 0.22 * fold + 0.006 * (threads - 0.5) * (1 + 2 * weave)
    nrm = np.ones((size, size, 4), np.float32)
    nrm[..., :3] = tiles.normals(height[::-1], size / 40.0)[::-1]
    mt._save(nrm, Path(f"{path}_normal.png"), "Non-Color")
    return colour


def cloth_material(name, image, rough=0.9):
    """Cloth from cloth_texture's colour map, with its normal map when there is one."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = image
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = rough
    src = Path(bpy.path.abspath(image.filepath))
    npath = src.with_name(f"{src.stem}_normal.png")
    if npath.exists():
        nimg = bpy.data.images.load(str(npath), check_existing=True)
        nimg.colorspace_settings.name = "Non-Color"
        tn = nt.nodes.new("ShaderNodeTexImage")
        tn.image = nimg
        nm = nt.nodes.new("ShaderNodeNormalMap")
        nt.links.new(tn.outputs["Color"], nm.inputs["Color"])
        nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    return m
