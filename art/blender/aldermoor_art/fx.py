"""Combat effect sprites painted with numpy: glows, flares, sparks, rings, rune circles, frost,
scorch marks, lightning and wisps, and the tiling strips trails and shields scroll through.

Every sprite is three data channels, packed into the effects atlas as linear RGBA8:

- R, heat: where the sprite is hottest. The effects shader looks it up in a colour ramp, so one
  sprite serves every school (a fire ramp makes it burn, a frost ramp makes it freeze).
- G, cover: how much of the sprite is there (its opacity).
- B, erosion order: as an effect ages its erosion threshold rises and the sprite dissolves where
  B is below it, so B is high where a shape should last longest (its heart, a line's centre) and
  low where it should break up first (wisps, edges, one end of a sweep).

Coordinates run -1..1 across a sprite with +y up and the top row first. Sprites fade to nothing
before their border, so neighbours in the atlas never bleed into them.
"""

import math

import numpy as np

from . import tiles


def grid(w, h=None):
    """Pixel-centre coordinates in -1..1 (x right, y up), top row first."""
    h = h or w
    x = (np.arange(w, dtype=np.float32) + 0.5) / w * 2 - 1
    y = 1 - (np.arange(h, dtype=np.float32) + 0.5) / h * 2
    return np.meshgrid(x, y)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def pack(heat, cover, erode):
    """Three fields in 0..1 to an RGBA8 sprite (alpha unused and opaque)."""
    h, w = np.shape(heat)
    out = np.ones((h, w, 4), np.float32)
    out[..., 0] = np.clip(heat, 0, 1)
    out[..., 1] = np.clip(cover, 0, 1)
    out[..., 2] = np.clip(erode, 0, 1)
    return out


