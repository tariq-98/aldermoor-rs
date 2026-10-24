"""Worked materials for armour and kit: hammered steel, brass and leather, with occlusion.

Plate and back items are built per body type, so they share tiling textures rather than baking
their own: a colour map, an ORM map (occlusion R, roughness G, metalness B) and a normal map per
surface, written once to humanoid/textures/. UVs are box-projected in metres, so the texel density
matches on a helmet and a greave. Occlusion goes into COLOR_0 per vertex (from rays against the
body, the clothes underneath and the armour itself), which glTF multiplies into the base colour:
gaps between plates, the undersides of rims and the insides of gauntlets darken.
"""

import math
import random
from pathlib import Path

import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from . import materials as mt, scene as sc, tiles

TEX = sc.OUT / "humanoid" / "textures"


# --- Surfaces -----------------------------------------------------------------------------------

def _scratches(size, count, seed, length=(0.02, 0.12), width=1.2):
    """Thin straight scratches (periodic), as a 0..1 mask."""
    rng = np.random.default_rng(seed)
    out = np.zeros((size, size))
    yy, xx = np.mgrid[0:size, 0:size].astype(np.float32)
    for _ in range(count):
        cx, cy = rng.uniform(0, size, 2)
        ang = rng.uniform(0, math.pi)
        L = rng.uniform(*length) * size
        dx, dy = math.cos(ang), math.sin(ang)
        px = (xx - cx + size / 2) % size - size / 2
        py = (yy - cy + size / 2) % size - size / 2
        along = px * dx + py * dy
        across = -px * dy + py * dx
        m = (np.abs(along) < L / 2) & (np.abs(across) < width * 2)
        out = np.maximum(out, m * np.exp(-(across / width) ** 2) * rng.uniform(0.4, 1.0))
    return out


def steel(size=1024, seed=21, base=(0.38, 0.39, 0.41)):
    """Hammered and brushed steel: soft dents, brushing, fine scratches and dull patches."""
    f1, edge, idx, n = tiles.voronoi(size, 40, seed, warp=18)
    dents = 1 - np.clip(f1 / 26.0, 0, 1) ** 1.5
    brush = tiles.stretched(size, 18.0, 1.0, seed + 1, power=1.2)
    mottle = tiles.noise(size, 2, seed + 2)
    dull = np.clip((tiles.noise(size, 3, seed + 3) - 0.55) / 0.2, 0, 1)
    scratch = _scratches(size, 28, seed + 4, length=(0.01, 0.06), width=0.8)
    k = (0.88 + 0.12 * mottle + 0.04 * brush - 0.14 * dull + 0.07 * scratch)
    colour = tiles._tint(base, k)
    rough = np.clip(0.44 + 0.08 * brush + 0.2 * dull + 0.08 * scratch, 0.04, 1)
    metal = np.clip(1.0 - 0.15 * dull, 0, 1)
    height = 0.45 * dents + 0.15 * brush - 0.3 * scratch
    return colour, height, 4.0, rough, metal


def brass(size=1024, seed=31):
    colour, height, strength, rough, metal = steel(size, seed, base=(0.78, 0.55, 0.26))
    patina = np.clip((tiles.noise(size, 4, seed + 9) - 0.5) / 0.25, 0, 1)
    colour = colour * (1 - 0.35 * patina[..., None]) + np.array([0.18, 0.2, 0.12])[None, None, :] * 0.35 * patina[..., None]
    return colour, height, strength, np.clip(rough + 0.15 * patina, 0, 1), metal


def leather(size=1024, seed=41, base=(0.14, 0.07, 0.032)):
    """Smooth worn leather: a fine grain, soft blotches, a few creases and scuffs."""
    grain = tiles.noise(size, 160, seed, power=0.9)
    crease = np.exp(-(np.abs(tiles.stretched(size, 4.0, 1.0, seed + 1) - 0.5) / 0.012) ** 2)
    crease *= tiles.noise(size, 3, seed + 4) > 0.55
    blotch = tiles.noise(size, 3, seed + 2)
    scuff = np.clip((tiles.noise(size, 9, seed + 3) - 0.64) / 0.15, 0, 1)
    k = (0.84 + 0.22 * blotch + 0.05 * grain - 0.2 * crease + 0.28 * scuff)
    colour = tiles._tint(base, k)
    rough = np.clip(0.6 + 0.06 * grain + 0.08 * crease - 0.1 * scuff, 0, 1)
    height = 0.3 * grain - 0.7 * crease + 0.2 * blotch
    return colour, height, 3.0, rough, np.zeros_like(rough)


