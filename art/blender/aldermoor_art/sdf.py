"""Signed distance fields: organic shapes from smooth unions of simple primitives.

A shape is a tree of nodes (ellipsoids, round cones, smooth unions and subtractions), evaluated
on a voxel grid and meshed by surface nets; the vertices are then relaxed and projected back onto
the surface, so the mesh is smooth and exact. Muscles flow into one another through the smooth
minimum's blend radius `k`, and sockets and creases are carved with smooth subtraction.

    shape = Union([Ellipsoid(...), RoundCone(...)], k=0.03)
    ob = mesh(shape, step=0.006, name="body")

Distances are in metres. Nodes report bounds, so each is evaluated only near where it matters.
"""

import math

import bpy
import numpy as np
from mathutils import Matrix

BIG = 1e3


def _v(x):
    return np.asarray(x, dtype=np.float64)


def rot_x(deg):
    return np.array(Matrix.Rotation(math.radians(deg), 3, "X"))


def rot_y(deg):
    return np.array(Matrix.Rotation(math.radians(deg), 3, "Y"))


def rot_z(deg):
    return np.array(Matrix.Rotation(math.radians(deg), 3, "Z"))


def aim(direction, up=(0, 0, 1)):
    """A rotation taking local +Y to `direction`, local +Z as close to `up` as possible."""
    y = _v(direction) / np.linalg.norm(direction)
    x = np.cross(y, _v(up))
    if np.linalg.norm(x) < 1e-6:
        x = np.cross(y, _v((1, 0, 0)))
    x /= np.linalg.norm(x)
    z = np.cross(x, y)
    return np.stack([x, y, z], axis=1)


def smin(a, b, k):
    if k <= 0:
        return np.minimum(a, b)
    h = np.maximum(k - np.abs(a - b), 0.0) / k
    return np.minimum(a, b) - h * h * k * 0.25


def smax(a, b, k):
    return -smin(-a, -b, k)


class Node:
    def bounds(self):
        raise NotImplementedError

    def __call__(self, p):
        raise NotImplementedError


class Ellipsoid(Node):
    """Radii r along the local axes; `rot` (3x3) turns local into world."""

    def __init__(self, c, r, rot=None):
        self.c = _v(c)
        self.r = _v(r)
        self.R = None if rot is None else _v(rot)

    def __call__(self, p):
        q = p - self.c
        if self.R is not None:
            q = q @ self.R
        k0 = np.linalg.norm(q / self.r, axis=-1)
        k1 = np.linalg.norm(q / (self.r * self.r), axis=-1)
        return k0 * (k0 - 1.0) / np.maximum(k1, 1e-9)

    def bounds(self):
        e = self.r if self.R is None else np.sqrt(((self.R * self.r[None, :]) ** 2).sum(1))
        return self.c - e, self.c + e


class RoundCone(Node):
    """A capsule from a (radius ra) to b (radius rb): a cone capped by two spheres."""

    def __init__(self, a, b, ra, rb):
        self.a, self.b = _v(a), _v(b)
        self.ra, self.rb = float(ra), float(rb)

    def __call__(self, p):
        a, b, r1, r2 = self.a, self.b, self.ra, self.rb
        ba = b - a
        l2 = ba @ ba
        rr = r1 - r2
        a2 = l2 - rr * rr
        il2 = 1.0 / l2
        pa = p - a
        y = pa @ ba
        z = y - l2
        x2 = ((pa * l2 - y[..., None] * ba) ** 2).sum(-1)
        y2 = y * y * l2
        z2 = z * z * l2
        k = np.sign(rr) * rr * rr * x2
        d_b = np.sqrt(x2 + z2) * il2 - r2
        d_a = np.sqrt(x2 + y2) * il2 - r1
        d_m = (np.sqrt(np.maximum(x2 * a2 * il2, 0.0)) + y * rr) * il2 - r1
        return np.where(np.sign(z) * a2 * z2 > k, d_b, np.where(np.sign(y) * a2 * y2 < k, d_a, d_m))

    def bounds(self):
        r = max(self.ra, self.rb)
        return np.minimum(self.a, self.b) - r, np.maximum(self.a, self.b) + r