def segment_distance(x, y, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    t = np.clip(((x - ax) * dx + (y - ay) * dy) / max(dx * dx + dy * dy, 1e-12), 0, 1)
    return np.hypot(x - ax - t * dx, y - ay - t * dy)


def polyline_distance(x, y, points, closed=False):
    d = np.full(np.shape(x), np.inf, np.float32)
    pairs = list(zip(points[:-1], points[1:]))
    if closed:
        pairs.append((points[-1], points[0]))
    for a, b in pairs:
        d = np.minimum(d, segment_distance(x, y, a, b))
    return d


def stroke(d, width, aa):
    """Coverage of a line `width` either side of its centre, antialiased over `aa`."""
    return smooth(width + aa, width - aa, d)


def angular_noise(theta, octaves, seed, base=3):
    """Smooth noise around a circle (periodic in theta), 0..1."""
    rng = np.random.default_rng(seed)
    n = np.zeros_like(theta)
    total = 0.0
    for o in range(octaves):
        k = base * 2 ** o
        amp = 0.5 ** o
        for _ in range(2):
            n += amp * np.sin(theta * k + rng.uniform(0, math.tau)) * 0.5
            total += amp * 0.5
    return n / (2 * total) + 0.5


def noise_field(n, scale, seed, power=2.0):
    """Non-tiling use of the periodic noise (it simply doesn't wrap anything here)."""
    return tiles.noise(n, scale, seed, power).astype(np.float32)


# --- Light ---------------------------------------------------------------------------------------

def glow(n=256):
    """A round light: a white-hot pinpoint inside a wide halo that dies away before the edge."""
    x, y = grid(n)
    r = np.hypot(x, y)
    k = 3.4
    halo = np.clip((np.exp(-k * r * r) - math.exp(-k)) / (1 - math.exp(-k)), 0, 1)
    core = np.exp(-(r / 0.13) ** 2)
    mid = np.exp(-(r / 0.34) ** 2)
    heat = 0.16 * halo + 0.38 * mid + 0.62 * core
    cover = 0.55 * halo ** 1.4 + 0.3 * mid + 0.4 * core
    return pack(heat / heat.max(), cover / cover.max(), 1 - r)


def flare(n=256, seed=3):
    """A star of light: four long rays, four short ones between them and a soft heart."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)

    def ray(angle, length, width):
        ca, sa = math.cos(angle), math.sin(angle)
        along = np.abs(x * ca + y * sa) / length
        across = -x * sa + y * ca
        w = width * (1.0 - 0.8 * np.clip(along, 0, 1)) + 1e-4
        return np.exp(-(across / w) ** 2) * np.clip(1 - along, 0, 1) ** 1.8

    long_rays = np.maximum(ray(0, 0.98, 0.05), ray(math.pi / 2, 0.98, 0.05))
    short_rays = np.maximum(ray(math.pi / 4, 0.5, 0.035), ray(3 * math.pi / 4, 0.5, 0.035))
    shimmer = 0.85 + 0.15 * angular_noise(theta, 3, seed, 8)
    core = np.exp(-(r / 0.1) ** 2)
    halo = np.clip((np.exp(-6 * r * r) - math.exp(-6)) / (1 - math.exp(-6)), 0, 1)
    heat = long_rays * 0.75 * shimmer + short_rays * 0.5 + core + 0.25 * halo
    cover = long_rays * shimmer + short_rays * 0.7 + core + 0.35 * halo
    return pack(np.clip(heat, 0, 1), np.clip(cover, 0, 1), 1 - r)


def spark(w=64, h=256):
    """A streak to stretch along a spark's flight: brightest just behind its head (the top)."""
    x, y = grid(w, h)
    along = (y + 1) / 2
    body = smooth(0.0, 0.75, along) * (1 - smooth(0.86, 0.98, along))
    width = 0.18 + 0.55 * along ** 1.5
    across = np.exp(-(x / width) ** 2)
    heat = body * across ** 2.5
    cover = body * across
    return pack(heat, cover, along)


def shock(n=256, seed=11):
    """A shock ring running outwards: a crisp leading edge and a glow trailing in behind it,
    torn into streaks."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    edge = 0.8 + 0.035 * (angular_noise(theta, 3, seed, 4) - 0.5)
    lead = smooth(edge + 0.05, edge - 0.005, r)
    trail = smooth(edge - 0.42, edge, r) ** 2.4
    streaks = 0.55 + 0.45 * angular_noise(theta, 4, seed + 1, 24)
    ring = lead * trail
    heat = ring ** 1.6 * streaks
    cover = ring * (0.65 + 0.35 * streaks)
    chunks = angular_noise(theta, 2, seed + 2, 5)
    return pack(heat, cover, 0.35 + 0.65 * chunks * smooth(edge - 0.5, edge, r))


def ring(n=256, seed=17):
    """A soft band of light for auras: a bright line at its middle with a glow either side and
    a little sparkle round it."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    line = np.exp(-((r - 0.74) / 0.035) ** 2)
    band = np.exp(-((r - 0.72) / 0.13) ** 2)
    inside = smooth(0.78, 0.2, r) * 0.12
    sparkle = angular_noise(theta, 4, seed, 16) ** 3
    heat = line * (0.75 + 0.25 * sparkle) + 0.35 * band
    cover = np.clip(line + 0.55 * band + inside, 0, 1) * smooth(0.97, 0.9, r)
    return pack(np.clip(heat, 0, 1), cover, angular_noise(theta, 2, seed + 5, 3))


# --- Rune circles ------------------------------------------------------------------------------

def ornate_ring(n=256, seed=97):
    """A ring for auras at the feet, drawn like a seal: a bold outer band, a thin inner line,
    dashes between them and small diamonds at the quarters, with a glow either side."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    aa = 1.5 / n
    lines = _circle_coverage(r, 0.86, 0.03, aa)
    lines = np.maximum(lines, _circle_coverage(r, 0.7, 0.008, aa))
    dashes = 36
    nearest = np.round(theta / (math.tau / dashes)) * (math.tau / dashes)
    across = np.abs(np.sin(theta - nearest)) * r
    dash = stroke(across, 0.012, aa) * smooth(0.735, 0.745, r) * smooth(0.805, 0.795, r)
    lines = np.maximum(lines, dash * 0.8)
    for k in range(8):
        a = k * math.tau / 8
        cx, cy = 0.78 * math.cos(a), 0.78 * math.sin(a)
        ca, sa = math.cos(a), math.sin(a)
        u = (x - cx) * ca + (y - cy) * sa
        v = -(x - cx) * sa + (y - cy) * ca
        diamond = np.abs(u) / 0.07 + np.abs(v) / 0.045
        lines = np.maximum(lines, smooth(1.0 + aa * 20, 1.0 - aa * 20, diamond) * (1.0 if k % 2 == 0 else 0.7))
    glow = np.exp(-np.minimum(np.abs(r - 0.86), np.abs(r - 0.7)) / 0.05) * smooth(1.0, 0.94, r)
    sparkle = angular_noise(theta, 4, seed, 24) ** 4
    heat = np.clip(lines * (0.8 + 0.2 * sparkle) + 0.3 * glow, 0, 1)
    cover = np.clip(lines + 0.45 * glow, 0, 1)
    sweep = (math.pi / 2 - theta) % math.tau / math.tau
    return pack(heat, cover, 1 - sweep * 0.97)


# Elder Futhark, drawn as strokes in a box 0.6 wide and 1 tall, base at y = 0.
RUNES = {
    "fehu": [[(0, 0), (0, 1)], [(0, 0.55), (0.45, 0.85)], [(0, 0.8), (0.38, 1.0)]],
    "uruz": [[(0, 0), (0, 1), (0.5, 0.72), (0.5, 0)]],
    "thurisaz": [[(0, 0), (0, 1)], [(0, 0.72), (0.42, 0.5), (0, 0.28)]],
    "ansuz": [[(0, 0), (0, 1)], [(0, 1), (0.42, 0.76)], [(0, 0.74), (0.42, 0.5)]],
    "raidho": [[(0, 0), (0, 1), (0.42, 0.8), (0, 0.56), (0.45, 0)]],
    "kaunan": [[(0.42, 0.88), (0.02, 0.5), (0.42, 0.12)]],
    "gebo": [[(0, 0), (0.6, 1)], [(0.6, 0), (0, 1)]],
    "wunjo": [[(0, 0), (0, 1), (0.4, 0.8), (0, 0.6)]],
    "hagalaz": [[(0, 0), (0, 1)], [(0.5, 0), (0.5, 1)], [(0, 0.66), (0.5, 0.34)]],
    "naudiz": [[(0.22, 0), (0.22, 1)], [(0, 0.66), (0.44, 0.4)]],
    "isa": [[(0.15, 0), (0.15, 1)]],
    "jera": [[(0.3, 1.0), (0.04, 0.76), (0.3, 0.52)], [(0.2, 0.48), (0.46, 0.24), (0.2, 0.0)]],
    "eiwaz": [[(0.22, 0), (0.22, 1)], [(0.22, 1), (0.48, 0.8)], [(0.22, 0), (-0.04, 0.2)]],
    "perthro": [[(0, 0), (0, 1), (0.36, 0.8)], [(0, 0), (0.36, 0.2)], [(0.36, 0.8), (0.22, 0.6)], [(0.36, 0.2), (0.22, 0.4)]],
    "algiz": [[(0.25, 0), (0.25, 1)], [(0.25, 0.52), (0.0, 0.9)], [(0.25, 0.52), (0.5, 0.9)]],
    "sowilo": [[(0.42, 1), (0.04, 0.68), (0.42, 0.34), (0.04, 0.0)]],
    "tiwaz": [[(0.25, 0), (0.25, 1)], [(0.25, 1), (0.0, 0.72)], [(0.25, 1), (0.5, 0.72)]],
    "berkanan": [[(0, 0), (0, 1), (0.4, 0.76), (0.02, 0.5), (0.4, 0.24), (0, 0)]],
    "ehwaz": [[(0, 0), (0, 1), (0.25, 0.7), (0.5, 1), (0.5, 0)]],
    "mannaz": [[(0, 0), (0, 1), (0.5, 0.5)], [(0.5, 0), (0.5, 1), (0, 0.5)]],
    "laguz": [[(0.05, 0), (0.05, 1), (0.42, 0.76)]],
    "ingwaz": [[(0.25, 1), (0.5, 0.5), (0.25, 0), (0, 0.5), (0.25, 1)]],
    "dagaz": [[(0, 0), (0, 1), (0.6, 0), (0.6, 1), (0, 0)]],
    "othala": [[(0.25, 1), (0.48, 0.66), (0.02, 0.22), (0, 0)], [(0.25, 1), (0.02, 0.66), (0.48, 0.22), (0.5, 0)]],
}


def _glyph_distance(x, y, centre_angle, radius, height, name):
    """Distance to a rune standing on a circle: its base towards the centre, upright outwards."""
    strokes = RUNES[name]
    # Local frame: u along the circle (clockwise reading), v outwards.
    ca, sa = math.cos(centre_angle), math.sin(centre_angle)
    px = x - ca * radius
    py = y - sa * radius
    u = px * sa - py * ca
    v = px * ca + py * sa
    width = max(max(p[0] for s in strokes for p in s), 0.1)
    # Glyph space: x across 0..width, y up 0..1, centred on the circle.
    gx = u / height + width / 2
    gy = v / height + 0.5
    d = np.full(np.shape(x), np.inf, np.float32)
    for s in strokes:
        d = np.minimum(d, polyline_distance(gx, gy, s))
    return d * height


def _polygon(n, radius, phase):
    return [(radius * math.cos(phase + i * math.tau / n), radius * math.sin(phase + i * math.tau / n)) for i in range(n)]


def _circle_coverage(r, radius, width, aa):
    return stroke(np.abs(r - radius), width, aa)


def runes(n=512, seed=23):
    """A casting circle: two outer rings with a band of runes between, a ring of beads, a
    hexagram and an inner ring round a small sun. Erosion runs round it, so a rising threshold
    draws the circle in by hand, sweep by sweep."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    aa = 1.5 / n
    lines = np.zeros_like(r)
    lines = np.maximum(lines, _circle_coverage(r, 0.955, 0.012, aa))
    lines = np.maximum(lines, _circle_coverage(r, 0.905, 0.005, aa))
    lines = np.maximum(lines, _circle_coverage(r, 0.785, 0.007, aa))
    names = list(RUNES)
    rng = np.random.default_rng(seed)
    order = rng.permutation(len(names))
    count = 24
    glyph = np.full(np.shape(r), np.inf, np.float32)
    for i in range(count):
        a = math.pi / 2 - i * math.tau / count
        glyph = np.minimum(glyph, _glyph_distance(x, y, a, 0.845, 0.075, names[order[i % len(names)]]))
    lines = np.maximum(lines, stroke(glyph, 0.0065, aa))
    # Beads on a ring just inside the runes.
    beads = 48
    nearest = np.round(theta / (math.tau / beads)) * (math.tau / beads)
    bx, by = 0.745 * np.cos(nearest), 0.745 * np.sin(nearest)
    lines = np.maximum(lines, stroke(np.hypot(x - bx, y - by), 0.009, aa))
    # A hexagram inscribed in the beads, and its inner ring.
    tri_a = _polygon(3, 0.7, math.pi / 2)
    tri_b = _polygon(3, 0.7, -math.pi / 2)
    star = np.minimum(polyline_distance(x, y, tri_a, closed=True), polyline_distance(x, y, tri_b, closed=True))
    lines = np.maximum(lines, stroke(star, 0.007, aa))
    lines = np.maximum(lines, _circle_coverage(r, 0.36, 0.006, aa))
    lines = np.maximum(lines, _circle_coverage(r, 0.33, 0.003, aa))
    # A small sun at the middle: a ring and twelve rays.
    lines = np.maximum(lines, _circle_coverage(r, 0.085, 0.008, aa))
    spokes = 12
    nearest = np.round(theta / (math.tau / spokes)) * (math.tau / spokes)
    across = np.abs(np.sin(theta - nearest)) * r
    rays = stroke(across, 0.005, aa) * smooth(0.12, 0.14, r) * smooth(0.25, 0.22, r)
    lines = np.maximum(lines, rays)
    glow = np.exp(-np.minimum.reduce([np.abs(r - 0.955), np.abs(r - 0.785), glyph, star, np.abs(r - 0.36)]) / 0.022)
    glow *= smooth(1.0, 0.95, r)
    heat = np.clip(lines + 0.28 * glow, 0, 1)
    cover = np.clip(lines * 0.95 + 0.4 * glow, 0, 1)
    sweep = (math.pi / 2 - theta) % math.tau / math.tau
    return pack(heat, cover, 1 - sweep * 0.97)


def sigil(n=512, seed=29):
    """A darker circle for shadow and arcane: a ring of ticks like a dial, a pentagram with
    beads at its points and two rings of runes turning against each other."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    aa = 1.5 / n
    lines = np.maximum(_circle_coverage(r, 0.95, 0.009, aa), _circle_coverage(r, 0.81, 0.006, aa))
    ticks = 72
    nearest = np.round(theta / (math.tau / ticks)) * (math.tau / ticks)
    across = np.abs(np.sin(theta - nearest)) * r
    long_tick = (np.round(theta / (math.tau / ticks)) % 6 == 0)
    reach = np.where(long_tick, 0.84, 0.88)
    lines = np.maximum(lines, stroke(across, 0.004, aa) * smooth(reach - 0.01, reach, r) * smooth(0.935, 0.925, r))
    points = _polygon(5, 0.66, math.pi / 2)
    pentagram = [points[i * 2 % 5] for i in range(5)]
    star = polyline_distance(x, y, pentagram, closed=True)
    lines = np.maximum(lines, stroke(star, 0.007, aa))
    lines = np.maximum(lines, _circle_coverage(r, 0.66, 0.005, aa))
    for px, py in points:
        d = np.hypot(x - px, y - py)
        lines = np.maximum(lines, _circle_coverage(d, 0.055, 0.006, aa))
        lines = np.maximum(lines, stroke(d, 0.018, aa))
    names = list(RUNES)
    rng = np.random.default_rng(seed)
    glyph = np.full(np.shape(r), np.inf, np.float32)
    for i in range(10):
        a = math.pi / 2 + math.pi / 5 + i * math.tau / 10
        glyph = np.minimum(glyph, _glyph_distance(x, y, a, 0.4, 0.07, names[rng.integers(len(names))]))
    lines = np.maximum(lines, stroke(glyph, 0.0055, aa))
    lines = np.maximum(lines, _circle_coverage(r, 0.3, 0.005, aa))
    glow = np.exp(-np.minimum.reduce([np.abs(r - 0.95), np.abs(r - 0.81), star, glyph, np.abs(r - 0.66)]) / 0.025)
    glow *= smooth(1.0, 0.95, r)
    heat = np.clip(lines + 0.25 * glow, 0, 1)
    cover = np.clip(lines * 0.95 + 0.42 * glow, 0, 1)
    sweep = (math.pi / 2 - theta) % math.tau / math.tau
    return pack(heat, cover, 1 - sweep * 0.97)


# --- Shapes ------------------------------------------------------------------------------------

def star(n=128):
    """A five-pointed cartoon star for a dazed head: a solid heart, a bright edge and a glow."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x) - math.pi / 2
    k = 5
    sector = ((theta + math.pi / k) % (math.tau / k)) - math.pi / k
    outer, inner = 0.78, 0.36
    # Distance to the straight edge between an outer tip and the next inner corner.
    ax, ay = outer, 0.0
    bx, by = inner * math.cos(math.pi / k), inner * math.sin(math.pi / k)
    px, py = r * np.cos(np.abs(sector)), r * np.sin(np.abs(sector))
    ex, ey = bx - ax, by - ay
    t = np.clip(((px - ax) * ex + (py - ay) * ey) / (ex * ex + ey * ey), 0, 1)
    dist = np.hypot(px - ax - t * ex, py - ay - t * ey)
    side = np.sign((px - ax) * ey - (py - ay) * ex)
    signed = dist * side
    aa = 2.0 / n
    fill = smooth(aa, -aa, signed)
    edge = np.exp(-(np.abs(signed) / 0.04) ** 2)
    glow = np.exp(-np.maximum(signed, 0) / 0.08) * (1 - fill)
    heat = np.clip(0.55 * fill + 0.45 * fill * np.exp(-(r / 0.3) ** 2) + edge * 0.6 + 0.3 * glow, 0, 1)
    cover = np.clip(fill + 0.6 * glow, 0, 1) * smooth(1.0, 0.92, r)
    return pack(heat, cover, 1 - r)


def snowflake(n=128):
    """Six arms with branches, for frost: thin bright lines in a faint glow."""
    x, y = grid(n)
    r = np.hypot(x, y)
    segments = []
    for i in range(6):
        a = math.pi / 2 + i * math.tau / 6
        ca, sa = math.cos(a), math.sin(a)
        segments.append(((0, 0), (0.85 * ca, 0.85 * sa)))
        for at, length in ((0.38, 0.24), (0.6, 0.17)):
            bx, by = at * ca, at * sa
            for turn in (-1, 1):
                b = a + turn * math.pi / 3
                segments.append(((bx, by), (bx + length * math.cos(b), by + length * math.sin(b))))
    d = np.full(np.shape(x), np.inf, np.float32)
    for a, b in segments:
        d = np.minimum(d, segment_distance(x, y, a, b))
    aa = 1.5 / n
    hexagon = polyline_distance(x, y, _polygon(6, 0.16, math.pi / 2), closed=True)
    lines = np.maximum(stroke(d, 0.022, aa), stroke(hexagon, 0.02, aa))
    glow = np.exp(-np.minimum(d, hexagon) / 0.06) * smooth(1.0, 0.85, r)
    heat = np.clip(lines * 0.9 + 0.3 * glow + 0.6 * np.exp(-(r / 0.12) ** 2), 0, 1)
    cover = np.clip(lines + 0.45 * glow, 0, 1)
    return pack(heat, cover, 1 - r)


def shard(n=128, seed=31):
    """An ice shard: a long kite of two facets (the left catching the light), with glinting
    edges. Flung from frost impacts and shattering."""
    x, y = grid(n)
    top, right, bottom, left = (0.02, 0.96), (0.3, 0.12), (0.0, -0.96), (-0.24, 0.02)
    poly = [top, right, bottom, left]

    def side(a, b):
        return (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])

    inside = np.ones_like(x, bool)
    for a, b in zip(poly, poly[1:] + poly[:1]):
        inside &= side(a, b) <= 0
    edge_d = polyline_distance(x, y, poly, closed=True)
    ridge = segment_distance(x, y, top, bottom)
    aa = 2.0 / n
    fill = smooth(aa, -aa, np.where(inside, -edge_d, edge_d))
    lit = np.where(x < (top[0] + (bottom[0] - top[0]) * (top[1] - y) / (top[1] - bottom[1])), 0.75, 0.35)
    shading = lit + 0.25 * np.exp(-(ridge / 0.03) ** 2) + 0.3 * np.exp(-(edge_d / 0.04) ** 2)
    noise = noise_field(n, 4, seed)
    heat = np.clip(fill * shading * (0.85 + 0.15 * noise), 0, 1)
    cover = np.clip(fill * (0.6 + 0.4 * shading), 0, 1)
    return pack(heat, cover, fill * (0.4 + 0.6 * noise))