def wool(size=512, seed=51, base=(0.03, 0.05, 0.14)):
    """Woven wool: crossing threads, slubs and soft blotches."""
    v, u = np.mgrid[0:size, 0:size] / size
    k = 48  # Threads per tile each way.
    warp = 0.5 + 0.5 * np.sin(2 * np.pi * k * u)
    weft = 0.5 + 0.5 * np.sin(2 * np.pi * k * v)
    over = (np.floor(k * u) + np.floor(k * v)) % 2
    threads = np.where(over > 0, warp, weft)
    slubs = tiles.stretched(size, 6.0, 1.0, seed, power=1.3)
    blotch = tiles.noise(size, 3, seed + 1)
    kk = 0.82 + 0.16 * threads + 0.08 * slubs + 0.08 * blotch
    colour = tiles._tint(base, kk)
    return colour, threads + 0.4 * slubs, 2.0, np.full((size, size), 0.92), np.zeros((size, size))


SURFACES = {"steel": (steel, 0.5), "brass": (brass, 0.4), "leather": (leather, 0.35)}


def surface_for(palette_name):
    """The worked surface for a palette material, or None to keep the flat one."""
    from . import shapes as sh
    if palette_name in WORKED:
        return WORKED[palette_name]
    if palette_name.startswith("wool_") or palette_name in ("canvas", "linen"):
        base = sh.PALETTE[palette_name][0]
        SURFACES.setdefault(palette_name, (lambda base=base: wool(base=base), 0.15))
        return palette_name
    if palette_name == "leather_tan":
        base = sh.PALETTE[palette_name][0]
        SURFACES.setdefault("leather_tan", (lambda base=base: leather(seed=43, base=base), 0.35))
        return "leather_tan"
    return None


def _save_orm(path, rough, metal):
    s = rough.shape[0]
    orm = np.ones((s, s, 4), np.float32)
    orm[..., 1] = rough
    orm[..., 2] = metal
    return mt._save(orm[::-1].copy(), Path(path), "Non-Color")


_cache = {}


