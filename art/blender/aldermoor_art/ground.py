"""The Brambleford valley's ground, river and sky, painted with numpy.

Four tileable ground layers (lush grass, dry meadow, packed earth and weathered limestone), the
weights that blend them across the valley, the river's ripples, and an equirectangular sky.
The weights come from the world crate's own height, slope, roads and river
(crates/world/examples/ground_fields.rs), so the paths are worn exactly where the game's roads
run and the rock shows exactly where the ground is steep.

Conventions:
- Colours are linear until they're written; ground and sky colour maps are sRGB JPEGs.
- Normal maps follow glTF (+Y toward the top of the image).
- Ground layers tile. The weights cover the valley once: row 0 is its north edge (z = −280),
  column 0 its west edge, matching the renderer's `bounds` mapping.
- The sky maps direction (x, y, z) to u = atan2(z, x) / 2π + ½ and v = acos(y) / π, as the
  renderer samples it, so its sun sits where the look's `sun_direction` points.
"""

import json
from pathlib import Path

import numpy as np

from .tiles import noise, normals, stretched, voronoi


def _smooth(a, b, x):
    t = np.clip((x - a) / (b - a), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def _window(cx, cy, r):
    xs = np.arange(int(np.floor(cx)) - r, int(np.floor(cx)) + r + 2)
    ys = np.arange(int(np.floor(cy)) - r, int(np.floor(cy)) + r + 2)
    return np.meshgrid(xs, ys)


# --- Grass ---------------------------------------------------------------------------------------

def _blades(size, seed, count, palette, length, width, soil, lean_spread=0.7, clump_scale=5):
    """Grass seen from above: tapered blades over dark soil, thicker in clumps whose blades lean
    the same way, each darker at its base and lighter towards its tip. Returns (colour, height)."""
    rng = np.random.default_rng(seed)
    colour = np.empty((size, size, 3))
    colour[:] = soil
    height = np.zeros((size, size))
    clumps = noise(size, clump_scale, seed + 1)
    lean = noise(size, 3, seed + 2) * 4 * np.pi
    tone = noise(size, 7, seed + 3)
    palette = np.asarray(palette, np.float64)
    for k in range(count):
        cx, cy = rng.uniform(0, size, 2)
        ix, iy = int(cx) % size, int(cy) % size
        dense = clumps[iy, ix]
        if rng.random() > 0.3 + 0.7 * dense:
            continue
        L = rng.uniform(*length) * (0.75 + 0.5 * dense)
        W = rng.uniform(*width)
        a = lean[iy, ix] + rng.normal(0.0, lean_spread)
        ca, sa = np.cos(a), np.sin(a)
        X, Y = _window(cx, cy, int(L + W + 2))
        dx, dy = X - cx, Y - cy
        u = dx * ca + dy * sa
        v = -dx * sa + dy * ca
        t = np.clip(u / L, 0.0, 1.0)
        half = 0.5 * W * (1.0 - t) ** 0.7 + 0.25
        cover = np.clip(half - np.abs(v) + 0.5, 0, 1) * np.clip(u + 0.5, 0, 1) * np.clip(L - u + 0.5, 0, 1)
        hit = cover > 0.01
        if not hit.any():
            continue
        i, j = rng.integers(len(palette), size=2)
        base = palette[i] + (palette[j] - palette[i]) * rng.random()
        base = base * (0.8 + 0.4 * tone[iy, ix]) * rng.uniform(0.85, 1.15)
        shade = (0.5 + 0.65 * t[hit]) * (1.0 - 0.2 * (np.abs(v[hit]) / np.maximum(half[hit], 1e-6)) ** 2)
        c = cover[hit][:, None]
        yi, xi = Y[hit] % size, X[hit] % size
        colour[yi, xi] = colour[yi, xi] * (1 - c) + np.clip(base[None, :] * shade[:, None], 0, 1) * c
        h = k / count * 0.35 + 0.45 + 0.55 * t[hit]
        height[yi, xi] = height[yi, xi] * (1 - c[:, 0]) + h * c[:, 0]
    return colour, height


def _flowers(colour, height, seed, count, kinds):
    """Scatters small flowers: (petal colour, centre colour, radius in pixels, share)."""
    size = colour.shape[0]
    rng = np.random.default_rng(seed)
    shares = np.array([k[3] for k in kinds], np.float64)
    shares /= shares.sum()
    for _ in range(count):
        petal, centre, radius, _ = kinds[rng.choice(len(kinds), p=shares)]
        r = radius * rng.uniform(0.8, 1.25)
        cx, cy = rng.uniform(0, size, 2)
        X, Y = _window(cx, cy, int(r + 2))
        dx, dy = X - cx, Y - cy
        d = np.sqrt(dx * dx + dy * dy)
        petals = 1.0 + 0.25 * np.cos(5.0 * np.arctan2(dy, dx) + rng.uniform(0, 6.3))
        cover = np.clip(r * petals - d + 0.5, 0, 1)
        hit = cover > 0.01
        yi, xi = Y[hit] % size, X[hit] % size
        c = np.where(d[hit] < r * 0.35, 1.0, 0.0)[:, None]
        tint = np.asarray(petal) * (1 - c) + np.asarray(centre) * c
        a = cover[hit][:, None]
        colour[yi, xi] = colour[yi, xi] * (1 - a) + tint * a * rng.uniform(0.85, 1.1)
        height[yi, xi] = np.maximum(height[yi, xi], 1.05 * cover[hit])


def grass(size=1024, seed=31):
    """Lush turf for the valley floor: dense green blades in clumps, the odd clover flower."""
    palette = [(0.035, 0.085, 0.018), (0.06, 0.14, 0.025), (0.09, 0.19, 0.035), (0.12, 0.22, 0.045)]
    colour, height = _blades(size, seed, 115000, palette, (9, 22), (1.6, 2.8), (0.016, 0.026, 0.01))
    _flowers(colour, height, seed + 9, 40, [((0.62, 0.62, 0.56), (0.5, 0.36, 0.03), 3.0, 1.0)])
    return colour, height, 5.0, 0.9 + 0.0 * height


def meadow(size=1024, seed=37):
    """Drier meadow grass on higher ground and trodden verges: straw and olive blades,
    buttercups, daisies and clover."""
    palette = [(0.13, 0.13, 0.04), (0.18, 0.16, 0.055), (0.09, 0.13, 0.03), (0.06, 0.1, 0.022)]
    colour, height = _blades(size, seed, 100000, palette, (11, 27), (1.5, 2.6), (0.03, 0.03, 0.014), lean_spread=0.9)
    _flowers(colour, height, seed + 9, 260, [
        ((0.6, 0.4, 0.02), (0.45, 0.28, 0.01), 2.6, 3.0),
        ((0.7, 0.7, 0.63), (0.55, 0.38, 0.03), 3.0, 2.0),
        ((0.24, 0.09, 0.3), (0.3, 0.12, 0.35), 2.4, 1.0),
    ])
    return colour, height, 5.0, 0.92 + 0.0 * height


# --- Earth and rock ------------------------------------------------------------------------------

def _cells(size, points, seed, jitter=0.9, warp=0.0, aspect=1.0):
    """Periodic Voronoi like tiles.voronoi, plus each pixel's offset (x, y, in pixels, across
    the wrap) from its cell's centre, so cells can carry tilted faces and still tile. `aspect`
    above 1 makes the cells that much wider than tall."""
    rng = np.random.default_rng(seed)
    gx = max(1, int(round(np.sqrt(points / aspect))))
    gy = max(1, int(round(points / gx)))
    centres = np.array([((i + 0.5 + (rng.random() - 0.5) * jitter) / gx * size,
                         (j + 0.5 + (rng.random() - 0.5) * jitter) / gy * size) for j in range(gy) for i in range(gx)])
    yy, xx = np.mgrid[0:size, 0:size].astype(np.float64)
    if warp:
        xx = (xx + (noise(size, 3, seed + 101) - 0.5) * 2 * warp) % size
        yy = (yy + (noise(size, 3, seed + 102) - 0.5) * 2 * warp) % size
    f1 = np.full((size, size), 1e9)
    f2 = np.full((size, size), 1e9)
    idx = np.zeros((size, size), np.int64)
    ox = np.zeros((size, size))
    oy = np.zeros((size, size))
    for k, (cx, cy) in enumerate(centres):
        dx = (xx - cx + size / 2) % size - size / 2
        dy = (yy - cy + size / 2) % size - size / 2
        d = np.sqrt(dx * dx + dy * dy)
        closer = d < f1
        f2 = np.where(closer, f1, np.minimum(f2, d))
        idx = np.where(closer, k, idx)
        ox = np.where(closer, dx, ox)
        oy = np.where(closer, dy, oy)
        f1 = np.where(closer, d, f1)
    return f1, f2 - f1, idx, len(centres), ox, oy


def _spots(colour, height, seed, count, radius, tints, alpha):
    """Blotchy crusts (lichen and the like): soft-edged irregular spots of the given tints."""
    size = colour.shape[0]
    rng = np.random.default_rng(seed)
    tints = np.asarray(tints)
    for _ in range(count):
        r = rng.uniform(*radius)
        cx, cy = rng.uniform(0, size, 2)
        X, Y = _window(cx, cy, int(r * 1.4 + 2))
        dx, dy = X - cx, Y - cy
        a = np.arctan2(dy, dx)
        edge = r * (1.0 + 0.22 * np.sin(3 * a + rng.uniform(0, 6.3)) + 0.12 * np.sin(7 * a + rng.uniform(0, 6.3)))
        cover = np.clip((edge - np.sqrt(dx * dx + dy * dy)) / 1.5, 0, 1) * alpha
        hit = cover > 0.01
        yi, xi = Y[hit] % size, X[hit] % size
        c = cover[hit][:, None]
        tint = tints[rng.integers(len(tints))] * rng.uniform(0.85, 1.15)
        colour[yi, xi] = colour[yi, xi] * (1 - c) + tint[None, :] * c
        height[yi, xi] += 0.08 * cover[hit]


def _pebbles(colour, height, seed, count, radius, tones):
    """Small stones pressed into the ground, each with a darker rim where it sits in the soil."""
    size = colour.shape[0]
    rng = np.random.default_rng(seed)
    tones = np.asarray(tones)
    for _ in range(count):
        r = rng.uniform(*radius) * (1.0 + 2.0 * (rng.random() < 0.04))
        e = rng.uniform(0.55, 1.0)
        a = rng.uniform(0, np.pi)
        cx, cy = rng.uniform(0, size, 2)
        X, Y = _window(cx, cy, int(r + 3))
        dx, dy = X - cx, Y - cy
        u = (dx * np.cos(a) + dy * np.sin(a)) / r
        v = (-dx * np.sin(a) + dy * np.cos(a)) / (r * e)
        d = np.sqrt(u * u + v * v)
        wobble = 1.0 + 0.12 * np.sin(3.0 * np.arctan2(v, u) + rng.uniform(0, 6.3))
        stone = np.clip((wobble - d) * r + 0.5, 0, 1)
        rim = np.clip(1.0 - np.abs(d - wobble * 1.08) * 4.0, 0, 1) * (1 - stone)
        hit = (stone > 0.01) | (rim > 0.01)
        yi, xi = Y[hit] % size, X[hit] % size
        tone = tones[rng.integers(len(tones))] * rng.uniform(0.8, 1.15)
        lit = 0.8 + 0.35 * np.clip(-u[hit] * 0.5 - v[hit] * 0.5 + 0.5, 0, 1)
        s = stone[hit][:, None]
        colour[yi, xi] = colour[yi, xi] * (1 - s) * (1 - 0.35 * rim[hit][:, None]) + (tone[None, :] * lit[:, None]) * s
        dome = np.sqrt(np.clip(1.0 - (d[hit] / wobble[hit]) ** 2, 0, 1))
        height[yi, xi] = height[yi, xi] * (1 - s[:, 0]) + (0.55 + 0.6 * dome) * s[:, 0] - 0.12 * rim[hit]


def earth(size=1024, seed=41):
    """Packed earth for roads, yards and riverbanks: dusty and damp patches, fine grain, a few
    soft cracks, and gravel and pebbles pressed in."""
    broad = noise(size, 2, seed)
    mid = noise(size, 8, seed + 1)
    fine = noise(size, 60, seed + 2, power=1.2)
    grain = noise(size, 400, seed + 3, power=0.4)
    dust = _smooth(0.5, 0.78, 0.7 * broad + 0.3 * mid)
    damp = _smooth(0.42, 0.15, broad)
    k = (0.78 + 0.28 * mid + 0.14 * fine + 0.2 * (grain - 0.5))[..., None]
    colour = np.array((0.16, 0.11, 0.066))[None, None, :] * k
    colour = colour * (1 - 0.45 * dust[..., None]) + np.array((0.25, 0.2, 0.135))[None, None, :] * k * 0.45 * dust[..., None]
    colour *= (1.0 - 0.3 * damp)[..., None]
    _, edge, _, _ = voronoi(size, 30, seed + 4, warp=50)
    crack = np.exp(-(edge / 0.9) ** 2) * (noise(size, 6, seed + 5) > 0.62)
    colour *= (1.0 - 0.3 * crack)[..., None]
    height = 0.55 * mid + 0.3 * fine + 0.2 * grain - 0.35 * crack
    gravel = [(0.2, 0.19, 0.17), (0.26, 0.22, 0.17), (0.15, 0.13, 0.11), (0.3, 0.27, 0.22), (0.22, 0.16, 0.1)]
    _pebbles(colour, height, seed + 6, 2600, (0.9, 2.0), gravel)
    _pebbles(colour, height, seed + 7, 420, (2.2, 5.0), gravel)
    return colour, height, 6.0, 0.9 - 0.15 * damp


def _partings(size, seed, spacing):
    """Distance (pixels) from each pixel to the nearest bedding parting: near-horizontal lines at
    uneven spacing (`spacing` = (min, max) pixels) that undulate along their length."""
    rng = np.random.default_rng(seed)
    gaps = []
    while sum(gaps) < size:
        gaps.append(rng.uniform(*spacing))
    lines = np.cumsum(np.array(gaps) * size / sum(gaps))
    yy = np.mgrid[0:size, 0:size][0].astype(np.float64)
    wave = (noise(size, 2, seed + 1) - 0.5) * 70 + (noise(size, 9, seed + 2) - 0.5) * 16
    y = (yy + wave) % size
    d = np.full((size, size), 1e9)
    for line in lines:
        dy = np.abs(y - line)
        d = np.minimum(d, np.minimum(dy, size - dy))
    return d


def limestone(size=1024, seed=47):
    """Weathered limestone for the ledges and steep slopes: irregular blocks, wider than tall,
    split by joints and by bedding partings that undulate and fade in and out. Each block is
    toned and tilted a little differently and its edges are broken and worn; moss and dirt
    gather in the partings, rain streaks run down, and there are pits and lichen. Reads as a
    bedded cliff from the side and as limestone pavement from above."""
    rng = np.random.default_rng(seed)
    _, edge, idx, n, ox, oy = _cells(size, 30, seed, jitter=1.0, warp=45, aspect=2.4)
    tone = rng.uniform(0.82, 1.12, n)
    tilt = rng.normal(0.0, 1.0, (n, 2)) * 0.006
    broad = noise(size, 2, seed + 3)
    mid = noise(size, 9, seed + 4)
    mottle = noise(size, 22, seed + 5)
    grit = noise(size, 160, seed + 6, power=0.7)
    rain = stretched(size, 1.0, 12.0, seed + 7, power=1.2)
    parting = _partings(size, seed + 20, (80, 200))
    bed = np.exp(-(parting / (2.2 + 2.8 * noise(size, 6, seed + 21))) ** 2) * _smooth(0.22, 0.5, noise(size, 3, seed + 22))
    joint = np.exp(-(edge / (1.2 + 1.8 * noise(size, 5, seed + 23))) ** 2) * _smooth(0.2, 0.45, noise(size, 4, seed + 24))
    crack = np.maximum(bed, joint)
    chip = noise(size, 40, seed + 25)
    worn = np.exp(-(np.minimum(edge, parting) / (5.0 + 8.0 * chip)) ** 2)
    pits = _smooth(0.16, 0.06, noise(size, 110, seed + 8, power=1.0))
    k = tone[idx] * (0.8 + 0.2 * broad + 0.12 * mid + 0.16 * (mottle - 0.5) + 0.12 * grit)
    k = k * (1.0 - 0.66 * crack) * (1.0 - 0.22 * worn) * (1.0 - 0.28 * pits)
    k = k * (1.0 - 0.16 * _smooth(0.55, 0.82, rain))
    colour = np.array((0.245, 0.228, 0.195))[None, None, :] * k[..., None]
    iron = (_smooth(0.62, 0.86, noise(size, 4, seed + 9)) * 0.3)[..., None]
    colour = colour * (1 - iron) + np.array((0.28, 0.2, 0.115))[None, None, :] * k[..., None] * iron
    # Grime in the partings and joints, with moss here and there.
    mossy = _smooth(0.45, 0.7, noise(size, 5, seed + 11))[..., None]
    grime = np.array((0.06, 0.055, 0.042))[None, None, :] * (1 - mossy) + np.array((0.05, 0.068, 0.028))[None, None, :] * mossy
    dirt = (crack * 0.75)[..., None]
    colour = colour * (1 - dirt) + grime * dirt
    lichen = (_smooth(0.62, 0.74, noise(size, 12, seed + 12)) * (0.5 + 0.5 * grit) * 0.45)[..., None]
    colour = colour * (1 - lichen) + np.array((0.2, 0.22, 0.085))[None, None, :] * lichen
    height = 0.5 * tone[idx] + tilt[idx, 0] * ox + tilt[idx, 1] * oy + 0.18 * mid + 0.1 * mottle + 0.1 * grit
    height = height - 1.0 * bed - 0.8 * joint - 0.4 * worn - 0.25 * pits
    _spots(colour, height, seed + 13, 60, (3.0, 10.0), [(0.2, 0.22, 0.08), (0.2, 0.22, 0.08), (0.34, 0.34, 0.3)], 0.32)
    return colour, height, 8.0, 0.84 + 0.06 * grit


LAYERS = {
    # name: (painter, metres per tile, roughness)
    "grass": (grass, 4.0, 0.9),
    "meadow": (meadow, 5.0, 0.92),
    "earth": (earth, 3.5, 0.88),
    "limestone": (limestone, 6.0, 0.82),
}


# --- The river -----------------------------------------------------------------------------------

def ripples(size=1024, seed=53):
    """Small river ripples: smooth swells with finer chop on top. Returns a height field."""
    swell = noise(size, 4, seed, power=2.4)
    chop = noise(size, 24, seed + 1, power=1.8)
    fine = noise(size, 90, seed + 2, power=1.4)
    return 0.6 * swell + 0.3 * chop + 0.12 * fine


# --- The valley's weights ------------------------------------------------------------------------

def _box_distance(X, Z, centre, half, yaw):
    """Distance from each point to a box turned by `yaw` (as world::Solid turns it), 0 inside."""
    dx, dz = X - centre[0], Z - centre[1]
    s, c = np.sin(yaw), np.cos(yaw)
    lx = np.abs(c * dx - s * dz) - half[0]
    lz = np.abs(s * dx + c * dz) - half[1]
    return np.sqrt(np.maximum(lx, 0) ** 2 + np.maximum(lz, 0) ** 2)


def weights(fields, places, sun, seed=61):
    """Layer weights (grass, meadow, earth, limestone) for every texel of the valley."""
    size = places["size"]
    bounds = places["bounds"]
    planes = np.fromfile(fields, "<f4").reshape(6, size, size).astype(np.float64)
    h, nx, ny, nz, path, river = planes
    step = 2.0 * bounds / size
    axis = -bounds + (np.arange(size) + 0.5) * step
    X, Z = np.meshgrid(axis, axis)

    # Outcrops only break through where the ground already tilts a little, not on flat tops.
    def slope_ok(sl):
        return _smooth(0.02, 0.06, sl)

    grain = noise(size, 280, seed, power=1.0)
    fine = noise(size, 50, seed + 1)
    mid = noise(size, 12, seed + 2)
    macro = noise(size, 3, seed + 3)
    spots = noise(size, 90, seed + 4)
    slope = 1.0 - ny

    # Rock: steep ground, the odd outcrop on the high tops, and stones in the riverbed.
    rock = _smooth(0.1, 0.22, slope + 0.05 * (fine - 0.5) + 0.03 * (grain - 0.5))
    # Low riverbanks stay soft, but where the river cuts through the hills the gorge walls are rock.
    rock *= np.maximum(_smooth(10.0, 16.0, np.abs(river) + 2.0 * (fine - 0.5)), _smooth(9.0, 15.0, h))
    rock += _smooth(0.74, 0.84, fine) * _smooth(30.0, 45.0, h) * slope_ok(slope) * 0.6
    rock += (np.abs(river) < 6.0) * _smooth(0.62, 0.74, fine) * 0.35
    rock = np.clip(rock, 0, 1)

    # Earth: the roads and the square, the riverbanks, yards round the houses, bare ground
    # under the trees, and the odd bare patch.
    road = 1.0 - _smooth(1.2, 2.3, path + 0.35 * (grain - 0.5) + 0.25 * (fine - 0.5))
    verge = 1.0 - _smooth(1.8, 4.2, path + 0.6 * (fine - 0.5))
    bank = 1.0 - _smooth(6.0, 9.5, np.abs(river) + 1.2 * (fine - 0.5) + 0.6 * (grain - 0.5))
    yard = np.zeros_like(h)
    for x, z, yaw, scale in places["houses"]:
        d = _box_distance(X, Z, (x, z), (3.5 * scale, 3.1 * scale), yaw)
        yard = np.maximum(yard, 1.0 - _smooth(0.3, 2.4, d + 0.8 * (grain - 0.5)))
    litter = np.zeros_like(h)
    canopy = np.zeros_like(h)
    for x, z, _, scale in places["trees"]:
        d = np.sqrt((X - x) ** 2 + (Z - z) ** 2)
        litter = np.maximum(litter, 1.0 - _smooth(0.8 * scale, 2.0 * scale, d + 0.5 * (grain - 0.5)))
        canopy = np.maximum(canopy, 1.0 - _smooth(2.0 * scale, 4.0 * scale, d))
    bare = _smooth(0.8, 0.88, spots) * 0.55
    dirt = np.maximum.reduce([road, bank * 0.9, yard * 0.75, litter * 0.55, bare]) * (1.0 - rock)

    # Meadow or lush grass: drier on high ground and slopes facing the sun, lusher by the river,
    # trodden along the roads and shaded under the trees.
    facing = np.clip((nx * sun[0] + nz * sun[2]) / max(np.hypot(sun[0], sun[2]), 1e-6) * 3.0, -1, 1)
    dry = 0.62 * macro + 0.38 * mid + 0.2 * _smooth(8.0, 30.0, h) + 0.08 * facing - 0.3 * (1.0 - _smooth(8.0, 22.0, np.abs(river)))
    patches = _smooth(0.6, 0.76, noise(size, 25, seed + 6)) * 0.75
    meadow_share = np.maximum.reduce([_smooth(0.52, 0.74, dry), patches, verge * 0.85, canopy * 0.6])
    rest = np.clip(1.0 - rock - dirt, 0, 1)
    out = np.stack([rest * (1.0 - meadow_share), rest * meadow_share, dirt, rock], -1)
    out /= np.maximum(out.sum(-1, keepdims=True), 1e-6)
    return out


# --- The sky -------------------------------------------------------------------------------------

def _sample(field, u, v):
    """Bilinear lookup in a periodic square field at continuous texel coordinates."""
    n = field.shape[0]
    x0 = np.floor(u).astype(np.int64)
    y0 = np.floor(v).astype(np.int64)
    fx, fy = u - x0, v - y0
    x0 %= n
    y0 %= n
    x1, y1 = (x0 + 1) % n, (y0 + 1) % n
    return (field[y0, x0] * (1 - fx) * (1 - fy) + field[y0, x1] * fx * (1 - fy)
            + field[y1, x0] * (1 - fx) * fy + field[y1, x1] * fx * fy)


SKY = {
    "zenith": (0.11, 0.26, 0.66),
    "horizon": (0.5, 0.63, 0.78),
    "warm": (0.98, 0.78, 0.55),
    "below": (0.3, 0.36, 0.38),
    "cloud_lit": (1.0, 0.97, 0.92),
    "cloud_shade": (0.5, 0.56, 0.66),
    "sun": (1.0, 0.95, 0.85),
}


def sky(width, height, sun, seed=71, cover=0.56):
    """A fair-weather sky: blue overhead fading to pale haze at the horizon, the sun's glow and
    disc, and scattered cumulus lit from the sun's side. Linear RGB, `height` rows (row 0 the
    zenith) by `width` columns."""
    sun = np.asarray(sun, np.float64)
    sun /= np.linalg.norm(sun)
    col = {k: np.asarray(v, np.float64) for k, v in SKY.items()}
    n = 1024
    shapes = 0.55 * noise(n, 3, seed) + 0.3 * noise(n, 9, seed + 1) + 0.15 * noise(n, 36, seed + 2)
    shapes = (shapes - shapes.min()) / (shapes.max() - shapes.min())
    wisps = noise(n, 60, seed + 3, power=1.4)
    sun_flat = np.array([sun[0], sun[2]])
    if np.linalg.norm(sun_flat) > 1e-6:
        sun_flat /= np.linalg.norm(sun_flat)
    out = np.empty((height, width, 3), np.float32)
    phi = ((np.arange(width) + 0.5) / width - 0.5) * 2.0 * np.pi
    for r0 in range(0, height, 128):
        rows = np.arange(r0, min(r0 + 128, height))
        theta = (rows + 0.5) / height * np.pi
        T, P = np.meshgrid(theta, phi, indexing="ij")
        d = np.stack([np.cos(P) * np.sin(T), np.cos(T), np.sin(P) * np.sin(T)], -1)
        y = d[..., 1]
        mu = np.clip(d @ sun, -1, 1)
        up = np.clip(y, 0, 1)
        base = col["horizon"] + (col["zenith"] - col["horizon"]) * (up ** 0.55)[..., None]
        glow = (0.35 * np.exp(-(1 - mu) * 7.0) * (1 - up) ** 2)[..., None] * col["warm"]
        halo = (0.5 * np.exp(-(1 - mu) * 90.0) + 1.2 * np.exp(-(1 - mu) * 1400.0))[..., None] * col["sun"]
        c = base + glow + halo
        below = _smooth(0.0, -0.2, y)[..., None]
        c = c * (1 - below) + col["below"] * below
        # Clouds on a dome: flatter towards the horizon, thinning out before it.
        scale = 140.0
        k = scale / (np.maximum(y, 0.0) + 0.1)
        u, v = d[..., 0] * k, d[..., 2] * k
        dens = _smooth(cover, cover + 0.2, _sample(shapes, u, v) + 0.08 * (_sample(wisps, u * 2, v * 2) - 0.5))
        dens *= _smooth(0.03, 0.2, y)
        ahead = _sample(shapes, u + sun_flat[0] * 9.0, v + sun_flat[1] * 9.0)
        lit = 1.0 - np.clip((ahead - _sample(shapes, u, v)) * 5.0, 0, 1) * 0.65
        lining = 0.25 * np.exp(-(1 - mu) * 12.0)
        cloud = col["cloud_shade"] + (col["cloud_lit"] - col["cloud_shade"]) * np.clip(lit + lining, 0, 1)[..., None]
        cloud = cloud * (1 - 0.25 * (1 - up)[..., None]) + c * 0.25 * (1 - up)[..., None]
        c = c * (1 - 0.92 * dens[..., None]) + cloud * 0.92 * dens[..., None]
        out[rows] = c.astype(np.float32)
    return out


def write_layer(name, colour, height, strength, out_dir, save_colour):
    """Writes a ground layer's colour (sRGB JPEG, via Blender) and normal map (exact PNG)."""
    from . import png
    colour_path = Path(out_dir) / f"{name}_colour.jpg"
    normal_path = Path(out_dir) / f"{name}_normal.png"
    save_colour(colour, colour_path)
    png.write(normal_path, normals(height, strength))
    return colour_path, normal_path


def look(sun):
    """The valley's light: a warm late-morning sun, cool sky fill, pale haze, filmic grading."""
    return {
        "sun_direction": [round(float(v), 4) for v in sun],
        "sun_colour": [1.0, 0.9, 0.78],
        "sun_intensity": 3.2,
        "ambient": [0.17, 0.21, 0.29],
        "sky": list(SKY["horizon"]),
        "fog_colour": list(SKY["horizon"]),
        "fog_density": 0.0013,
        "exposure": 1.0,
        # A gentle saturation lift (8%) around Rec. 709 luminance.
        "grade": [[1.063, -0.057, -0.006], [-0.017, 1.023, -0.006], [-0.017, -0.057, 1.074]],
        "tone_map": "aces",
        "wind_strength": 1.0,
    }


def surfaces(layer_files, weights_file, water_normal, sky_file, bounds):
    """The renderer's surface inputs (render::surfaces), paths relative to the assets root."""
    return {
        "terrain": {
            "layers": [
                {"colour": c, "normal": n, "uv_metres": LAYERS[name][1], "roughness": LAYERS[name][2]}
                for name, (c, n) in layer_files.items()
            ],
            "weights": weights_file,
            "bounds": [-bounds, -bounds, bounds, bounds],
        },
        "water": {
            "normal": water_normal,
            "uv_metres": 6.0,
            "normal_strength": 0.55,
            "flow": [0.35, 0.06],
            "deep_colour": [0.012, 0.045, 0.05],
            "shallow_colour": [0.07, 0.15, 0.12],
            "absorption": [0.6, 0.25, 0.18],
            "roughness": 0.06,
            "opacity": 0.9,
            "reflection_strength": 0.6,
            "ior": 1.333,
        },
        "sky": {"texture": sky_file, "rotation_radians": 0.0, "intensity": 1.0},
    }


def places(path):
    return json.loads(Path(path).read_text())