class FlatCone(Node):
    """A round cone squashed across `up` to `flat` of its thickness: a lock of fur lying on a skin."""

    def __init__(self, a, b, ra, rb, up, flat=0.5):
        self.cone = RoundCone(a, b, ra, rb)
        ab = _v(b) - _v(a)
        ab /= np.linalg.norm(ab)
        u = _v(up) - ab * (_v(up) @ ab)
        self.u = u / max(np.linalg.norm(u), 1e-9)
        self.a = _v(a)
        self.flat = flat

    def bounds(self):
        return self.cone.bounds()

    def __call__(self, p):
        h = (p - self.a) @ self.u
        q = p + self.u[None, :] * (h * (1.0 / self.flat - 1.0))[:, None]
        return self.cone(q) * self.flat


def chain(points, radii, k=0.0):
    """Round cones through a run of points: legs, tails, locks of fur."""
    return Union([RoundCone(points[i], points[i + 1], radii[i], radii[i + 1]) for i in range(len(points) - 1)], k)


class Union(Node):
    def __init__(self, children, k=0.0):
        self.children = [c for c in children if c is not None]
        self.k = k
        self._bounds = None
        self._child_bounds = None

    def bounds(self):
        if self._bounds is None:
            bs = [c.bounds() for c in self.children]
            self._child_bounds = bs
            self._bounds = (np.min([b[0] for b in bs], axis=0), np.max([b[1] for b in bs], axis=0))
        return self._bounds

    def __call__(self, p):
        out = np.full(p.shape[:-1], BIG)
        pad = self.k + MARGIN
        self.bounds()
        plo, phi = p.min(axis=0), p.max(axis=0)
        for c, (lo, hi) in zip(self.children, self._child_bounds):
            # Children nowhere near these points are skipped without testing each point.
            if np.any(hi + pad < plo) or np.any(lo - pad > phi):
                continue
            m = np.all((p >= lo - pad) & (p <= hi + pad), axis=-1)
            if not m.any():
                continue
            d = np.full(p.shape[:-1], BIG)
            d[m] = c(p[m])
            out = smin(out, d, self.k)
        return out


class Subtract(Node):
    """`a` with `b` carved out of it, the cut's edge rounded by k."""

    def __init__(self, a, b, k=0.0):
        self.a, self.b, self.k = a, b, k

    def bounds(self):
        return self.a.bounds()

    def __call__(self, p):
        d = self.a(p)
        lo, hi = self.b.bounds()
        pad = self.k + MARGIN
        m = np.all((p >= lo - pad) & (p <= hi + pad), axis=-1)
        if m.any():
            cut = np.full(p.shape[:-1], BIG)
            cut[m] = self.b(p[m])
            d = smax(d, -cut, self.k)
        return d


class Mirror(Node):
    """The child and its reflection across x = 0 (pass the left-hand side, x > 0)."""

    def __init__(self, child, k=0.0):
        self.child, self.k = child, k

    def bounds(self):
        lo, hi = self.child.bounds()
        m = max(abs(lo[0]), abs(hi[0]))
        return np.array([-m, lo[1], lo[2]]), np.array([m, hi[1], hi[2]])

    def __call__(self, p):
        q = p.copy()
        q[..., 0] = np.abs(q[..., 0])
        return self.child(q)


class Transform(Node):
    """The child moved by a rigid 4x4 matrix (a rotation and a translation)."""

    def __init__(self, child, M):
        M = np.asarray(M, dtype=np.float64)
        self.child, self.R, self.t = child, M[:3, :3], M[:3, 3]

    def bounds(self):
        lo, hi = self.child.bounds()
        c = np.array([[x, y, z] for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])])
        w = c @ self.R.T + self.t
        return w.min(0), w.max(0)

    def __call__(self, p):
        return self.child((p - self.t) @ self.R)


class Displace(Node):
    """The child's surface pushed out by fn(p) metres (fn of world points, at most `reach`)."""

    def __init__(self, child, fn, reach):
        self.child, self.fn, self.reach = child, fn, reach

    def bounds(self):
        lo, hi = self.child.bounds()
        return lo - self.reach, hi + self.reach

    def __call__(self, p):
        return self.child(p) - self.fn(p)


MARGIN = 0.03


