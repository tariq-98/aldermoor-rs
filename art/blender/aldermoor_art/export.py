"""glTF export with the settings the engine and the workshop expect.

Pieces are written as `.gltf` + `.bin` next to a shared `textures/` folder, so parts of one outfit
share their texture files instead of embedding copies.

Every piece and rigid item also gets levels of detail beside it: `<name>.lod1.gltf` and
`<name>.lod2.gltf`, decimated to `LODS` of the triangles, on the same skeleton and materials.
Decimation keeps region borders (so hiding stays exact) and open edges where they are, and moves
UV seams only when it must.
Meshes with morph targets (bows) have no levels of detail.
"""

from pathlib import Path

import bmesh
import bpy
import numpy as np

from . import scene

LODS = (0.5, 0.2)


def _select(objects):
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for o in objects:
        o.hide_set(False)
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]


def lod_path(path, n):
    path = Path(path)
    return path.with_name(f"{path.stem}.lod{n}{path.suffix}")


# --- Levels of detail -------------------------------------------------------------------------

def _protected(me):
    """(locked, seams): vertices decimation must keep (region borders and open edges) and
    vertices on UV seams, which it may move only as a last resort."""
    bm = bmesh.new()
    bm.from_mesh(me)
    locked, seams = set(), set()
    reg = bm.loops.layers.float.get("_REGION")
    uv = bm.loops.layers.uv.active
    for e in bm.edges:
        faces = list(e.link_faces)
        if len(faces) != 2:
            locked.update(v.index for v in e.verts)
            continue
        if reg is not None and round(faces[0].loops[0][reg]) != round(faces[1].loops[0][reg]):
            locked.update(v.index for v in e.verts)
            continue
        if uv is not None:
            for v in e.verts:
                a = next(l[uv].uv for l in faces[0].loops if l.vert == v)
                b = next(l[uv].uv for l in faces[1].loops if l.vert == v)
                if (a - b).length > 1e-5:
                    seams.update(x.index for x in e.verts)
                    break
    bm.free()
    return locked, seams - locked


def _snap_regions(me):
    """One region per face again (decimation can blend corner values at a collapse)."""
    if "_REGION" not in me.attributes:
        return
    attr = me.attributes["_REGION"]
    vals = np.empty(len(attr.data), dtype=np.float32)
    attr.data.foreach_get("value", vals)
    starts = np.empty(len(me.polygons), dtype=np.int32)
    totals = np.empty(len(me.polygons), dtype=np.int32)
    me.polygons.foreach_get("loop_start", starts)
    me.polygons.foreach_get("loop_total", totals)
    for s, t in zip(starts, totals):
        vals[s:s + t] = np.round(np.median(vals[s:s + t]))
    attr.data.foreach_set("value", vals)


def decimated(ob, ratio):
    """A decimated copy of a mesh object (same parent, modifiers and materials), or None."""
    if ob.data.shape_keys:
        return None
    c = ob.copy()
    c.data = ob.data.copy()
    bpy.context.scene.collection.objects.link(c)
    locked, seams = _protected(c.data)
    # Blender's collapse never moves a vertex of weight 0, and moves light ones last.
    group = "_lod_free"
    vg = c.vertex_groups.new(name=group)
    vg.add([i for i in range(len(c.data.vertices)) if i not in locked and i not in seams], 1.0, "REPLACE")
    if seams:
        vg.add(sorted(seams), 0.05, "REPLACE")
    mod = c.modifiers.new("lod", "DECIMATE")
    mod.decimate_type = "COLLAPSE"
    mod.ratio = ratio
    mod.use_collapse_triangulate = True
    mod.vertex_group = group
    mod.vertex_group_factor = 1.0
    scene.activate(c)
    bpy.ops.object.modifier_move_to_index(modifier="lod", index=0)
    bpy.ops.object.modifier_apply(modifier="lod")
    c.vertex_groups.remove(c.vertex_groups[group])
    _snap_regions(c.data)
    return c


def _with_lods(path, objects, write):
    """Writes LOD0 with `write`, then each level of detail from decimated mesh copies."""
    write(path, objects)
    meshes = [o for o in objects if o.type == "MESH"]
    others = [o for o in objects if o.type != "MESH"]
    if not meshes or any(m.data.shape_keys for m in meshes):
        return path
    for n, ratio in enumerate(LODS, 1):
        copies = [decimated(m, ratio) for m in meshes]
        write(lod_path(path, n), others + copies)
        scene.remove(copies)
    return path


# --- Exports ----------------------------------------------------------------------------------

def _piece(path, objects, sockets):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    _select(objects)
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLTF_SEPARATE",
        export_keep_originals=True,
        use_selection=True,
        export_apply=False,
        export_animations=False,
        export_skins=True,
        export_morph=False,
        export_attributes=True,
        export_extras=True,
        export_yup=True,
        export_def_bones=not sockets,
        export_rest_position_armature=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
    )
    return path


def piece(path, objects, sockets=False, lods=True):
    """A skinned piece (or the body): its armature plus meshes, no animation.

    Only bodies carry the socket bones (`sockets=True`); clothing binds to the body's skeleton by
    bone name, so leaving the sockets out keeps a socket change from touching every piece.
    """
    write = lambda p, objs: _piece(p, objs, sockets)  # noqa: E731
    return _with_lods(Path(path), objects, write) if lods else write(Path(path), objects)


def animations(path, arm):
    """The clip library: the reference skeleton and every action, no meshes."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    _select([arm])
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_anim_single_armature=True,
        export_force_sampling=True,
        export_frame_step=1,
        export_optimize_animation_size=True,
        export_optimize_animation_keep_anim_armature=True,
        export_anim_slide_to_zero=True,
        export_reset_pose_bones=True,
        export_rest_position_armature=True,
        export_skins=True,
        export_yup=True,
        export_def_bones=False,
        export_extras=True,
    )
    return path


def _rigid(path, objects):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    _select(objects)
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLTF_SEPARATE",
        export_keep_originals=True,
        use_selection=True,
        export_apply=True,
        export_animations=False,
        export_attributes=True,
        export_extras=True,
        export_yup=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
    )
    return path


def rigid(path, objects, lods=True):
    """An unskinned item (a sword, a shield) authored around its attachment point."""
    return _with_lods(Path(path), objects, _rigid) if lods else _rigid(Path(path), objects)
