"""Meshes for combat effects, modelled in Blender and exported as GLB.

- `shell`: a ward's bubble, a Goldberg sphere of hexagonal cells (and twelve pentagons). Each
  cell is its own fan, so the effects shader can draw glowing cell walls and light cells one at a
  time. Vertex colour R runs 0 at a cell's centre to 1 at its wall; G is a random value per cell;
  B is the cell's height on the sphere (0 at the bottom, 1 at the top).
- `column`: an open cylinder of radius 1 from y = 0 to 1 (light rising from the ground). Vertex
  colour R is 1 at the foot and 0 at the top, G the way round; UVs run round and up.
- `crystal`: a frost lance's ice, a long faceted crystal pointing along +z from z = -0.5 to 0.5,
  flat shaded. Vertex colour R is a random value per facet, G how far along it is.
- `arrow`: a solid, lit arrow with its tip at the origin and its shaft back along -z: an ash
  shaft, an iron head and three fletchings. Drawn like any other mesh.

Coordinates in this module are glTF's (y up, +z forward); `_b` turns them into Blender's.
"""

import math
import random

import bmesh
import bpy
from mathutils import Vector

from . import scene as sc


def _b(x, y, z):
    """glTF (y up) to Blender (z up): the exporter's +Y-up conversion undoes this."""
    return (x, -z, y)


def _colours(me, values):
    """Per-corner colours (one tuple per loop, in loop order) as the mesh's only colour layer."""
    attr = me.color_attributes.new("Col", "FLOAT_COLOR", "CORNER")
    for i, c in enumerate(values):
        attr.data[i].color = c
    me.color_attributes.active_color = attr
    return attr


def _uvs(me, values):
    layer = me.uv_layers.new(name="UVMap")
    for i, uv in enumerate(values):
        layer.data[i].uv = uv


def _object(name, verts, faces, corner_colours, corner_uvs=None, smooth=True, normals=None):
    me = bpy.data.meshes.new(name)
    me.from_pydata([_b(*v) for v in verts], [], faces)
    me.update()
    _colours(me, corner_colours)
    if corner_uvs is not None:
        _uvs(me, corner_uvs)
    for p in me.polygons:
        p.use_smooth = smooth
    if normals is not None:
        me.normals_split_custom_set_from_vertices([Vector(_b(*n)) for n in normals])
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    return ob


def _material(name, colour, metallic=0.0, roughness=0.6):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    bsdf = m.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*colour, 1)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    return m


def shell(subdivisions=3, seed=7):
    """A Goldberg sphere of radius 1: the dual of an icosphere, one fan per cell."""
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=subdivisions, radius=1.0)
    bm.verts.ensure_lookup_table()
    rng = random.Random(seed)
    verts, faces, colours, normals = [], [], [], []
    for v in bm.verts:
        centre = v.co.normalized()
        # Corners: the centres of the faces around this vertex, in order round it.
        corners = [f.calc_center_median().normalized() for f in v.link_faces]
        tangent = (corners[0] - centre * corners[0].dot(centre)).normalized()
        bitangent = centre.cross(tangent)
        corners.sort(key=lambda c: math.atan2(c.dot(bitangent), c.dot(tangent)))
        cell = rng.random()
        height = (centre.z + 1) / 2  # Blender z is up here (the icosphere is ours, not glTF's)
        base = len(verts)
        # Store glTF coordinates: Blender (x, y, z) is glTF (x, z, -y).
        to_gltf = lambda p: (p.x, p.z, -p.y)  # noqa: E731
        verts.append(to_gltf(centre))
        normals.append(to_gltf(centre))
        for c in corners:
            verts.append(to_gltf(c))
            normals.append(to_gltf(c))
        k = len(corners)
        for i in range(k):
            faces.append((base, base + 1 + i, base + 1 + (i + 1) % k))
            colours += [(0.0, cell, height, 1.0), (1.0, cell, height, 1.0), (1.0, cell, height, 1.0)]
    bm.free()
    return _object("shell", verts, faces, colours, normals=normals)


def column(segments=32):
    """An open cylinder, radius 1, from y = 0 to 1, with its seam's vertices doubled."""
    verts, faces, colours, uvs = [], [], [], []
    for i in range(segments + 1):
        a = i / segments * math.tau
        x, z = math.cos(a), math.sin(a)
        verts += [(x, 0.0, z), (x, 1.0, z)]
    for i in range(segments):
        a, b = i * 2, i * 2 + 2
        # Outward-facing quad: bottom, next bottom, next top, top.
        faces.append((a, a + 1, b + 1, b))
        u0, u1 = i / segments, (i + 1) / segments
        colours += [(1.0, u0, 0, 1), (0.0, u0, 0, 1), (0.0, u1, 0, 1), (1.0, u1, 0, 1)]
        uvs += [(u0, 0.0), (u0, 1.0), (u1, 1.0), (u1, 0.0)]
    ob = _object("column", verts, faces, colours, corner_uvs=uvs)
    return ob


