"""The environment kit's building blocks: tiled materials, world-scale UVs, wind weights.

Buildings and props use shared tiling surfaces (tiles.py) with UVs in metres, so the texture
density is the same on a cottage wall and a bridge parapet, and every prop shares the same few
textures. Foliage carries a `_WIND` vertex attribute (0 where it's rooted, 1 at the tips) for the
renderer's sway, and `COLOR_0` for painted shading (darker inside canopies, grime at the foot of
walls), which glTF multiplies into the base colour.
"""

import math
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

from . import materials as mt, scene as sc, shapes as sh, tiles

OUT = sc.OUT / "environment"
TEX = OUT / "textures"

_surfaces = {}


def surface(name):
    """A material for a tiled surface ('plaster', 'stone', 'thatch'…), textures written once."""
    if name in _surfaces:
        try:
            _surfaces[name][0].name
            return _surfaces[name]
        except ReferenceError:
            pass
    fn, metres = tiles.SURFACES[name]
    col_path, nrm_path = TEX / f"{name}_color.jpg", TEX / f"{name}_normal.png"
    if col_path.exists() and nrm_path.exists():
        col = bpy.data.images.load(str(col_path), check_existing=True)
        col.colorspace_settings.name = "sRGB"
        nor = bpy.data.images.load(str(nrm_path), check_existing=True)
        nor.colorspace_settings.name = "Non-Color"
        rough = ROUGH.get(name, 0.85)
    else:
        colour, height, strength, rough_map = fn()
        col, nor, rough = tiles.save(TEX, name, colour, height, strength, rough_map)
        ROUGH[name] = rough
    m = bpy.data.materials.new(f"env.{name}")
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = col
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Col"
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    sock = {s.identifier: s for s in mul.inputs}
    sock["Factor_Float"].default_value = 1.0
    nt.links.new(t.outputs["Color"], sock["A_Color"])
    nt.links.new(vc.outputs["Color"], sock["B_Color"])
    nt.links.new(next(o for o in mul.outputs if o.identifier == "Result_Color"), bsdf.inputs["Base Color"])
    tn = nt.nodes.new("ShaderNodeTexImage")
    tn.image = nor
    nm = nt.nodes.new("ShaderNodeNormalMap")
    nt.links.new(tn.outputs["Color"], nm.inputs["Color"])
    nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    bsdf.inputs["Roughness"].default_value = rough
    m["metres"] = metres
    _surfaces[name] = (m, metres)
    return _surfaces[name]


ROUGH = {"plaster": 0.92, "stone": 0.82, "planks": 0.8, "planks_dark": 0.8, "thatch": 0.95, "roof_tiles": 0.8,
         "cobbles": 0.85, "bark": 0.9, "rock": 0.9, "foliage": 0.8, "needles": 0.85}


