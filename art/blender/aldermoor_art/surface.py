"""The surface pass: procedural wood, leather, metal, paint and cloth, baked into one material.

Gear is modelled with flat palette materials (shapes.PALETTE). This pass gives each of them
detail in shader nodes (grain along the item's length, pores, brushed metal, polished edges,
grime in crevices, chipped paint, weave), then bakes everything with Cycles into two textures on
the item's own UVs:
- `<id>_color.jpg`: base colour (sRGB);
- `<id>_orm.jpg`: occlusion (R), roughness (G) and metalness (B), as glTF reads them.
The item then has one material for all of it, plus separate ones for glowing and see-through
parts (crystals, flames, glass), which keep their own. Fewer materials means fewer draw calls.
"""

from pathlib import Path

import bpy
import numpy as np

from . import materials as mt, shapes as sh

FAMILIES = {
    "wood": ("wood", "wood_light", "wood_dark"),
    "leather": ("leather", "leather_dark", "leather_tan"),
    "metal": ("steel", "dark_steel", "iron", "brass", "gold", "silver"),
    "paint": ("paint_red", "paint_blue", "paint_cream"),
    "cloth": ("cord", "canvas", "linen", "wool_red", "wool_red_trim", "wool_blue", "wool_green", "wool_grey", "wool_cream"),
    "bone": ("bone",),
    "feather": ("feather", "feather_red"),
}
FAMILY_OF = {name: fam for fam, names in FAMILIES.items() for name in names}
GRIMY = {"iron", "dark_steel"}  # Metals that collect rust and grime in their crevices.


def keeps_own(name):
    """Glowing and see-through parts keep their own materials."""
    return name in sh.GLOW or name in sh.TRANSLUCENT or name.startswith("crystal") or name not in FAMILY_OF


# --- Node helpers -----------------------------------------------------------------------------

class G:
    """A tiny builder over one material's node tree."""

    def __init__(self, material):
        self.nt = material.node_tree
        self.n = self.nt.nodes
        self.l = self.nt.links
        tc = self.n.new("ShaderNodeTexCoord")
        self.obj = tc.outputs["Object"]
        # Edges from the bevel shader: where a rounded normal leaves the true one. Unlike
        # pointiness, it doesn't depend on how finely the part is tessellated.
        geo = self.n.new("ShaderNodeNewGeometry")
        bev = self.n.new("ShaderNodeBevel")
        bev.samples = 8
        bev.inputs["Radius"].default_value = 0.0012  # Under the thickness of thin parts (bands, faces).
        dot = self.n.new("ShaderNodeVectorMath")
        dot.operation = "DOT_PRODUCT"
        self.link(bev.outputs["Normal"], dot.inputs[0])
        self.link(geo.outputs["Normal"], dot.inputs[1])
        self.edge_raw = self.math("SUBTRACT", 1.0, dot.outputs["Value"])

    def link(self, a, b):
        self.l.new(a, b)

    def value(self, v):
        n = self.n.new("ShaderNodeValue")
        n.outputs[0].default_value = v
        return n.outputs[0]

    def rgb(self, c):
        n = self.n.new("ShaderNodeRGB")
        n.outputs[0].default_value = (*c, 1.0)
        return n.outputs[0]

    def mapped(self, scale):
        m = self.n.new("ShaderNodeMapping")
        m.inputs["Scale"].default_value = scale
        self.link(self.obj, m.inputs["Vector"])
        return m.outputs["Vector"]

    def noise(self, scale, detail=2.0, rough=0.5, distortion=0.0):
        n = self.n.new("ShaderNodeTexNoise")
        self.link(self.mapped(scale), n.inputs["Vector"])
        n.inputs["Scale"].default_value = 1.0
        n.inputs["Detail"].default_value = detail
        n.inputs["Roughness"].default_value = rough
        n.inputs["Distortion"].default_value = distortion
        return n.outputs["Fac"]

    def voronoi(self, scale):
        n = self.n.new("ShaderNodeTexVoronoi")
        self.link(self.mapped(scale), n.inputs["Vector"])
        n.inputs["Scale"].default_value = 1.0
        return n.outputs["Distance"]

    def ao(self, distance):
        n = self.n.new("ShaderNodeAmbientOcclusion")
        n.inputs["Distance"].default_value = distance
        n.samples = 16
        return n.outputs["AO"]

    def ramp(self, x, a, b):
        """0 below a, 1 above b (clamped, linear)."""
        n = self.n.new("ShaderNodeMapRange")
        n.clamp = True
        self.link(x, n.inputs["Value"])
        n.inputs["From Min"].default_value = a
        n.inputs["From Max"].default_value = b
        return n.outputs["Result"]

    def math(self, op, a, b=None):
        n = self.n.new("ShaderNodeMath")
        n.operation = op
        for i, v in enumerate((a, b)):
            if v is None:
                continue
            if isinstance(v, (int, float)):
                n.inputs[i].default_value = v
            else:
                self.link(v, n.inputs[i])
        return n.outputs[0]

    def around(self, x, centre, amount):
        """centre + (x - 0.5) * 2 * amount: a value varying about `centre`."""
        return self.math("ADD", self.math("MULTIPLY", self.math("SUBTRACT", x, 0.5), 2 * amount), centre)

    def _mix_node(self, blend="MIX"):
        n = self.n.new("ShaderNodeMix")
        n.data_type = "RGBA"
        n.blend_type = blend
        sock = {s.identifier: s for s in n.inputs}
        out = {s.identifier: s for s in n.outputs}
        return n, sock, out

    def mix(self, fac, a, b, blend="MIX"):
        """Colour mix; a and b are sockets or RGB tuples, fac a socket or float."""
        n, sock, out = self._mix_node(blend)
        if isinstance(fac, (int, float)):
            sock["Factor_Float"].default_value = fac
        else:
            self.link(fac, sock["Factor_Float"])
        for key, v in (("A_Color", a), ("B_Color", b)):
            if isinstance(v, tuple):
                sock[key].default_value = (*v, 1.0)
            else:
                self.link(v, sock[key])
        return out["Result_Color"]

    def scale_colour(self, colour, k):
        """colour × k, k a float socket: a multiply mix with a grey made from k."""
        grey = self.n.new("ShaderNodeCombineColor")
        for i in range(3):
            self.link(k, grey.inputs[i])
        return self.mix(1.0, colour, grey.outputs[0], "MULTIPLY")


