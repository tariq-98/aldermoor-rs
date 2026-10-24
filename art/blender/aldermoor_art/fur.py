"""Fur for any skinned body: painted strands that follow the fur's flow, a matching normal map,
baked occlusion, and geometric tufts that break up the silhouette.

The fur grows along a flow field: a direction on the surface at every vertex (down the limbs,
back from the nose, along the tail). Everything else follows it:
- `GBuffer` rasterises the mesh into its UV layout once: world position, normal, flow and any
  vertex attributes per texel, plus each texel's size in metres.
- `streaks` smears world-space noise along the flow (line integral convolution in texture
  space), so strands and clumps keep the same size in metres wherever the UVs are denser.
- `normal_map` turns the strand height into a tangent-space normal map (glTF: +Y up).
- `tufts` grows little curved blades of fur from the surface. Each vertex takes its UV, normal
  and skin weights from the body under it, so tufts wear the body's colours and markings, hide
  with the body's regions and bend with its bones.
"""

import math
import random
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import materials as mt, regions as rg, scene as sc


# --- Noise ------------------------------------------------------------------------------------

def _hash(ix, iy, iz, seed):
    h = (ix * 73856093) ^ (iy * 19349663) ^ (iz * 83492791) ^ (seed * 2654435761)
    h = h & 0xFFFFFFFF
    h ^= h >> 13
    h = (h * 0x5BD1E995) & 0xFFFFFFFF
    h ^= h >> 15
    return (h & 0xFFFFFF) / float(0x1000000)


def value_noise(p, cell, seed):
    """Smooth 3D value noise in 0..1 at points p (N x 3, metres) with lattice spacing `cell`."""
    q = p / cell
    i = np.floor(q).astype(np.int64)
    f = q - i
    u = f * f * (3 - 2 * f)
    out = np.zeros(len(p))
    for dx in (0, 1):
        wx = u[:, 0] if dx else 1 - u[:, 0]
        for dy in (0, 1):
            wy = u[:, 1] if dy else 1 - u[:, 1]
            for dz in (0, 1):
                wz = u[:, 2] if dz else 1 - u[:, 2]
                out += wx * wy * wz * _hash(i[:, 0] + dx, i[:, 1] + dy, i[:, 2] + dz, seed)
    return out


def fbm(p, cell, seed, octaves=3, gain=0.5):
    acc, amp, norm = np.zeros(len(p)), 1.0, 0.0
    for k in range(octaves):
        acc += amp * value_noise(p, cell / (2 ** k), seed + 17 * k)
        norm += amp
        amp *= gain
    return acc / norm


def standardise(x):
    return (x - x.mean()) / max(x.std(), 1e-9)


# --- Flow ---------------------------------------------------------------------------------------

def bone_flow(ob, skel, rules):
    """Per-vertex fur direction from skin weights.

    `rules(bone name, head, tail, position)` returns a world direction for a vertex's bone (or
    None to skip the bone). Directions blend by weight and are projected onto the surface.
    """
    me = ob.data
    mw = ob.matrix_world
    rot = mw.to_3x3()
    groups = {g.index: g.name for g in ob.vertex_groups}
    bones = {b.name: (skel.matrix_world @ b.head_local, skel.matrix_world @ b.tail_local) for b in skel.data.bones}
    out = np.zeros((len(me.vertices), 3))
    for v in me.vertices:
        p = mw @ v.co
        n = (rot @ v.normal).normalized()
        acc = Vector()
        for g in v.groups:
            name = groups.get(g.group)
            if not name or name not in bones or g.weight < 0.02:
                continue
            d = rules(name, bones[name][0], bones[name][1], p)
            if d is not None:
                acc += Vector(d).normalized() * g.weight
        acc = acc - n * acc.dot(n)
        if acc.length < 1e-6:
            acc = Vector((0, 0, -1)) - n * (-n.z)
            if acc.length < 1e-6:
                acc = n.orthogonal()
        out[v.index] = acc.normalized()
    return out


# --- Rasterising into texture space ----------------------------------------------------------

