"""Small mesh-building kit for hard-surface gear: lathes, lofts, sweeps and materials.

Everything is built in metres around the item's attachment point (see docs/ASSETS.md: up is
+Z, front is -Y). Shapes come out as quads with smooth shading; `finish` adds bevels and
weighted normals so stylised metal and wood catch the light.
"""

import math

import bmesh
import bpy
from mathutils import Matrix, Vector

_materials = {}

# Shared stylised palette: linear base colour, metallic, roughness.
PALETTE = {
    "steel": ((0.46, 0.47, 0.5), 1.0, 0.4),
    "dark_steel": ((0.22, 0.23, 0.26), 1.0, 0.42),
    "iron": ((0.30, 0.30, 0.31), 1.0, 0.55),
    "brass": ((0.78, 0.55, 0.22), 1.0, 0.35),
    "gold": ((0.95, 0.68, 0.25), 1.0, 0.25),
    "silver": ((0.85, 0.86, 0.88), 1.0, 0.22),
    "leather": ((0.14, 0.065, 0.03), 0.0, 0.62),
    "leather_dark": ((0.09, 0.05, 0.03), 0.0, 0.58),
    "leather_tan": ((0.24, 0.13, 0.06), 0.0, 0.66),
    "wood": ((0.16, 0.08, 0.035), 0.0, 0.7),
    "wood_light": ((0.32, 0.19, 0.09), 0.0, 0.72),
    "wood_dark": ((0.12, 0.06, 0.03), 0.0, 0.68),
    "cord": ((0.45, 0.38, 0.26), 0.0, 0.85),
    "canvas": ((0.30, 0.24, 0.15), 0.0, 0.9),
    "linen": ((0.72, 0.66, 0.54), 0.0, 0.9),
    "wool_red": ((0.20, 0.018, 0.016), 0.0, 0.92),
    "wool_red_trim": ((0.36, 0.24, 0.08), 0.0, 0.8),
    "wool_blue": ((0.025, 0.05, 0.14), 0.0, 0.92),
    "wool_green": ((0.03, 0.075, 0.032), 0.0, 0.92),
    "wool_grey": ((0.09, 0.085, 0.08), 0.0, 0.92),
    "wool_cream": ((0.42, 0.37, 0.28), 0.0, 0.92),
    "paint_red": ((0.30, 0.025, 0.02), 0.0, 0.6),
    "paint_blue": ((0.05, 0.12, 0.35), 0.0, 0.6),
    "paint_cream": ((0.50, 0.43, 0.30), 0.0, 0.65),
    "feather": ((0.80, 0.78, 0.72), 0.0, 0.8),
    "feather_red": ((0.55, 0.08, 0.05), 0.0, 0.8),
    "crystal_blue": ((0.20, 0.55, 0.95), 0.0, 0.08),
    "crystal_green": ((0.25, 0.85, 0.45), 0.0, 0.08),
    "crystal_violet": ((0.55, 0.25, 0.95), 0.0, 0.08),
    "bone": ((0.78, 0.74, 0.62), 0.0, 0.6),
    "glass": ((0.75, 0.8, 0.78), 0.0, 0.05),
    "stone": ((0.30, 0.29, 0.27), 0.0, 0.85),
    "moss": ((0.10, 0.18, 0.05), 0.0, 0.9),
    "rune": ((0.25, 0.75, 1.0), 0.0, 0.3),
    "flame": ((1.0, 0.62, 0.22), 0.0, 0.5),
}
GLOW = {"crystal_blue": 1.5, "crystal_green": 1.5, "crystal_violet": 1.5, "flame": 6.0, "rune": 5.0}
TRANSLUCENT = {"glass": 0.25}


def material(name):
    if name in _materials:
        try:
            _materials[name].name
            return _materials[name]
        except ReferenceError:
            pass
    colour, metallic, rough = PALETTE[name]
    m = bpy.data.materials.new(f"gear.{name}")
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = rough
    if name in GLOW:
        bsdf.inputs["Emission Color"].default_value = (*colour, 1)
        bsdf.inputs["Emission Strength"].default_value = GLOW[name]
    if name.startswith("crystal"):
        bsdf.inputs["Coat Weight"].default_value = 0.6
    if name in TRANSLUCENT:
        bsdf.inputs["Alpha"].default_value = TRANSLUCENT[name]
        if hasattr(m, "surface_render_method"):
            m.surface_render_method = "BLENDED"
        else:
            m.blend_method = "BLEND"
    _materials[name] = m
    return m


def obj(name, bm, mat=None, smooth=True):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    if mat:
        me.materials.append(material(mat))
    for p in me.polygons:
        p.use_smooth = smooth
    return ob


