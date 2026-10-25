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


def stretched(size, sx, sy, seed, power=2.0, scale=0.0):
    """Periodic noise stretched: wavelengths `sx` times longer along x than y (grain, straw).
    As in `noise`, `scale` leaves out features broader than the tile over `scale` / 2."""
    rng = np.random.default_rng(seed)
    cx = np.fft.fftfreq(size)[None, :] * size
    cy = np.fft.fftfreq(size)[:, None] * size
    fx, fy = cx * sx, cy * sy
    f = np.sqrt(fx * fx + fy * fy)
    amp = np.where(f > 0, 1.0 / np.maximum(f, 1e-6) ** power, 0.0) * (np.hypot(cx, cy) >= scale * 0.5)
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


def _course_cells(size, seed, row_px, length_px, warp):
    """Irregular stones laid in courses, wrapping on every side: a periodic Voronoi of seeds laid
    along rows `row_px` (low, high) tall and spaced `length_px` along them, measured in a frame
    squashed along x so the cells come out long and low, with outlines bent by up to `warp` pixels.

    Returns, per pixel: the distance in pixels to the nearest joint, which way is away from it
    (unit x, y; y runs down the image), the stone's index and how many stones there are.
    """
    rng = np.random.default_rng(seed)
    heights = []
    while sum(heights) < size:
        heights.append(rng.uniform(*row_px))
    heights = np.array(heights) * size / sum(heights)
    tops = np.concatenate([[0.0], np.cumsum(heights)])
    pts = []
    for r, h in enumerate(heights):
        lengths = []
        while sum(lengths) < size:
            lengths.append(rng.uniform(*length_px))
        lengths = np.array(lengths) * size / sum(lengths)
        x0 = rng.uniform(0, size)
        for x, l in zip(np.cumsum(lengths) - lengths / 2, lengths):
            pts.append(((x0 + x) % size, tops[r] + h * (0.5 + rng.uniform(-0.12, 0.12))))
    stretch = np.mean(length_px) / np.mean(row_px)
    reach = (int(length_px[1] * 1.6 + warp), int(row_px[1] * 1.6 + warp))
    # The outlines bend smoothly: every pixel measures from a slightly displaced point.
    wx = ((noise(size, 7, seed + 101) - 0.5) * 2 * warp + (noise(size, 30, seed + 103) - 0.5) * 0.4 * warp).astype(np.float32)
    wy = ((noise(size, 7, seed + 102) - 0.5) * 2 * warp + (noise(size, 30, seed + 104) - 0.5) * 0.4 * warp).astype(np.float32)
    f1 = np.full((size, size), 1e9, np.float32)
    index = np.zeros((size, size), np.int32)
    windows = []
    for k, (cx, cy) in enumerate(pts):
        xs = np.arange(int(cx) - reach[0], int(cx) + reach[0] + 1)
        ys = np.arange(int(cy) - reach[1], int(cy) + reach[1] + 1)
        win = np.ix_(ys % size, xs % size)
        px, py = xs[None, :] + wx[win], ys[:, None] + wy[win]
        d = np.sqrt(((px - cx) / stretch) ** 2 + (py - cy) ** 2)
        closer = d < f1[win]
        f1[win] = np.where(closer, d, f1[win])
        index[win] = np.where(closer, k, index[win])
        windows.append((win, px, py))
    # The distance to the cell's edge is the nearest of the bisectors with its neighbours, taken
    # as a smooth minimum so it doesn't crease where the nearest neighbour changes.
    P = np.array(pts, np.float32)
    soft = 3.0
    total = np.zeros((size, size), np.float32)
    for k, (win, px, py) in enumerate(windows):
        n = index[win]
        # The window is laid out round seed k; take the image of each pixel's own seed nearest it.
        kx, ky = P[k]
        nx = P[n, 0] + np.round((px - P[n, 0]) / size) * size
        ny = P[n, 1] + np.round((py - P[n, 1]) / size) * size
        sx, sy = (kx - nx) / stretch, ky - ny
        length = np.maximum(np.hypot(sx, sy), 1e-3)
        ux, uy = sx / length, sy / length
        mx, my = ((px - nx) / stretch), (py - ny)
        bis = (mx * ux + my * uy) - length / 2
        bis_px = -bis / np.hypot(ux / stretch, uy)
        bis_px = np.where(n == k, 1e4, np.maximum(bis_px, 0.0))
        total[win] += np.exp(-bis_px / soft)
    joint = -soft * np.log(np.maximum(total, 1e-30))
    gy, gx = (np.roll(joint, -1, 0) - np.roll(joint, 1, 0)) / 2, (np.roll(joint, -1, 1) - np.roll(joint, 1, 1)) / 2
    g = np.maximum(np.hypot(gx, gy), 0.05)
    return joint, gx / g, gy / g, index, len(pts)


