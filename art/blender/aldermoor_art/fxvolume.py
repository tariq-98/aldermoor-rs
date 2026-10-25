"""Flipbooks rendered from Cycles volumes: a fire puff, a smoke puff and an explosion.

Each is one life, first frame to last: a particle plays its flipbook once over its own lifetime.
The volume is a noise-shaped ball whose radius, rise, density and heat change frame by frame.
Fire and the explosion are emission over absorption, rendered white, so the frame's brightness
is its heat and the effects shader colours it through a ramp; smoke scatters a key light from
above and a sky fill, so its brightness is its shading.

Frames come back as numpy arrays in the sprite channels (fx.py): heat or shading, cover, erosion.
"""

import math
import tempfile
from pathlib import Path

import bpy
import numpy as np

from . import fx
from . import scene as sc

FRAMES = 16


def _engine(size, samples):
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = samples
    scene.cycles.use_denoising = False
    scene.cycles.volume_step_rate = 0.5
    scene.cycles.volume_max_steps = 256
    scene.cycles.max_bounces = 2
    scene.cycles.volume_bounces = 1
    scene.render.film_transparent = True
    scene.render.resolution_x = size
    scene.render.resolution_y = size
    scene.render.resolution_percentage = 100
    scene.render.pixel_aspect_x = 1
    scene.render.pixel_aspect_y = 1
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0
    scene.view_settings.gamma = 1
    scene.render.image_settings.file_format = "OPEN_EXR"
    scene.render.image_settings.color_depth = "32"
    scene.render.image_settings.color_mode = "RGBA"
    return scene


def _camera(scene):
    cam_data = bpy.data.cameras.new("fx_camera")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = 2.0
    cam = bpy.data.objects.new("fx_camera", cam_data)
    scene.collection.objects.link(cam)
    cam.location = (0, -6, 0)
    cam.rotation_euler = (math.pi / 2, 0, 0)
    scene.camera = cam


def _world(scene, strength):
    world = bpy.data.worlds.new("fx_world")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes["Background"]
    bg.inputs[0].default_value = (0.55, 0.62, 0.75, 1)
    bg.inputs[1].default_value = strength


class _Graph:
    def __init__(self, material):
        self.nt = material.node_tree
        self.nodes = self.nt.nodes
        self.links = self.nt.links
        for n in list(self.nodes):
            self.nodes.remove(n)

    def node(self, kind, **inputs):
        n = self.nodes.new(kind)
        for k, v in inputs.items():
            n.inputs[k].default_value = v
        return n

    def math(self, op, a, b=None, clamp=False):
        n = self.nodes.new("ShaderNodeMath")
        n.operation = op
        n.use_clamp = clamp
        self.feed(n.inputs[0], a)
        if b is not None:
            self.feed(n.inputs[1], b)
        return n.outputs[0]

    def feed(self, socket, value):
        if isinstance(value, (int, float)):
            socket.default_value = value
        else:
            self.links.new(value, socket)


def _domain(scene, kind):
    """A cube spanning the camera's view whose volume is a noise-eroded ball. Returns the nodes
    whose values change per frame."""
    bpy.ops.mesh.primitive_cube_add(size=2.0)
    cube = bpy.context.object
    cube.name = f"fx_{kind}"
    mat = bpy.data.materials.new(f"fx_{kind}")
    mat.use_nodes = True
    cube.data.materials.append(mat)
    g = _Graph(mat)
    coords = g.node("ShaderNodeTexCoord")
    # Per-frame values: the ball's centre (rise), radius, noise time, erosion and heat.
    centre = g.node("ShaderNodeCombineXYZ")
    radius = g.node("ShaderNodeValue")
    erosion = g.node("ShaderNodeValue")
    heat = g.node("ShaderNodeValue")
    density = g.node("ShaderNodeValue")
    offset = g.node("ShaderNodeVectorMath")
    offset.operation = "SUBTRACT"
    g.links.new(coords.outputs["Object"], offset.inputs[0])
    g.links.new(centre.outputs[0], offset.inputs[1])
    dist = g.node("ShaderNodeVectorMath")
    dist.operation = "LENGTH"
    g.links.new(offset.outputs[0], dist.inputs[0])
    falloff = g.math("DIVIDE", dist.outputs["Value"], radius.outputs[0])
    shape = g.math("SUBTRACT", 1.0, falloff)
    noise = g.nodes.new("ShaderNodeTexNoise")
    noise.noise_dimensions = "4D"
    noise.inputs["Scale"].default_value = {"fire": 2.4, "smoke": 2.4, "burst": 2.6}[kind]
    noise.inputs["Detail"].default_value = {"fire": 6.0, "smoke": 10.0, "burst": 10.0}[kind]
    noise.inputs["Roughness"].default_value = {"fire": 0.58, "smoke": 0.66, "burst": 0.64}[kind]
    noise.inputs["Distortion"].default_value = {"fire": 0.6, "smoke": 0.25, "burst": 0.4}[kind]
    g.links.new(offset.outputs[0], noise.inputs["Vector"])
    centred = g.math("SUBTRACT", noise.outputs["Fac"], 0.5)
    wobble = g.math("MULTIPLY", centred, {"fire": 1.9, "smoke": 1.5, "burst": 1.6}[kind])
    field = g.math("ADD", shape, wobble)
    eroded = g.math("SUBTRACT", field, erosion.outputs[0])
    soft = g.math("MULTIPLY", eroded, {"fire": 2.6, "smoke": 3.6, "burst": 3.6}[kind], clamp=True)
    amount = g.math("MULTIPLY", soft, density.outputs[0])
    out = g.node("ShaderNodeOutputMaterial")
    if kind == "smoke":
        vol = g.nodes.new("ShaderNodeVolumePrincipled")
        vol.inputs["Color"].default_value = (0.82, 0.82, 0.82, 1)
        vol.inputs["Anisotropy"].default_value = 0.25
        g.links.new(amount, vol.inputs["Density"])
        g.links.new(vol.outputs[0], out.inputs["Volume"])
    else:
        absorb = g.nodes.new("ShaderNodeVolumeAbsorption")
        absorb.inputs["Color"].default_value = (0.0, 0.0, 0.0, 1)
        g.links.new(amount, absorb.inputs["Density"])
        emit = g.nodes.new("ShaderNodeEmission")
        emit.inputs["Color"].default_value = (1, 1, 1, 1)
        # Hottest at the heart of the ball: the field squared, times this frame's heat.
        core = g.math("MULTIPLY", soft, soft)
        glow = g.math("MULTIPLY", core, heat.outputs[0])
        g.links.new(glow, emit.inputs["Strength"])
        add = g.nodes.new("ShaderNodeAddShader")
        g.links.new(absorb.outputs[0], add.inputs[0])
        g.links.new(emit.outputs[0], add.inputs[1])
        g.links.new(add.outputs[0], out.inputs["Volume"])
    return {"centre": centre, "radius": radius, "noise": noise, "erosion": erosion, "heat": heat, "density": density}


