"""Masonry you can climb: dry-stone field walls and the ruined hall on the hill.

The `world` crate decides where every piece stands and the boxes you climb onto
(crates/world/src/masonry.rs); its `ground_fields` example writes them to art/out/world/places.json
with the drawn ground under each piece. Each prop here is built to fill its blocks exactly, so
the stone you see is the stone you stand on. A climbable top carries a worn, lighter lip of
dressed stone (docs/ART.md, "Reading the world"); masonry too tall to climb ends in broken,
mossy rubble instead.

Pieces are authored in their own frame, Blender's way up (+Z up, x right, y = −z in the game's
frame), with their origin on the ground where `world` puts it. Levels of detail are built here
rather than decimated: a 1 m wall is seen at LOD 2 from about 11 m, so LOD 2 has to read.
"""

import json
import math
import random

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

from . import env, fur, scene as sc, sdf

PLACES = sc.ROOT / "art" / "out" / "world" / "places.json"

# Field walls: the coping's slabs (thickness, overhang past the body), and the body's batter.
SLAB = 0.12
OVERHANG = 0.035
BASE_WIDTH = 1.0
SINK = 0.3


def places():
    """What `world` places and how it's built, from its ground_fields example (run it first)."""
    run = "run `cargo run -p world --release --example ground_fields -- art/out/world`"
    if not PLACES.exists():
        raise SystemExit(f"{PLACES} is missing: {run}")
    places = json.loads(PLACES.read_text())
    if "cottages" not in places:
        raise SystemExit(f"{PLACES} is out of date: {run}")
    return places


def pieces():
    """The masonry `world` places, keyed by prop id."""
    return {m["prop"]: m for m in places()["masonry"]}


def blender(p):
    """A point in the game's frame (x, up, z) in Blender's (x, −z, up)."""
    return Vector((p[0], -p[2], p[1]))


class Ground:
    """The drawn ground round a piece, in its own frame: height above its origin at Blender (x, y)."""

    def __init__(self, g):
        self.x0, self.z0 = g["from"]
        self.step = g["step"]
        self.nx, self.nz = g["size"]
        self.h = np.array(g["heights"], np.float64).reshape(self.nz, self.nx)

    def __call__(self, x, y):
        i = min(max((x - self.x0) / self.step, 0.0), self.nx - 1.001)
        j = min(max((-y - self.z0) / self.step, 0.0), self.nz - 1.001)
        i0, j0 = int(i), int(j)
        fi, fj = i - i0, j - j0
        h = self.h
        return float((h[j0, i0] * (1 - fi) + h[j0, i0 + 1] * fi) * (1 - fj) + (h[j0 + 1, i0] * (1 - fi) + h[j0 + 1, i0 + 1] * fi) * fj)


def _smooth(a, b, x):
    t = min(max((x - a) / (b - a), 0.0), 1.0)
    return t * t * (3 - 2 * t)


def _noise(x, y, seed):
    """Smooth value noise in 0..1 (cheap, for colour and bulges)."""
    from mathutils.noise import noise as perlin
    return 0.5 + 0.5 * perlin(Vector((x, y, seed * 7.31)))


def _box(name, centre, size, mat, rot=None, bevel=0.0, segments=1, jitter=0.0, rng=None):
    """A box (size along its own x, y, z), turned by `rot`, optionally bevelled and roughened."""
    ob = env.box(name, centre, size, mat, rot=rot)
    if bevel:
        mod = ob.modifiers.new("bevel", "BEVEL")
        mod.width = bevel
        mod.segments = segments
        mod.limit_method = "ANGLE"
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=mod.name)
    if jitter and rng is not None:
        for v in ob.data.vertices:
            v.co += Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-0.3, 0.3))) * jitter
    return ob


def _lump(name, centre, radius, scale, yaw, mat, rng, subdiv=2):
    """A rough stone: a noisy, squashed sphere turned by `yaw`."""
    from mathutils.noise import noise as perlin
    off = Vector((rng.random() * 50, rng.random() * 50, rng.random() * 50))
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=subdiv, radius=1.0)
    turn = Matrix.Rotation(yaw, 3, "Z")
    for v in bm.verts:
        d = v.co.normalized()
        k = 1.0 + 0.22 * perlin(d * 1.7 + off) + 0.08 * perlin(d * 4.5 + off)
        v.co = Vector(centre) + turn @ Vector((d.x * scale[0], d.y * scale[1], d.z * scale[2])) * radius * k
    return env.mesh(name, bm, mat, smooth=True)


def _uvs_in_world(objects, origin):
    """World-scale UVs measured in the valley's frame, so the stone runs on unbroken from one
    piece of wall into the next."""
    for ob in objects:
        ob.matrix_world = Matrix.Translation(origin)
        env.world_uvs(ob)
        ob.matrix_world = Matrix.Identity(4)


# --- Field walls ---------------------------------------------------------------------------------

def _stations(line, every):
    """Indices of the coping line's points kept at a level of detail: both ends, every corner
    (where the line turns), and points about `every` metres apart."""
    keep = [0]
    run = 0.0
    for i in range(1, len(line) - 1):
        run += (line[i].xy - line[i - 1].xy).length
        a, b = (line[i] - line[i - 1]).xy.normalized(), (line[i + 1] - line[i]).xy.normalized()
        if a.dot(b) < 0.999 or run >= every - 1e-3:
            keep.append(i)
            run = 0.0
    keep.append(len(line) - 1)
    return keep


def _frames(points):
    """At each point of a polyline: the direction along it and the mitre (how far to push a face
    out at a corner to keep it parallel to both stretches)."""
    out = []
    for i, p in enumerate(points):
        a = (p - points[i - 1]).xy.normalized() if i > 0 else None
        b = (points[i + 1] - p).xy.normalized() if i + 1 < len(points) else None
        t = (a + b).normalized() if a is not None and b is not None else (a if b is None else b)
        mitre = 1.0 / max(t.dot(a if a is not None else b), 0.5)
        out.append((t, mitre))
    return out