def bubble(n=64):
    """A poison bubble: a thin rim with a bright glint near its top left."""
    x, y = grid(n)
    r = np.hypot(x, y)
    aa = 2.0 / n
    rim = stroke(np.abs(r - 0.78), 0.06, aa)
    inner = smooth(0.8, 0.3, r) * 0.18
    glint = np.exp(-(((x + 0.32) ** 2 + (y - 0.36) ** 2) / 0.02))
    heat = np.clip(rim * 0.6 + glint + inner * 0.5, 0, 1)
    cover = np.clip(rim + inner + glint, 0, 1)
    return pack(heat, cover, 1 - r)


def ripple(n=256, seed=37):
    """Rings spreading on water for a water field, broken by a little noise."""
    x, y = grid(n)
    warp = (noise_field(n, 4, seed) - 0.5) * 0.06
    r = np.hypot(x, y) + warp
    bands = 0.5 + 0.5 * np.cos(r * 34.0)
    crest = bands ** 6
    fade = smooth(0.98, 0.7, r) * smooth(0.0, 0.25, r)
    heat = crest * fade
    cover = (0.25 + 0.75 * crest) * fade
    return pack(heat, cover, noise_field(n, 3, seed + 1))


def scorch(n=256, seed=41):
    """A burn left where fire landed: charred ground (cover) split by cracks that still glow
    (heat), its edge ragged with noise."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    edge = 0.72 + 0.16 * (angular_noise(theta, 4, seed, 3) - 0.5) + 0.08 * (noise_field(n, 6, seed + 1) - 0.5)
    body = smooth(edge, edge - 0.3, r)
    _, gap, _, _ = tiles.voronoi(n, 40, seed + 2, warp=6.0)
    cracks = np.exp(-(gap / 2.2) ** 2) * smooth(edge - 0.05, edge * 0.3, r)
    soot = noise_field(n, 5, seed + 3)
    heat = np.clip(cracks * (0.6 + 0.4 * soot) * smooth(edge, 0.0, r) ** 0.5, 0, 1)
    cover = np.clip(body * (0.55 + 0.45 * soot), 0, 1)
    return pack(heat, cover, np.clip(body * 0.7 + soot * 0.3, 0, 1))


def frost(n=256, seed=43):
    """Hoarfrost spreading over the ground: fine crystal branches growing out from the middle
    over a mottled rime, with points that glint."""
    x, y = grid(n)
    r = np.hypot(x, y)
    rng = np.random.default_rng(seed)
    lines = np.full(np.shape(x), np.inf, np.float32)

    def grow(px, py, angle, length, depth):
        steps = 6
        for _ in range(steps):
            angle += rng.normal(0, 0.25)
            nx = px + math.cos(angle) * length / steps
            ny = py + math.sin(angle) * length / steps
            nonlocal lines
            lines = np.minimum(lines, segment_distance(x, y, (px, py), (nx, ny)) * (1.0 + depth * 0.7))
            if depth < 2 and rng.random() < 0.35:
                grow(nx, ny, angle + rng.choice([-1, 1]) * rng.uniform(0.6, 1.1), length * 0.45, depth + 1)
            px, py = nx, ny

    for i in range(26):
        a = i * math.tau / 26 + rng.uniform(-0.1, 0.1)
        start = rng.uniform(0.0, 0.15)
        grow(start * math.cos(a), start * math.sin(a), a, rng.uniform(0.55, 0.85), 0)
    aa = 1.5 / n
    branches = stroke(lines, 0.006, aa)
    rime = noise_field(n, 5, seed + 1) * smooth(0.9, 0.2, r)
    glints = (noise_field(n, 48, seed + 2, power=0.5) > 0.82) * smooth(0.85, 0.3, r)
    fade = smooth(0.95, 0.55, r)
    heat = np.clip(branches * 0.7 * fade + glints * 0.9, 0, 1)
    cover = np.clip((branches * 0.7 + rime * 0.85 + 0.12) * fade, 0, 1)
    return pack(heat, cover, np.clip(1 - r + 0.3 * rime, 0, 1))


def bolt_path(rng, start, end, roughness, depth):
    """Midpoint displacement: a jagged line from start to end."""
    points = [start, end]
    amp = roughness
    for _ in range(depth):
        out = [points[0]]
        for a, b in zip(points[:-1], points[1:]):
            mx, my = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
            dx, dy = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dx, dy) or 1.0
            off = rng.normal(0, amp) * length
            out += [(mx - dy / length * off, my + dx / length * off), b]
        points = out
        amp *= 0.55
    return points


def lightning(w=256, h=64, seed=47):
    """A crackling bolt across the strip, with forks: a white core inside a blue-white glow."""
    x, y = grid(w, h)
    aspect = w / h
    xs = x * aspect
    rng = np.random.default_rng(seed)
    main = bolt_path(rng, (-aspect * 0.97, 0.0), (aspect * 0.97, 0.0), 0.06, 6)
    d = polyline_distance(xs, y, main)
    forks = np.full(np.shape(x), np.inf, np.float32)
    for _ in range(3):
        i = rng.integers(len(main) // 5, len(main) * 4 // 5)
        a = main[i]
        angle = rng.uniform(-0.7, 0.7) + (0 if rng.random() < 0.5 else math.pi)
        length = rng.uniform(0.6, 1.4)
        b = (a[0] + math.cos(angle) * length, max(-0.85, min(0.85, a[1] + math.sin(angle) * length * 0.4)))
        forks = np.minimum(forks, polyline_distance(xs, y, bolt_path(rng, a, b, 0.12, 4)))
    aa = 2.0 / h
    core = np.maximum(stroke(d, 0.03, aa), stroke(forks, 0.016, aa) * 0.8)
    glow = np.exp(-d / 0.12) + 0.6 * np.exp(-forks / 0.08)
    ends = smooth(aspect, aspect * 0.85, np.abs(xs))
    heat = np.clip(core + 0.3 * glow, 0, 1) * ends
    cover = np.clip(core + 0.5 * glow, 0, 1) * ends * smooth(1.0, 0.8, np.abs(y))
    return pack(heat, cover, np.clip(1 - np.abs(x), 0, 1))


# --- Flipbooks -----------------------------------------------------------------------------------

def _lattice(ix, iy, iz, seed):
    """A value in 0..1 for each integer lattice point."""
    h = (ix * 73856093) ^ (iy * 19349663) ^ (iz * 83492791) ^ (seed * 2654435761)
    h = (h ^ (h >> 13)) * 1274126177
    h = h ^ (h >> 16)
    return (h & 0xFFFFFF).astype(np.float32) / float(0xFFFFFF)


def value_noise(x, y, z, seed):
    """Smooth 3D value noise in 0..1 (no tiling), for flames and smoke that evolve in time."""
    xi, yi, zi = np.floor(x).astype(np.int64), np.floor(y).astype(np.int64), np.floor(z).astype(np.int64)
    xf, yf, zf = x - xi, y - yi, z - zi
    u, v, w = xf * xf * (3 - 2 * xf), yf * yf * (3 - 2 * yf), zf * zf * (3 - 2 * zf)
    out = 0.0
    for dz in (0, 1):
        for dy in (0, 1):
            for dx in (0, 1):
                corner = _lattice(xi + dx, yi + dy, zi + dz, seed)
                weight = (u if dx else 1 - u) * (v if dy else 1 - v) * (w if dz else 1 - w)
                out = out + corner * weight
    return out


def fbm(x, y, z, seed, octaves=5, gain=0.5):
    total, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        f = 2.0 ** o
        total = total + value_noise(x * f, y * f, z * f + o * 17.3, seed + o * 101) * amp
        norm += amp
        amp *= gain
    return total / norm


def flame(n=192, frames=16, seed=59):
    """One flame's life: it licks up from a rounded base, grows tall, tears into tongues at its
    tip, lifts away and dies down. Three crisp zones of heat (a red body, an orange heart and a
    yellow-white core near its base) give the ramp hard bands to paint, as hand-painted flames
    have, and three scales of turbulence keep its edges busy. Fire everywhere is many of these,
    each sized differently."""
    x, y = grid(n)
    base = -0.88
    aa = 1.5 / n
    out = []
    for f in range(frames):
        t = (f + 0.5) / frames
        grow = smooth(0.0, 0.28, t)
        height = 0.6 + 1.15 * grow
        lift = 0.6 * smooth(0.5, 1.0, t)
        h = y - base - lift
        v = np.clip(h / height, 0.0, 1.0)
        coarse = fbm(x * 1.9, (y - t * 2.6) * 1.5, t * 1.3, seed, octaves=4)
        mid = fbm(x * 4.2, (y - t * 4.0) * 3.2, t * 2.0 + 9.0, seed + 7, octaves=4)
        fine = fbm(x * 9.0, (y - t * 6.5) * 7.0, t * 3.0 + 21.0, seed + 13, octaves=3)
        sway = (coarse - 0.5) * 0.7 * v ** 0.9
        xs = x + sway + (mid - 0.5) * 0.12 * v + (fine - 0.5) * 0.035
        dying = smooth(0.62, 1.0, t)
        width = 0.42 * (1 - v) ** 0.72 * (0.5 + 0.5 * smooth(0.0, 0.16, v)) * (1 - 0.3 * dying)
        tongues = v + (mid - 0.5) * 0.95 + (coarse - 0.5) * 0.35 + (fine - 0.5) * 0.22 + dying * 0.75
        outer = smooth(width + aa, width - aa, np.abs(xs)) * smooth(1.02, 0.98, tongues)
        outer = outer * smooth(-0.04, 0.06, h) * smooth(1.05, 0.9, h / height)
        # Dying down: it thins from the bottom up rather than squatting.
        outer = outer * (1 - dying * smooth(0.55, -0.1, v + (fine - 0.5) * 0.5)) * (1 - 0.55 * dying)
        heart = smooth(width * 0.6 + aa, width * 0.6 - aa, np.abs(xs)) * smooth(0.8, 0.76, tongues + 0.1)
        core_width = width * 0.3 * (1 - 0.3 * v)
        core = smooth(core_width + aa, core_width - aa, np.abs(xs - (fine - 0.5) * 0.03))
        core = core * smooth(0.52, 0.48, tongues + 0.25 + v * 0.35)
        # The body darkens towards its edge, so the ramp paints it a red rim round orange.
        depth = smooth(0.0, 0.4, (width - np.abs(xs)) / np.maximum(width, 1e-3))
        heat = (0.2 + 0.13 * depth + 0.33 * heart + 0.34 * core) * outer
        heat = np.clip(heat * (0.86 + 0.28 * fine) * (1 - 0.35 * t), 0, 1)
        cover = np.clip(outer * (0.88 + 0.12 * mid), 0, 1)
        erode = np.clip(outer * (1 - v) * 0.55 + fine * 0.3 + mid * 0.15, 0, 1)
        out.append(pack(heat, cover * smooth(1.0, 0.9, np.abs(x)) * smooth(1.0, 0.93, y), erode))
    return out


def puff(n=128, frames=16, seed=73, balls=11):
    """Smoke as a painter draws it: a cluster of round lumps that swells, rises and thins, each
    lump shaded from above-left with a soft band of shadow beneath, its outline broken by noise.
    Heat is the shading, so any ramp colours it as lit smoke."""
    x, y = grid(n)
    rng = np.random.default_rng(seed)
    lumps = [(rng.uniform(-0.32, 0.32), rng.uniform(-0.25, 0.2), rng.uniform(0.1, 0.24), rng.uniform(-0.2, 0.2)) for _ in range(balls)]
    out = []
    for f in range(frames):
        t = (f + 0.5) / frames
        grow = 0.55 + 0.6 * (1 - (1 - t) ** 2)
        field = np.zeros_like(x)
        for cx, cy, r0, drift in lumps:
            px, py = cx * grow * 1.25 + drift * t, cy * grow + 0.22 * t
            r = r0 * grow
            field = field + np.exp(-((x - px) ** 2 + (y - py) ** 2) / (r * r))
        noise = fbm(x * 3.2, (y - t * 0.8) * 3.2, t * 0.9, seed, octaves=5)
        fine = fbm(x * 8.0, (y - t * 0.8) * 8.0, t * 0.9 + 4.0, seed + 5, octaves=3)
        rise = 0.38 + 0.42 * smooth(0.3, 1.0, t)
        shape = field + (noise - 0.5) * 0.7 + (fine - 0.5) * 0.25
        density = smooth(rise, rise + 0.35, shape)
        gy, gx = np.gradient(field)
        normal_x, normal_y = -gx * n * 0.12, gy * n * 0.12
        lit = np.clip(0.5 + 0.5 * (normal_x * -0.6 + normal_y * 0.8) / np.sqrt(1 + normal_x ** 2 + normal_y ** 2), 0, 1)
        shade = np.clip(0.3 + 0.5 * smooth(0.15, 0.85, lit) + (noise - 0.5) * 0.35 + (fine - 0.5) * 0.25, 0, 1)
        thin = smooth(0.4, 1.0, t)
        cover = density * (0.75 + 0.25 * fine) * (1 - 0.6 * thin) * smooth(1.0, 0.88, np.hypot(x, y))
        out.append(pack(shade, cover, np.clip(smooth(rise, rise + 0.5, shape) * 0.7 + noise * 0.3, 0, 1)))
    return out


def swirl(n=128, frames=16, seed=79, arms=3):
    """A ball of magic turning on itself: a white-hot core, three spiral arms that twist out to
    a bright rim, flecks of light caught in them. Loops: it turns once over the flipbook."""
    x, y = grid(n)
    r = np.hypot(x, y)
    theta = np.arctan2(y, x)
    rng = np.random.default_rng(seed)
    flecks = [(rng.uniform(0, math.tau), rng.uniform(0.25, 0.8)) for _ in range(14)]
    out = []
    for f in range(frames):
        phase = f / frames * math.tau
        c, s_ = math.cos(phase), math.sin(phase)
        xr, yr = x * c + y * s_, -x * s_ + y * c
        noise = fbm(xr * 3.0 + 7.0, yr * 3.0 + 7.0, 0.5, seed, octaves=4)
        spiral = 0.5 + 0.5 * np.cos(arms * (theta - phase) + 7.5 * r + (noise - 0.5) * 2.0)
        arm = spiral ** 5 * smooth(0.92, 0.35, r) * smooth(0.06, 0.2, r)
        core = np.exp(-(r / 0.17) ** 2)
        rim = np.exp(-((r - 0.66) / 0.07) ** 2) * (0.5 + 0.5 * noise)
        glints = np.zeros_like(x)
        for a, d in flecks:
            ang = a + phase
            glints = np.maximum(glints, np.exp(-((x - d * math.cos(ang)) ** 2 + (y - d * math.sin(ang)) ** 2) / 0.0012))
        heat = np.clip(core + arm * (0.55 + 0.35 * noise) + rim * 0.45 + glints * 0.9, 0, 1)
        cover = np.clip(core + arm * 0.9 + rim * 0.7 + glints + 0.18 * smooth(0.9, 0.15, r), 0, 1)
        out.append(pack(heat, cover * smooth(1.0, 0.92, r), np.clip(arm + core + glints, 0, 1)))
    return out


def starburst(n=256, seed=83, rays=28):
    """An impact's flash: thin rays of different lengths flung out from a hot point, as in a
    hand-drawn hit."""
    x, y = grid(n)
    r = np.hypot(x, y)
    rng = np.random.default_rng(seed)
    light = np.zeros_like(x)
    for _ in range(rays):
        a = rng.uniform(0, math.tau)
        length = rng.uniform(0.35, 0.97)
        width = rng.uniform(0.008, 0.022)
        ca, sa = math.cos(a), math.sin(a)
        along = x * ca + y * sa
        across = -x * sa + y * ca
        ray = np.exp(-(across / (width * (1 - np.clip(along / length, 0, 1)) + 0.002)) ** 2)
        ray = ray * np.clip(1 - along / length, 0, 1) ** 1.3 * (along > 0)
        light = np.maximum(light, ray * rng.uniform(0.6, 1.0))
    core = np.exp(-(r / 0.12) ** 2)
    glow = np.exp(-(r / 0.35) ** 2) * 0.35
    heat = np.clip(light * 0.85 + core + glow * 0.5, 0, 1)
    cover = np.clip(light + core + glow, 0, 1) * smooth(1.0, 0.95, r)
    return pack(heat, cover, 1 - r)


def wisp(n=128, frames=16, seed=53):
    """Magic coming loose: four filaments uncoil from a bright knot, turn and fray away. One
    life from first frame to last; arcane, shadow and holy all tint it."""
    x, y = grid(n)
    rng = np.random.default_rng(seed)
    arms = [(rng.uniform(0, math.tau), rng.uniform(0.75, 1.0), rng.choice([-1, 1]) * rng.uniform(0.8, 1.2)) for _ in range(4)]
    out = []
    aa = 1.5 / n
    for f in range(frames):
        t = (f + 0.5) / frames
        life = math.sin(math.pi * min(t * 1.15, 1.0))
        reach = 0.25 + 0.65 * (1 - (1 - t) ** 2)
        heat = np.zeros_like(x)
        cover = np.zeros_like(x)
        erode = np.zeros_like(x)
        for phase, length, turn in arms:
            s = np.linspace(0, 1, 28)
            angle = phase + turn * (s * 2.4 + t * 2.0)
            radius = (0.04 + s * length) * reach
            pts = list(zip(radius * np.cos(angle), radius * np.sin(angle)))
            d = np.full(np.shape(x), np.inf, np.float32)
            along = np.zeros_like(x)
            for i, (a, b) in enumerate(zip(pts[:-1], pts[1:])):
                di = segment_distance(x, y, a, b)
                closer = di < d
                along = np.where(closer, i / (len(pts) - 1), along)
                d = np.minimum(d, di)
            width = 0.018 * (1 - along) + 0.004
            line = stroke(d, width, aa) * (1 - along) ** 0.6
            halo = np.exp(-d / (0.05 * (1.2 - along)))
            heat = np.maximum(heat, line * 0.95 + 0.35 * halo * (1 - along))
            cover = np.maximum(cover, line + 0.5 * halo * (1 - along))
            erode = np.maximum(erode, (1 - along) * smooth(0.25, 0.0, d))
        knot = np.exp(-(np.hypot(x, y) / (0.12 * (1.2 - t))) ** 2) * (1 - t) ** 1.5
        heat = np.clip((heat + knot) * life, 0, 1)
        cover = np.clip((cover + knot) * life, 0, 1) * smooth(1.0, 0.85, np.hypot(x, y))
        out.append(pack(heat, cover, np.clip(erode + knot, 0, 1)))
    return out


def sheet(frames, cols):
    """Frames (each h × w × 4) laid out left to right, top to bottom."""
    h, w = frames[0].shape[:2]
    rows = math.ceil(len(frames) / cols)
    out = np.zeros((rows * h, cols * w, 4), np.float32)
    out[..., 3] = 1
    for i, f in enumerate(frames):
        r, c = divmod(i, cols)
        out[r * h:(r + 1) * h, c * w:(c + 1) * w] = f
    return out


# --- Tiling strips -------------------------------------------------------------------------------

def tile_trail(n=256, seed=61):
    """A trail's body: crisp streaks running along u (it scrolls and repeats along a trail) over
    a softer wash, with flecks of light, fading across v so a ribbon's edges dissolve."""
    x, y = grid(n)
    v = (y + 1) / 2
    across = np.sin(np.pi * np.clip(v, 0, 1)) ** 1.2
    wash = tiles.stretched(n, 6.0, 1.0, seed).astype(np.float32)
    streaks = smooth(0.52, 0.72, tiles.stretched(n, 14.0, 1.0, seed + 1).astype(np.float32))
    fine = smooth(0.6, 0.8, tiles.stretched(n, 30.0, 1.0, seed + 2).astype(np.float32))
    flecks = (tiles.noise(n, 60, seed + 3, power=0.4) > 0.86).astype(np.float32)
    heat = np.clip((wash * 0.35 + streaks * 0.55 + fine * 0.3 + flecks * 0.6) * across, 0, 1)
    cover = np.clip((0.3 + 0.35 * wash + 0.45 * streaks + 0.3 * flecks) * across, 0, 1)
    erode = tiles.noise(n, 4, seed + 4).astype(np.float32)
    return pack(heat, cover, erode)


