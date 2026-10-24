"""Review renders: neutral studio lighting, framed on whatever is visible."""

import math

import bpy
from mathutils import Vector

from . import scene as sc


def _bounds(objects):
    pts = []
    for ob in objects:
        if ob.type == "MESH" and not ob.hide_render:
            pts += sc.evaluated_points(ob)
    lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    return lo, hi


def studio(floor=True):
    """Lights, world and a floor, created once per scene."""
    scene = bpy.context.scene
    if "review_key" in bpy.data.objects:
        return
    for eng in ("BLENDER_EEVEE_NEXT", "BLENDER_EEVEE"):
        try:
            scene.render.engine = eng
            break
        except TypeError:
            pass
    scene.view_settings.view_transform = "AgX"
    world = bpy.data.worlds.new("review_world")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.36, 0.38, 0.42, 1)
    bg.inputs[1].default_value = 0.55
    for name, energy, size, color in (
        ("review_key", 520, 2.5, (1.0, 0.95, 0.88)),
        ("review_fill", 200, 3.0, (0.85, 0.9, 1.0)),
        ("review_rim", 420, 2.0, (1.0, 1.0, 1.0)),
    ):
        light = bpy.data.objects.new(name, bpy.data.lights.new(name, "AREA"))
        light.data.energy = energy
        light.data.size = size
        light.data.color = color
        scene.collection.objects.link(light)
    if floor:
        me = bpy.data.meshes.new("review_floor")
        me.from_pydata([(-6, -6, 0), (6, -6, 0), (6, 6, 0), (-6, 6, 0)], [], [(0, 1, 2, 3)])
        ob = bpy.data.objects.new("review_floor", me)
        mat = bpy.data.materials.new("review_floor")
        mat.use_nodes = True
        mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.45, 0.44, 0.42, 1)
        me.materials.append(mat)
        scene.collection.objects.link(ob)
    cam = bpy.data.objects.new("review_cam", bpy.data.cameras.new("review_cam"))
    scene.collection.objects.link(cam)
    scene.camera = cam


def render(path, objects, yaw=25.0, size=(900, 1100), pad=1.0, look=None, height=None, focus=None, elev=0.0):
    """Renders the objects from the front (yaw 0 looks at the -Y face), rotated by yaw degrees.

    `focus` frames a region instead: a (centre, half-height) pair in world space. `elev` raises
    the camera by that many degrees (90 looks straight down).
    """
    studio()
    scene = bpy.context.scene
    bpy.context.view_layer.update()
    lo, hi = _bounds(objects)
    if focus:
        c, half = focus
        h = half * 2
    else:
        c = (lo + hi) / 2
        h = (hi.z - lo.z) * pad
        # Wide things (rocks, bridges) are framed by their width too.
        span = max(hi.x - lo.x, hi.y - lo.y) * pad
        h = max(h, span * size[1] / size[0] * 0.9)
    if "review_floor" in bpy.data.objects:
        bpy.data.objects["review_floor"].location.z = lo.z
    a = math.radians(yaw)
    cam = bpy.data.objects["review_cam"]
    cam.data.lens = 50
    aspect = size[0] / size[1]
    # Blender fits the 36 mm sensor to the longer side of the frame.
    half = math.atan(18 / cam.data.lens)
    vfov_half = half if aspect <= 1 else math.atan(math.tan(half) / aspect)
    dist = (h * 1.08 / 2) / math.tan(vfov_half)
    e = math.radians(min(elev, 89.5))
    eye = Vector((c.x - math.sin(a) * math.cos(e) * dist, c.y - math.cos(a) * math.cos(e) * dist, c.z + math.sin(e) * dist + h * 0.06))
    cam.location = eye
    cam.rotation_euler = (c - eye).to_track_quat("-Z", "Y").to_euler()
    for name, off in (("review_key", (-2.5, -3.0, 2.5)), ("review_fill", (3.0, -1.5, 0.5)), ("review_rim", (0.5, 3.0, 2.0))):
        light = bpy.data.objects[name]
        # Lights turn with the camera so every view is lit the same way.
        x, y, z = off
        light.location = Vector((c.x + x * math.cos(a) + y * math.sin(a), c.y - x * math.sin(a) + y * math.cos(a), c.z + z))
        light.rotation_euler = (c - light.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.resolution_x, scene.render.resolution_y = size
    scene.render.resolution_percentage = 100
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    return path


def montage(path, files, cols, max_width=2400):
    """Tiles same-sized-or-smaller images into one sheet (rows top to bottom)."""
    import numpy as np

    ims = []
    for f in files:
        img = bpy.data.images.load(str(f), check_existing=False)
        w, h = img.size
        px = np.empty(w * h * 4, dtype=np.float32)
        img.pixels.foreach_get(px)
        ims.append(px.reshape(h, w, 4))
        bpy.data.images.remove(img)
    w = max(i.shape[1] for i in ims)
    h = max(i.shape[0] for i in ims)
    rows = (len(ims) + cols - 1) // cols
    sheet = np.zeros((h * rows, w * cols, 4), dtype=np.float32)
    sheet[..., :3] = 0.16
    sheet[..., 3] = 1
    for k, im in enumerate(ims):
        r, c = k // cols, k % cols
        y0 = (rows - 1 - r) * h  # Blender images start at the bottom row.
        sheet[y0 + h - im.shape[0]:y0 + h, c * w:c * w + im.shape[1]] = im
    step = max(1, int(np.ceil(sheet.shape[1] / max_width)))
    sheet = sheet[::step, ::step]
    out = bpy.data.images.new("montage", sheet.shape[1], sheet.shape[0])
    out.pixels.foreach_set(sheet.ravel())
    out.filepath_raw = str(path)
    out.file_format = "PNG"
    out.save()
    bpy.data.images.remove(out)
    return path