def evaluate(shape, lo, hi, step):
    """The field on a grid from lo to hi: (values[nx, ny, nz], origin)."""
    lo, hi = _v(lo), _v(hi)
    n = np.ceil((hi - lo) / step).astype(int) + 1
    xs = lo[0] + np.arange(n[0]) * step
    zs = lo[2] + np.arange(n[2]) * step
    out = np.empty(tuple(n), np.float32)
    slab = max(1, 400000 // (n[0] * n[2]))
    for j0 in range(0, n[1], slab):
        ys = lo[1] + np.arange(j0, min(n[1], j0 + slab)) * step
        P = np.stack(np.meshgrid(xs, ys, zs, indexing="ij"), -1)
        out[:, j0:j0 + len(ys), :] = shape(P.reshape(-1, 3)).reshape(P.shape[:-1])
    return out, lo


_CORNERS = np.array([(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 0, 1), (1, 0, 1), (0, 1, 1), (1, 1, 1)])
_EDGES = [(a, b) for a in range(8) for b in range(a + 1, 8) if np.abs(_CORNERS[a] - _CORNERS[b]).sum() == 1]


def surface_nets(F, origin, step):
    """Quads on the zero surface of a grid field (negative inside): (verts, quads)."""
    nx, ny, nz = F.shape
    inside = F < 0
    cells = np.zeros((nx - 1, ny - 1, nz - 1), np.uint8)
    for k, (dx, dy, dz) in enumerate(_CORNERS):
        cells |= inside[dx:nx - 1 + dx, dy:ny - 1 + dy, dz:nz - 1 + dz].astype(np.uint8) << k
    active = (cells != 0) & (cells != 255)
    idx = np.argwhere(active)
    vid = np.full(active.shape, -1, np.int64)
    vid[active] = np.arange(len(idx))
    vals = np.stack([F[idx[:, 0] + dx, idx[:, 1] + dy, idx[:, 2] + dz] for dx, dy, dz in _CORNERS], 1).astype(np.float64)
    acc = np.zeros((len(idx), 3))
    cnt = np.zeros(len(idx))
    for a, b in _EDGES:
        va, vb = vals[:, a], vals[:, b]
        cross = (va < 0) != (vb < 0)
        t = np.where(cross, va / np.where(cross, va - vb, 1.0), 0.0)
        pt = _CORNERS[a][None, :] + t[:, None] * (_CORNERS[b] - _CORNERS[a])[None, :]
        acc += pt * cross[:, None]
        cnt += cross
    verts = origin + (idx + acc / np.maximum(cnt, 1)[:, None]) * step
    quads = []
    # An edge along each axis whose ends differ in sign is crossed by the quad of the four cells
    # around it, wound so the face looks out of the solid.
    for axis in range(3):
        sl_a = [slice(None)] * 3
        sl_b = [slice(None)] * 3
        sl_a[axis] = slice(0, -1)
        sl_b[axis] = slice(1, None)
        ia, ib = inside[tuple(sl_a)], inside[tuple(sl_b)]
        change = ia != ib
        # The four cells need indices - 1 on the two other axes.
        o1, o2 = [ax for ax in range(3) if ax != axis]
        if axis == 1:
            o1, o2 = 2, 0
        pos = np.argwhere(change)
        pos = pos[(pos[:, o1] >= 1) & (pos[:, o2] >= 1) & (pos[:, o1] < F.shape[o1] - 1) & (pos[:, o2] < F.shape[o2] - 1)]
        if not len(pos):
            continue
        out_dir = ia[tuple(pos.T)]  # True when the inside is at the lower end: the face looks +axis.

        def cell(d1, d2):
            c = pos.copy()
            c[:, o1] -= d1
            c[:, o2] -= d2
            return vid[c[:, 0], c[:, 1], c[:, 2]]

        c0, c1, c2, c3 = cell(1, 1), cell(0, 1), cell(0, 0), cell(1, 0)
        q = np.stack([c0, c1, c2, c3], 1)
        q[~out_dir] = q[~out_dir][:, ::-1]
        quads.append(q)
    quads = np.concatenate(quads)
    quads = quads[(quads >= 0).all(1)]
    return verts, quads


def _neighbours(quads):
    e = np.concatenate([quads[:, [0, 1]], quads[:, [1, 2]], quads[:, [2, 3]], quads[:, [3, 0]]])
    e = np.concatenate([e, e[:, ::-1]])
    return e


def sample(shape, pts, chunk=30000):
    """The field at scattered points, taken in slices along y so unions can skip far children."""
    if len(pts) <= chunk:
        return shape(pts)
    order = np.argsort(pts[:, 1], kind="stable")
    out = np.empty(len(pts))
    for i in range(0, len(pts), chunk):
        idx = order[i:i + chunk]
        out[idx] = shape(pts[idx])
    return out


def project(shape, pts, iters=3, h=1e-4):
    """Moves points onto the zero surface along the field's gradient."""
    pts = pts.copy()
    for _ in range(iters):
        d = sample(shape, pts)
        g = np.stack([(sample(shape, pts + o) - sample(shape, pts - o)) / (2 * h) for o in np.eye(3) * h], -1)
        gl = (g * g).sum(-1)
        pts -= (d / np.maximum(gl, 1e-8))[:, None] * g
    return pts


def relax(shape, verts, quads, iters=4, lam=0.5):
    """Laplacian relaxation, then projection back onto the surface: even, exact vertices."""
    e = _neighbours(quads)
    deg = np.bincount(e[:, 0], minlength=len(verts)).astype(np.float64)
    for _ in range(iters):
        acc = np.zeros_like(verts)
        np.add.at(acc, e[:, 0], verts[e[:, 1]])
        avg = acc / np.maximum(deg, 1)[:, None]
        verts = verts + lam * (avg - verts)
        verts = project(shape, verts, iters=2)
    return verts


def surface_samples(shape, lo, hi, count, seed=1, iters=6):
    """Points on the surface inside a box, with their outward normals: (points, normals)."""
    rng = np.random.default_rng(seed)
    pts = rng.uniform(lo, hi, size=(count, 3))
    pts = project(shape, pts, iters=iters)
    ok = np.abs(sample(shape, pts)) < 1e-4
    pts = pts[ok]
    h = 1e-4
    g = np.stack([(sample(shape, pts + o) - sample(shape, pts - o)) / (2 * h) for o in np.eye(3) * h], -1)
    g /= np.maximum(np.linalg.norm(g, axis=-1, keepdims=True), 1e-9)
    return pts, g


def thin(points, spacing, seed=1):
    """A random subset no two of which are closer than `spacing` (indices)."""
    order = np.random.default_rng(seed).permutation(len(points))
    keep = []
    cells = {}
    for i in order:
        p = points[i]
        key = tuple((p // spacing).astype(int))
        near = [j for dx in (-1, 0, 1) for dy in (-1, 0, 1) for dz in (-1, 0, 1)
                for j in cells.get((key[0] + dx, key[1] + dy, key[2] + dz), ())]
        if all(np.linalg.norm(points[j] - p) >= spacing for j in near):
            keep.append(i)
            cells.setdefault(key, []).append(i)
    return np.array(keep, dtype=int)


def march(shape, origin, direction, far, tol=1e-5):
    """The first point where a ray from inside the shape leaves it (by bisection)."""
    o, d = _v(origin), _v(direction) / np.linalg.norm(direction)
    a, b = 0.0, far
    if shape(o[None, :])[0] >= 0 or shape((o + d * far)[None, :])[0] < 0:
        return None
    while b - a > tol:
        m = (a + b) / 2
        if shape((o + d * m)[None, :])[0] < 0:
            a = m
        else:
            b = m
    return o + d * (a + b) / 2


def gradient(shape, p, h=1e-4):
    p = _v(p)[None, :]
    g = np.array([(shape(p + o)[0] - shape(p - o)[0]) / (2 * h) for o in np.eye(3) * h])
    return g / max(np.linalg.norm(g), 1e-9)


def mesh(shape, step, name="sdf", lo=None, hi=None, relax_iters=4):
    """Meshes a shape into a new object (quads, smooth shading)."""
    if lo is None or hi is None:
        blo, bhi = shape.bounds()
        lo = blo - 3 * step if lo is None else lo
        hi = bhi + 3 * step if hi is None else hi
    F, origin = evaluate(shape, lo, hi, step)
    verts, quads = surface_nets(F, origin, step)
    if relax_iters:
        verts = relax(shape, verts, quads, relax_iters)
    me = bpy.data.meshes.new(name)
    me.vertices.add(len(verts))
    me.vertices.foreach_set("co", verts.astype(np.float32).ravel())
    me.loops.add(len(quads) * 4)
    me.loops.foreach_set("vertex_index", quads.astype(np.int32).ravel())
    me.polygons.add(len(quads))
    me.polygons.foreach_set("loop_start", np.arange(0, len(quads) * 4, 4, dtype=np.int32))
    me.update(calc_edges=True)
    me.validate()
    for p in me.polygons:
        p.use_smooth = True
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    return ob
