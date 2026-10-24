"""Wolves, rebuilt: anatomy lofted part by part, then fur, eyes, a nose and teeth.

The body is a union of lofted parts (torso, neck, head, legs, paws, tail) remeshed into one
surface: a deep chest and a tucked waist, a thick neck, a wedge head with broad cheekbones
and a tapering muzzle, muscled upper legs, paws with toes, and a thick tail. The lower jaw is
its own part, so the mouth can open. Everything is in the rig's joint table `J` (metres, facing
-Y, standing on z = 0), times `scale`.

Fur comes from aldermoor_art/fur.py: strands along the coat's flow, a normal map, occlusion
and the markings painted straight into the colour map (beasts aren't tinted), and tufts for the
silhouette: a heavy ruff round the neck and shoulders, cheek fluff, a fringe under the chest
and belly, feathering behind the forelegs, breeches on the haunches and a bushy tail.
"""

import math
import random
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from . import fur, materials as mt, scene as sc, sdf, shapes as sh


def tube(points, radii, n=14):
    """A tube through points with parallel-transported rings (no twist)."""
    secs = []
    side = None
    for i, (p, r) in enumerate(zip(points, radii)):
        tan = (points[min(i + 1, len(points) - 1)] - points[max(i - 1, 0)]).normalized()
        if side is None:
            side = tan.cross(Vector((1, 0, 0)))
            if side.length < 1e-3:
                side = tan.cross(Vector((0, 1, 0)))
        side = (side - tan * side.dot(tan)).normalized()
        up = side.cross(tan).normalized()
        secs.append([p] if r <= 0 else sh.ring(p, side, up, r, r, n))
    return sh.loft(secs)


# --- The body ---------------------------------------------------------------------------------------

def _E(c, r, rx=0.0, ry=0.0, rz=0.0):
    rot = None
    if rx or ry or rz:
        rot = sdf.rot_z(rz) @ sdf.rot_y(ry) @ sdf.rot_x(rx)
    return sdf.Ellipsoid(c, r, rot)


# The head is modelled level, round the cranium's centre, then carried lower and pitched
# nose-down on the neck. Everything on the head (eyes, nose, teeth, ears, its joints) shares it.
HEAD_PIVOT = (0.0, -0.578, 1.03)
HEAD_AT = (0.0, -0.61, 0.925)
HEAD_PITCH = 8.0
# The mouth: a slit between the upper lip and the lower jaw, in the head's frame. Each half
# slopes down outwards, so the upper lip hangs over the jaw. It is modelled a little open, wide
# enough to mesh cleanly, and every clip carries the jaw closed by JAW_CLOSE radians.
SLIT_Z = 0.936
SLIT_HALF = 0.006
JAW_CLOSE = 0.06
SLIT_X = 0.016
SLIT_TILT = 20.0


def slit_z(ax):
    """The middle of the mouth's slit (head frame) at distance ax from the midline."""
    return SLIT_Z - (ax - SLIT_X) * math.tan(math.radians(SLIT_TILT))
# From inside the skull out through the left eye (head frame).
EYE_RAY = ((0.0, -0.60, 1.03), (0.64, -0.77, 0.08))
EYE_LENS = (0.0145, 0.0085, 0.006)
EYE_SOCKET = (0.0135, 0.0078, 0.009)
TOES = ((-0.025, -0.032), (-0.0085, -0.045), (0.0085, -0.045), (0.025, -0.032))


def head_matrix():
    """Head frame to body frame (scale 1)."""
    return (Matrix.Translation(HEAD_AT) @ Matrix.Rotation(math.radians(HEAD_PITCH), 4, "X")
            @ Matrix.Translation(-Vector(HEAD_PIVOT)))


def to_head(q):
    """Body-frame points (N, 3, scale 1) in the head's frame."""
    Mi = np.array(head_matrix().inverted())
    return q @ Mi[:3, :3].T + Mi[:3, 3]


def _paw(c, W, L):
    cx, cy, cz = c
    parts = [_E((cx, cy, cz), (0.034 * W * L, 0.044 * L, 0.026))]
    for dx, dy in TOES:
        parts.append(_E((cx + dx * W * L, cy + dy * L, 0.017), (0.012 * W * L, 0.015 * L, 0.017)))
    return sdf.Union(parts, k=0.01)


def paws(B=1.0):
    """Paw centres and their size (left side, scale 1): front, hind."""
    return [((0.08 * B, -0.33, 0.027), 1.0), ((0.085 * B, 0.425, 0.026), 0.92)]


