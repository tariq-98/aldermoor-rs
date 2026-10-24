"""Assembles characters from the exported assets the way the game does (for review renders)."""

import json

import bpy
import numpy as np

from . import scene as sc


def catalog():
    return json.loads((sc.OUT / "catalog" / "humanoid.json").read_text())


CAT = catalog()


def piece(pid):
    return next(p for p in CAT["pieces"] if p["id"] == pid)


def hidden_for(target, body_type, equipped):
    """Regions to drop on `target` ('body' or a piece id), given everything equipped."""
    layer = 0.0 if target == "body" else piece(target)["layer"]
    out = set()
    for pid in equipped:
        p = piece(pid)
        if p["layer"] <= layer:
            continue
        out |= set(p["hides"][body_type].get(target, []))
    return out


def drop_regions(mesh, hidden):
    if not hidden or "_REGION" not in mesh.data.attributes:
        return
    names = CAT["regions"]
    idx = {names.index(r) for r in hidden}
    attr = mesh.data.attributes["_REGION"]
    vals = np.empty(len(attr.data), dtype=np.float32)
    attr.data.foreach_get("value", vals)
    me = mesh.data
    if attr.domain == "POINT":
        bad_v = set(np.nonzero(np.isin(np.rint(vals).astype(int), list(idx)))[0].tolist())
        bad = [p.index for p in me.polygons if all(v in bad_v for v in p.vertices)]
    else:
        bad = [p.index for p in me.polygons if int(round(vals[p.loop_start])) in idx]
    import bmesh
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[bm.faces[i] for i in bad], context="FACES")
    bm.to_mesh(me)
    bm.free()


def tint(mesh, kind, colour):
    for slot in mesh.material_slots:
        m = slot.material
        if m is None or not m.name.startswith(kind):
            continue
        nt = m.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        link = bsdf.inputs["Base Color"].links
        if not link:
            continue
        src = link[0].from_socket
        mul = nt.nodes.new("ShaderNodeMix")
        mul.data_type = "RGBA"
        mul.blend_type = "MULTIPLY"
        mul.inputs["Factor"].default_value = 1.0
        nt.links.new(src, mul.inputs["A"])
        mul.inputs["B"].default_value = (*[min(1.0, 2 * c) for c in colour], 1)
        # Twice the tone can exceed 1 for pale tones; a second multiply carries the remainder.
        over = [max(1.0, 2 * c) for c in colour]
        if max(over) > 1.0:
            mul2 = nt.nodes.new("ShaderNodeMix")
            mul2.data_type = "RGBA"
            mul2.blend_type = "MULTIPLY"
            mul2.inputs["Factor"].default_value = 1.0
            nt.links.new(mul.outputs["Result"], mul2.inputs["A"])
            mul2.inputs["B"].default_value = (*over, 1)
            nt.links.new(mul2.outputs["Result"], bsdf.inputs["Base Color"])
        else:
            nt.links.new(mul.outputs["Result"], bsdf.inputs["Base Color"])


def load(rel):
    return sc.import_gltf(sc.OUT / rel)


def assemble(look):
    """Returns (skeleton, meshes) for a look dict; see review_humanoid.LOOKS."""
    sc.reset()
    bt = look["body"]
    body_objs = load(CAT["body_types"][bt]["file"])
    skel = sc.armature_of(body_objs)
    meshes = {"body": sc.meshes_of(body_objs)}
    for pid in look["pieces"]:
        objs = load(piece(pid)["files"][bt])
        meshes[pid] = sc.meshes_of(objs)
        for ob in meshes[pid]:
            sc.bind(ob, skel)
        sc.remove([o for o in objs if o.type == "ARMATURE"])
    covers_hair = any(piece(p)["covers_hair"] for p in look["pieces"])
    face_hidden = "face" in hidden_for("body", bt, look["pieces"])
    for key in ("hair", "brows", "beard"):
        hid = look.get(key)
        if not hid or (key == "hair" and covers_hair) or (key != "hair" and face_hidden):
            continue
        h = next(x for x in CAT["hair"] if x["id"] == hid)
        objs = load(h["files"][bt])
        meshes[hid] = sc.meshes_of(objs)
        for ob in meshes[hid]:
            sc.bind(ob, skel)
        sc.remove([o for o in objs if o.type == "ARMATURE"])
    for target, obs in meshes.items():
        hidden = hidden_for(target, bt, look["pieces"]) if target == "body" or target in look["pieces"] else set()
        for ob in obs:
            drop_regions(ob, hidden)
    tone = dict((t["id"], t["linear"]) for t in CAT["skin_tones"])[look["tone"]]
    hair_col = dict((t["id"], t["linear"]) for t in CAT["hair_colours"])[look["hair_colour"]]
    for obs in meshes.values():
        for ob in obs:
            tint(ob, "skin", tone)
            tint(ob, "hair", hair_col)
    # Pose with a clip from the shared library.
    lib = load(CAT["animations"]["file"])
    sc.remove(lib)
    act = bpy.data.actions[look["clip"]]
    skel.animation_data_create()
    skel.animation_data.action = act
    if hasattr(skel.animation_data, "action_slot") and act.slots:
        skel.animation_data.action_slot = act.slots[0]
    bpy.context.scene.frame_set(look["frame"])
    return skel, [o for obs in meshes.values() for o in obs]