# --- Families ----------------------------------------------------------------------------------

def graph(material, name):
    """Builds detail for a palette material; returns (colour, roughness, metallic) sockets."""
    base, metal, rough = sh.PALETTE[name]
    g = G(material)
    fam = FAMILY_OF[name]
    edge = g.ramp(g.edge_raw, 0.01, 0.1)
    if fam == "wood":
        streak = g.noise((42, 42, 2.5), detail=6, rough=0.62, distortion=0.35)
        fine = g.noise((230, 230, 14), detail=2)
        v = g.math("ADD", g.math("MULTIPLY", streak, 0.75), g.math("MULTIPLY", fine, 0.25))
        k = g.around(v, 1.0, 0.45)
        col = g.scale_colour(g.rgb(base), k)
        col = g.mix(g.math("MULTIPLY", edge, 0.45), col, tuple(min(1.0, c * 1.45) for c in base))
        r = g.math("SUBTRACT", g.around(fine, rough, 0.06), g.math("MULTIPLY", edge, 0.12))
        return col, r, g.value(0.0)
    if fam == "leather":
        blotch = g.noise((14, 14, 14), detail=4)
        pores = g.ramp(g.voronoi((260, 260, 260)), 0.0, 0.35)
        crease = g.ramp(g.math("ABSOLUTE", g.math("SUBTRACT", g.noise((26, 26, 26), detail=3, distortion=1.5), 0.5)), 0.0, 0.035)
        k = g.math("MULTIPLY", g.around(blotch, 1.0, 0.18), g.math("ADD", g.math("MULTIPLY", pores, 0.12), 0.88))
        k = g.math("MULTIPLY", k, g.math("ADD", g.math("MULTIPLY", crease, 0.22), 0.78))
        col = g.scale_colour(g.rgb(base), k)
        col = g.mix(g.math("MULTIPLY", edge, 0.55), col, tuple(min(1.0, c * 1.6) for c in base))
        r = g.math("SUBTRACT", g.around(blotch, rough, 0.08), g.math("MULTIPLY", edge, 0.18))
        return col, r, g.value(0.0)
    if fam == "metal":
        brushed = g.noise((420, 420, 9), detail=2)
        smudge = g.noise((6, 6, 6), detail=5)
        cavity = g.math("SUBTRACT", 1.0, g.ao(0.025))
        k = g.math("MULTIPLY", g.around(brushed, 1.0, 0.06), g.math("ADD", g.math("MULTIPLY", edge, 0.22), 1.0))
        col = g.scale_colour(g.rgb(base), k)
        grime = (0.07, 0.04, 0.02) if name in GRIMY else tuple(c * 0.45 for c in base)
        dirt = g.math("MULTIPLY", g.ramp(cavity, 0.15, 0.7), 0.75 if name in GRIMY else 0.45)
        col = g.mix(dirt, col, grime)
        r = g.around(smudge, rough, 0.14)
        r = g.math("ADD", g.math("SUBTRACT", r, g.math("MULTIPLY", edge, 0.16)), g.math("MULTIPLY", dirt, 0.35))
        m = g.math("SUBTRACT", 1.0, g.math("MULTIPLY", dirt, 0.6 if name in GRIMY else 0.3))
        return col, r, m
    if fam == "paint":
        chips = g.noise((16, 16, 16), detail=6, rough=0.65)
        mask = g.ramp(g.math("ADD", chips, g.math("MULTIPLY", edge, 0.4)), 0.66, 0.7)
        k = g.around(g.noise((9, 9, 9), detail=3), 1.0, 0.1)
        col = g.mix(mask, g.scale_colour(g.rgb(base), k), (0.2, 0.11, 0.05))
        r = g.math("ADD", g.math("MULTIPLY", mask, 0.2), rough)
        return col, r, g.value(0.0)
    if fam == "cloth":
        wx = g.noise((900, 120, 900), detail=1)
        wz = g.noise((120, 900, 120), detail=1)
        blotch = g.noise((10, 10, 10), detail=3)
        k = g.math("MULTIPLY", g.around(g.math("MULTIPLY", wx, wz), 1.0, 0.12), g.around(blotch, 1.0, 0.1))
        return g.scale_colour(g.rgb(base), k), g.around(blotch, rough, 0.04), g.value(0.0)
    if fam == "bone":
        blotch = g.noise((22, 22, 22), detail=4)
        cavity = g.math("SUBTRACT", 1.0, g.ao(0.02))
        col = g.scale_colour(g.rgb(base), g.around(blotch, 1.0, 0.14))
        col = g.mix(g.math("MULTIPLY", g.ramp(cavity, 0.1, 0.6), 0.7), col, (0.2, 0.14, 0.07))
        return col, g.around(blotch, rough, 0.1), g.value(0.0)
    # Feathers: streaks along the vane.
    streak = g.noise((300, 300, 12), detail=2)
    return g.scale_colour(g.rgb(base), g.around(streak, 1.0, 0.16)), g.value(rough), g.value(0.0)


