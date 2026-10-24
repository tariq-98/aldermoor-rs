"""Scene helpers shared by every build script."""

from pathlib import Path

import bpy

# The Universal Animation Library is authored at 30 frames a second. Importing at the same rate
# keeps every key on a whole frame.
FPS = 30

ROOT = Path(__file__).resolve().parents[3]
VENDOR = ROOT / "art" / "vendor"
OUT = ROOT / "assets"


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.fps = FPS
    scene.render.fps_base = 1.0
    return scene


def import_gltf(path):
    """Imports a glTF file and returns the new objects, minus Quaternius's stray icospheres."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path), bone_heuristic="BLENDER")
    new = [o for o in bpy.data.objects if o not in before]
    for name in [o.name for o in new if o.name.startswith("Icosphere")]:
        bpy.data.objects.remove(bpy.data.objects[name])
    return [o for o in bpy.data.objects if o not in before]


def alive(objects):
    """Drops objects that have since been deleted."""
    out = []
    for o in objects:
        try:
            o.name
        except ReferenceError:
            continue
        out.append(o)
    return out


def armature_of(objects):
    arms = [o for o in alive(objects) if o.type == "ARMATURE"]
    assert len(arms) == 1, f"expected one armature, got {[a.name for a in arms]}"
    return arms[0]


def meshes_of(objects):
    return [o for o in alive(objects) if o.type == "MESH"]


def remove(objects):
    names = [o.name for o in alive(objects)]
    for n in names:
        if n in bpy.data.objects:
            bpy.data.objects.remove(bpy.data.objects[n])


def activate(obj, mode="OBJECT"):
    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")
    for o in bpy.context.selected_objects:
        o.select_set(False)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    if mode != "OBJECT":
        bpy.ops.object.mode_set(mode=mode)


def bind(mesh, arm):
    """Parents a mesh to an armature with a single armature modifier, keeping it in place."""
    mw = mesh.matrix_world.copy()
    mesh.parent = arm
    mesh.matrix_parent_inverse = arm.matrix_world.inverted()
    mesh.matrix_world = mw
    mods = [m for m in mesh.modifiers if m.type == "ARMATURE"]
    for m in mods[1:]:
        mesh.modifiers.remove(m)
    mod = mods[0] if mods else mesh.modifiers.new("Armature", "ARMATURE")
    mod.object = arm
    mod.use_vertex_groups = True
    mod.use_deform_preserve_volume = False


def evaluated_points(obj):
    """World-space vertex positions after modifiers (the current pose)."""
    dg = bpy.context.evaluated_depsgraph_get()
    ev = obj.evaluated_get(dg)
    me = ev.to_mesh()
    pts = [ev.matrix_world @ v.co for v in me.vertices]
    ev.to_mesh_clear()
    return pts
