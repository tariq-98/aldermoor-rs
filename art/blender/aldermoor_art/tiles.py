"""Tileable surface textures for buildings and props, painted with numpy.

Every pattern is periodic (noise from wrapped Fourier spectra, Voronoi cells measured across the
wrap), so the textures repeat without seams on box-projected UVs. Each surface writes a colour
map (sRGB JPEG), a normal map from its own height field (linear PNG, glTF's +Y-up convention) and
returns its mean roughness, which the material uses as a constant.
"""

from pathlib import Path

import numpy as np

from . import materials as mt


def noise(size, scale, seed, power=2.0):
    """Periodic fractal noise in 0..1: random phases with a 1/f^power falloff above `scale`."""
    rng = np.random.default_rng(seed)
    fx = np.fft.fftfreq(size)[None, :] * size
    fy = np.fft.fftfreq(size)[:, None] * size
    f = np.sqrt(fx * fx + fy * fy)
    amp = np.where(f > 0, 1.0 / np.maximum(f, 1e-6) ** power, 0.0) * (f >= scale * 0.5)
    spec = amp * np.exp(2j * np.pi * rng.random((size, size)))
    n = np.real(np.fft.ifft2(spec))
    n -= n.min()
    return n / max(n.max(), 1e-9)


def stretched(size, sx, sy, seed, power=2.0):
    """Periodic noise stretched: wavelengths `sx` times longer along x than y (grain, straw)."""
    rng = np.random.default_rng(seed)
    fx = np.fft.fftfreq(size)[None, :] * size * sx
    fy = np.fft.fftfreq(size)[:, None] * size * sy
    f = np.sqrt(fx * fx + fy * fy)
    amp = np.where(f > 0, 1.0 / np.maximum(f, 1e-6) ** power, 0.0)
    spec = amp * np.exp(2j * np.pi * rng.random((size, size)))
    n = np.real(np.fft.ifft2(spec))
    n -= n.min()
    return n / max(n.max(), 1e-9)


def voronoi(size, points, seed, aspect=(1.0, 1.0), jitter=1.0, warp=0.0):
    """Periodic Voronoi: (F1 distance, F2 - F1, cell index) per pixel, distances in pixels.

    `warp` (pixels) bends the coordinates with periodic noise first, so cell edges curve the way
    real stones' do instead of running straight.
    """
    rng = np.random.default_rng(seed)
    gx = int(round(np.sqrt(points * aspect[0] / aspect[1])))
    gy = max(1, int(round(points / max(gx, 1))))
    cells = []
    for j in range(gy):
        for i in range(gx):
            cx = (i + 0.5 + (rng.random() - 0.5) * jitter) / gx * size
            cy = (j + 0.5 + (rng.random() - 0.5) * jitter) / gy * size
            cells.append((cx, cy))
    cells = np.array(cells)
    yy, xx = np.mgrid[0:size, 0:size].astype(np.float32)
    if warp:
        xx = (xx + (noise(size, 3, seed + 101) - 0.5) * 2 * warp) % size
        yy = (yy + (noise(size, 3, seed + 102) - 0.5) * 2 * warp) % size
    f1 = np.full((size, size), 1e9, np.float32)
    f2 = np.full((size, size), 1e9, np.float32)
    idx = np.zeros((size, size), np.int32)
    for k, (cx, cy) in enumerate(cells):
        dx = np.abs(xx - cx)
        dy = np.abs(yy - cy)
        dx = np.minimum(dx, size - dx)
        dy = np.minimum(dy, size - dy)
        d = np.sqrt(dx * dx + dy * dy)
        closer = d < f1
        f2 = np.where(closer, f1, np.minimum(f2, d))
        idx = np.where(closer, k, idx)
        f1 = np.where(closer, d, f1)
    return f1, f2 - f1, idx, len(cells)