def tile_energy(n=256, seed=89):
    """Crackling energy that tiles: a web of thin bright cell walls over a darker, mottled
    body, for warnings' fills and wards to scroll through. B is a noise for flicker."""
    _, gap, _, _ = tiles.voronoi(n, 46, seed, warp=7.0)
    web = np.exp(-(gap / 1.6) ** 2)
    fine_gap = tiles.voronoi(n, 180, seed + 1, warp=3.0)[1]
    fine = np.exp(-(fine_gap / 1.1) ** 2)
    mottle = tiles.noise(n, 5, seed + 2).astype(np.float32)
    heat = np.clip(web * 0.85 + fine * 0.3 + mottle * 0.25, 0, 1)
    cover = np.clip(0.2 + web * 0.65 + fine * 0.2 + mottle * 0.3, 0, 1)
    return pack(heat, cover, tiles.noise(n, 3, seed + 3).astype(np.float32))


def tile_noise(n=256, seed=67):
    """Three unrelated tiling noises (R, G, B) for flicker, breakup and drifting patterns."""
    out = np.ones((n, n, 4), np.float32)
    for c, (scale, s) in enumerate(((3, seed), (6, seed + 1), (12, seed + 2))):
        field = tiles.noise(n, scale, s).astype(np.float32)
        out[..., c] = (field - field.min()) / max(field.max() - field.min(), 1e-6)
    return out