def _ease_out(t, k=2.0):
    return 1 - (1 - t) ** k


# Per-frame parameters for each kind, t from 0 (birth) to 1 (death): centre height, radius,
# noise time, erosion, heat and density.
def _fire(t):
    return dict(rise=-0.35 + 0.75 * t, radius=0.32 + 0.38 * _ease_out(t), time=t * 1.6,
                erosion=-0.1 + 0.95 * t ** 1.4, heat=14.0 * (1 - t) ** 1.2 + 1.2, density=4.0)


def _smoke(t):
    return dict(rise=-0.25 + 0.4 * t, radius=0.3 + 0.55 * _ease_out(t, 1.6), time=t * 0.9,
                erosion=-0.05 + 0.8 * t ** 1.6, heat=0.0, density=7.0 * (1 - 0.35 * t))


def _burst(t):
    return dict(rise=0.1 * t, radius=0.22 + 0.68 * _ease_out(t, 3.0), time=t * 1.2,
                erosion=-0.05 + 1.0 * t ** 1.25, heat=40.0 * (1 - t) ** 2.0 + 1.5, density=12.0)


KINDS = {"fire": _fire, "smoke": _smoke, "burst": _burst}


def _read_exr(path):
    img = bpy.data.images.load(str(path))
    w, h = img.size
    px = np.empty(w * h * 4, np.float32)
    img.pixels.foreach_get(px)
    bpy.data.images.remove(img)
    # Blender's pixels run bottom row first.
    return px.reshape(h, w, 4)[::-1].copy()


def flipbook(kind, size=128, samples=64, frames=FRAMES, seed=5):
    """Renders `frames` frames of `kind` and returns them as sprites."""
    sc.reset()
    scene = _engine(size, samples)
    _camera(scene)
    if kind == "smoke":
        _world(scene, 0.55)
        sun_data = bpy.data.lights.new("fx_sun", "SUN")
        sun_data.energy = 4.5
        sun = bpy.data.objects.new("fx_sun", sun_data)
        scene.collection.objects.link(sun)
        sun.rotation_euler = (math.radians(-35), math.radians(-25), 0)
    else:
        _world(scene, 0.0)
    nodes = _domain(scene, kind)
    params = KINDS[kind]
    raw = []
    with tempfile.TemporaryDirectory() as tmp:
        for f in range(frames):
            t = (f + 0.5) / frames
            p = params(t)
            nodes["centre"].inputs["Z"].default_value = p["rise"]
            nodes["radius"].outputs[0].default_value = p["radius"]
            nodes["noise"].inputs["W"].default_value = p["time"] + seed * 7.31
            nodes["erosion"].outputs[0].default_value = p["erosion"]
            nodes["heat"].outputs[0].default_value = p["heat"]
            nodes["density"].outputs[0].default_value = p["density"]
            path = Path(tmp) / f"{kind}_{f:02d}.exr"
            scene.render.filepath = str(path)
            bpy.ops.render.render(write_still=True)
            raw.append(_read_exr(path))
    alpha = [np.clip(r[..., 3], 0, 1) for r in raw]
    lum = [r[..., 0] * 0.2126 + r[..., 1] * 0.7152 + r[..., 2] * 0.0722 for r in raw]
    if kind == "smoke":
        # Shading independent of how dense the smoke is: unpremultiply.
        shade = [np.where(a > 0.02, l / np.maximum(a, 0.02), 0) for l, a in zip(lum, alpha)]
        top = np.percentile(np.concatenate([s[a > 0.1] for s, a in zip(shade, alpha)]), 99.0)
        heat = [np.clip(s / top, 0, 1) for s in shade]
    else:
        top = np.percentile(np.concatenate([l.ravel() for l in lum]), 99.7)
        heat = [np.clip(l / top, 0, 1) for l in lum]
    out = []
    breakup = fx.noise_field(size, 6, seed + 100)
    x, y = fx.grid(size)
    r = np.hypot(x, y)
    vignette = fx.smooth(1.0, 0.86, r)
    for h, a in zip(heat, alpha):
        erode = np.clip(a ** 0.7 * 0.75 + breakup * 0.25, 0, 1)
        out.append(fx.pack(h * vignette, a * vignette, erode))
    return out
