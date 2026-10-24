"""Textures and materials: shared texture files, tintable skin and hair."""

from pathlib import Path

import bpy
import numpy as np

_done = {}


def _srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def _linear_to_srgb(c):
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(np.clip(c, 0, None), 1 / 2.4) - 0.055)


def _load(path):
    img = bpy.data.images.load(str(path), check_existing=True)
    return img


def _pixels(img):
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    return px.reshape(h, w, 4)


def _save(px, path, colorspace="sRGB"):
    """Writes an RGBA float array (bottom row first, as Blender stores it) to PNG or JPEG."""
    h, w, _ = px.shape
    img = bpy.data.images.new("__publish__", w, h, alpha=px[..., 3].min() < 1.0)
    img.colorspace_settings.name = colorspace
    img.pixels.foreach_set(px.ravel())
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    img.filepath_raw = str(path)
    img.file_format = "JPEG" if path.suffix == ".jpg" else "PNG"
    if path.suffix == ".jpg":
        bpy.context.scene.render.image_settings.quality = 90
    img.save()
    bpy.data.images.remove(img)
    out = _load(path)
    out.reload()
    out.colorspace_settings.name = colorspace
    return out


def resized(img, max_size):
    px = _pixels(img)
    h, w, _ = px.shape
    while max(h, w) > max_size:
        px = px.reshape(h // 2, 2, w // 2, 2, 4).mean(axis=(1, 3))
        h, w = h // 2, w // 2
    return px


def publish(img, out_dir, max_size=2048, kind="color"):
    """Copies an image into the shared textures folder at no more than max_size pixels.

    Colour maps without alpha become JPEG; data maps (normal, ORM, roughness) stay PNG.
    Returns the new image, which the glTF exporter then references in place.
    """
    key = (img.filepath, max_size, kind)
    if key in _done:
        try:
            _done[key].name  # A scene reset frees every image.
            return _done[key]
        except ReferenceError:
            del _done[key]
    stem = Path(bpy.path.abspath(img.filepath) or img.name).stem
    px = resized(img, max_size)
    has_alpha = px[..., 3].min() < 0.999
    ext = ".jpg" if kind == "color" and not has_alpha else ".png"
    out = _save(px, Path(out_dir) / f"{stem}{ext}", "sRGB" if kind == "color" else "Non-Color")
    _done[key] = out
    return out


def tint_detail(img, out_dir, name, max_size=2048):
    """A tone-neutral version of a skin or hair colour map, plus its mean colour (linear RGB).

    The renderer multiplies the detail map by 2 x the chosen tone, so a tone equal to the mean
    reproduces the original painting, lips and blush included.
    """
    px = resized(img, max_size)
    rgb = _srgb_to_linear(px[..., :3])
    lum = rgb @ np.array([0.2126, 0.7152, 0.0722])
    used = (lum > 0.01) & (px[..., 3] > 0.5)
    mean = rgb[used].mean(axis=0)
    detail = np.clip(rgb / mean * 0.5, 0, 1)
    out = np.concatenate([_linear_to_srgb(detail), px[..., 3:]], axis=-1).astype(np.float32)
    # Unused texels stay neutral, so filtering at UV seams doesn't darken the edges.
    out[~used, :3] = _linear_to_srgb(np.array([0.5, 0.5, 0.5]))
    image = _save(out, Path(out_dir) / f"{name}.jpg", "sRGB")
    return image, [float(x) for x in mean]


def images_of(material):
    return [n for n in material.node_tree.nodes if n.type == "TEX_IMAGE" and n.image]


def base_color_node(material):
    bsdf = next(n for n in material.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    link = bsdf.inputs["Base Color"].links
    node = link[0].from_node if link else None
    if node is not None and node.type == "MIX":
        # The importer multiplies by COLOR_0: look through the mix for the texture.
        for inp in node.inputs:
            if inp.links and inp.links[0].from_node.type == "TEX_IMAGE":
                return inp.links[0].from_node
    return node if node is not None and node.type == "TEX_IMAGE" else None


def simplify(mesh):
    """Drops Quaternius's all-white vertex colours and wires textures straight to the shader."""
    me = mesh.data
    # Remove by name: references shift as attributes are removed.
    for name in [ca.name for ca in me.color_attributes]:
        me.color_attributes.remove(me.color_attributes[name])
    for slot in mesh.material_slots:
        m = slot.material
        if m is None or not m.use_nodes:
            continue
        nt = m.node_tree
        bsdf = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
        if bsdf is None:
            continue
        tex = base_color_node(m)
        # An alpha link forces PNG on export; drop it when the texture is fully opaque.
        alpha = bsdf.inputs["Alpha"].links
        if alpha and alpha[0].from_node.type == "TEX_IMAGE":
            img = alpha[0].from_node.image
            if img is None or not img.has_data or _pixels(img)[..., 3].min() > 0.99:
                nt.links.remove(alpha[0])
                m.blend_method = "OPAQUE" if hasattr(m, "blend_method") else None
        link = bsdf.inputs["Base Color"].links
        if link and link[0].from_node.type == "MIX":
            mix = link[0].from_node
            if tex is not None:
                nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
            for n in [n for n in nt.nodes if n.type == "VERTEX_COLOR"]:
                nt.nodes.remove(n)
            nt.nodes.remove(mix)


def publish_material(material, out_dir, max_size=2048):
    """Points every texture of an imported material at a published copy."""
    base = base_color_node(material)
    for node in images_of(material):
        kind = "color" if base is not None and node.name == base.name else "data"
        node.image = publish(node.image, out_dir, max_size, kind)


def find_image(name, *dirs):
    for d in dirs:
        for ext in (".png", ".jpg"):
            p = Path(d) / f"{name}{ext}"
            if p.exists():
                return _load(p)
    raise FileNotFoundError(name)


def repair_missing(material, *dirs):
    """Quaternius glTFs sometimes name textures that aren't in the folder (`X_png.png`)."""
    for node in images_of(material):
        img = node.image
        if img.has_data or Path(bpy.path.abspath(img.filepath)).exists():
            continue
        stem = Path(img.filepath).stem.replace("_png", "")
        try:
            node.image = find_image(stem, *dirs)
            node.image.colorspace_settings.name = img.colorspace_settings.name
        except FileNotFoundError:
            pass