def tile_hex(n=256, cols=7, rows=4):
    """A tiling honeycomb (7 × 4 cells, regular to 1%): bright cell walls (heat), faint cells
    (cover) and a random value per cell in B, so cells can light up one at a time."""
    x, y = grid(n)
    sx, sy = 1.0, math.sqrt(3)
    px = (x + 1) / 2 * cols * sx
    py = (y + 1) / 2 * rows * sy
    ax, ay = np.mod(px, sx) - sx / 2, np.mod(py, sy) - sy / 2
    bx, by = np.mod(px - sx / 2, sx) - sx / 2, np.mod(py - sy / 2, sy) - sy / 2
    use_a = np.hypot(ax, ay) < np.hypot(bx, by)
    cx, cy = np.where(use_a, ax, bx), np.where(use_a, ay, by)
    # Pointy-top hexagons whose neighbours are 1 apart: walls at an apothem of 0.5.
    hexd = np.maximum(np.abs(cx), 0.5 * np.abs(cx) + math.sqrt(3) / 2 * np.abs(cy))
    wall = smooth(0.41, 0.48, hexd)
    ix = np.where(use_a, np.floor(px / sx), np.floor((px - sx / 2) / sx)) % cols
    iy = np.where(use_a, np.floor(py / sy), np.floor((py - sy / 2) / sy)) % rows
    rnd = np.mod(np.sin(ix * 12.9898 + iy * 78.233 + use_a * 37.719) * 43758.5453, 1.0)
    return pack(wall, 0.2 + 0.8 * wall, rnd)