def anatomy(B=1.0, seed=1):
    """The wolf at scale 1 as a field, and its left eye in the body frame: (centre, normal,
    long axis, short axis).

    Facing -Y on z = 0, about 0.82 m at the withers. The left side is modelled and mirrored.
    """
    W, D = B, 1 + (B - 1) * 0.6
    torso = sdf.Union([
        _E((0, -0.16, 0.615), (0.13 * W, 0.25, 0.19 * D), rx=6),        # ribcage, deepest at the front
        _E((0, -0.30, 0.52), (0.085 * W, 0.12, 0.10 * D)),               # brisket
        _E((0, -0.40, 0.62), (0.075 * W, 0.06, 0.09 * D)),               # point of the chest
        _E((0, -0.24, 0.745), (0.085 * W, 0.13, 0.075)),                 # withers
        _E((0, 0.18, 0.665), (0.105 * W, 0.2, 0.108 * D)),               # loin, tucked up
        _E((0, 0.41, 0.675), (0.108 * W, 0.14, 0.11 * D)),               # rump
        _E((0, 0.5, 0.715), (0.055 * W, 0.07, 0.045)),                   # croup
    ], k=0.06)
    neck = sdf.Union([
        sdf.RoundCone((0, -0.33, 0.72), (0, -0.56, 0.9), 0.13 * W, 0.095 * W),
        _E((0, -0.43, 0.85), (0.11 * W, 0.15, 0.095), rx=-38),          # the mane over the nape
        _E((0, -0.5, 0.76), (0.075 * W, 0.08, 0.08), rx=-38),           # throat
        _E((0, -0.43, 0.65), (0.08 * W, 0.06, 0.11)),                    # the bib of the chest
    ], k=0.05)
    fore = sdf.Union([
        _E((0.08 * W, -0.30, 0.70), (0.035 * W, 0.07, 0.13), rx=-31),   # shoulder blade
        _E((0.088 * W, -0.37, 0.61), (0.045 * W, 0.05, 0.05)),          # point of the shoulder
        sdf.RoundCone((0.09 * W, -0.37, 0.6), (0.09 * W, -0.29, 0.42), 0.055 * W, 0.042 * W),
        _E((0.087 * W, -0.27, 0.52), (0.042 * W, 0.055, 0.085), rx=-15),  # triceps
        _E((0.09 * W, -0.255, 0.415), (0.028 * W, 0.03, 0.032)),         # elbow
    ], k=0.03)
    hind = sdf.Union([
        _E((0.082 * W, 0.36, 0.56), (0.056 * W, 0.08, 0.13), rx=-28),   # front of the thigh
        _E((0.078 * W, 0.45, 0.56), (0.052 * W, 0.075, 0.13), rx=-10),  # hamstrings
        _E((0.092 * W, 0.30, 0.445), (0.035 * W, 0.035, 0.04)),          # stifle
    ], k=0.04)
    (fp, fl), (hp, hl) = paws(B)
    fore_lower = sdf.Union([
        sdf.RoundCone((0.09 * W, -0.28, 0.40), (0.08 * W, -0.29, 0.135), 0.036 * W, 0.021 * W),  # forearm
        _E((0.088 * W, -0.295, 0.33), (0.032 * W, 0.034, 0.075)),       # its muscles
        _E((0.08 * W, -0.292, 0.12), (0.022 * W, 0.024, 0.022)),         # wrist
        sdf.RoundCone((0.08 * W, -0.292, 0.12), (0.08 * W, -0.31, 0.05), 0.02 * W, 0.021 * W),  # pastern
        _paw(fp, W, fl),
    ], k=0.015)
    hind_lower = sdf.Union([
        sdf.RoundCone((0.092 * W, 0.30, 0.44), (0.09 * W, 0.46, 0.2), 0.04 * W, 0.021 * W),  # gaskin
        _E((0.09 * W, 0.40, 0.355), (0.03 * W, 0.04, 0.085), rx=34),    # calf
        _E((0.09 * W, 0.47, 0.195), (0.02 * W, 0.026, 0.024)),          # point of the hock
        sdf.RoundCone((0.09 * W, 0.465, 0.19), (0.085 * W, 0.445, 0.05), 0.02 * W, 0.019 * W),
        _paw(hp, W, hl),
    ], k=0.015)
    tail = sdf.chain([(0, 0.5, 0.735), (0, 0.6, 0.69), (0, 0.68, 0.6), (0, 0.745, 0.49), (0, 0.785, 0.39), (0, 0.80, 0.31)],
                     [0.04 * W, 0.055 * W, 0.062 * W, 0.058 * W, 0.04 * W, 0.012 * W], k=0.02)
    # The head, in its own frame.
    head = sdf.Union([
        _E((0, -0.578, 1.03), (0.068 * W, 0.088, 0.06)),                # cranium
        _E((0, -0.522, 1.045), (0.045 * W, 0.045, 0.042)),               # occiput
        _E((0, -0.632, 1.04), (0.052 * W, 0.042, 0.032)),                # forehead
        _E((0.055 * W, -0.615, 0.985), (0.042 * W, 0.06, 0.045)),        # cheek
        _E((0.078 * W, -0.57, 0.96), (0.04 * W, 0.055, 0.055)),          # the ruff behind the cheek
        sdf.RoundCone((0, -0.665, 0.995), (0, -0.785, 0.985), 0.04 * W, 0.03 * W),  # muzzle
        _E((0, -0.715, 0.952), (0.04 * W, 0.075, 0.026)),                # upper lip
        sdf.RoundCone((0, -0.62, 0.928), (0, -0.778, 0.93), 0.032 * W, 0.018 * W),  # lower jaw
        _E((0, -0.755, 0.922), (0.022 * W, 0.026, 0.014)),               # chin
    ], k=0.025)
    # The eye: where a ray from inside the skull leaves it, in an almond socket.
    p = sdf.march(head, EYE_RAY[0], EYE_RAY[1], 0.2)
    n = sdf.gradient(head, p)
    h = np.cross((0.0, 0.0, 1.0), n)
    h /= np.linalg.norm(h)
    v = np.cross(n, h)
    a = math.radians(18)
    long = h * math.cos(a) + v * math.sin(a)
    short = np.cross(n, long)
    frame = np.stack([long, short, n], axis=1)
    slit = _E((SLIT_X, -0.735, SLIT_Z), (0.042, 0.092, SLIT_HALF), ry=SLIT_TILT)
    cuts = sdf.Union([sdf.Ellipsoid(p, EYE_SOCKET, frame), slit])
    head = sdf.Subtract(head, cuts, k=0.003)
    M = head_matrix()
    R = np.array(M.to_3x3())
    full = sdf.Union([sdf.Union([sdf.Union([torso, neck, fore, hind], k=0.05), fore_lower, hind_lower], k=0.02),
                      tail], k=0.03)
    base = sdf.Union([full, sdf.Transform(head, np.array(M))], k=0.035)
    full = sdf.Union([base] + locks(base, p, B, seed), k=0.006)
    centre = p - n * (EYE_LENS[2] * 0.6)
    eye = (np.array(M @ Vector(centre)), R @ n, R @ long, R @ short)
    return sdf.Mirror(base), sdf.Mirror(full), eye