class GBuffer:
    """Every texel the mesh covers in its UV layout, with interpolated surface data.

    `attrs` are per-vertex arrays (N x k or N) to interpolate as well. Arrays are indexed by
    filled texel; `iy`, `ix` give the texel (row 0 is v = 0, Blender's bottom row).
    """

    def __init__(self, ob, size, flow=None, attrs=None, uv="UVMap"):
        self.size = size
        me = ob.data
        me.calc_loop_triangles()
        mw = ob.matrix_world
        rot = mw.to_3x3()
        nv = len(me.vertices)
        co = np.empty(nv * 3, np.float64)
        me.vertices.foreach_get("co", co)
        co = co.reshape(-1, 3)
        m = np.array(mw)
        world = co @ m[:3, :3].T + m[:3, 3]
        uvs = np.empty(len(me.loops) * 2, np.float64)
        me.uv_layers[uv].data.foreach_get("uv", uvs)
        uvs = uvs.reshape(-1, 2)
        nl = np.empty(len(me.loops) * 3, np.float64)
        me.corner_normals.foreach_get("vector", nl)
        nl = nl.reshape(-1, 3) @ np.array(rot).T
        tris = me.loop_triangles
        n_tri = len(tris)
        tri_loops = np.empty(n_tri * 3, np.int64)
        tri_verts = np.empty(n_tri * 3, np.int64)
        tri_poly = np.empty(n_tri, np.int64)
        tris.foreach_get("loops", tri_loops)
        tris.foreach_get("vertices", tri_verts)
        tris.foreach_get("polygon_index", tri_poly)
        tri_loops = tri_loops.reshape(-1, 3)
        tri_verts = tri_verts.reshape(-1, 3)
        parts_t, parts_w, parts_y, parts_x = [], [], [], []
        texel_m = np.zeros(n_tri)
        uv_to_world = np.zeros((n_tri, 2, 3))  # Rows: world change per texel step in u and v.
        for t in range(n_tri):
            tuv = uvs[tri_loops[t]] * size
            P = world[tri_verts[t]]
            a, b, c = tuv
            v0, v1 = b - a, c - a
            den = v0[0] * v1[1] - v1[0] * v0[1]
            if abs(den) < 1e-12:
                continue
            # World change per texel along u and along v.
            inv = np.array([[v1[1], -v1[0]], [-v0[1], v0[0]]]) / den
            E = np.stack([P[1] - P[0], P[2] - P[0]])  # 2 x 3
            uv_to_world[t] = inv @ E
            area_w = 0.5 * np.linalg.norm(np.cross(P[1] - P[0], P[2] - P[0]))
            texel_m[t] = math.sqrt(max(area_w, 1e-14) / max(abs(den) * 0.5, 1e-14))
            lo = np.clip(np.floor(tuv.min(0)).astype(int), 0, size - 1)
            hi = np.clip(np.ceil(tuv.max(0)).astype(int), 0, size - 1)
            xs, ys = np.meshgrid(np.arange(lo[0], hi[0] + 1), np.arange(lo[1], hi[1] + 1))
            px = np.stack([xs.ravel() + 0.5, ys.ravel() + 0.5], 1)
            d = px - a
            w1 = (d[:, 0] * v1[1] - v1[0] * d[:, 1]) / den
            w2 = (v0[0] * d[:, 1] - d[:, 0] * v0[1]) / den
            w0 = 1 - w1 - w2
            inside = (w0 >= -1e-3) & (w1 >= -1e-3) & (w2 >= -1e-3)
            if not inside.any():
                continue
            k = int(inside.sum())
            parts_t.append(np.full(k, t))
            parts_w.append(np.stack([w0, w1, w2], 1)[inside])
            parts_y.append(ys.ravel()[inside])
            parts_x.append(xs.ravel()[inside])
        tri = np.concatenate(parts_t)
        bc = np.clip(np.concatenate(parts_w), 0, 1)
        bc /= bc.sum(1, keepdims=True)
        iy = np.concatenate(parts_y)
        ix = np.concatenate(parts_x)
        # Later triangles win where islands overlap; keep one sample per texel.
        key = iy * size + ix
        _, first = np.unique(key[::-1], return_index=True)
        keep = len(key) - 1 - first
        self.tri, self.bc, self.iy, self.ix = tri[keep], bc[keep], iy[keep], ix[keep]
        self.poly = tri_poly[self.tri]
        vi = tri_verts[self.tri]
        li = tri_loops[self.tri]
        w = self.bc[:, :, None]
        self.pos = (world[vi] * w).sum(1)
        n = (nl[li] * w).sum(1)
        self.nrm = n / np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-9)
        self.texel_m = texel_m[self.tri]
        self.uv_to_world = uv_to_world[self.tri]
        self.mask = np.zeros((size, size), bool)
        self.mask[self.iy, self.ix] = True
        if flow is not None:
            f = (flow[vi] * w).sum(1)
            f -= self.nrm * (f * self.nrm).sum(1, keepdims=True)
            self.flow = f / np.maximum(np.linalg.norm(f, axis=1, keepdims=True), 1e-9)
            # The flow in texels per metre: solve [du dv] @ uv_to_world = f (least squares).
            A = self.uv_to_world  # n x 2 x 3
            AAt = np.einsum("nij,nkj->nik", A, A)
            Af = np.einsum("nij,nj->ni", A, self.flow)
            det = AAt[:, 0, 0] * AAt[:, 1, 1] - AAt[:, 0, 1] * AAt[:, 1, 0]
            det = np.where(np.abs(det) < 1e-18, 1e-18, det)
            g0 = (AAt[:, 1, 1] * Af[:, 0] - AAt[:, 0, 1] * Af[:, 1]) / det
            g1 = (-AAt[:, 1, 0] * Af[:, 0] + AAt[:, 0, 0] * Af[:, 1]) / det
            self.flow_uv = np.stack([g0, g1], 1)  # Texels per metre along the flow.
        self.attrs = {}
        for name, arr in (attrs or {}).items():
            arr = np.asarray(arr, np.float64)
            if arr.ndim == 1:
                self.attrs[name] = (arr[vi] * self.bc).sum(1)
            else:
                self.attrs[name] = (arr[vi] * w).sum(1)

    def image(self, values, fill=0.0, channels=None):
        """Scatters per-texel values into a full image (rows bottom-up), unfilled = `fill`."""
        values = np.asarray(values)
        shape = (self.size, self.size) + values.shape[1:]
        img = np.full(shape, fill, np.float64)
        img[self.iy, self.ix] = values
        return img

    def sample(self, img, x, y):
        """Bilinear lookup of a full image at texel coordinates (x, y)."""
        s = self.size
        x = np.clip(x - 0.5, 0, s - 1.001)
        y = np.clip(y - 0.5, 0, s - 1.001)
        x0 = np.floor(x).astype(int)
        y0 = np.floor(y).astype(int)
        fx = x - x0
        fy = y - y0
        a = img[y0, x0] * (1 - fx) + img[y0, x0 + 1] * fx
        b = img[y0 + 1, x0] * (1 - fx) + img[y0 + 1, x0 + 1] * fx
        return a * (1 - fy) + b * fy


