"""glTF export with the settings the engine and the workshop expect.

Pieces are written as `.gltf` + `.bin` next to a shared `textures/` folder, so parts of one outfit
share their texture files instead of embedding copies.
"""

from pathlib import Path

import bpy

from . import scene


def _select(objects):
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for o in bpy.context.view_layer.objects:
        o.select_set(False)
    for o in objects:
        o.hide_set(False)
        o.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]


def piece(path, objects):
    """A skinned piece (or the body): its armature plus meshes, no animation."""
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
        export_def_bones=False,
        export_rest_position_armature=True,
        export_materials="EXPORT",
        export_image_format="AUTO",
    )
    return path


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


def rigid(path, objects):
    """An unskinned item (a sword, a shield) authored around its attachment point."""
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