def loft(sections, closed=True, cap_start=True, cap_end=True):
    """Joins rings of points (same count each) into a tube. A ring may collapse to one point."""
    bm = bmesh.new()
    rings = []
    for sec in sections:
        rings.append([bm.verts.new(p) for p in sec])
    for a, b in zip(rings, rings[1:]):
        n = len(a)
        m = len(b)
        if m == 1 and n == 1:
            continue
        if m == 1 or n == 1:
            tip, ring = (b[0], a) if m == 1 else (a[0], b)
            for i in range(len(ring) if closed else len(ring) - 1):
                j = (i + 1) % len(ring)
                f = (ring[i], ring[j], tip) if m == 1 else (ring[j], ring[i], tip)
                bm.faces.new(f)
            continue
        for i in range(n if closed else n - 1):
            j = (i + 1) % n
            bm.faces.new((a[i], a[j], b[j], b[i]))
    if closed:
        if cap_start and len(rings[0]) > 2:
            bm.faces.new(list(reversed(rings[0])))
        if cap_end and len(rings[-1]) > 2:
            bm.faces.new(rings[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return bm


def lathe(profile, segments=24, axis_z=True):
    """Revolves (radius, z) points around Z. Radius 0 makes a pole."""
    sections = []
    for r, z in profile:
        if r <= 1e-6:
            sections.append([Vector((0, 0, z))])
        else:
            sections.append([Vector((r * math.cos(a), r * math.sin(a), z))
                             for a in (2 * math.pi * i / segments for i in range(segments))])
    return loft(sections)


def ring(centre, x_axis, y_axis, rx, ry, n, power=2.0):
    """A superellipse ring: power 2 is an ellipse, higher is boxier."""
    pts = []
    for i in range(n):
        a = 2 * math.pi * i / n
        c, s = math.cos(a), math.sin(a)
        x = math.copysign(abs(c) ** (2 / power), c) * rx
        y = math.copysign(abs(s) ** (2 / power), s) * ry
        pts.append(centre + x_axis * x + y_axis * y)
    return pts


def sweep(path, radius_fn, n=12, power=2.0, ratio=1.0, up=Vector((0, 0, 1))):
    """A tube along a polyline. radius_fn(t) gives the radius at t in 0..1."""
    sections = []
    total = sum((b - a).length for a, b in zip(path, path[1:]))
    acc = 0.0
    side = None
    for i, p in enumerate(path):
        if i:
            acc += (path[i] - path[i - 1]).length
        t = acc / total if total else 0
        tan = (path[min(i + 1, len(path) - 1)] - path[max(i - 1, 0)]).normalized()
        # The first frame comes from `up`; later ones are carried along so the tube can't twist.
        if side is None:
            side = tan.cross(up)
            if side.length < 1e-4:
                side = tan.cross(Vector((1, 0, 0)))
        side = (side - tan * side.dot(tan)).normalized()
        nrm = side.cross(tan).normalized()
        r = radius_fn(t)
        sections.append(ring(p, side, nrm, r, r * ratio, n, power) if r > 1e-6 else [p])
    return loft(sections)


def join(objects, name):
    """Joins objects into the first; returns it renamed."""
    for o in bpy.context.selected_objects:
        o.select_set(False)
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.join()
    ob = bpy.context.view_layer.objects.active
    ob.name = name
    ob.data.name = name
    return ob


def finish(ob, bevel=0.0015, segments=2, angle=35, subdivide=0, weld=False):
    """Bevels hard edges, optional subdivision, weighted normals. Applies everything."""
    if weld:
        bm = bmesh.new()
        bm.from_mesh(ob.data)
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
        bm.to_mesh(ob.data)
        bm.free()
    if subdivide:
        m = ob.modifiers.new("sub", "SUBSURF")
        m.levels = m.render_levels = subdivide
    if bevel > 0:
        m = ob.modifiers.new("bevel", "BEVEL")
        m.width = bevel
        m.segments = segments
        m.limit_method = "ANGLE"
        m.angle_limit = math.radians(angle)
        m.harden_normals = False
    m = ob.modifiers.new("wn", "WEIGHTED_NORMAL")
    m.keep_sharp = True
    for o in bpy.context.selected_objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)
    for mod in list(ob.modifiers):
        bpy.ops.object.modifier_apply(modifier=mod.name)
    return ob


def transform(bm, matrix):
    bmesh.ops.transform(bm, matrix=matrix, verts=bm.verts)
    return bm


def triangles(objects):
    n = 0
    for ob in objects:
        n += sum(len(p.vertices) - 2 for p in ob.data.polygons)
    return n


def uv_unwrap(ob, angle=66):
    """Smart UV projection, so baked or painted detail can be added later."""
    for o in bpy.context.selected_objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = ob
    ob.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(angle), island_margin=0.01)
    bpy.ops.object.mode_set(mode="OBJECT")


def ribbon(path, normals, width, thickness):
    """A flat strap along a path. `normals` give, per point, the direction the strap's face looks."""
    sections = []
    for i, p in enumerate(path):
        tan = (path[min(i + 1, len(path) - 1)] - path[max(i - 1, 0)]).normalized()
        n = normals[i].normalized()
        n = (n - tan * n.dot(tan)).normalized()
        w = tan.cross(n).normalized() * (width / 2)
        t = n * thickness
        sections.append([p - w, p + w, p + w + t, p - w + t])
    return loft(sections)