TAIL = [(0, 0.5, 0.735), (0, 0.6, 0.69), (0, 0.68, 0.6), (0, 0.745, 0.49), (0, 0.785, 0.39), (0, 0.80, 0.31)]


def _along_tail(p):
    """The tail's direction (root to tip) nearest p, and the distance from its axis."""
    best = (1e9, None)
    for a, b in zip(TAIL, TAIL[1:]):
        a, b = np.array(a), np.array(b)
        ab = b - a
        t = np.clip((p - a) @ ab / (ab @ ab), 0, 1)
        d = np.linalg.norm(p - (a + ab * t))
        if d < best[0]:
            best = (d, ab / np.linalg.norm(ab))
    return best[1], best[0]


def locks(base, eye_local, B=1.0, seed=1):
    """Sculpted locks of fur, layered like shingles along the coat's lie: the mane, cheek ruffs,
    the bib, a fringe under the belly, feathering behind the elbows, breeches and the tail.
    Round cones rooted in the surface, tilted a little off it; the left side only (mirrored)."""
    rng = np.random.default_rng(seed)
    R = np.array(head_matrix().to_3x3())
    ez = eye_local[2]

    def local(p):
        return to_head(p[None, :])[0]

    zones = [
        # name, box, keep(p, n), lie(p, n), length, root half-width, lift (degrees), spacing
        ("mane", ((0.0, -0.64, 0.66), (0.17, -0.2, 1.0)),
         lambda p, n: n[2] > -0.25 and local(p)[1] > -0.54 and (p[1] < -0.3 or p[2] > 0.78),
         lambda p, n: (0, 0.8, -0.45), (0.05, 0.08), (0.013, 0.017), 8, 0.022),
        ("cheeks", ((0.03, -0.7, 0.82), (0.16, -0.5, 1.0)),
         lambda p, n: -0.625 < local(p)[1] < -0.52 and local(p)[2] < ez - 0.004 and abs(p[0]) > 0.05 and n[0] > 0.3,
         lambda p, n: R @ np.array((0.5, 1.0, -0.35)), (0.035, 0.05), (0.009, 0.012), 14, 0.015),
        ("bib", ((0.0, -0.62, 0.5), (0.12, -0.34, 0.86)),
         lambda p, n: n[1] < -0.3 and n[2] < 0.4 and local(p)[1] > -0.6,
         lambda p, n: (0, 0.25, -1), (0.04, 0.055), (0.01, 0.013), 8, 0.02),
        ("belly", ((0.0, -0.36, 0.36), (0.12, 0.25, 0.62)),
         lambda p, n: n[2] < -0.55, lambda p, n: (0, 0.45, -1), (0.03, 0.04), (0.008, 0.01), 14, 0.02),
        ("elbows", ((0.03, -0.3, 0.28), (0.16, -0.18, 0.46)),
         lambda p, n: n[1] > 0.35, lambda p, n: (0, 0.4, -1), (0.022, 0.03), (0.006, 0.008), 12, 0.014),
        ("breeches", ((0.02, 0.42, 0.38), (0.17, 0.6, 0.7)),
         lambda p, n: n[1] > 0.3 and _along_tail(p)[1] > 0.075, lambda p, n: (0, 0.35, -1), (0.035, 0.05), (0.009, 0.012), 10, 0.018),
        ("tail", ((0.0, 0.5, 0.24), (0.11, 0.9, 0.8)),
         lambda p, n: p[1] > 0.55 and _along_tail(p)[1] < 0.08, lambda p, n: _along_tail(p)[0], (0.05, 0.07), (0.013, 0.017), 12, 0.022),
    ]
    out = []
    for k, (name, (lo, hi), keep, lie, Lr, rr, lift, spacing) in enumerate(zones):
        lo, hi = np.array(lo), np.array(hi)
        lo[0], hi[0] = lo[0] * B, hi[0] * B
        pts, nrm = sdf.surface_samples(base, lo, hi, 5000, seed * 31 + k)
        inside = np.all((pts >= lo) & (pts <= hi), axis=1) & (pts[:, 0] >= 0)
        ok = np.array([bool(i) and keep(q, m) for i, q, m in zip(inside, pts, nrm)], bool)
        pts, nrm = pts[ok], nrm[ok]
        for i in sdf.thin(pts, spacing * B, seed * 17 + k):
            q, m = pts[i], nrm[i]
            d = np.asarray(lie(q, m), dtype=np.float64)
            t = d - m * (d @ m)
            if np.linalg.norm(t) < 1e-6:
                continue
            t /= np.linalg.norm(t)
            side = np.cross(m, t)
            t = t + side * rng.uniform(-0.18, 0.18)
            t /= np.linalg.norm(t)
            a = math.radians(lift + rng.uniform(-4, 4))
            d = t * math.cos(a) + m * math.sin(a)
            L = rng.uniform(*Lr) * B
            r0 = rng.uniform(*rr) * B
            # Flattened against the skin, with a soft point.
            out.append(sdf.FlatCone(q - m * r0 * 0.25, q + d * L, r0, max(0.0025, r0 * 0.3), m, flat=0.45))
    return out