def _faces(size, seed, cells, palette, width, falloff, proud):
    """Shared stone faces for `drystone` and `rubble`: (face mask, height, colour, below). Each
    stone gets its own colour from `palette`, tone and set, is pillowed toward its middle, worn
    lighter along its top edge and shadowed along its bottom one, weathered in blotches, pitted and
    chipped, and carries lichen. `below` is 1 in a joint under a stone, where it's darkest."""
    joint, ax, ay, index, count = cells
    rng = np.random.default_rng(seed)
    # Joints wander in width, opening into voids here and there.
    w = width * (0.55 + 0.9 * noise(size, 12, seed + 1) ** 2 + 1.6 * np.clip((noise(size, 25, seed + 2) - 0.72) / 0.1, 0, 1))
    d = joint - w
    face = np.clip(d / 1.5, 0, 1)
    inside = np.clip(d, 0, None)
    # Near a joint only: deep inside a stone the second-nearest cell can switch, which would crease
    # anything measured from the joint's direction.
    edge = np.exp(-inside / 4.0) * (inside < 12.0)
    pillow = 1 - np.exp(-inside / falloff)
    grain = noise(size, 100, seed + 3, power=1.0)
    strata = stretched(size, 7.0, 1.0, seed + 4, power=1.5)
    blotch = noise(size, 9, seed + 5, power=1.6)
    pits = np.clip((noise(size, 240, seed + 6, power=0.6) - 0.74) / 0.05, 0, 1)
    chips = np.clip((noise(size, 34, seed + 7) - 0.66) / 0.08, 0, 1) * edge
    lumps = noise(size, 14, seed + 12, power=1.8)
    height = face * (0.85 * pillow + rng.uniform(-proud, proud, count)[index]
                     + 0.35 * lumps + 0.08 * grain + 0.1 * strata - 0.25 * pits - 0.6 * chips)
    stone = np.asarray(palette)[rng.integers(0, len(palette), count)[index]]
    stone = stone * rng.uniform(0.8, 1.1, count)[index][..., None]
    top = np.clip(ay, 0, 1) * edge  # The top edge, where the joint is above.
    bottom = np.clip(-ay, 0, 1) * edge
    k = (0.8 + 0.34 * blotch + 0.06 * (grain - 0.5) + 0.1 * (strata - 0.5) - 0.3 * pits - 0.3 * chips
         + 0.2 * top - 0.3 * bottom + 0.12 * (lumps - 0.5))
    stone = stone * (k * (0.58 + 0.42 * np.clip(inside / 8.0, 0, 1) ** 0.6))[..., None]
    # Crustose lichen in soft-edged patches: pale grey-green, and now and then orange.
    blot = noise(size, 22, seed + 8, power=1.4) + 0.4 * noise(size, 80, seed + 9, power=1.0)
    pale = np.clip((blot - 1.0) / 0.07, 0, 1) * face
    blot = noise(size, 28, seed + 10, power=1.4) + 0.4 * noise(size, 100, seed + 11, power=1.0)
    orange = np.clip((blot - 1.1) / 0.05, 0, 1) * face
    blot = noise(size, 40, seed + 14, power=1.3) + 0.4 * noise(size, 140, seed + 15, power=1.0)
    dark = np.clip((blot - 1.06) / 0.05, 0, 1) * face
    stone = stone * (1 - 0.6 * dark[..., None]) + np.array([0.07, 0.07, 0.065]) * 0.6 * dark[..., None]
    stone = stone * (1 - 0.8 * pale[..., None]) + np.array([0.55, 0.57, 0.47]) * 0.8 * pale[..., None]
    stone = stone * (1 - 0.7 * orange[..., None]) + np.array([0.5, 0.33, 0.09]) * 0.7 * orange[..., None]
    below = np.clip(ay, 0, 1) * (1 - face)
    return face, height, stone, below


def drystone(size=1024, seed=21):
    """Dry-stone walling: flat limestone laid in rough courses without mortar, so every joint is a
    dark shadowed gap, opening here and there round a pinning stone. Pale and buff stones with
    their bedding showing, lichen on the faces, a little moss deep in the joints."""
    cells = _course_cells(size, seed, (34, 112), (75, 340), 8.0)
    palette = [(0.53, 0.47, 0.37), (0.48, 0.45, 0.39), (0.5, 0.43, 0.34), (0.41, 0.36, 0.29), (0.55, 0.5, 0.42)]
    face, height, stone, below = _faces(size, seed, cells, palette, 4.6, 13.0, 0.45)
    void = 1 - face
    # Pinning stones wedged in the wider gaps, catching a little light.
    pins = np.clip((noise(size, 70, seed + 12) - 0.55) / 0.1, 0, 1)
    moss = np.clip((noise(size, 10, seed + 13) - 0.68) / 0.08, 0, 1)
    gap = np.array([0.03, 0.029, 0.026]) * (1 + 2.4 * pins - 0.4 * below)[..., None]
    gap = gap * (1 - 0.8 * moss[..., None]) + np.array([0.045, 0.06, 0.022]) * 0.8 * moss[..., None]
    colour = stone * face[..., None] + gap * void[..., None]
    height = height + void * (-1.6 + 0.6 * pins)
    return colour, height, 7.0, 0.85 + 0.12 * void