def field_wall(piece, heads, lod, seed):
    """One piece of dry-stone field wall at a level of detail (0, 1 or 2): a battered body of
    walling stone under a coping of flat worn slabs, laid exactly on the line you walk along, with
    footings along its foot. Rough grass round it comes from the ground's weights, not the prop.
    `heads` says whether its start and end are the wall's ends (closed off) or joints with the
    next piece. Returns its objects."""
    rng = random.Random(seed)
    ground = Ground(piece["ground"])
    width = 2 * piece["blocks"][0]["half"][1]
    line = [blender(p) for p in piece["line"]]
    keep = _stations(line, (0.5, 1.0, 2.0)[lod])
    pts = [line[i] for i in keep]
    frames = _frames(pts)
    drystone, dressed = env.surface("drystone")[0], env.surface("dressed")[0]
    # Arc length along the piece at each kept point.
    along = [0.0]
    for a, b in zip(pts, pts[1:]):
        along.append(along[-1] + (b.xy - a.xy).length)
    length = along[-1]

    # The body: both faces lofted along the line, battered from BASE_WIDTH at the ground to just
    # inside the coping, bellied a little, sunk SINK into the ground.
    rows = (0.0, 0.3, 0.65, 1.0) if lod < 2 else (0.0, 1.0)
    bm = bmesh.new()
    sides = []
    for side in (1, -1):
        ring = []
        for (p, (t, mitre)), s in zip(zip(pts, frames), along):
            n = Vector((-t.y, t.x))
            top = p.z - SLAB + 0.01
            g = ground(*(p.xy + n * side * BASE_WIDTH / 2 * mitre))
            col = []
            for k, f in enumerate((-1.0,) + rows):
                z = g - SINK if f < 0 else g + (top - g) * f
                half = (BASE_WIDTH / 2 + (width / 2 - OVERHANG - BASE_WIDTH / 2) * max(f, 0.0)) * mitre
                belly = 0.025 * (_noise(s * 0.9, z * 1.4, seed + side) - 0.5) * math.sin(math.pi * max(f, 0.0)) * 2
                xy = p.xy + n * side * (half + belly)
                col.append(bm.verts.new((xy.x, xy.y, z)))
            ring.append(col)
        sides.append(ring)
    for side, ring in zip((1, -1), sides):
        for a, b in zip(ring, ring[1:]):
            for k in range(len(a) - 1):
                quad = (a[k], b[k], b[k + 1], a[k + 1])
                bm.faces.new(quad if side < 0 else quad[::-1])
    left, right = sides
    for i in range(len(pts) - 1):
        bm.faces.new((left[i][-1], left[i + 1][-1], right[i + 1][-1], right[i][-1]))
    for at, end in ((0, heads[0]), (-1, heads[1])):
        if end:
            loop = left[at] + right[at][::-1]
            bm.faces.new(loop if at == 0 else loop[::-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    # UVs in the valley's frame, running unbroken along each straight stretch (and on into the
    # next piece): across a face along its stretch, up it by height.
    origin = blender(piece["origin"])
    uv = bm.loops.layers.uv.new("UVMap")
    scale = 1.0 / drystone["metres"]
    for f in bm.faces:
        c = f.calc_center_median()
        if f.normal.z > 0.9:
            axis = None
        else:
            k = min(range(len(pts) - 1), key=lambda i: ((pts[i].xy + pts[i + 1].xy) / 2 - c.xy).length)
            axis = (pts[k + 1] - pts[k]).xy.normalized()
            if abs(f.normal.xy.normalized().dot(axis)) > 0.7:  # an end of the wall
                axis = Vector((-axis.y, axis.x))
        for loop in f.loops:
            w = loop.vert.co + origin
            loop[uv].uv = (w.x * scale, w.y * scale) if axis is None else (w.xy.dot(axis) * scale, w.z * scale)
    body = env.mesh("body", bm, drystone)
    parts = [body]

    # Coping: flat slabs laid end to end along the line, a square one on each corner.
    def at(u):
        """Point, direction and coping height `u` metres along the piece."""
        k = max(i for i in range(len(along) - 1) if along[i] <= u + 1e-6) if u < length else len(along) - 2
        f = (u - along[k]) / max(along[k + 1] - along[k], 1e-6)
        p = pts[k].lerp(pts[k + 1], min(max(f, 0.0), 1.0))
        return p, (pts[k + 1] - pts[k]).xy.normalized(), (pts[k + 1].z - pts[k].z) / max(along[k + 1] - along[k], 1e-6)

    corners = [along[i] for i in range(1, len(pts) - 1) if frames[i][0].dot((pts[i] - pts[i - 1]).xy.normalized()) < 0.999]
    slabs = []
    u = 0.0 if heads[0] else rng.uniform(0.004, 0.012)
    stops = corners + [length]
    for c in stops:
        end = c - (width / 2 + OVERHANG + 0.012 if c < length else (0.0 if heads[1] else 0.008))
        while u < end - 0.05:
            l = rng.uniform(0.5, 0.85)
            if end - (u + l) < 0.3:
                l = end - u
            slabs.append((u + l / 2, l, False))
            u += l + rng.uniform(0.012, 0.028)
        if c < length:
            slabs.append((c, width + 2 * OVERHANG, True))
            u = c + width / 2 + OVERHANG + 0.012
    for mid, l, corner in slabs:
        p, d, slope = at(mid)
        if corner:
            # Square to the stretch coming into the corner, level at the corner's height.
            i = along.index(mid)
            p, d, slope = pts[i], (pts[i] - pts[i - 1]).xy.normalized(), 0.0
        yaw = math.atan2(d.y, d.x) + (0 if corner else math.radians(rng.uniform(-1.2, 1.2)))
        rot = Matrix.Rotation(yaw, 3, "Z") @ Matrix.Rotation(-math.atan(slope), 3, "Y") @ Matrix.Rotation(
            math.radians(rng.uniform(-1.0, 1.0)), 3, "X")
        th = rng.uniform(0.1, 0.15)
        w = width + 2 * OVERHANG + rng.uniform(-0.03, 0.03)
        centre = Vector((p.x, p.y, p.z)) - rot @ Vector((0, 0, th / 2))
        slab = _box("slab", centre, (l, w, th), dressed, rot=rot, bevel=(0.035, 0.0, 0.0)[lod], segments=1)
        if lod < 2:
            # Weathered and knocked about: each corner of the outline wanders a little, top and
            # bottom together, so the top stays flat where you stand.
            shift = {sx * 2 + sy: Vector((rng.uniform(-1, 1) * 0.03, rng.uniform(-1, 1) * 0.025, 0)) for sx in (0, 1) for sy in (0, 1)}
            inv = rot.inverted()
            for v in slab.data.vertices:
                local = inv @ (v.co - centre)
                v.co += rot @ shift[(local.x > 0) * 2 + (local.y > 0)]
        parts.append(slab)

    if lod == 0:
        # Footings: the big stones of the bottom course, half in the ground.
        for side in (1, -1):
            u = rng.uniform(0.2, 0.9)
            while u < length - 0.3:
                p, d, _ = at(u)
                n = Vector((-d.y, d.x)) * side
                g = ground(*(p.xy + n * BASE_WIDTH / 2))
                r = rng.uniform(0.13, 0.2)
                c = p.xy + n * (BASE_WIDTH / 2 + rng.uniform(-0.02, 0.04))
                parts.append(_lump("footing", Vector((c.x, c.y, g + r * 0.1)), r, (rng.uniform(1.3, 1.9), 1.0, 0.75),
                                   math.atan2(d.y, d.x), dressed, rng, subdiv=1))
                u += rng.uniform(1.6, 2.6)

    _uvs_in_world(parts[1:], origin)
    for ob in parts:
        env.shade(ob, _wall_shade(ground, seed, ob.name.split(".")[0]))
    # One mesh, so the renderer draws it a material at a time, not a stone at a time.
    return [env.join(parts, "wall")]


# Cliff ledges: each a band of pale stone this thick, its front worn round over this much.
LEDGE_THICK = 0.34
LEDGE_LIP = 0.07


def terrace_ledges(piece, lod, seed):
    """Ledges up a limestone cliff at a level of detail (0, 1 or 2): bands of pale, harder stone
    along the strata, each laid on its blocks (`world::ledges`) from the front edge you hang from
    back into the rock, its top the ledge you stand on, its front worn round and its underside in
    shadow. The rock between them is the cliff itself, the terrain. Returns its objects."""
    rng = random.Random(seed)
    ground = Ground(piece["ground"])
    dressed = env.surface("dressed")[0]
    step = (0.25, 0.5, 1.5)[lod]
    bands = {}
    for b in piece["blocks"]:
        bands.setdefault(round(b["top"], 4), []).append(_rect(b))
    parts = []
    for k, (top, rects) in enumerate(sorted(bands.items())):
        rects.sort()
        x0, x1 = rects[0][0], rects[-1][1]
        n = max(2, math.ceil((x1 - x0) / step))
        xs = [x0 + (x1 - x0) * i / n for i in range(n + 1)]

        def edge(x, i):
            """The front (i = 2) or back (i = 3) at x: the block's own, eased into the next one's
            over the last 0.3 m before their joint, so the band reads as one stratum."""
            j = next((j for j, r in enumerate(rects) if x <= r[1] + 1e-6), len(rects) - 1)
            y = rects[j][i]
            for jj in (j - 1, j + 1):
                if 0 <= jj < len(rects):
                    joint = rects[j][0] if jj < j else rects[j][1]
                    w = max(0.0, 1.0 - abs(x - joint) / 0.3) * 0.5
                    y += (rects[jj][i] - rects[j][i]) * w
            return y

        rows = []
        for i, x in enumerate(xs):
            # The front wanders a few centimetres either side of the edge you hang from.
            wobble = 0.0 if lod == 2 else 0.045 * (_noise(x * 0.9, top * 0.37, seed + k) - 0.5) * 2
            front, back = edge(x, 2) + wobble, edge(x, 3)
            under = LEDGE_THICK * (0.85 + 0.3 * _noise(x * 0.6, top, seed + 11 + k)) if lod < 2 else LEDGE_THICK
            lip = LEDGE_LIP if lod < 2 else 0.0
            # Its cross-section, from the top at the back round the worn front to the underside,
            # which tucks back under the lip.
            prof = [(back, top), (front + lip, top), (front + lip * 0.3, top - lip * 0.3), (front, top - lip),
                    (front + 0.03, top - under * 0.7), (front + 0.08, top - under), (back, top - under)]
            if lod == 2:
                prof = [(back, top), (front, top), (front + 0.05, top - under), (back, top - under)]
            rows.append([(x, y, z) for y, z in prof])
        bm = bmesh.new()
        verts = [[bm.verts.new(p) for p in row] for row in rows]
        m = len(verts[0])
        for a, b in zip(verts, verts[1:]):
            for i in range(m):
                bm.faces.new((a[i], a[(i + 1) % m], b[(i + 1) % m], b[i]))
        bm.faces.new(verts[0])
        bm.faces.new(verts[-1][::-1])
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        parts.append(env.mesh("ledge", bm, dressed))
    origin = blender(piece["origin"])
    _uvs_in_world(parts, origin)
    for ob in parts:
        env.shade(ob, _ledge_shade(seed))
    # One mesh, so the renderer draws it a material at a time.
    return [env.join(parts, "ledges")]


def _ledge_shade(seed):
    """COLOR_0 for cliff ledges: the worn top pale, catching the light (a pale edge is always a
    hold), the front a little darker, the underside in shadow; broad patches of weathering."""
    def fn(co, n):
        patch = 0.9 + 0.12 * _noise(co.x * 0.35, co.z * 0.5, seed + 3)
        k = 0.96 if n.z > 0.5 else (0.5 if n.z < -0.5 else 0.8)
        k *= patch
        return k, k * 0.99, k * 0.95
    return fn


def _wall_shade(ground, seed, kind):
    """COLOR_0 for walling: darker and greener toward the damp ground, broad patches of
    weathering. A slab's worn top stays at full brightness, the lip you climb onto; the footings
    take the walling's tone."""
    def fn(co, n):
        h = co.z - ground(co.x, co.y)
        damp = 0.62 + 0.38 * _smooth(0.0, 0.5, h)
        patch = 0.9 + 0.14 * _noise(co.x * 0.35, co.y * 0.35, seed + 3)
        green = 1 - _smooth(0.0, 0.35, h)
        if kind == "slab":
            k = (1.0 if n.z > 0.7 else 0.8 if n.z > -0.3 else 0.6) * (0.92 + 0.08 * _noise(co.x * 2, co.y * 2, seed))
            return k, k, k
        k = (0.86 if kind == "body" else 0.68) * damp * patch
        return k * (1 - 0.07 * green), k, k * (1 - 0.14 * green)
    return fn


def _tuft(at, rng, mat, lod):
    """A tuft of rough grass: a few curving blades, lighter at the tips."""
    bm = bmesh.new()
    col = bm.loops.layers.color.new("Col")
    for _ in range(rng.randint(9, 13) if lod == 0 else 6):
        a = rng.uniform(0, 2 * math.pi)
        d = Vector((math.cos(a), math.sin(a), 0))
        side = Vector((-d.y, d.x, 0))
        length, lean = rng.uniform(0.3, 0.6), rng.uniform(0.25, 0.55)
        base = at + d * rng.uniform(0.0, 0.12)
        rows = []
        for i in range(4):
            t = i / 3
            w = 0.022 * (1 - t) + 0.002
            c = base + d * (lean * length * t * t) + Vector((0, 0, length * t * (1 - 0.15 * t)))
            rows.append((bm.verts.new(c - side * w), bm.verts.new(c + side * w), t))
        for (a0, b0, t0), (a1, b1, t1) in zip(rows, rows[1:]):
            f = bm.faces.new((a0, b0, b1, a1))
            for loop in f.loops:
                t = t0 if loop.vert in (a0, b0) else t1
                k = 0.42 + 0.6 * t
                loop[col] = (k * 0.95, k, k * 0.78, 1.0)
    ob = env.mesh("tuft", bm, mat, smooth=True)
    ob.data.uv_layers.new(name="UVMap")
    return ob


def heads(all_pieces):
    """For each field-wall piece, whether its start and end are ends of the wall (rather than
    joints with the next piece): by whether another piece's line starts or ends there."""
    ends = {}
    for pid, m in all_pieces.items():
        if not m["line"]:
            continue
        o = Vector(m["origin"])
        ends[pid] = [o + Vector(m["line"][0]), o + Vector(m["line"][-1])]
    out = {}
    for pid, (a, b) in ends.items():
        others = [e for q, es in ends.items() if q != pid for e in es]
        out[pid] = tuple(all((p - e).length > 0.05 for e in others) for p in (a, b))
    return out


# --- The ruined hall -----------------------------------------------------------------------------
#
# Its blocks come from `world` in the hall's own frame (here x across, y = −z along, the gable at
# −y facing the village). What stands too tall to climb is drawn taller still, so it never looks
# like a ledge: the gable's buttresses and the door's jambs, which carry a pointed arch. The rest
# has broken down in steps, each top a course of worn, pale capstones. The gable rises to 9 m with
# a lancet window, an oculus and an empty bellcote, and climbs (#249): its coping, the blocks' raked
# tops, is walked up to the bellcote, and its grips (a sill, jamb stones, a keystone) stand proud
# of its outer face. Everything you can climb has the same pale, worn top (docs/ART.md, "Reading
# the world").

CAP = 0.15
# Grip stones run this far back into the wall behind their proud fronts, and stand this tall.
GRIP_BACK, GRIP_HEIGHT = 0.12, 0.24
# Parts built by `_gable` from the gable's blocks rather than as stepped walls.
GABLE_PARTS = ("gable", "bellcote")


def _rect(b):
    """A block's footprint in Blender's frame: (x0, x1, y0, y1)."""
    cx, cy = b["center"][0], -b["center"][1]
    hx, hy = b["half"]
    return cx - hx, cx + hx, cy - hy, cy + hy


def _foot(ground, x0, x1, y0, y1):
    """Below the lowest ground under a footprint, so a wall never shows a gap at its foot."""
    return min(ground(x, y) for x in (x0, (x0 + x1) / 2, x1) for y in (y0, (y0 + y1) / 2, y1)) - 0.35


def _grid(ob, step):
    """Cuts a mesh along level and upright planes every `step` metres, so COLOR_0 has vertices to
    carry damp, moss and streaks."""
    if not step:
        return ob
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    lo = Vector([min(v.co[i] for v in bm.verts) for i in range(3)])
    hi = Vector([max(v.co[i] for v in bm.verts) for i in range(3)])
    for axis in range(3):
        n = Vector([1.0 if i == axis else 0.0 for i in range(3)])
        t = math.floor(lo[axis] / step) * step + step
        while t < hi[axis] - 0.02:
            if t > lo[axis] + 0.02:
                geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
                bmesh.ops.bisect_plane(bm, geom=geom, plane_co=n * t, plane_no=n)
            t += step
    bm.to_mesh(ob.data)
    bm.free()
    return ob


def _cut(ob, cutters):
    """Takes the cutters' volumes out of `ob` (windows, doorways), then deletes the cutters."""
    for c in cutters:
        mod = ob.modifiers.new("cut", "BOOLEAN")
        mod.operation = "DIFFERENCE"
        mod.solver = "EXACT"
        mod.object = c
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=mod.name)
    sc.remove(cutters)
    return ob


def _outline_prism(name, outline, y0, y1, mat):
    """A closed outline in the x–z plane, extruded from y0 to y1."""
    return env.prism(name, [(x, (y0 + y1) / 2, z) for x, z in outline], y1 - y0, (0, 1, 0), mat)


def _pointed(cx, w, spring, apex, n=10, below=None):
    """A pointed (two-centred) arch's opening in the x–z plane, centred on x = cx: up one jamb
    from `below` (or the springing), round both arcs to the apex and down the other."""
    h = apex - spring
    r = (h * h + w * w / 4) / w
    left = [(cx - w / 2 + r - r * math.cos(a), spring + r * math.sin(a)) for a in np.linspace(0, math.asin(h / r), n)]
    right = [(2 * cx - x, z) for x, z in reversed(left[:-1])]
    pts = left + right
    if below is not None:
        pts = [(cx - w / 2, below)] + pts + [(cx + w / 2, below)]
    return pts


def _arch_stones(cx, y0, y1, w, spring, apex, depth, mat, rng, count=11, lod=0):
    """Voussoirs round a pointed arch: wedge stones as deep as the wall, set along the curve."""
    h = apex - spring
    r = (h * h + w * w / 4) / w
    top = math.asin(h / r)
    stones = []
    for side in (-1, 1):
        half = count // 2 + 1
        for k in range(half):
            a0, a1 = top * k / half, top * (k + 1) / half
            ring = []
            for a in (a0, a1):
                for rr in (r, r + depth):
                    x = cx + side * (w / 2 - r + rr * math.cos(a))
                    ring.append((x, spring + rr * math.sin(a)))
            # (inner a0, outer a0, inner a1, outer a1) as a quad outline, inset a hair for the joint.
            quad = [ring[0], ring[2], ring[3], ring[1]]
            c = Vector((sum(p[0] for p in quad) / 4, 0, sum(p[1] for p in quad) / 4))
            quad = [(c.x + (x - c.x) * 0.95, c.z + (z - c.z) * 0.95) for x, z in quad]
            st = _outline_prism("voussoir", quad, y0 - 0.02, y1 + 0.02, mat)
            stones.append(st)
    return stones


def ruin_hall(piece, lod, seed):
    """The ruined hall at a level of detail (0, 1 or 2). Returns its objects."""
    rng = random.Random(seed)
    ground = Ground(piece["ground"])
    gable_blocks = piece
    # Everything but the gable sees it as one wall, first, as it stood before it climbed, so the
    # rest of the hall comes out stone for stone as it did.
    pieces = [b for b in piece["blocks"] if b["part"] in GABLE_PARTS]
    whole = {"part": "gable", "center": [0.0, pieces[0]["center"][1]], "half": [max(abs(b["center"][0]) + b["half"][0] for b in pieces), pieces[0]["half"][1]],
             "yaw": 0.0, "top": None, "rise": 0.0}
    piece = dict(piece, blocks=[whole] + [b for b in piece["blocks"] if b["part"] not in GABLE_PARTS])
    rubble, dressed = env.surface("rubble")[0], env.surface("dressed")[0]
    step = (0.45, 0.75, None)[lod]
    walls, stones = [], []

    def mass(name, x0, x1, y0, y1, z1, z0=None):
        z0 = _foot(ground, x0, x1, y0, y1) if z0 is None else z0
        ob = env.box(name, ((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2), (x1 - x0, y1 - y0, z1 - z0), rubble)
        walls.append(ob)
        return ob

    def capstones(x0, x1, y0, y1, top, along):
        """A course of worn flat stones on a climbable top: the lighter lip you climb onto."""
        length = (y1 - y0) if along == "y" else (x1 - x0)
        u, k = 0.0, 0
        while u < length - 0.02:
            l = rng.uniform(0.45, 0.8)
            if length - (u + l) < 0.3:
                l = length - u
            g = 0.012 if u + l < length - 0.01 else 0.0
            a, b = u + (0.006 if u > 0 else 0.0), u + l - g / 2
            over = 0.025
            if along == "y":
                box = (x0 - over + rng.uniform(-0.01, 0.01), x1 + over + rng.uniform(-0.01, 0.01), y0 + a, y0 + b)
            else:
                box = (x0 + a, x0 + b, y0 - over + rng.uniform(-0.01, 0.01), y1 + over + rng.uniform(-0.01, 0.01))
            th = rng.uniform(0.13, 0.17)
            c = ((box[0] + box[1]) / 2, (box[2] + box[3]) / 2, top - th / 2)
            st = _box("capstone", c, (box[1] - box[0], box[3] - box[2], th), dressed,
                      bevel=(0.03, 0.02, 0.0)[lod], segments=(2, 1, 1)[lod])
            stones.append(st)
            u += l
            k += 1

    # Each block, by what it is.
    jambs = sorted((b for b in piece["blocks"] if b["part"] == "door jamb"), key=lambda b: -b["center"][1])
    for b in piece["blocks"]:
        x0, x1, y0, y1 = _rect(b)
        part = b["part"]
        if b["top"] is not None and part != "fallen":
            along = "y" if (y1 - y0) > (x1 - x0) else "x"
            mass(part, x0, x1, y0, y1, b["top"] - CAP + 0.02)
            capstones(x0, x1, y0, y1, b["top"], along)
        elif part == "fallen":
            stones.append(_fallen(b, ground, dressed, rng, lod))
    gable = _gable(gable_blocks, ground, rubble, dressed, rng, lod, step, walls, stones)
    for b in piece["blocks"]:
        if b["part"] == "buttress":
            _buttress(_rect(b), ground, rubble, dressed, rng, lod, walls, stones)
    _door(jambs, ground, rubble, dressed, rng, lod, walls, stones)
    if lod < 2:
        _plinth(piece, ground, dressed, rng, lod, stones)
        _rubble(piece, ground, dressed, rng, lod, stones)
    green = []
    if lod < 2:
        green += _ivy(ground, lod, rng)
        green += _ruin_tufts(piece, ground, rng, lod)

    for ob in walls:
        _grid(ob, step)
    origin = blender(piece["origin"])
    _uvs_in_world(walls + stones, origin)
    for ob in walls:
        env.shade(ob, _ruin_shade(ground, seed, "wall"))
    for ob in stones:
        env.shade(ob, _ruin_shade(ground, seed, ob.name.split(".")[0]))
    # One mesh, so the renderer draws it a material at a time, not a stone at a time.
    return [env.join(walls + stones + green, "ruin")]


def _fallen(b, ground, mat, rng, lod):
    """A dressed stone fallen from the walls, lying where its block says, its top the step."""
    cx, cy = b["center"][0], -b["center"][1]
    hx, hy = b["half"]
    g = ground(cx, cy)
    height = b["top"] - g + 0.12
    rot = Matrix.Rotation(b["yaw"], 3, "Z") @ Matrix.Rotation(math.radians(rng.uniform(-2, 2)), 3, "X")
    st = _box("fallen", Vector((cx, cy, b["top"] - height / 2)), (2 * hx, 2 * hy, height), mat, rot=rot,
              bevel=(0.05, 0.03, 0.0)[lod], segments=(2, 1, 1)[lod])
    if lod < 2:
        for v in st.data.vertices:
            top = v.co.z > b["top"] - 0.04
            v.co += Vector((rng.uniform(-1, 1) * 0.035, rng.uniform(-1, 1) * 0.035, 0 if top else rng.uniform(-0.04, 0.0)))
    return st


def _top_at(b, x):
    """A raked block's top at x, from its top at its middle and its rise."""
    return b["top"] + b["rise"] * (x - b["center"][0])


def _lift(rise, thick, over):
    """How far the top of a coping stands above the rake it sits on: stones `thick` deep, their
    middles `over` above the rake, on a rake rising `rise` a metre."""
    return over + thick / 2 / math.cos(math.atan(abs(rise)))


def _gable(piece, ground, rubble, dressed, rng, lod, step, walls, stones):
    """The gable end: 7.2 m across, eaves at 6 m and the apex at 9 m, a lancet window and an
    oculus through it, raked coping up both slopes and an empty bellcote on top, all where its
    blocks say: the coping's top is their raked tops, the bellcote's coping its blocks'. Then a
    dressed stone for each of its grips, standing proud of the face."""
    halves = sorted((b for b in piece["blocks"] if b["part"] == "gable"), key=lambda b: b["center"][0])
    bells = sorted((b for b in piece["blocks"] if b["part"] == "bellcote"), key=lambda b: b["center"][0])
    x0, _, y0, y1 = _rect(halves[0])
    x1 = _rect(halves[-1])[1]
    eaves = _top_at(halves[0], x0) - _lift(halves[0]["rise"], 0.24, 0.12)
    apex = _top_at(halves[0], 0.0) - _lift(halves[0]["rise"], 0.24, 0.12)
    bw = -_rect(bells[0])[0]
    bell_eaves = _top_at(bells[0], -bw) - _lift(bells[0]["rise"], 0.16, 0.09)
    bell_apex = _top_at(bells[0], 0.0) - _lift(bells[0]["rise"], 0.16, 0.09)
    z0 = _foot(ground, x0, x1, y0, y1)
    # Up the rakes to the bellcote, which stands on the masonry 0.8 m below the apex.
    shoulder = apex - (apex - eaves) * bw / x1
    outline = [(x0, z0), (x1, z0), (x1, eaves), (bw, shoulder), (bw, apex - 0.8), (-bw, apex - 0.8), (-bw, shoulder), (x0, eaves)]
    ob = _outline_prism("gable", outline, y0, y1, rubble)
    cutters = [_outline_prism("lancet", _pointed(0.0, 1.1, 5.3, 6.45, below=2.6), y0 - 0.3, y1 + 0.3, rubble)]
    if lod < 2:
        ring = [(0.32 * math.cos(a), 7.55 + 0.32 * math.sin(a)) for a in np.linspace(0, 2 * math.pi, 16, endpoint=False)]
        cutters.append(_outline_prism("oculus", ring, y0 - 0.3, y1 + 0.3, rubble))
    _cut(ob, cutters)
    walls.append(ob)
    # Dressed stone round the window: the arch, and jambs and sill.
    if lod < 2:
        stones += _arch_stones(0.0, y0, y1, 1.1, 5.3, 6.45, 0.26, dressed, rng, count=9)
        z = 2.6
        while z < 5.25:
            h = rng.uniform(0.32, 0.45)
            h = min(h, 5.3 - z)
            for side in (-1, 1):
                w = 0.3 if (round(z * 10) % 2) else 0.42
                xi = side * 0.55
                stones.append(_box("jamb", Vector((xi + side * w / 2, (y0 + y1) / 2, z + h / 2)), (w, y1 - y0 + 0.04, h - 0.012), dressed,
                                   bevel=0.015 if lod == 0 else 0.0, segments=1))
            z += h
    # Raked coping up both slopes to the bellcote, kneelers at the eaves.
    for side in (-1, 1):
        a = Vector((x1 if side > 0 else x0, 0, eaves))
        tip = Vector((side * bw, 0, shoulder))
        run = (tip - a).length
        d = (tip - a).normalized()
        n = max(4, int(run / 0.55))
        for k in range(n):
            if lod == 2 and k % 2:
                continue
            l = run / n * (2 if lod == 2 else 1)
            c = a + d * (run * (k + 0.5) / n + (run / n / 2 if lod == 2 else 0))
            centre = Vector((c.x, (y0 + y1) / 2, c.z)) + Vector((0, 0, 0.12))
            stones.append(_box("coping", centre, (l - 0.015, y1 - y0 + 0.12, 0.24), dressed, rot=Matrix.Rotation(-math.atan2(d.z, d.x), 3, "Y"),
                               bevel=0.025 if lod == 0 else 0.0, segments=1))
        end = x1 if side > 0 else x0
        stones.append(_box("kneeler", Vector((end - side * 0.25, (y0 + y1) / 2, eaves - 0.1)), (0.75, y1 - y0 + 0.14, 0.42), dressed,
                           bevel=0.03 if lod == 0 else 0.0, segments=1))
    # The bellcote: a pierced gable on the apex, the bell long gone.
    yc = (y0 + y1) / 2
    bell = _outline_prism("bellcote", [(-bw, apex - 0.9), (bw, apex - 0.9), (bw, bell_eaves), (0.0, bell_apex), (-bw, bell_eaves)],
                          y0 + 0.08, y1 - 0.08, rubble)
    _cut(bell, [_outline_prism("bell", _pointed(0.0, 0.56, apex + 0.35, apex + 0.85, below=apex - 0.05), y0 - 0.3, y1 + 0.3, rubble)])
    walls.append(bell)
    rake = bell_apex - bell_eaves
    for side in (-1, 1):
        mid = Vector((side * bw / 2, yc, bell_eaves + rake / 2 + 0.09))
        stones.append(_box("coping", mid, (math.hypot(bw, rake), y1 - y0 - 0.06, 0.16), dressed,
                           rot=Matrix.Rotation(side * math.atan2(rake, bw), 3, "Y"), bevel=0.02 if lod == 0 else 0.0, segments=1))
    # The grips: dressed stones proud of the face, their tops the edges you hang from, at every
    # level of detail, since they mark the route. The sill runs through the wall under the window;
    # the rest are corbels, their pale tops over a shadowed underside that slopes back to the wall.
    for g in piece.get("grips", []):
        a, b = blender(g["a"]), blender(g["b"])
        front, top, cx, length = a.y, a.z, (a.x + b.x) / 2, abs(b.x - a.x)
        if g["part"] == "lancet sill":
            stones.append(_box("grip", Vector((cx, (front + y1 + 0.02) / 2, top - 0.09)), (length, y1 + 0.02 - front, 0.18), dressed,
                               bevel=0.02 if lod == 0 else 0.0, segments=1))
            continue
        face = y0 + (0.08 if top > apex - 0.8 else 0.0)  # the bellcote stands back from the face
        h = 0.32 if g["part"] == "keystone" else GRIP_HEIGHT
        profile = [(face + GRIP_BACK, top), (front, top), (front, top - h * 0.45), (face, top - h), (face + GRIP_BACK, top - h)]
        stones.append(env.prism("grip", [(cx, y, z) for y, z in profile], length, (1, 0, 0), dressed))
    return ob


def _buttress(rect, ground, rubble, dressed, rng, lod, walls, stones):
    """A two-stage buttress clasping a corner of the gable, its weatherings capped in dressed stone."""
    x0, x1, y0, y1 = rect
    z0 = _foot(ground, x0, x1, y0, y1)
    inner = y1 - 0.62  # the upper stage is set back from the buttress's face
    lower = env.box("buttress", ((x0 + x1) / 2, (y0 + y1) / 2, (z0 + 2.5) / 2), (x1 - x0, y1 - y0, 2.5 - z0), rubble)
    upper = env.box("buttress", ((x0 + x1) / 2, (inner + y1) / 2, (2.5 + 5.2) / 2), (x1 - x0 - 0.12, y1 - inner, 2.7), rubble)
    walls += [lower, upper]
    # Weatherings: sloped caps shedding water off each stage.
    for zb, ya, yb, w in ((2.5, y0, inner, x1 - x0), (5.2, inner, y1, x1 - x0 - 0.12)):
        run = yb - ya
        rise = run * 0.6
        prism = env.prism("weathering", [((x0 + x1) / 2, ya - 0.04, zb), ((x0 + x1) / 2, yb, zb), ((x0 + x1) / 2, yb, zb + rise)], w + 0.06, (1, 0, 0), dressed)
        stones.append(prism)
    # Quoins up the buttress's outer corners.
    if lod < 2:
        for x in (x0, x1):
            z = z0 + 0.35
            k = 0
            while z < 2.4:
                h = rng.uniform(0.3, 0.42)
                long = 0.42 if k % 2 else 0.28
                sx = 1 if x == x1 else -1
                c = Vector((x - sx * long / 2 + sx * 0.015, y0 + 0.2 - 0.015, z + h / 2))
                stones.append(_box("quoin", c, (long, 0.4, h - 0.012), dressed, bevel=0.015 if lod == 0 else 0.0, segments=1))
                z += h
                k += 1


def _door(jambs, ground, rubble, dressed, rng, lod, walls, stones):
    """The doorway between the west wall's two jambs: the jambs standing to a broken top, a pointed
    arch over the opening, and the wall above it broken away."""
    if len(jambs) < 2:
        return
    far, near = [_rect(b) for b in jambs]  # far: next to the gable (−y); near: toward the rear
    x0, x1 = far[0], far[1]
    open0, open1 = far[3], near[2]  # the opening along y
    width = open1 - open0
    yc = (open0 + open1) / 2
    walls.append(_broken(env, rubble, x0, x1, far[2], far[3], _foot(ground, x0, x1, far[2], far[3]), (5.3, 4.7), rng, lod))
    walls.append(_broken(env, rubble, x0, x1, near[2], near[3], _foot(ground, x0, x1, near[2], near[3]), (4.15, 3.85), rng, lod))
    # The arch: masonry over the opening with the pointed head cut out of it, broken off on top.
    spring, apex = 2.15, 3.45
    over = _broken(env, rubble, x0, x1, open0 - 0.02, open1 + 0.02, spring - 0.3, (4.6, 4.15), rng, lod)
    head = _pointed(yc, width, spring, apex, below=spring - 0.5)
    cut = env.prism("doorway", [((x0 + x1) / 2, y, z) for y, z in head], (x1 - x0) + 0.6, (1, 0, 0), rubble)
    _cut(over, [cut])
    walls.append(over)
    if lod < 2:
        for st in _arch_stones(yc, x0, x1, width, spring, apex, 0.24, dressed, rng, count=9):
            # Built across x and deep along y: swap the two so the arch spans along the wall.
            for v in st.data.vertices:
                v.co = Vector((v.co.y, v.co.x, v.co.z))
            st.data.flip_normals()
            stones.append(st)
        # Jamb stones up both sides of the opening.
        for y, sy in ((open0, 1), (open1, -1)):
            z = _foot(ground, x0, x1, y - 0.3, y + 0.3) + 0.35
            k = 0
            while z < spring - 0.05:
                h = min(rng.uniform(0.3, 0.42), spring - z)
                long = 0.4 if k % 2 else 0.26
                c = Vector(((x0 + x1) / 2, y - sy * long / 2 + sy * 0.015, z + h / 2))
                stones.append(_box("jamb", c, (x1 - x0 + 0.03, long, h - 0.012), dressed, bevel=0.015 if lod == 0 else 0.0, segments=1))
                z += h
                k += 1


def _broken(env, mat, x0, x1, y0, y1, z0, tops, rng, lod):
    """Masonry from z0 up to a broken top: high at y0's end (tops[0]) and low at y1's (tops[1]),
    the line between them ragged where stones have fallen."""
    n = max(2, int((y1 - y0) / 0.22)) if lod < 2 else 2
    ys = np.linspace(y0, y1, n + 1)
    # Jagged: stones gone here and there, the odd one still standing proud.
    hs = [tops[0] + (tops[1] - tops[0]) * i / n + (rng.uniform(-0.32, 0.2) if 0 < i < n else 0) for i in range(n + 1)]
    outline = [(y0, z0), (y1, z0)]
    for i in range(n, 0, -1):
        mid = ys[i - 1] + (ys[i] - ys[i - 1]) * rng.uniform(0.35, 0.65)
        outline += [(ys[i], hs[i]), (mid, (hs[i] + hs[i - 1]) / 2 + rng.uniform(-0.15, 0.1))]
    outline += [(ys[0], hs[0])]
    return env.prism("wall", [((x0 + x1) / 2, y, z) for y, z in outline], x1 - x0, (1, 0, 0), mat)


def _ruin_shade(ground, seed, kind):
    """COLOR_0 for the ruin: damp and green at the foot, moss on what faces up and on the shaded
    north faces, rain streaks; the worn tops of what you climb (capstones, the gable's coping and
    kneelers, its grips) stay pale, the lip your hands find."""
    def fn(co, n):
        h = co.z - ground(co.x, co.y)
        damp = 0.6 + 0.4 * _smooth(0.0, 0.7, h)
        patch = 0.86 + 0.18 * _noise(co.x * 0.4, co.y * 0.4 + co.z * 0.3, seed + 5)
        streak = 1 - 0.14 * _smooth(0.55, 0.75, _noise(co.x * 3.0, co.y * 3.0, seed + 9)) * _smooth(1.0, 3.0, h)
        north = _smooth(-0.2, -0.9, n.y)  # faces toward −y, the village, stay in shade
        pale = kind in ("capstone", "grip", "coping", "kneeler")
        moss = max(_smooth(0.6, 0.95, n.z) * (not pale), 0.5 * north * _smooth(0.4, 0.8, _noise(co.x * 0.8, co.z * 0.8, seed + 2)),
                   1 - _smooth(0.0, 0.3, h))
        if pale:
            k = (0.94 if n.z > 0.7 else 0.76 if n.z > -0.3 else 0.58) * (0.92 + 0.08 * _noise(co.x * 2, co.y * 2, seed))
        elif kind in ("fallen", "rubble"):
            k = 0.66 * damp * patch
        else:
            k = (0.84 if kind in ("wall", "gable", "buttress", "bellcote") or kind.endswith("wall") else 0.78) * damp * patch * streak
        g = 0.55 * moss
        return k * (1 - 0.18 * g), k * (1 - 0.02 * g), k * (1 - 0.35 * g)
    return fn


def _plinth(piece, ground, mat, rng, lod, stones):
    """A chamfered base course round the outside of the walls still standing above knee height."""
    for b in piece["blocks"]:
        if b["part"] in ("fallen",) or (b["top"] is not None and b["top"] < 0.9):
            continue
        x0, x1, y0, y1 = _rect(b)
        cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        faces = []
        if b["part"] in ("gable",):
            faces.append(("y", y0, x0 + 1.0, x1 - 1.0, -1))
        elif b["part"] == "buttress":
            faces += [("y", y0, x0, x1, -1), ("x", x0, y0, y1, -1), ("x", x1, y0, y1, 1)]
        elif (y1 - y0) > (x1 - x0):
            faces.append(("x", x0 if cx < 0 else x1, y0, y1, -1 if cx < 0 else 1))
        else:
            faces.append(("y", y1 if cy > 0 else y0, x0, x1, 1 if cy > 0 else -1))
        for axis, at, a, bb, out in faces:
            u = a
            while u < bb - 0.05:
                l = min(rng.uniform(0.5, 0.9), bb - u)
                mid = u + l / 2
                g = ground(*((at, mid) if axis == "x" else (mid, at)))
                h = 0.42 + g
                # A course 0.12 proud of the wall, its top weathered back at 45 degrees.
                pts = [(0.0, g - 0.3), (0.12, g - 0.3), (0.12, h - 0.1), (0.0, h + 0.02)]
                if axis == "x":
                    prof = [(at + out * d, mid, z) for d, z in pts]
                    ob = env.prism("plinth", prof, l - 0.012, (0, 1, 0), mat)
                else:
                    prof = [(mid, at + out * d, z) for d, z in pts]
                    ob = env.prism("plinth", prof, l - 0.012, (1, 0, 0), mat)
                stones.append(ob)
                u += l


def _rubble(piece, ground, mat, rng, lod, stones):
    """Spills of fallen stone: out through the breach in the rear wall and along the foot of the
    broken east wall. Low enough to walk through."""
    spills = [((0.0, 5.6), (1.1, 1.6), 18), ((-4.2, 2.6), (0.6, 2.6), 12), ((-1.2, 2.6), (1.0, 1.4), 7)]
    for (cx, cy), (rx, ry), count in spills:
        for _ in range(count if lod == 0 else count // 2):
            a = rng.uniform(0, 2 * math.pi)
            t = math.sqrt(rng.random())
            x, y = cx + math.cos(a) * rx * t, cy + math.sin(a) * ry * t
            r = rng.uniform(0.1, 0.24) * (1.2 - 0.5 * t)
            g = ground(x, y)
            stones.append(_lump("rubble", Vector((x, y, g + r * 0.25)), r, (rng.uniform(1.0, 1.6), 1.0, 0.65), rng.uniform(0, 6.3), mat, rng,
                                subdiv=1))


def _ivy(ground, lod, rng):
    """Ivy up the gable's outer face beside the window, and a little on the broken east wall: vines
    wandering up and across the stone, leafy clumps along them, thinning as they climb, with the
    wall showing through between."""
    foliage = env.surface("foliage")[0]
    out = []
    for face, (u0, u1), top, axis, vines in ((-5.7, (0.7, 3.4), 6.4, "y", 5), (-3.6, (-3.3, -1.0), 3.4, "x", 3)):
        parts = []
        for _ in range(vines):
            u = rng.uniform(u0, u1)
            z = ground(*((u, face) if axis == "y" else (face, u))) - 0.05
            reach = top * rng.uniform(0.45, 1.0)
            heading = rng.uniform(-0.6, 0.6)
            while z < reach:
                r = (0.22 * (1.0 - 0.5 * z / top) + 0.06) * rng.uniform(0.7, 1.25)
                # Clusters of leaves, pressed flat to the stone: most of each blob is in the wall.
                for _ in range(3):
                    du, dz = rng.uniform(-1, 1) * r, rng.uniform(-0.6, 0.6) * r
                    rr = r * rng.uniform(0.55, 0.85)
                    n = rng.uniform(0.0, 0.03)
                    c = (u + du, face - n, z + dz) if axis == "y" else (face - n, u + du, z + dz)
                    radii = (rr, 0.1, rr * 0.8) if axis == "y" else (0.1, rr, rr * 0.8)
                    parts.append(sdf.Ellipsoid(np.array(c), np.array(radii)))
                heading = max(-1.0, min(1.0, heading + rng.uniform(-0.5, 0.5)))
                u = min(max(u + heading * r * 0.9, u0), u1)
                z += r * rng.uniform(0.6, 1.0)
        field = sdf.Union(parts, k=0.05)
        seed = rng.randint(1, 9999)
        field = sdf.Displace(field, lambda p, s=seed: 0.03 * (2 * fur.fbm(p, 0.07, s, octaves=2) - 1), reach=0.035)
        ob = sdf.mesh(field, 0.04 if lod == 0 else 0.07, "ivy", relax_iters=2)
        # Surface nets can fling a vertex off where the leaves thin to nothing: drop those.
        lo = np.min([q.c - q.r for q in parts], axis=0) - 0.15
        hi = np.max([q.c + q.r for q in parts], axis=0) + 0.15
        bm = bmesh.new()
        bm.from_mesh(ob.data)
        stray = [v for v in bm.verts if np.any(np.array(v.co) < lo) or np.any(np.array(v.co) > hi)]
        bmesh.ops.delete(bm, geom=stray, context="VERTS")
        bm.to_mesh(ob.data)
        bm.free()
        # What's inside the wall is pressed flat against its face.
        for v in ob.data.vertices:
            if axis == "y":
                v.co.y = min(v.co.y, face - 0.004)
            else:
                v.co.x = min(v.co.x, face - 0.004)
        tris = sum(len(p.vertices) - 2 for p in ob.data.polygons)
        target = 2100 if lod == 0 else 800
        if tris > target:
            dec = ob.modifiers.new("decimate", "DECIMATE")
            dec.ratio = target / tris
            sc.activate(ob)
            bpy.ops.object.modifier_apply(modifier=dec.name)
        for p in ob.data.polygons:
            p.use_smooth = True
        ob.data.materials.append(foliage)
        env.box_uvs(ob)
        wall = Vector((0, -1, 0)) if axis == "y" else Vector((-1, 0, 0))
        env.shade(ob, lambda co, n_, w=wall, f=face, ax=axis: _ivy_shade(co, n_, w, f, ax))
        env.wind(ob, lambda co, f=face, ax=axis: min(1.0, abs((co.y if ax == "y" else co.x) - f) / 0.25) * 0.3)
        out.append(ob)
    return out


def _ivy_shade(co, n, wall, face, axis):
    """Ivy's COLOR_0: dark where it presses against the wall, lighter on the outside, a deeper
    green than the meadow's foliage."""
    off = abs((co.y if axis == "y" else co.x) - face)
    k = 0.26 + 0.36 * min(1.0, off / 0.12) + 0.08 * max(0.0, n.dot(wall))
    sun = _noise(co.x * 2.5, co.z * 2.5, 31)
    return k * (0.55 + 0.25 * sun), k * (0.8 + 0.12 * sun), k * 0.46


def _ruin_tufts(piece, ground, rng, lod):
    """Grass at the foot of the walls and in the corners of the hall."""
    grass = env.flat("grass", (0.36, 0.52, 0.16), rough=0.85)
    grass.use_backface_culling = False
    tufts = []
    for b in piece["blocks"]:
        if b["part"] == "fallen":
            continue
        x0, x1, y0, y1 = _rect(b)
        per = max(1, int(((x1 - x0) + (y1 - y0)) * 2 / (1.4 if lod == 0 else 3.0)))
        for _ in range(per):
            side = rng.randrange(4)
            t = rng.random()
            x, y = ((x0 - 0.12, y0 + (y1 - y0) * t), (x1 + 0.12, y0 + (y1 - y0) * t), (x0 + (x1 - x0) * t, y0 - 0.12),
                    (x0 + (x1 - x0) * t, y1 + 0.12))[side]
            tufts.append(_tuft(Vector((x, y, ground(x, y) - 0.03)), rng, grass, lod))
    if not tufts:
        return []
    ob = env.join(tufts, "grass")
    env.wind(ob, lambda co: min(1.0, max(0.0, co.z - ground(co.x, co.y)) / 0.4) ** 1.5)
    for p in ob.data.polygons:
        p.use_smooth = True
    return [ob]