def _decimate(ob, target):
    tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    if tris > target:
        dec = ob.modifiers.new("dec", "DECIMATE")
        dec.ratio = target / tris
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=dec.name)


def mesh_body(shape, s, step, target, name="body"):
    """A field meshed at `step` (scale 1), decimated to about `target` triangles, at scale s."""
    ob = sdf.mesh(shape, step, name)
    _decimate(ob, target)
    ob.data.transform(Matrix.Scale(s, 4))
    for p in ob.data.polygons:
        p.use_smooth = True
    return ob


def ears(J, s, bulk=1.0):
    """Broad triangular ears, cupped forward, their bases sunk into the skull."""
    parts = []
    for sd, x in (("l", 1), ("r", -1)):
        base = J[f"ear_{sd}"] * s
        tip = J[f"ear_end_{sd}"] * s
        up = (tip - base).normalized()
        front = Vector((x * 0.3, -1, 0))
        front = (front - up * front.dot(up)).normalized()
        out = up.cross(front).normalized()
        if out.x * x < 0:
            out = -out
        height = (tip - base).length + 0.018 * s
        start = base - up * 0.018 * s
        rings = []
        for j in range(11):
            t = j / 10
            c = start + up * (height * t) + front * (0.008 * s * math.sin(math.pi * t))
            if j == 10:
                rings.append([c])
                break
            w = 0.078 * s * bulk * (1 - t) ** 0.95
            d = 0.03 * s * (1 - t) ** 0.7
            r = []
            for i in range(16):
                a = 2 * math.pi * i / 16
                bx = math.cos(a) * w / 2
                by = math.sin(a) * d / 2
                if by < 0:
                    by *= -0.55  # The front is cupped: the opening is a hollow.
                r.append(c + out * bx - front * by)
            rings.append(r)
        bm = sh.loft(rings, cap_start=True)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        parts.append(sh.obj(f"ear_{sd}", bm))
    return parts