def flat(name, colour, rough=0.8, metal=0.0, glow=0.0):
    """A plain material (glass, iron, paint, leaves), also multiplied by vertex colour."""
    m = bpy.data.materials.get(f"env.{name}")
    if m:
        return m
    m = bpy.data.materials.new(f"env.{name}")
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    vc = nt.nodes.new("ShaderNodeVertexColor")
    vc.layer_name = "Col"
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    sock = {s.identifier: s for s in mul.inputs}
    sock["Factor_Float"].default_value = 1.0
    sock["A_Color"].default_value = (*colour, 1)
    nt.links.new(vc.outputs["Color"], sock["B_Color"])
    nt.links.new(next(o for o in mul.outputs if o.identifier == "Result_Color"), bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if glow:
        bsdf.inputs["Emission Color"].default_value = (*colour, 1)
        bsdf.inputs["Emission Strength"].default_value = glow
    return m


# --- Building blocks --------------------------------------------------------------------------

def mesh(name, bm, mat, smooth=False):
    """A mesh object from a bmesh with one material."""
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    me.materials.append(mat)
    for p in me.polygons:
        p.use_smooth = smooth
    return ob


def box(name, centre, size, mat, bevel=0.0, rot=None):
    """An axis-aligned box (optionally turned), bevelled for soft stylised edges."""
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    m = Matrix.Translation(Vector(centre)) @ (rot.to_4x4() if rot is not None else Matrix()) @ Matrix.Diagonal((*size, 1.0))
    bmesh.ops.transform(bm, matrix=m, verts=bm.verts)
    ob = mesh(name, bm, mat)
    if bevel:
        mod = ob.modifiers.new("bevel", "BEVEL")
        mod.width = bevel
        mod.segments = 2
        mod.limit_method = "ANGLE"
        sc.activate(ob)
        bpy.ops.object.modifier_apply(modifier=mod.name)
    return ob


def prism(name, points, depth, axis, mat):
    """Extrudes a closed 2D outline (points in the plane normal to `axis`) by `depth` along it."""
    bm = bmesh.new()
    a = Vector(axis).normalized()
    front = [bm.verts.new(Vector(p) - a * depth / 2) for p in points]
    back = [bm.verts.new(Vector(p) + a * depth / 2) for p in points]
    bm.faces.new(front)
    bm.faces.new(list(reversed(back)))
    n = len(points)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((front[i], front[j], back[j], back[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return mesh(name, bm, mat)


def join(objects, name):
    objects = [o for o in sc.alive(objects)]
    return sh.join(objects, name)


# --- UVs, shading and wind ---------------------------------------------------------------------

UP = Vector((0, 0, 1))


def world_uvs(ob, rotate_slopes=True):
    """UVs in metres over each material's tile size: v up the face, u along it.

    Horizontal faces map x and y. Faces of one material keep a consistent scale, so tiles line
    up across a wall and the texel density matches everywhere.
    """
    me = ob.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active
    mw = ob.matrix_world
    metres = [(s.material.get("metres", 1.0) if s.material else 1.0) for s in ob.material_slots] or [1.0]
    for p in me.polygons:
        n = (mw.to_3x3() @ p.normal).normalized()
        k = 1.0 / metres[p.material_index] if p.material_index < len(metres) else 1.0
        if abs(n.z) > 0.92:
            u_ax, v_ax = Vector((1, 0, 0)), Vector((0, 1, 0))
        else:
            u_ax = UP.cross(n).normalized()
            v_ax = n.cross(u_ax).normalized()
        for li in p.loop_indices:
            co = mw @ me.vertices[me.loops[li].vertex_index].co
            uv.data[li].uv = (co.dot(u_ax) * k, co.dot(v_ax) * k)


def shade(ob, fn):
    """Writes COLOR_0 ('Col') from fn(world position, world normal) -> (r, g, b)."""
    me = ob.data
    if "Col" in me.color_attributes:
        me.color_attributes.remove(me.color_attributes["Col"])
    attr = me.color_attributes.new("Col", "BYTE_COLOR", "CORNER")
    mw = ob.matrix_world
    nm = mw.to_3x3()
    for p in me.polygons:
        n = (nm @ p.normal).normalized()
        for li in p.loop_indices:
            co = mw @ me.vertices[me.loops[li].vertex_index].co
            r, g, b = fn(co, n)
            attr.data[li].color = (r, g, b, 1.0)
    me.color_attributes.active_color = attr


def grime(base_z=0.0, height=0.7, strength=0.35, eaves=None):
    """Darker towards the ground (rain splash and soil) and, optionally, under the eaves."""
    def fn(co, n):
        k = 1.0 - strength * max(0.0, 1.0 - (co.z - base_z) / height) ** 1.5
        if eaves is not None:
            z0, depth = eaves
            k *= 1.0 - 0.25 * max(0.0, 1.0 - abs(co.z - z0) / depth) * (1 if co.z < z0 else 0)
        return k, k, k
    return fn


def wind(ob, fn):
    """Writes the `_WIND` float attribute (per vertex) from fn(world position) -> 0..1."""
    me = ob.data
    if "_WIND" in me.attributes:
        me.attributes.remove(me.attributes["_WIND"])
    attr = me.attributes.new("_WIND", "FLOAT", "POINT")
    mw = ob.matrix_world
    vals = [max(0.0, min(1.0, fn(mw @ v.co))) for v in me.vertices]
    attr.data.foreach_set("value", vals)


def soften_normals(ob, centre, amount=1.0):
    """Points normals away from `centre` (foliage reads as one soft mass, like BotW's trees)."""
    me = ob.data
    mw = ob.matrix_world
    rot = mw.to_3x3()
    inv = rot.inverted()
    c = Vector(centre)
    arr = [None] * len(me.loops)
    for p in me.polygons:
        fn = (rot @ p.normal).normalized()
        for li in p.loop_indices:
            co = mw @ me.vertices[me.loops[li].vertex_index].co
            n = fn.lerp((co - c).normalized(), amount).normalized()
            arr[li] = (inv @ n).normalized()
    me.normals_split_custom_set(arr)


def box_uvs(ob):
    """Box mapping for organic shapes (canopies, rocks, hay): each face projects along its
    dominant axis, so neighbours share a projection and seams fall only where the axis changes."""
    me = ob.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active
    mw = ob.matrix_world
    metres = [(s.material.get("metres", 1.0) if s.material else 1.0) for s in ob.material_slots] or [1.0]
    for p in me.polygons:
        n = mw.to_3x3() @ p.normal
        axis = max(range(3), key=lambda i: abs(n[i]))
        a, b = [i for i in range(3) if i != axis]
        k = 1.0 / metres[p.material_index] if p.material_index < len(metres) else 1.0
        for li in p.loop_indices:
            co = mw @ me.vertices[me.loops[li].vertex_index].co
            uv.data[li].uv = (co[a] * k, co[b] * k)


def beam_uvs(ob):
    """UVs for timber and boards: the grain (texture u) runs along the piece's longest axis."""
    me = ob.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active
    mw = ob.matrix_world
    pts = [mw @ v.co for v in me.vertices]
    ext = [max(p[i] for p in pts) - min(p[i] for p in pts) for i in range(3)]
    long_axis = Vector([1.0 if i == ext.index(max(ext)) else 0.0 for i in range(3)])
    metres = ob.material_slots[0].material.get("metres", 1.6) if ob.material_slots else 1.6
    for p in me.polygons:
        n = (mw.to_3x3() @ p.normal).normalized()
        u_ax = long_axis - n * long_axis.dot(n)
        if u_ax.length < 1e-3:
            u_ax = UP.cross(n) if abs(n.z) < 0.9 else Vector((1, 0, 0))
        u_ax = u_ax.normalized()
        v_ax = n.cross(u_ax).normalized()
        for li in p.loop_indices:
            co = mw @ me.vertices[me.loops[li].vertex_index].co
            uv.data[li].uv = (co.dot(u_ax) / metres, co.dot(v_ax) / metres)