def material(name):
    """A worked material ('steel', 'brass' or 'leather'), textures written once per build."""
    if name in _cache:
        try:
            _cache[name].name
            return _cache[name]
        except ReferenceError:
            pass
    fn, metres = SURFACES[name]
    paths = [TEX / f"kit_{name}_{k}" for k in ("color.jpg", "normal.png", "orm.jpg")]
    TEX.mkdir(parents=True, exist_ok=True)
    if all(p.exists() for p in paths):
        imgs = [bpy.data.images.load(str(p), check_existing=True) for p in paths]
        imgs[0].colorspace_settings.name = "sRGB"
        for im in imgs[1:]:
            im.colorspace_settings.name = "Non-Color"
    else:
        colour, height, strength, rough, metal = fn()
        col, nor, _ = tiles.save(TEX, f"kit_{name}", colour, height, strength, rough)
        orm = _save_orm(paths[2], rough, metal)
        imgs = [col, nor, orm]
    m = bpy.data.materials.new(f"kit.{name}")
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    t_col = nt.nodes.new("ShaderNodeTexImage")
    t_col.image = imgs[0]
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Col"
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    sock = {s.identifier: s for s in mul.inputs}
    sock["Factor_Float"].default_value = 1.0
    nt.links.new(t_col.outputs["Color"], sock["A_Color"])
    nt.links.new(vc.outputs["Color"], sock["B_Color"])
    nt.links.new(next(o for o in mul.outputs if o.identifier == "Result_Color"), bsdf.inputs["Base Color"])
    t_orm = nt.nodes.new("ShaderNodeTexImage")
    t_orm.image = imgs[2]
    sep = nt.nodes.new("ShaderNodeSeparateColor")
    nt.links.new(t_orm.outputs["Color"], sep.inputs[0])
    nt.links.new(sep.outputs[1], bsdf.inputs["Roughness"])
    nt.links.new(sep.outputs[2], bsdf.inputs["Metallic"])
    t_n = nt.nodes.new("ShaderNodeTexImage")
    t_n.image = imgs[1]
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(t_n.outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    m["metres"] = metres
    _cache[name] = m
    return m


# Palette materials (shapes.PALETTE) mapped onto worked ones.
WORKED = {"steel": "steel", "dark_steel": "steel", "iron": "steel", "silver": "steel",
          "brass": "brass", "gold": "brass", "leather": "leather", "leather_dark": "leather"}


def _palette_name(m):
    n = m.name
    if n.startswith("gear."):
        n = n[5:]
    return n.split(".")[0]


# --- UVs and occlusion ---------------------------------------------------------------------------

def box_uvs(ob):
    """Box-projected UVs in metres over each material's tile size."""
    me = ob.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active.data
    mw = ob.matrix_world
    rot = mw.to_3x3()
    metres = [(s.material.get("metres", 0.5) if s.material else 0.5) for s in ob.material_slots] or [0.5]
    for p in me.polygons:
        n = rot @ p.normal
        axis = max(range(3), key=lambda i: abs(n[i]))
        a, b = [i for i in range(3) if i != axis]
        k = 1.0 / metres[min(p.material_index, len(metres) - 1)]
        for li in p.loop_indices:
            co = mw @ me.vertices[me.loops[li].vertex_index].co
            uv[li].uv = (co[a] * k, co[b] * k)


def occluder_tree(objects):
    verts, polys = [], []
    dg = bpy.context.evaluated_depsgraph_get()
    for ob in sc.alive(objects):
        if ob.type != "MESH":
            continue
        ev = ob.evaluated_get(dg)
        me = ev.to_mesh()
        base = len(verts)
        verts += [ev.matrix_world @ v.co for v in me.vertices]
        polys += [[base + i for i in p.vertices] for p in me.polygons]
        ev.to_mesh_clear()
    return BVHTree.FromPolygons(verts, polys)


def vertex_ao(ob, tree, samples=18, distance=0.07, strength=0.65, seed=3):
    """Writes COLOR_0 ('Col', per corner) with ambient occlusion from rays against `tree`."""
    rng = random.Random(seed)
    dirs = []
    for k in range(samples):
        # Cosine-weighted hemisphere around +Z, turned per vertex below.
        u, v = (k + 0.5) / samples, rng.random()
        r = math.sqrt(u)
        a = 2 * math.pi * v
        dirs.append(Vector((r * math.cos(a), r * math.sin(a), math.sqrt(max(0.0, 1 - u)))))
    me = ob.data
    mw = ob.matrix_world
    rot = mw.to_3x3()
    occ = []
    for vert in me.vertices:
        p = mw @ vert.co
        n = (rot @ vert.normal).normalized()
        t = n.orthogonal().normalized()
        b = n.cross(t)
        hits = 0
        for d in dirs:
            w = t * d.x + b * d.y + n * d.z
            if tree.ray_cast(p + n * 0.0015, w, distance)[0] is not None:
                hits += 1
        occ.append(hits / samples)
    if "Col" in me.color_attributes:
        me.color_attributes.remove(me.color_attributes["Col"])
    attr = me.color_attributes.new("Col", "BYTE_COLOR", "CORNER")
    for li, loop in enumerate(me.loops):
        k = 1.0 - strength * occ[loop.vertex_index]
        attr.data[li].color = (k, k, k, 1.0)
    me.color_attributes.active_color = attr


def work(parts, occluders, keep_uvs=()):
    """Swaps palette materials for worked ones, then UVs and occlusion, on every part.

    Parts in `keep_uvs` keep their own UVs and material (a cape's painted cloth) but still get
    occlusion.
    """
    for ob in sc.alive(parts):
        if ob in keep_uvs:
            continue
        for slot in ob.material_slots:
            if slot.material is None:
                continue
            name = surface_for(_palette_name(slot.material))
            if name:
                slot.material = material(name)
        box_uvs(ob)
    tree = occluder_tree(list(occluders) + list(parts))
    for ob in sc.alive(parts):
        vertex_ao(ob, tree)
        for slot in ob.material_slots:
            if slot.material is not None:
                use_vertex_colour(slot.material)


def use_vertex_colour(m):
    """Multiplies COLOR_0 into a material's base colour if it doesn't already."""
    nt = m.node_tree
    if any(n.type == "VERTEX_COLOR" for n in nt.nodes):
        return
    bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
    if bsdf is None:
        return
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Col"
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    sock = {s.identifier: s for s in mul.inputs}
    sock["Factor_Float"].default_value = 1.0
    links = bsdf.inputs["Base Color"].links
    if links:
        nt.links.new(links[0].from_socket, sock["A_Color"])
    else:
        sock["A_Color"].default_value = bsdf.inputs["Base Color"].default_value
    nt.links.new(vc.outputs["Color"], sock["B_Color"])
    nt.links.new(next(o for o in mul.outputs if o.identifier == "Result_Color"), bsdf.inputs["Base Color"])