# --- Flow and markings ------------------------------------------------------------------------------

def flow_rules(J, s):
    nose = (head_matrix() @ Vector((0, -0.81, 0.99))) * s

    def rule(name, head, tail, p):
        if name in ("head", "jaw"):
            d = p - nose
            d.z -= 0.25 * d.length
            return d
        if name in ("hips", "spine_1", "spine_2", "chest"):
            return Vector((0, 1, -0.45))
        if name in ("neck_1", "neck_2"):
            return Vector((0, 0.8, -1))
        if name.startswith(("ear", "tail", "scapula", "humerus", "radius", "pastern", "fpaw", "femur", "tibia", "hock", "hpaw")):
            return tail - head
        return None

    return rule


def markings(pos, nrm, s, palette, eye, ear, seed=1):
    """Colour per sample (linear RGB): a grizzled coat with a dark saddle and shoulder bars,
    tawny legs and ears, pale underparts, cheeks and brows, and dark lips, eye rims, tail tip,
    tail gland and a pencil stripe down each foreleg. Also how strongly each sample grizzles."""
    q = pos / s
    x, y, z = q[:, 0], q[:, 1], q[:, 2]
    ax = np.abs(x)
    nx, ny, nz = nrm[:, 0], nrm[:, 1], nrm[:, 2]
    nxo = nx * np.sign(x + 1e-9)
    # The head's markings are placed in its own frame.
    hq = to_head(q)
    hn = nrm @ np.array(head_matrix().to_3x3())
    hy, hz = hq[:, 1], hq[:, 2]
    hny, hnz = hn[:, 1], hn[:, 2]
    ec = to_head((np.asarray(eye[0]) / s)[None, :])[0]
    e1 = fur.fbm(q, 0.07, seed, octaves=3) - 0.5
    e2 = fur.fbm(q, 0.022, seed + 3, octaves=2) - 0.5
    head = (hy < -0.5) & (hz > 0.86)
    tail = y > 0.52
    legs = z < 0.5
    # The saddle over the back, the shoulder bars, the top of the tail and the nape.
    back = fur.ramp(0.0, 0.45, nz + 0.6 * e1) * fur.ramp(0.6, 0.7, z) * (y > -0.5) * ~tail
    bar = np.exp(-((y + 0.27 + 0.06 * e1) / 0.045) ** 2) * fur.ramp(0.5, 0.62, z) * (nz > -0.4) * ~head * 0.75
    tail_top = tail * fur.ramp(-0.1, 0.5, ny + 0.5 * nz + 0.5 * e1)
    nape = (y < -0.36) & ~head
    nape = nape * fur.ramp(0.0, 0.5, nz + 0.3 * ny + 0.5 * e1)
    sad = np.clip(np.maximum.reduce([back, bar, tail_top * 0.9, nape * 0.8]), 0, 1)
    # Tawny: the outsides of the legs, the lower flanks, the backs of the ears and the crown.
    ears = ear > 0.5
    ear_front = ears * fur.ramp(0.0, 0.45, -hny + 0.3 * nxo)
    tawny = np.maximum.reduce([legs * fur.ramp(0.5, 0.4, z) * fur.ramp(-0.3, 0.2, nxo + 0.15 * e1),
                               fur.ramp(0.62, 0.52, z + 0.08 * e1) * (y > -0.45) * (y < 0.5) * 0.6,
                               ears * (1 - ear_front) * 0.75, head * ~ears * fur.ramp(1.04, 1.08, hz + 0.02 * e1) * 0.4])
    # Pale: belly, chest, throat, inner legs, paws, the face's lower half, brows, insides of ears.
    under = fur.ramp(-0.2, -0.6, nz + 0.4 * e1) * ~head * ~tail
    bib = (y < -0.36) * (z < 0.85) * (z > 0.5) * fur.ramp(-0.2, -0.7, ny + 0.4 * e1) * ~head
    throat = (y < -0.42) * (z > 0.66) * fur.ramp(-0.1, -0.6, 0.6 * nz + 0.4 * ny + 0.3 * e1) * ~(head & (hz > ec[2] - 0.03))
    inner = legs * fur.ramp(-0.1, -0.5, nxo + 0.2 * e1)
    feet = fur.ramp(0.11, 0.07, z + 0.03 * e1)
    cheeks = head * ~ears * fur.ramp(ec[2] - 0.01, ec[2] - 0.03, hz + 0.03 * e2) * fur.ramp(-0.54, -0.6, hy) * fur.ramp(0.012, 0.03, ax)
    lip_white = head * (hy < -0.64) * fur.ramp(0.968, 0.952, hz + 0.01 * e2) * fur.ramp(0.008, 0.025, ax)
    brow = np.exp(-(((ax - ec[0] + 0.004) / 0.012) ** 2 + ((hy - ec[1] - 0.006) / 0.012) ** 2 + ((hz - ec[2] - 0.024) / 0.008) ** 2))
    tail_under = tail * fur.ramp(0.0, -0.6, ny + 0.4 * e1) * 0.6
    ear_in = ear_front * 0.85
    pale = np.clip(np.maximum.reduce([under, bib, throat, inner * 0.8, feet * 0.7, cheeks, lip_white, brow * 0.9, tail_under, ear_in]), 0, 1)
    # Dark points: the lips and the inside of the mouth, round the eyes, the tail's tip and gland,
    # a stripe down the front of each foreleg, the bridge of the nose.
    rim = np.exp(-(np.sqrt((ax - ec[0]) ** 2 + (hy - ec[1]) ** 2 + (hz - ec[2]) ** 2) / 0.016) ** 6) * head
    sz = SLIT_Z - (ax - SLIT_X) * math.tan(math.radians(SLIT_TILT))
    lips = head * fur.ramp(-0.63, -0.655, hy) * np.exp(-((hz - sz) / 0.009) ** 2)
    mouth = head * (hy < -0.645) * (np.abs(hz - sz) < SLIT_HALF + 0.002)
    gums = mouth * fur.ramp(0.018, 0.011, ax)
    ear_tip = ears * fur.ramp(1.135, 1.16, hz)
    tip = tail * fur.ramp(0.42, 0.34, z + 0.03 * e1)
    gland = np.exp(-((ax / 0.022) ** 2 + ((y - 0.6) / 0.03) ** 2 + ((z - 0.745) / 0.025) ** 2))
    stripe = (y < -0.2) * (z < 0.36) * (z > 0.14) * np.exp(-((ax - 0.084) / 0.009) ** 2) * fur.ramp(-0.3, -0.75, ny)
    bridge = head * (hy < -0.68) * fur.ramp(0.3, 0.8, hnz) * 0.35
    dark = np.clip(np.maximum.reduce([rim * 0.95, lips, mouth, tip, gland * 0.8, stripe * 0.55, bridge, ear_tip * 0.8]), 0, 1)
    coat = np.array(palette["coat"])
    col = coat[None, :] * np.ones((len(q), 1))
    col = col + (np.array(palette["tawny"]) - col) * tawny[:, None]
    col = col + (np.array(palette["saddle"]) - col) * sad[:, None]
    col = col + (np.array(palette["belly"]) - col) * pale[:, None]
    col = col + (np.array(palette["nose"]) - col) * dark[:, None]
    col = col + (np.array((0.1, 0.022, 0.022)) - col) * (gums * 0.85)[:, None]
    # Grizzle: guard hairs are banded, so the back and flanks fleck more than the underparts.
    grizzle = (0.12 + 0.3 * np.clip(1 - pale, 0, 1)) * (1 - dark)
    return col, grizzle


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