def bleed(img, mask, passes=8):
    """Spreads filled texels into empty neighbours, so mips and filtering never pull in black."""
    img = img.copy()
    filled = mask.copy()
    extra = img.shape[2:] if img.ndim > 2 else ()
    for _ in range(passes):
        acc = np.zeros_like(img)
        cnt = np.zeros(filled.shape)
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            sf = np.roll(filled, (dy, dx), (0, 1))
            si = np.roll(img, (dy, dx), (0, 1))
            acc += si * (sf[..., None] if extra else sf)
            cnt += sf
        add = (~filled) & (cnt > 0)
        if extra:
            img[add] = acc[add] / cnt[add][:, None]
        else:
            img[add] = acc[add] / cnt[add]
        filled = filled | add
    return img


# --- Strands -----------------------------------------------------------------------------------

def streaks(gb, field, length, steps=12):
    """Line integral convolution: averages `field` (per texel) along the flow for +/- `length`
    metres, with a triangle kernel, staying inside the texel's own UV island where it can."""
    s = gb.size
    img = bleed(gb.image(field), gb.mask, 3)
    guv = bleed(gb.image(gb.flow_uv), gb.mask, 3)
    mask = gb.mask
    acc = field.copy()
    wsum = np.ones(len(field))
    step_m = length / steps
    for direction in (1.0, -1.0):
        x = gb.ix + 0.5
        y = gb.iy + 0.5
        alive = np.ones(len(field), bool)
        for k in range(1, steps + 1):
            xi = np.clip(x.astype(int), 0, s - 1)
            yi = np.clip(y.astype(int), 0, s - 1)
            g = guv[yi, xi]
            x = x + direction * g[:, 0] * step_m
            y = y + direction * g[:, 1] * step_m
            xi = np.clip(x.astype(int), 0, s - 1)
            yi = np.clip(y.astype(int), 0, s - 1)
            alive &= mask[yi, xi]
            wk = (1.0 - k / (steps + 1)) * alive
            acc += wk * gb.sample(img, x, y)
            wsum += wk
    return acc / wsum