# --- Baking ------------------------------------------------------------------------------------

def _palette_name(material):
    return material.name.split(".", 1)[1].rsplit(".", 1)[0] if material.name.startswith("gear.") else material.name


def _occlusion_group():
    """The node group Blender's glTF exporter reads occlusion from."""
    g = bpy.data.node_groups.get("glTF Material Output")
    if g is None:
        g = bpy.data.node_groups.new("glTF Material Output", "ShaderNodeTree")
        g.interface.new_socket("Occlusion", in_out="INPUT", socket_type="NodeSocketFloat")
    return g


def _image(name, size):
    img = bpy.data.images.new(name, size, size, alpha=False, float_buffer=True)
    img.colorspace_settings.name = "Non-Color"
    return img


def _pixels(img):
    w, h = img.size
    px = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(px)
    return px.reshape(h, w, 4)


def bake(ob, out_dir, stem, size):
    """Bakes the object's palette materials into `<stem>_color.jpg` and `<stem>_orm.jpg`.

    Returns the new material (the object's faces are reassigned to it).
    """
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    scene.cycles.samples = 1
    scene.render.bake.margin = 8
    scene.render.bake.margin_type = "EXTEND"
    scene.render.bake.use_clear = True
    if scene.world is None:
        scene.world = bpy.data.worlds.new("bake")
    scene.world.light_settings.distance = 0.04
    slots = [s.material for s in ob.material_slots]
    bakeable = [m for m in slots if m is not None and not keeps_own(_palette_name(m))]
    if not bakeable:
        return None
    # Work on copies, so the shared palette materials stay as they are.
    copies = {}
    for m in bakeable:
        c = m.copy()
        copies[m.name] = c
    for i, s in enumerate(ob.material_slots):
        if s.material and s.material.name in copies:
            s.material = copies[s.material.name]
    targets = {}
    passes = {}
    for orig, m in copies.items():
        col, rough, metal = graph(m, _palette_name(bpy.data.materials[orig]))
        out = next(n for n in m.node_tree.nodes if n.type == "OUTPUT_MATERIAL")
        emit = m.node_tree.nodes.new("ShaderNodeEmission")
        m.node_tree.links.new(emit.outputs[0], out.inputs["Surface"])
        tex = m.node_tree.nodes.new("ShaderNodeTexImage")
        passes[m.name] = (emit, col, rough, metal)
        targets[m.name] = tex
    # Glowing parts bake too (their texels go unused), so every material needs a target.
    for s in ob.material_slots:
        m = s.material
        if m is None or m.name in targets:
            continue
        if not m.use_nodes:
            m.use_nodes = True
        targets[m.name] = m.node_tree.nodes.new("ShaderNodeTexImage")
    sh_sel = [o for o in bpy.context.selected_objects]
    for o in sh_sel:
        o.select_set(False)
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    images = {}
    for key in ("colour", "rough", "metal", "ao"):
        img = _image(f"{stem}_{key}", size)
        for name, tex in targets.items():
            tex.image = img
            bpy.data.materials[name].node_tree.nodes.active = tex
        if key == "ao":
            scene.cycles.samples = 48
            bpy.ops.object.bake(type="AO")
            scene.cycles.samples = 1
        else:
            for name, (emit, col, rough, metal) in passes.items():
                nt = bpy.data.materials[name].node_tree
                for link in list(emit.inputs["Color"].links):
                    nt.links.remove(link)
                src = {"colour": col, "rough": rough, "metal": metal}[key]
                if src.type == "VALUE" and key != "colour":
                    # A float into a colour input: through a grey.
                    grey = nt.nodes.new("ShaderNodeCombineColor")
                    for i in range(3):
                        nt.links.new(src, grey.inputs[i])
                    src = grey.outputs[0]
                nt.links.new(src, emit.inputs["Color"])
            bpy.ops.object.bake(type="EMIT")
        images[key] = _pixels(img)
        bpy.data.images.remove(img)
    out_dir = Path(out_dir)
    colour = images["colour"].copy()
    colour[..., :3] = mt._linear_to_srgb(np.clip(colour[..., :3], 0, 1))
    colour[..., 3] = 1
    col_img = mt._save(colour, out_dir / f"{stem}_color.jpg", "sRGB")
    orm = np.ones_like(colour)
    orm[..., 0] = np.clip(images["ao"][..., 0], 0, 1) * 0.85 + 0.15  # Never fully black.
    orm[..., 1] = np.clip(images["rough"][..., 0], 0.04, 1)
    orm[..., 2] = np.clip(images["metal"][..., 0], 0, 1)
    orm_img = mt._save(orm, out_dir / f"{stem}_orm.jpg", "Non-Color")
    # The one material the baked parts share.
    m = bpy.data.materials.new(f"gear.{stem}")
    m.use_nodes = True
    nt = m.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    t_col = nt.nodes.new("ShaderNodeTexImage")
    t_col.image = col_img
    nt.links.new(t_col.outputs["Color"], bsdf.inputs["Base Color"])
    t_orm = nt.nodes.new("ShaderNodeTexImage")
    t_orm.image = orm_img
    sep = nt.nodes.new("ShaderNodeSeparateColor")
    nt.links.new(t_orm.outputs["Color"], sep.inputs[0])
    nt.links.new(sep.outputs[1], bsdf.inputs["Roughness"])
    nt.links.new(sep.outputs[2], bsdf.inputs["Metallic"])
    occ = nt.nodes.new("ShaderNodeGroup")
    occ.node_tree = _occlusion_group()
    nt.links.new(sep.outputs[0], occ.inputs["Occlusion"])
    # Reassign faces: baked materials become the one; the rest keep theirs.
    old = [s.material for s in ob.material_slots]
    new_slots = [m] + [mm for mm in old if mm is not None and mm not in copies.values()]
    index = {}
    for i, mm in enumerate(old):
        index[i] = 0 if mm in copies.values() else new_slots.index(mm)
    mats = np.empty(len(ob.data.polygons), dtype=np.int32)
    ob.data.polygons.foreach_get("material_index", mats)
    ob.data.materials.clear()
    for mm in new_slots:
        ob.data.materials.append(mm)
    ob.data.polygons.foreach_set("material_index", np.array([index[i] for i in mats], dtype=np.int32))
    ob.data.update()
    for c in copies.values():
        bpy.data.materials.remove(c)
    for engine in ("BLENDER_EEVEE_NEXT", "BLENDER_EEVEE"):
        try:
            scene.render.engine = engine
            break
        except TypeError:
            pass
    return m