def crystal(sides=6, seed=11):
    """Ice: a long six-sided crystal with a sharp point forward and a shorter one behind, and
    a slight twist so its facets catch light differently. Flat shaded."""
    rng = random.Random(seed)
    rings = [(-0.5, 0.0), (-0.36, 0.55), (0.12, 1.0), (0.32, 0.72), (0.5, 0.0)]
    verts, faces, colours = [], [], []
    for z, radius in rings:
        twist = z * 0.6
        for i in range(sides):
            a = i / sides * math.tau + twist
            wobble = 1.0 + rng.uniform(-0.12, 0.12) if 0 < radius < 1.0 else 1.0
            verts.append((math.cos(a) * radius * wobble, math.sin(a) * radius * wobble, z))
    for r in range(len(rings) - 1):
        for i in range(sides):
            a = r * sides + i
            b = r * sides + (i + 1) % sides
            c = (r + 1) * sides + (i + 1) % sides
            d = (r + 1) * sides + i
            faces.append((a, b, c, d))
            facet = rng.random()
            along = (r + 0.5) / (len(rings) - 1)
            colours += [(facet, along, 0, 1)] * 4
    ob = _object("crystal", verts, faces, colours, smooth=False)
    # Merge the collapsed tips so they're points rather than rings of zero-area faces.
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    bm.to_mesh(ob.data)
    bm.free()
    return ob


def arrow():
    """Tip at the origin, shaft back along -z: 0.67 m of ash, a leaf-shaped iron head and three
    fletchings near the nock (one red cock feather, two pale). One mesh, a material each."""
    mats = [
        _material("fx.arrow.wood", (0.46, 0.33, 0.2), roughness=0.7),
        _material("fx.arrow.iron", (0.42, 0.43, 0.45), metallic=0.85, roughness=0.38),
        _material("fx.arrow.feather", (0.72, 0.16, 0.1), roughness=0.85),
        _material("fx.arrow.pale", (0.86, 0.84, 0.78), roughness=0.85),
    ]
    for m in mats:
        # Thin vanes are seen from both sides.
        m.use_backface_culling = False
    bm = bmesh.new()

    def face(points, material):
        f = bm.faces.new([bm.verts.new(_b(*p)) for p in points])
        f.material_index = material
        f.smooth = material == 0
        return f

    r, n = 0.0045, 8
    ring = lambda z: [(r * math.cos(i * math.tau / n), r * math.sin(i * math.tau / n), z) for i in range(n)]  # noqa: E731
    front, back = ring(-0.075), ring(-0.75)
    for i in range(n):
        j = (i + 1) % n
        face((front[i], back[i], back[j], front[j]), 0)
    face(list(reversed(front)), 0)
    face(back, 0)
    tip = (0.0, 0.0, 0.0)
    wide = [(0.017, 0, -0.05), (0, 0.004, -0.05), (-0.017, 0, -0.05), (0, -0.004, -0.05)]
    neck = [(0.005, 0, -0.08), (0, 0.005, -0.08), (-0.005, 0, -0.08), (0, -0.005, -0.08)]
    for i in range(4):
        j = (i + 1) % 4
        face((tip, wide[i], wide[j]), 1)
        face((wide[i], neck[i], neck[j], wide[j]), 1)
    face(list(reversed(neck)), 1)
    vane = [(0.004, -0.61), (0.022, -0.66), (0.024, -0.72), (0.004, -0.73)]
    for k, material in enumerate((2, 3, 3)):
        a = math.pi / 2 + k * math.tau / 3
        ca, sa = math.cos(a), math.sin(a)
        face([(ca * d, sa * d, z) for d, z in vane], material)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-6)
    me = bpy.data.meshes.new("arrow")
    bm.to_mesh(me)
    bm.free()
    for m in mats:
        me.materials.append(m)
    ob = bpy.data.objects.new("arrow", me)
    bpy.context.scene.collection.objects.link(ob)
    return [ob]


def export(path, objects):
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    path.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_animations=False,
        export_yup=True,
        export_normals=True,
        export_texcoords=True,
        export_vertex_color="ACTIVE",
        export_all_vertex_colors=False,
        export_active_vertex_color_when_no_material=True,
        export_materials="EXPORT",
    )


def build(out_dir):
    """Writes every effect mesh to `out_dir` (assets/effects)."""
    written = []
    for name, make in (("shell", shell), ("column", column), ("crystal", crystal)):
        sc.reset()
        ob = make()
        export(out_dir / f"{name}.glb", [ob])
        written.append(f"{name}.glb")
    sc.reset()
    export(out_dir / "arrow.glb", arrow())
    written.append("arrow.glb")
    return written