def normals(height, strength):
    """A tangent-space normal map (glTF: +Y up) from a periodic height field."""
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * 0.5 * strength
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * 0.5 * strength
    n = np.stack([-dx, dy, np.ones_like(height)], -1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    return n * 0.5 + 0.5


def save(out_dir, name, colour_linear, height, strength, rough):
    """Writes `<name>_color.jpg` and `<name>_normal.png`; returns (colour image, normal image, roughness).

    Arrays are top row first; Blender stores images bottom row first, so they are flipped here.
    """
    out_dir = Path(out_dir)
    size = colour_linear.shape[0]
    rgba = np.ones((size, size, 4), np.float32)
    rgba[..., :3] = mt._linear_to_srgb(np.clip(colour_linear, 0, 1))
    col = mt._save(rgba[::-1].copy(), out_dir / f"{name}_color.jpg", "sRGB")
    nrm = np.ones((size, size, 4), np.float32)
    nrm[..., :3] = normals(height, strength)
    nor = mt._save(nrm[::-1].copy(), out_dir / f"{name}_normal.png", "Non-Color")
    return col, nor, float(np.mean(rough))


def _tint(base, k):
    return np.clip(np.asarray(base)[None, None, :] * k[..., None], 0, 1)


# --- Surfaces ---------------------------------------------------------------------------------
# Each covers `metres` of surface per tile (the material's UV scale) and returns
# (linear colour, height, normal strength, roughness map).

def plaster(size=1024, seed=1):
    """Lime plaster: soft mottling, faint trowel strokes, a few hairline cracks."""
    mottle = noise(size, 2, seed)
    strokes = stretched(size, 3.0, 1.0, seed + 1)
    fine = noise(size, 60, seed + 2, power=1.2)
    f1, edge, _, _ = voronoi(size, 14, seed + 3)
    cracks = np.exp(-(edge / 1.2) ** 2) * (noise(size, 6, seed + 4) > 0.62)
    k = 0.9 + 0.14 * mottle + 0.04 * strokes + 0.03 * fine - 0.18 * cracks
    colour = _tint((0.78, 0.72, 0.6), k)
    height = 0.6 * mottle + 0.25 * strokes + 0.4 * fine - 1.5 * cracks
    return colour, height, 2.0, 0.9 + 0.05 * fine


def stone_wall(size=1024, seed=2):
    """Rubble stone in lime mortar: rounded stones of varied size and tone."""
    f1, edge, idx, n = voronoi(size, 34, seed, jitter=0.9, warp=22)
    rng = np.random.default_rng(seed)
    tone = rng.uniform(0.72, 1.15, n)[idx]
    hue = rng.uniform(-0.05, 0.05, n)[idx]
    bump = noise(size, 8, seed + 1)
    mortar = np.clip(1 - edge / 9.0, 0, 1)
    dome = np.clip(edge / 26.0, 0, 1) ** 0.5
    stone = np.stack([0.48 * tone + hue, 0.46 * tone, 0.41 * tone - hue], -1) * (0.85 + 0.25 * bump)[..., None]
    mortar_c = np.array([0.62, 0.59, 0.52])[None, None, :] * (0.9 + 0.1 * bump)[..., None]
    colour = stone * (1 - mortar[..., None]) + mortar_c * mortar[..., None]
    height = dome * 1.0 + 0.25 * bump - 0.6 * mortar
    return colour, height, 9.0, 0.82 + 0.1 * mortar


def planks(size=1024, seed=3, boards=6, base=(0.3, 0.19, 0.1)):
    """Weathered boards running along x, with grain, knots and dark gaps between them."""
    yy = np.mgrid[0:size, 0:size][0] / size
    board = np.floor(yy * boards).astype(int)
    rng = np.random.default_rng(seed)
    tone = rng.uniform(0.8, 1.15, boards)[board]
    grain = stretched(size, 12.0, 1.0, seed + 1, power=1.6)
    fine = stretched(size, 30.0, 1.0, seed + 2, power=1.2)
    within = (yy * boards) % 1.0
    gap = np.clip(1 - np.minimum(within, 1 - within) * boards * size / size / 0.02, 0, 1)
    f1, _, _, _ = voronoi(size, 10, seed + 3)
    knots = np.exp(-(f1 / 9.0) ** 2) * (noise(size, 3, seed + 4) > 0.55)
    k = tone * (0.82 + 0.3 * grain + 0.08 * fine) * (1 - 0.6 * gap) * (1 - 0.35 * knots)
    colour = _tint(base, k)
    height = 0.6 * grain + 0.2 * fine - 2.0 * gap - 0.5 * knots
    return colour, height, 5.0, 0.8 + 0.08 * gap


def thatch(size=1024, seed=4):
    """Straw thatch: stalks running down the slope (y) in overlapping courses with ragged ends."""
    straw = stretched(size, 1.0, 26.0, seed, power=1.1)
    strands = stretched(size, 1.0, 9.0, seed + 3, power=1.4)
    bundles = stretched(size, 1.0, 3.0, seed + 1, power=2.2)
    yy, xx = np.mgrid[0:size, 0:size] / size
    # Course ends wander and fray: the bottom of each course is a ragged line of stalk tips.
    ragged = (stretched(size, 1.0, 20.0, seed + 5, power=1.0) - 0.5) * 0.25 + (noise(size, 4, seed + 6) - 0.5) * 0.2
    course = (yy * 4 + ragged) % 1.0
    lip = np.clip((course - 0.78) / 0.22, 0, 1) ** 1.5
    shade = np.clip(course / 0.25, 0, 1)  # Darker just under the course above.
    k = (0.62 + 0.45 * straw + 0.2 * strands + 0.1 * bundles) * (0.62 + 0.38 * shade) * (1 - 0.3 * lip)
    weather = noise(size, 2, seed + 2)
    colour = _tint((0.58, 0.45, 0.22), k) * (0.82 + 0.25 * weather)[..., None]
    colour = colour * np.array([1.0, 0.98, 0.92])[None, None, :]
    height = 0.9 * straw + 0.4 * strands + 0.5 * bundles + 1.2 * shade - 0.8 * lip
    return colour, height, 9.0, 0.95 + 0.0 * straw


def roof_tiles(size=1024, seed=5, rows=5, cols=4):
    """Clay roof tiles: rounded courses, each tile a little different in tone."""
    yy, xx = np.mgrid[0:size, 0:size] / size
    r = (yy * rows) % 1.0
    c = (xx * cols + 0.5 * (np.floor(yy * rows) % 2)) % 1.0
    rng = np.random.default_rng(seed)
    tid = (np.floor(yy * rows) * cols + np.floor(xx * cols + 0.5 * (np.floor(yy * rows) % 2))).astype(int) % (rows * cols)
    tone = rng.uniform(0.82, 1.12, rows * cols)[tid]
    arch = np.sin(np.pi * c) ** 0.5
    overlap = np.clip((r - 0.86) / 0.14, 0, 1)
    grime = noise(size, 3, seed + 1)
    k = tone * (0.75 + 0.3 * arch) * (1 - 0.45 * overlap) * (0.88 + 0.2 * grime)
    colour = _tint((0.55, 0.24, 0.13), k)
    height = 0.8 * arch - 1.0 * overlap + 0.15 * noise(size, 30, seed + 2)
    return colour, height, 8.0, 0.75 + 0.1 * grime


def cobbles(size=1024, seed=6):
    """Rounded cobbles with soil between, for paths and yards."""
    f1, edge, idx, n = voronoi(size, 60, seed, jitter=0.85, warp=14)
    rng = np.random.default_rng(seed)
    tone = rng.uniform(0.75, 1.1, n)[idx]
    dome = np.clip(edge / 12.0, 0, 1) ** 0.5
    soil = np.clip(1 - edge / 5.0, 0, 1)
    bump = noise(size, 10, seed + 1)
    stone = np.stack([0.5 * tone, 0.48 * tone, 0.44 * tone], -1) * (0.9 + 0.15 * bump)[..., None]
    soil_c = np.array([0.24, 0.18, 0.11])[None, None, :]
    colour = stone * (1 - soil[..., None]) + soil_c * soil[..., None]
    return colour, dome - 0.4 * soil + 0.2 * bump, 8.0, 0.85 + 0.1 * soil


def bark(size=1024, seed=7, base=(0.2, 0.15, 0.1)):
    """Furrowed bark running along y (up the trunk)."""
    ridges = stretched(size, 1.0, 9.0, seed, power=1.5)
    plates = noise(size, 5, seed + 1)
    furrow = np.clip((0.45 - ridges) / 0.2, 0, 1)
    k = (0.75 + 0.35 * ridges + 0.1 * plates) * (1 - 0.45 * furrow)
    colour = _tint(base, k)
    return colour, ridges + 0.3 * plates - 0.8 * furrow, 9.0, 0.9 + 0.0 * plates


def rock(size=1024, seed=8):
    """Weathered rock: broad tonal patches, granular speckle and a few wandering cracks."""
    broad = noise(size, 2, seed)
    mid = noise(size, 7, seed + 6)
    grit = noise(size, 120, seed + 1, power=0.8)
    speck = (noise(size, 200, seed + 7, power=0.5) > 0.72).astype(np.float32)
    strata = stretched(size, 4.0, 1.0, seed + 4, power=1.6)
    f1, edge, _, _ = voronoi(size, 7, seed + 2, warp=60)
    crack = np.exp(-(edge / 1.6) ** 2) * (noise(size, 3, seed + 3) > 0.58)
    k = (0.62 + 0.5 * broad + 0.2 * mid + 0.12 * grit + 0.1 * strata - 0.08 * speck) * (1 - 0.5 * crack)
    colour = _tint((0.2, 0.19, 0.17), k)
    colour = colour * (1 + 0.1 * (noise(size, 2, seed + 5)[..., None] - 0.5) * np.array([1.0, 0.6, -0.4])[None, None, :])
    return colour, 0.8 * broad + 0.35 * mid + 0.25 * grit - 1.4 * crack, 8.0, 0.86 + 0.06 * grit


def foliage(size=1024, seed=9, base=(0.13, 0.25, 0.06), count=1500):
    """Dense leaves: pointed leaves laid over one another in varied greens, each with a midrib,
    darker at its stalk and lighter towards its tip, deep shade in the gaps. Tiles seamlessly."""
    rng = np.random.default_rng(seed)
    base = np.array(base)
    colour = np.empty((size, size, 3))
    colour[:] = base * 0.32
    height = np.zeros((size, size))
    clumps = noise(size, 3, seed + 1)
    for k in range(count):
        cx, cy = rng.uniform(0, size, 2)
        L = rng.uniform(22, 34)
        W = L * rng.uniform(0.36, 0.48)
        a = rng.uniform(0, 2 * np.pi)
        ca, sa = np.cos(a), np.sin(a)
        r = int(L + 2)
        xs = np.arange(int(cx) - r, int(cx) + r + 1)
        ys = np.arange(int(cy) - r, int(cy) + r + 1)
        X, Y = np.meshgrid(xs, ys)
        dx, dy = X - cx, Y - cy
        u = dx * ca + dy * sa
        v = -dx * sa + dy * ca
        t = np.clip((u / L + 1) / 2, 0, 1)
        half = W * np.sin(np.pi * t) ** 0.75 * (1 - 0.3 * t)
        inside = (np.abs(u) < L) & (np.abs(v) < half)
        if not inside.any():
            continue
        across = np.abs(v) / np.maximum(half, 1e-6)
        tone = rng.uniform(0.8, 1.2)
        tint = base * tone + np.array((0.05, 0.03, -0.01)) * rng.uniform(-1, 1.4)
        shade = (0.62 + 0.42 * t) * (1 - 0.18 * across ** 2) * (1 + 0.12 * np.exp(-(v / 1.3) ** 2))
        yi, xi = Y[inside] % size, X[inside] % size
        colour[yi, xi] = np.clip(tint[None, :] * shade[inside][:, None], 0, 1)
        height[yi, xi] = k / count * 0.6 + 0.3 * t[inside] + 0.1 * (1 - across[inside] ** 2)
    colour *= (0.78 + 0.4 * clumps)[..., None]
    return colour, height, 9.0, 0.78 + 0.0 * height


def needles(size=1024, seed=10):
    """Conifer needles: fine strokes in tufts, blue-green."""
    strokes = stretched(size, 1.0, 10.0, seed, power=1.0)
    tufts = noise(size, 6, seed + 1)
    k = (0.6 + 0.5 * strokes) * (0.7 + 0.4 * tufts)
    colour = _tint((0.07, 0.16, 0.08), k)
    return colour, strokes + 0.5 * tufts, 6.0, 0.85 + 0.0 * k


SURFACES = {
    "plaster": (plaster, 2.0),
    "stone": (stone_wall, 2.0),
    "planks": (planks, 1.6),
    "planks_dark": (lambda: planks(seed=13, base=(0.17, 0.1, 0.055)), 1.6),
    "thatch": (thatch, 2.0),
    "roof_tiles": (roof_tiles, 1.2),
    "cobbles": (cobbles, 2.0),
    "bark": (bark, 1.2),
    "rock": (rock, 2.5),
    "foliage": (foliage, 1.5),
    "needles": (needles, 1.2),
}