def paint(body, skel, J, s, palette, eye, out_dir, stem, size=2048, seed=1):
    """UVs, then the coat (colour with markings, strands and occlusion) and its normal map."""
    unwrap(body)
    m = bpy.data.materials.new(f"coat.{stem}")
    m.use_nodes = True
    body.data.materials.clear()
    body.data.materials.append(m)
    flows = fur.bone_flow(body, skel, flow_rules(J, s))
    ear = np.zeros(len(body.data.vertices))
    groups = {body.vertex_groups[n].index for n in ("ear_l", "ear_r") if n in body.vertex_groups}
    for v in body.data.vertices:
        if any(g.group in groups and g.weight > 0 for g in v.groups):
            ear[v.index] = 1.0
    gb = fur.GBuffer(body, size, flow=flows, attrs={"ear": ear})
    colour, grizzle = markings(gb.pos, gb.nrm, s, palette, eye, gb.attrs["ear"], seed)
    strands, clumps = fur.fur_fields(gb, strand=0.0013, clump=0.009, strand_len=0.014, clump_len=0.03, seed=seed)
    ao = np.clip(fur.bake_ao(body, size, distance=0.05)[gb.iy, gb.ix], 0, 1)
    shade = np.clip(1 + 0.18 * clumps + grizzle * strands, 0.25, 2.0) * (0.62 + 0.38 * ao)
    colour *= shade[:, None]
    height = np.clip(0.5 + 0.2 * clumps + 0.13 * strands, 0, 1)
    out_dir = Path(out_dir)
    img = fur.bleed(gb.image(colour, fill=0.0), gb.mask, 10)
    col_img = fur.save_rgb(mt._linear_to_srgb(np.clip(img, 0, 1)), out_dir / f"{stem}_coat.jpg", "sRGB")
    nrm_img = fur.save_rgb(fur.normal_map(gb, height, 0.0006), out_dir / f"{stem}_coat_normal.jpg", "Non-Color")
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
    bsdf.inputs["Roughness"].default_value = 0.88
    return m