def fur_fields(gb, strand=0.0011, clump=0.0075, strand_len=0.010, clump_len=0.02, seed=1):
    """(fine strands, clumps): zero-mean, unit-variance fields that run along the flow."""
    fine = value_noise(gb.pos, strand, seed)
    blobs = fbm(gb.pos, clump, seed + 5, octaves=2)
    s1 = standardise(streaks(gb, fine, strand_len, steps=10))
    s2 = standardise(streaks(gb, blobs, clump_len, steps=12))
    return s1, s2


def normal_map(gb, height, depth):
    """A tangent-space normal map (rows bottom-up, glTF +Y up) from per-texel height (0..1)
    scaled to `depth` metres of relief."""
    h = bleed(gb.image(height * depth), gb.mask, 6)
    t = bleed(gb.image(gb.texel_m), gb.mask, 6)
    t = np.maximum(t, 1e-5)
    dx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 0.5 / t
    dy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 0.5 / t
    n = np.stack([-dx, -dy, np.ones_like(h)], -1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    return n * 0.5 + 0.5


def bake_ao(ob, size, samples=32, distance=0.06):
    """Ambient occlusion in the object's UV layout, from Cycles (rows bottom-up, 0..1)."""
    scene = bpy.context.scene
    old_engine = scene.render.engine
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = samples
    scene.render.bake.margin = 4
    scene.render.bake.use_clear = True
    if scene.world is None:
        scene.world = bpy.data.worlds.new("bake")
    scene.world.light_settings.distance = distance
    hidden = [o for o in bpy.context.view_layer.objects if o.type == "MESH" and o != ob and not o.hide_render]
    for o in hidden:
        o.hide_render = True
    img = bpy.data.images.new("__ao__", size, size, alpha=False, float_buffer=True)
    img.colorspace_settings.name = "Non-Color"
    nodes = []
    for slot in ob.material_slots:
        m = slot.material
        if m is None:
            continue
        m.use_nodes = True
        t = m.node_tree.nodes.new("ShaderNodeTexImage")
        t.image = img
        m.node_tree.nodes.active = t
        nodes.append((m, t))
    sc.activate(ob)
    for o in bpy.context.selected_objects:
        o.select_set(o == ob)
    uv = ob.data.uv_layers.get("UVMap")
    if uv is not None:
        ob.data.uv_layers.active = uv
    bpy.ops.object.bake(type="AO")
    w, h = img.size
    px = np.empty(w * h * 4, np.float32)
    img.pixels.foreach_get(px)
    ao = px.reshape(h, w, 4)[..., 0].astype(np.float64)
    for m, t in nodes:
        m.node_tree.nodes.remove(t)
    bpy.data.images.remove(img)
    for o in hidden:
        o.hide_render = False
    scene.render.engine = old_engine
    return ao


def save_rgb(arr, path, colorspace):
    """Writes an (S, S, 3) array (rows bottom-up) as JPEG or PNG."""
    s = arr.shape[0]
    rgba = np.ones((s, s, 4), np.float32)
    rgba[..., :3] = np.clip(arr, 0, 1)
    return mt._save(rgba, Path(path), colorspace)


# --- Tufts --------------------------------------------------------------------------------------

class Surface:
    """Lookups on a body mesh: nearest point, its UV, normal, region and skin weights."""

    def __init__(self, ob, uv="UVMap"):
        self.ob = ob
        me = ob.data
        me.calc_loop_triangles()
        mw = ob.matrix_world
        self.mw = mw
        self.rot = mw.to_3x3()
        self.world = [mw @ v.co for v in me.vertices]
        self.tris = [(tuple(t.vertices), tuple(t.loops), t.polygon_index) for t in me.loop_triangles]
        self.tree = BVHTree.FromPolygons(self.world, [t[0] for t in self.tris])
        self.uv = me.uv_layers[uv].data
        self.normals = me.corner_normals
        self.regions = rg.read(ob) if "_REGION" in me.attributes else None
        self.groups = {g.index: g.name for g in ob.vertex_groups}

    def nearest(self, p):
        loc, _, idx, _ = self.tree.find_nearest(Vector(p))
        return loc, idx

    def bary(self, idx, p, clamp=True):
        """Barycentric coordinates of p's projection on a triangle (unclamped extrapolates)."""
        a, b, c = (self.world[i] for i in self.tris[idx][0])
        v0, v1, v2 = b - a, c - a, Vector(p) - a
        d00, d01, d11 = v0.dot(v0), v0.dot(v1), v1.dot(v1)
        d20, d21 = v2.dot(v0), v2.dot(v1)
        den = d00 * d11 - d01 * d01
        if abs(den) < 1e-18:
            return (1.0, 0.0, 0.0)
        v = (d11 * d20 - d01 * d21) / den
        w = (d00 * d21 - d01 * d20) / den
        u = 1 - v - w
        if not clamp:
            return (u, v, w)
        return (max(u, 0.0), max(v, 0.0), max(w, 0.0))

    def uv_at(self, idx, bc):
        loops = self.tris[idx][1]
        u = sum(self.uv[l].uv.x * k for l, k in zip(loops, bc))
        v = sum(self.uv[l].uv.y * k for l, k in zip(loops, bc))
        s = sum(bc) or 1.0
        return (u / s, v / s)

    def normal_at(self, idx, bc):
        loops = self.tris[idx][1]
        n = sum((Vector(self.normals[l].vector) * k for l, k in zip(loops, bc)), Vector())
        return (self.rot @ n).normalized()

    def region_at(self, idx):
        return self.regions[self.tris[idx][2]] if self.regions else 0

    def weights_at(self, idx, bc, limit=4):
        me = self.ob.data
        acc = {}
        for vi, k in zip(self.tris[idx][0], bc):
            for g in me.vertices[vi].groups:
                name = self.groups.get(g.group)
                if name:
                    acc[name] = acc.get(name, 0.0) + g.weight * k
        top = sorted(acc.items(), key=lambda kv: -kv[1])[:limit]
        s = sum(w for _, w in top) or 1.0
        return {n: w / s for n, w in top}


def scatter(surface, zone, count, min_dist, seed):
    """Up to `count` root points on the surface where `zone(p, n, region) -> weight` is > 0,
    area-weighted, at least `min_dist` apart. Returns [(point, triangle, barycentric)]."""
    rng = random.Random(seed)
    cands = []
    for idx, (verts, loops, poly) in enumerate(surface.tris):
        a, b, c = (surface.world[i] for i in verts)
        centre = (a + b + c) / 3
        area = (b - a).cross(c - a).length / 2
        n = (b - a).cross(c - a).normalized()
        w = zone(centre, n, surface.region_at(idx))
        if w > 0 and area > 0:
            cands.append((idx, area * w))
    if not cands:
        return []
    total = sum(w for _, w in cands)
    cum = []
    acc = 0.0
    for idx, w in cands:
        acc += w
        cum.append(acc)
    out = []
    tries = 0
    import bisect
    while len(out) < count and tries < count * 60:
        tries += 1
        r = rng.random() * total
        idx = cands[bisect.bisect_left(cum, r)][0]
        u, v = rng.random(), rng.random()
        if u + v > 1:
            u, v = 1 - u, 1 - v
        bc = (1 - u - v, u, v)
        a, b, c = (surface.world[i] for i in surface.tris[idx][0])
        p = a * bc[0] + b * bc[1] + c * bc[2]
        if any((p - q).length < min_dist for q, _, _ in out):
            continue
        out.append((p, idx, bc))
    return out


def tufts(surface, roots, flow_at, length, width, lift=0.6, droop=0.35, segments=3, seed=1, name="fur",
          length_jitter=0.3, spread=0.35, region=None, blend=0.55):
    """Little curved blades of fur growing along the flow from each root.

    `flow_at(p, n)` gives the growth direction (on the surface); `length` may be a number or a
    function of the root. Each blade starts just under the skin, rises by `lift` (radians) and
    droops back towards the surface by `droop` at its tip. A blade takes the UV, region and skin
    weights of its root (one colour, never across a UV seam) and, corner by corner, the normal
    of the body under it, so tufts colour, shade, hide and move with the body.
    """
    rng = random.Random(seed)
    bm = bmesh.new()
    uv_layer = bm.loops.layers.uv.new("UVMap")
    reg_layer = bm.loops.layers.float.new("_REGION")
    deform = bm.verts.layers.deform.verify()
    group_index = {}
    for p, idx, bc in roots:
        n = surface.normal_at(idx, bc)
        f = Vector(flow_at(p, n))
        f = (f - n * f.dot(n))
        if f.length < 1e-6:
            continue
        f.normalize()
        # Fan the blades a little sideways so they don't all point the same way.
        side = n.cross(f).normalized()
        f = (f + side * rng.uniform(-spread, spread)).normalized()
        side = n.cross(f).normalized()
        L = (length(p) if callable(length) else length) * rng.uniform(1 - length_jitter, 1 + length_jitter)
        W = width * rng.uniform(0.8, 1.2)
        a_lift = lift * rng.uniform(0.75, 1.25)
        weights = surface.weights_at(idx, bc)
        root_uv = surface.uv_at(idx, bc)
        reg = region if region is not None else surface.region_at(idx)
        rings = []
        for k in range(segments + 1):
            s = k / segments
            # Up from the skin, then curving back down towards it.
            rise = math.sin(a_lift) * s - (math.sin(a_lift) + droop) * 0.5 * s * s
            c = p - n * 0.0015 + f * (L * s * math.cos(a_lift * (1 - 0.5 * s))) + n * (L * max(rise, -0.02))
            w = W * (1 - s) ** 0.7
            th = w * 0.6
            if k == segments:
                rings.append([bm.verts.new(c)])
                break
            up = n
            rings.append([bm.verts.new(c + side * w * 0.5), bm.verts.new(c + up * th * 0.5),
                          bm.verts.new(c - side * w * 0.5), bm.verts.new(c - up * th * 0.5)])
        for ring in rings:
            for v in ring:
                for bone, wt in weights.items():
                    gi = group_index.setdefault(bone, len(group_index))
                    v[deform][gi] = wt
        faces = []
        for r0, r1 in zip(rings, rings[1:]):
            m = len(r0)
            for i in range(m):
                j = (i + 1) % m
                if len(r1) == 1:
                    faces.append(bm.faces.new((r0[i], r0[j], r1[0])))
                else:
                    faces.append(bm.faces.new((r0[i], r0[j], r1[j], r1[i])))
        for fc in faces:
            fc.smooth = True
            for loop in fc.loops:
                loop[uv_layer].uv = root_uv
                loop[reg_layer] = float(reg)
        bmesh.ops.recalc_face_normals(bm, faces=faces)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    for gname, gi in sorted(group_index.items(), key=lambda kv: kv[1]):
        ob.vertex_groups.new(name=gname)
    # Soft shading: every corner leans halfway to the normal of the body under it, so tufts
    # read as part of the coat yet still catch the light like little locks of fur.
    own = [Vector(n.vector) for n in me.corner_normals]
    normals = []
    for loop, n_own in zip(me.loops, own):
        q, tidx = surface.nearest(me.vertices[loop.vertex_index].co)
        n = surface.normal_at(tidx, surface.bary(tidx, q)) * blend + n_own * (1 - blend)
        normals.append(n.normalized())
    me.normals_split_custom_set(normals)
    return ob