def rubble(size=1024, seed=23):
    """Coursed rubble: roughly squared limestone bedded in lime mortar that has weathered back from
    the stones, darkened, and in places washed out, with rain streaks and lichen. The ruins' walls."""
    cells = _course_cells(size, seed, (48, 130), (80, 260), 6.0)
    palette = [(0.42, 0.4, 0.36), (0.39, 0.38, 0.35), (0.44, 0.4, 0.33), (0.34, 0.32, 0.28), (0.46, 0.44, 0.39)]
    face, height, stone, below = _faces(size, seed, cells, palette, 6.0, 8.0, 0.3)
    mortar = 1 - face
    washed = np.clip((noise(size, 9, seed + 12) - 0.6) / 0.08, 0, 1)
    grit = noise(size, 150, seed + 13, power=0.8)
    lime = np.array([0.27, 0.26, 0.23]) * (0.75 + 0.4 * grit + 0.2 * noise(size, 6, seed + 14) - 0.35 * below)[..., None]
    gap = lime * (1 - washed[..., None]) + np.array([0.03, 0.03, 0.026]) * washed[..., None]
    colour = stone * face[..., None] + gap * mortar[..., None]
    height = height + mortar * (-0.8 - 0.9 * washed + 0.15 * grit)
    # Rain runs down the wall in faint streaks.
    streaks = stretched(size, 1.0, 14.0, seed + 15, power=1.3)
    colour = colour * (0.86 + 0.22 * streaks)[..., None]
    return colour, height, 7.5, 0.84 + 0.1 * mortar


def dressed(size=1024, seed=25):
    """Dressed limestone worn smooth by weather and feet: copings, quoins, arches and steps. Soft
    weathering stains, fine pits, hairline cracks, the odd fossil, crusts of pale and dark lichen,
    and no joints: each stone is modelled."""
    broad = noise(size, 2, seed)
    mid = noise(size, 6, seed + 1)
    fine = noise(size, 120, seed + 2, power=0.9)
    stain = np.clip((noise(size, 5, seed + 8, power=1.6) - 0.45) / 0.3, 0, 1)
    pits = np.clip((noise(size, 260, seed + 3, power=0.6) - 0.74) / 0.04, 0, 1)
    f1, edge, _, _ = voronoi(size, 60, seed + 4, jitter=1.0)
    fossils = np.exp(-((f1 - 5.0) / 1.3) ** 2) * (noise(size, 5, seed + 5) > 0.7)
    _, crack_edge, _, _ = voronoi(size, 9, seed + 9, warp=50)
    cracks = np.exp(-(crack_edge / 1.3) ** 2) * (noise(size, 3, seed + 10) > 0.6)
    k = 0.84 + 0.22 * broad + 0.1 * mid + 0.07 * fine - 0.16 * stain - 0.25 * pits - 0.06 * fossils - 0.35 * cracks
    colour = _tint((0.42, 0.39, 0.33), k)
    colour = colour * (1 + 0.1 * (noise(size, 3, seed + 6)[..., None] - 0.5) * np.array([1.0, 0.5, -0.6])[None, None, :])
    blot = noise(size, 26, seed + 7, power=1.4) + 0.4 * noise(size, 90, seed + 11, power=1.0)
    pale = np.clip((blot - 1.0) / 0.07, 0, 1)
    blot = noise(size, 36, seed + 12, power=1.4) + 0.4 * noise(size, 120, seed + 13, power=1.0)
    dark = np.clip((blot - 1.06) / 0.05, 0, 1)
    colour = colour * (1 - 0.6 * dark[..., None]) + np.array([0.08, 0.08, 0.07]) * 0.6 * dark[..., None]
    colour = colour * (1 - 0.75 * pale[..., None]) + np.array([0.55, 0.56, 0.48]) * 0.75 * pale[..., None]
    height = 0.6 * broad + 0.3 * mid + 0.25 * fine - 0.8 * pits - 0.3 * fossils - 0.9 * cracks + 0.15 * pale
    return colour, height, 3.5, 0.8 + 0.06 * fine


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
    "drystone": (drystone, 1.6),
    "rubble": (rubble, 2.0),
    "dressed": (dressed, 1.2),
}