# --- Tufts, eyes, nose and teeth ----------------------------------------------------------------------

def ear_fur(body, skel, s, seed=1):
    """Pale fur lining the cups of the ears, growing up and out from inside."""
    surface = fur.Surface(body)
    objs = []
    for k, side in enumerate(("l", "r")):
        b = skel.data.bones[f"ear_{side}"]
        h, t = skel.matrix_world @ b.head_local, skel.matrix_world @ b.tail_local
        axis = (t - h).normalized()
        x = 1 if side == "l" else -1
        front = Vector((x * 0.3, -1, 0))
        front = (front - axis * front.dot(axis)).normalized()
        out = axis.cross(front).normalized()
        if out.x * x < 0:
            out = -out
        group = f"ear_{side}"

        def zone(p, n, region, h=h, t=t, axis=axis, front=front, group=group):
            idx = surface.nearest(p)[1]
            if surface.weights_at(idx, surface.bary(idx, p)).get(group, 0) < 0.3:
                return 0
            u = (p - h).dot(axis) / (t - h).length
            return 1 if u < 0.7 and n.dot(front) > 0.25 else 0

        roots = fur.scatter(surface, zone, 28, 0.0055 * s, seed * 5 + k)
        objs.append(fur.tufts(surface, roots, lambda p, n, axis=axis, out=out: axis + out * 0.3, length=0.026 * s,
                              width=0.007 * s, lift=0.45, droop=0.2, segments=2, seed=seed * 7 + k, name=f"fur_ear_{side}"))
    objs = [o for o in objs if len(o.data.polygons)]
    for o in objs:
        o.data.materials.append(body.data.materials[0])
    from .fox import fur_join
    ob = fur_join(objs, "fur")
    ob["lod_max"] = 1
    return ob


def _glossy(name, colour, rough):
    m = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1)
    bsdf.inputs["Roughness"].default_value = rough
    return m


def eye_texture(out_dir, stem, iris):
    """An amber (or red) iris with a round pupil and a dark rim, for the eye lenses' planar UVs."""
    size = 256
    v, u = np.mgrid[0:size, 0:size]
    u = (u + 0.5) / size - 0.5
    v = (v + 0.5) / size - 0.5
    r = np.sqrt(u * u + v * v) * 2 * EYE_LENS[0]  # metres from the lens's centre
    ang = np.arctan2(v, u)
    rng = np.random.default_rng(5)
    fibres = sum(np.sin(ang * rng.integers(24, 70) + rng.uniform(0, 6.28)) * rng.uniform(0.3, 1.0) for _ in range(6)) / 6
    iris_r, pupil_r = 0.0078, 0.0031
    t = np.clip(r / iris_r, 0, 1)
    inner = np.array(iris) * 1.15
    outer = np.array(iris) * np.array([0.55, 0.4, 0.3])
    col = inner[None, None, :] * (1 - t[..., None]) ** 0.6 + outer[None, None, :] * (1 - (1 - t[..., None]) ** 0.6)
    col = col * (1 + 0.14 * fibres)[..., None]
    ring = np.clip((r - iris_r * 0.85) / (iris_r * 0.15), 0, 1)
    col = col * (1 - 0.8 * ring)[..., None]
    col = col * (1 - np.clip((pupil_r - r) / 0.0004 + 0.5, 0, 1))[..., None]
    sclera = np.array((0.05, 0.035, 0.022))
    a = np.clip((iris_r - r) / 0.0004 + 0.5, 0, 1)
    col = col * a[..., None] + sclera[None, None, :] * (1 - a[..., None])
    rgba = np.ones((size, size, 4), np.float32)
    rgba[..., :3] = mt._linear_to_srgb(np.clip(col, 0, 1))
    return mt._save(rgba, Path(out_dir) / f"{stem}_eye.jpg", "sRGB")


def eyes(eye, s, out_dir, stem, iris=(0.85, 0.5, 0.08)):
    """Glossy lenses in almond sockets: their rims sit under the lids, their fronts just proud."""
    c, n, long, short = eye
    img = eye_texture(out_dir, stem, iris)
    m = bpy.data.materials.get(f"eye.{stem}") or bpy.data.materials.new(f"eye.{stem}")
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    bsdf.inputs["Roughness"].default_value = 0.06
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    parts = []
    for x in (1, -1):
        centre = Vector((c.x * x, c.y, c.z))
        axes = [Vector((a.x * x, a.y, a.z)) for a in (long, short, n)]
        bm = bmesh.new()
        bmesh.ops.create_uvsphere(bm, u_segments=24, v_segments=16, radius=1.0)
        uv = bm.loops.layers.uv.new("UVMap")
        for f in bm.faces:
            for loop in f.loops:
                p = loop.vert.co
                # Planar UVs across the lens, the same scale both ways so the iris stays round.
                loop[uv].uv = (0.5 + p.x * x * 0.5, 0.5 + p.y * EYE_LENS[1] / EYE_LENS[0] * 0.5)
        for vtx in bm.verts:
            p = vtx.co
            vtx.co = centre + (axes[0] * (p.x * EYE_LENS[0]) + axes[1] * (p.y * EYE_LENS[1]) + axes[2] * (p.z * EYE_LENS[2])) * s
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        ob = sh.obj(f"eye_{'l' if x > 0 else 'r'}", bm, None, smooth=True)
        ob.data.materials.append(m)
        parts.append(ob)
    return parts


def _to_body(ob, s):
    ob.data.transform(Matrix.Scale(s, 4) @ head_matrix())


def nose(s, bulk=1.0):
    """The nose leather, with nostrils."""
    pad = sdf.Union([
        _E((0, -0.802, 0.99), (0.022 * bulk, 0.017, 0.017)),
        _E((0, -0.79, 0.998), (0.017 * bulk, 0.02, 0.013)),
    ], k=0.006)
    nostril = sdf.Mirror(_E((0.01 * bulk, -0.817, 0.987), (0.0055, 0.007, 0.0035), rz=-25))
    ob = sdf.mesh(sdf.Subtract(pad, nostril, k=0.002), 0.0012, "nose", relax_iters=3)
    _decimate(ob, 900)
    _to_body(ob, s)
    ob.data.materials.append(_glossy("wolf_nose", (0.012, 0.01, 0.01), 0.32))
    return ob


def teeth(s, bulk=1.0):
    """Fangs and incisors, upper on the head and lower on the jaw: hidden until the mouth opens."""
    ivory = _glossy("wolf_teeth", (0.75, 0.7, 0.58), 0.35)
    upper, lower = [], []
    B = bulk
    for x in (1, -1):
        upper.append(sh.obj("fang", tube([Vector(p) for p in ((x * 0.0155 * B, -0.752, 0.948), (x * 0.015 * B, -0.754, 0.935),
                                                               (x * 0.0145 * B, -0.756, 0.922))], [0.0042, 0.003, 0.0], 8)))
        lower.append(sh.obj("fang", tube([Vector(p) for p in ((x * 0.013 * B, -0.765, 0.926), (x * 0.013 * B, -0.766, 0.938),
                                                              (x * 0.0125 * B, -0.767, 0.948))], [0.0036, 0.0024, 0.0], 8)))
        for k in range(3):
            px = x * (0.0035 + 0.0045 * k) * B
            py = -0.797 + 0.004 * k
            upper.append(sh.obj("incisor", tube([Vector((px, py, 0.948)), Vector((px, py - 0.001, 0.94))], [0.0019, 0.0], 6)))
            lower.append(sh.obj("incisor", tube([Vector((px * 0.9, py + 0.004, 0.926)), Vector((px * 0.9, py + 0.003, 0.933))],
                                                [0.0017, 0.0], 6)))
    for o in upper + lower:
        o.data.materials.append(ivory)
    up, lo = sh.join(upper, "teeth_upper"), sh.join(lower, "teeth_lower")
    _to_body(up, s)
    _to_body(lo, s)
    return up, lo


def claws(s, bulk=1.0):
    """Four blunt dark claws per paw, at the front of each toe: (front claws, hind claws)."""
    horn = _glossy("wolf_claw", (0.02, 0.018, 0.016), 0.4)
    out = []
    for (cx, cy, cz), L in paws(bulk):
        parts = []
        for x in (1, -1):
            for dx, dy in TOES:
                base = Vector(((cx + dx * bulk * L) * x, cy + dy * L - 0.011 * L, 0.02))
                tip = base + Vector((0, -0.012, -0.016)) * L
                parts.append(sh.obj("claw", tube([base * s, (base * 0.4 + tip * 0.6) * s, tip * s], [0.0042 * s, 0.003 * s, 0.0], 6)))
        ob = sh.join(parts, "claws")
        ob.data.materials.append(horn)
        out.append(ob)
    return out
