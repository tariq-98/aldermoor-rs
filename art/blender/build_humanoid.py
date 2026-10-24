"""Builds the humanoid family: skeletons, bodies, hair, outfits, the clip library and its catalogue.

    art/blender.sh -P art/blender/build_humanoid.py [-- --review]

Reads art/vendor/quaternius (see art/vendor.json) and writes assets/humanoid/ plus
assets/catalog/humanoid.json. `--review` also renders check images to art/out/review/.
"""

import json
import math
import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import cloth, export, fit, materials as mt, regions as rg, rig, scene as sc  # noqa: E402

V = sc.VENDOR / "quaternius"
OUT = sc.OUT / "humanoid"
TEX = OUT / "textures"
REVIEW = sc.ROOT / "art" / "out" / "review"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []

BODY_TYPES = {
    # Labels stay neutral: every outfit and hairstyle is offered on both frames.
    "male": {"label": "Broad frame", "source": "Male"},
    "female": {"label": "Slender frame", "source": "Female"},
}

HAIR = [
    # id, file, kind, label, default colour source
    ("hair.simple_parted", "Hair_SimpleParted", "hair", "Short, parted"),
    ("hair.buzzed", "Hair_Buzzed", "hair", "Buzzed"),
    ("hair.buzzed_soft", "Hair_BuzzedFemale", "hair", "Buzzed, soft line"),
    ("hair.buns", "Hair_Buns", "hair", "Twin buns"),
    ("hair.long", "Hair_Long", "hair", "Long"),
    ("beard.full", "Hair_Beard", "beard", "Full beard"),
    ("brows.strong", "Eyebrows_Regular", "brows", "Strong brows"),
    ("brows.fine", "Eyebrows_Female", "brows", "Fine brows"),
]

# Quaternius designed outfit parts to replace whole body sections ("only the head is required"),
# so they declare what they hide. The ray test still runs as a cross-check.
PART_HIDES = {
    "Body": ["chest", "belly"],
    "Arms": ["upperarm_hi", "upperarm_lo", "forearm_hi", "forearm_lo", "hand"],
    "Legs": ["hips", "thigh_hi", "thigh_lo", "shin_hi"],
    "Feet": ["shin_lo", "foot"],
    "Feet_Boots": ["shin_hi", "shin_lo", "foot"],
    "Head_Hood": ["scalp", "ears"],
    "Acc_Pauldron": [],
    "Acc_Pauldrons": [],
}

# Quaternius outfit parts mapped to our slots. Layers order what covers what: a piece hides
# regions on pieces with a lower layer.
OUTFITS = {
    "peasant": {
        "label": "Peasant",
        "pieces": [
            ("top", "torso_under", 1.1, ["Body", "Arms"], "Linen shirt"),
            ("trousers", "legs_under", 1.0, ["Legs"], "Work trousers"),
            ("shoes", "feet", 1.2, ["Feet"], "Wrapped shoes"),
        ],
        "variants": {"T_Peasant_BaseColor": ["T_Peasant_2_BaseColor"]},
        "variant_labels": {"T_Peasant_BaseColor": "Undyed linen", "T_Peasant_2_BaseColor": "Madder and walnut"},
    },
    "ranger": {
        "label": "Ranger",
        "pieces": [
            ("top", "torso_under", 1.1, ["Body", "Arms"], "Ranger jerkin"),
            ("trousers", "legs_under", 1.0, ["Legs"], "Ranger leggings"),
            ("boots", "feet", 1.2, ["Feet_Boots", "Feet"], "Ranger boots"),
            ("hood", "head", 2.5, ["Head_Hood"], "Ranger hood"),
            ("pauldron", "shoulders", 3.0, ["Acc_Pauldron", "Acc_Pauldrons"], "Ranger pauldron"),
        ],
        "variants": {"T_Ranger_BaseColor": ["T_Ranger_3_BaseColor"]},
        "variant_labels": {"T_Ranger_BaseColor": "Forest green", "T_Ranger_3_BaseColor": "Autumn brown"},
    },
}

SKIN_TONES = [
    # Linear RGB albedo. The renderer multiplies a skin's detail map by twice the tone. The
    # Quaternius paintings average (0.38, 0.19, 0.10), close to "warm".
    ("porcelain", [0.56, 0.34, 0.23]),
    ("fair", [0.48, 0.27, 0.17]),
    ("warm", [0.40, 0.21, 0.12]),
    ("olive", [0.32, 0.18, 0.09]),
    ("tan", [0.25, 0.13, 0.065]),
    ("brown", [0.16, 0.08, 0.042]),
    ("deep", [0.09, 0.045, 0.025]),
    ("ebony", [0.05, 0.026, 0.016]),
]
HAIR_COLOURS = [
    ("black", [0.012, 0.010, 0.009]),
    ("dark brown", [0.045, 0.025, 0.014]),
    ("chestnut", [0.13, 0.055, 0.025]),
    ("auburn", [0.25, 0.07, 0.03]),
    ("copper", [0.45, 0.16, 0.05]),
    ("honey", [0.48, 0.30, 0.12]),
    ("blonde", [0.70, 0.52, 0.28]),
    ("ash", [0.40, 0.38, 0.34]),
    ("white", [0.80, 0.80, 0.78]),
    ("moss", [0.10, 0.20, 0.07]),
    ("midnight", [0.03, 0.05, 0.16]),
]


def gl(v):
    """Blender Z-up to glTF Y-up."""
    return [round(v.x, 5), round(v.z, 5), round(-v.y, 5)]


def rel(path):
    return str(Path(path).relative_to(sc.OUT))


def reference_frames():
    objs = sc.import_gltf(V / "anims/UAL1_Standard.glb")
    arm = sc.armature_of(objs)
    frames = rig.rest_frames(arm)
    pelvis = arm.data.bones["pelvis"].head_local.copy()
    sc.remove(objs)
    for a in list(bpy.data.actions):
        bpy.data.actions.remove(a)
    return frames, pelvis


# --- The clip library -------------------------------------------------------------------------

def clip_tags(name):
    n = name.lower()
    tags = []
    for key, words in {
        "locomotion": ("walk", "jog", "sprint", "crouch", "swim", "jump", "roll", "slide", "climb", "ninja"),
        "combat": ("sword", "punch", "melee", "shield", "hit", "death", "knockback", "spell", "throw", "pistol"),
        "work": ("farm", "chopping", "fixing", "chest", "pickup", "push", "interact", "consume", "carry"),
        "social": ("talking", "wave", "yes", "idle_no", "dance", "fold", "sitting", "lay", "rail"),
        "undead": ("zombie",),
    }.items():
        if any(w in n for w in words):
            tags.append(key)
    return tags or ["idle"]


def fcurves_of(action):
    if getattr(action, "layers", None) and action.layers:
        bag = action.layers[0].strips[0].channelbags
        return bag[0].fcurves if len(bag) else []
    return action.fcurves


def build_animations(frames, pelvis):
    sc.reset()
    first = sc.import_gltf(V / "anims/UAL1_Standard.glb")
    arm = sc.armature_of(first)
    sc.remove(sc.meshes_of(first))
    second = sc.import_gltf(V / "anims/UAL2_Standard.glb")
    sc.remove([o for o in second])
    arm.name = "humanoid"
    arm.data.name = "humanoid"
    clips = []
    for action in sorted(bpy.data.actions, key=lambda a: a.name):
        if action.name.startswith("A_TPose"):
            bpy.data.actions.remove(action)
            continue
        fcs = fcurves_of(action)
        # Only rotations travel between skeletons; the pelvis also keeps its translation.
        for fc in [fc for fc in fcs if fc.data_path.endswith(".scale")
                   or (fc.data_path.endswith(".location") and '"pelvis"' not in fc.data_path)]:
            fcs.remove(fc)
        start, end = action.frame_range
        action.use_fake_user = True
        tags = clip_tags(action.name)
        clips.append({
            "name": action.name,
            "seconds": round((end - start) / sc.FPS, 3),
            "loop": "loop" in action.name.lower(),
            "group": tags[0],
            "tags": tags,
        })
    export.animations(OUT / "anims.glb", arm)
    return {"file": rel(OUT / "anims.glb"), "pelvis": gl(pelvis), "fps": sc.FPS, "clips": clips}


# --- Bodies, hair and outfits -----------------------------------------------------------------

def skin_material(mesh, tone_src, name):
    """Turns a mesh's skin material into the shared, tintable `skin` material."""
    for slot in mesh.material_slots:
        m = slot.material
        if m is None or not (m.name.startswith("MI_Superhero") or m.name.startswith("MI_Regular")):
            continue
        mt.repair_missing(m, V / "base-characters", V / "base-characters/Textures", V / "outfits/Textures/Base")
        node = mt.base_color_node(m)
        src = mt.find_image(tone_src, V / "base-characters/Textures", V / "outfits/Textures/Base")
        detail, mean = mt.tint_detail(src, TEX, f"{name}_SkinDetail")
        node.image = detail
        for n in mt.images_of(m):
            if n.name != node.name:
                n.image = mt.publish(n.image, TEX, 2048, "data")
        m.name = f"skin.{name}"
        m["tint"] = "skin"
        return mean
    return None


def publish_all(mesh, max_size=2048):
    for slot in mesh.material_slots:
        if slot.material and not slot.material.name.startswith("skin."):
            mt.repair_missing(slot.material, V / "base-characters", V / "base-characters/Textures",
                              V / "outfits/Textures/Base", V / "outfits/Textures/Peasant",
                              V / "outfits/Textures/Ranger", V / "hair")
            mt.publish_material(slot.material, TEX, max_size)


def body_tree(body, body_regions, exclude=()):
    excluded = {rg.INDEX[r] for r in exclude}
    return fit.tree([body], keep=lambda ob, i: body_regions[i] not in excluded)


def build_body_type(key, info, frames):
    sc.reset()
    Sex = info["source"]
    body_objs = sc.import_gltf(V / f"base-characters/Superhero_{Sex}_FullBody.gltf")
    body_arm = sc.armature_of(body_objs)
    # The outfits were modelled on their own skeleton; we adopt its joints and move the body.
    probe = sc.import_gltf(V / f"outfits/{Sex}_Peasant_Body.gltf")
    skel = sc.armature_of(probe)
    sc.remove(sc.meshes_of(probe))
    skel.name = "skeleton"
    skel.data.name = "skeleton"
    body_meshes = sc.meshes_of(body_objs)
    rig.refit(body_meshes, body_arm, skel)
    sc.remove([body_arm])
    rig.normalise_frames(skel, frames)
    body = next(m for m in body_meshes if m.name.lower().startswith("superhero"))
    eyes = next(m for m in body_meshes if m.name.startswith("Eyes"))
    brows = [m for m in body_meshes if m.name.startswith("Eyebrows")]
    sc.remove(brows)  # Eyebrows are chosen separately.
    body.name, eyes.name = "body", "eyes"
    for me in (body, eyes):
        mt.simplify(me)
    rig.add_sockets(skel, rig.socket_frames(skel, body))
    body_regions = rg.tag(body, skel, body=True)
    light = {"Male": "T_Superhero_Male_Ligh", "Female": "T_Superhero_Female_Light_BaseColor"}[Sex]
    skin_ref = skin_material(body, light, f"{key}_body")
    publish_all(eyes, 256)
    export.piece(OUT / key / "body.gltf", [skel, body, eyes])
    entry = {
        "label": info["label"],
        "file": rel(OUT / key / "body.gltf"),
        "pelvis": gl(skel.data.bones["pelvis"].head_local),
        "height": round(max(v.co.z for v in body.data.vertices), 3),
        "skin_reference": skin_ref,
        "sockets": sorted(b.name for b in skel.data.bones if b.name.startswith("socket_")),
        "colliders": cloth.colliders(skel, body),
    }

    # Hair, beards and brows: rigid on the Head bone, moved onto this head and pushed clear of it.
    head_regions = ("scalp", "face", "ears", "neck")
    head_tree = body_tree(body, body_regions, exclude=[r for r in rg.REGIONS if r not in head_regions])
    hair_entries = []
    for hid, file, kind, label in HAIR:
        objs = sc.import_gltf(V / f"hair/{file}.gltf")
        harm = sc.armature_of(objs)
        meshes = sc.meshes_of(objs)
        rig.refit(meshes, harm, skel)
        sc.remove([harm])
        moved = 0
        for me in meshes:
            me.name = hid
            mt.simplify(me)
            n, worst = fit.push_out(me, head_tree, margin=0.0015 if kind != "hair" else 0.003)
            moved += n
            for slot in me.material_slots:
                m = slot.material
                mt.repair_missing(m, V / "hair", V / "base-characters/Textures")
                node = mt.base_color_node(m)
                if node:
                    detail, mean = mt.tint_detail(node.image, TEX, f"{Path(node.image.filepath).stem}_Detail")
                    node.image = detail
                    m["tint"] = "hair"
                    m["reference"] = mean
                for nimg in mt.images_of(m):
                    if nimg.name != node.name:
                        nimg.image = mt.publish(nimg.image, TEX, 1024, "data")
                m.name = f"hair.{Path(file).stem}"
        path = OUT / key / "hair" / f"{hid}.gltf"
        export.piece(path, [skel] + meshes)
        hair_entries.append((hid, kind, label, rel(path), moved))
        sc.remove(meshes)

    # Outfits.
    piece_entries = []
    loaded = []  # (piece id, layer, meshes)
    for oid, outfit in OUTFITS.items():
        for pid, slot, layer, parts, label in outfit["pieces"]:
            meshes = []
            declared = set()
            for part in parts:
                path = V / f"outfits/{Sex}_{outfit['label']}_{part}.gltf"
                if not path.exists():
                    continue
                # Some boots are filed as plain "Feet"; the piece decides.
                declared |= set(PART_HIDES["Feet_Boots" if pid == "boots" else part])
                objs = sc.import_gltf(path)
                parm = sc.armature_of(objs)
                pm = sc.meshes_of(objs)
                rig.refit(pm, parm, skel)
                sc.remove([parm])
                meshes += pm
            if not meshes:
                continue
            full = f"{oid}.{pid}"
            for me in meshes:
                me["piece"], me["slot"] = full, slot
                mt.simplify(me)
                rg.tag(me, skel)
                skin_material(me, f"T_Regular_{Sex}_Dark_BaseColor" if Sex == "Male" else "T_Regular_Female_Dark_BaseColor", f"{key}_{oid}_{pid}")
                publish_all(me)
            loaded.append((full, slot, layer, label, meshes, sorted(declared, key=rg.INDEX.get)))

    # What each piece covers on the body. Cloth then has to clear the parts it leaves visible.
    for full, slot, layer, label, meshes, declared in loaded:
        cov = fit.coverage(body, body_regions, meshes, reach_in=0.04)
        hides = {"body": sorted(set(fit.hides(cov)) | set(declared), key=rg.INDEX.get)}
        for r in declared:
            if cov.get(r, 0.0) < 0.5:
                print(f"CHECK {key} {full}: declares {r} but covers {cov.get(r, 0.0):.0%} of it")
        visible_tree = body_tree(body, body_regions, exclude=hides["body"])
        pushed = 0
        for me in meshes:
            n, _ = fit.push_out(me, visible_tree, margin=0.003)
            pushed += n
        piece_entries.append({"id": full, "slot": slot, "layer": layer, "label": label,
                              "meshes": meshes, "hides": hides, "coverage": cov, "pushed": pushed})

    # Pairwise: a piece hides a region on a lower piece only where it really covers that piece.
    for p in piece_entries:
        for q in piece_entries:
            if q["layer"] >= p["layer"]:
                continue
            q_regions = {}
            for qm in q["meshes"]:
                q_regions[qm.name] = rg.read(qm)
            pair = []
            for qm in q["meshes"]:
                cov = fit.coverage(qm, q_regions[qm.name], p["meshes"], reach_out=0.06, reach_in=0.008)
                pair += fit.hides(cov, 0.95)
            pair = sorted(set(pair), key=rg.INDEX.get)
            if pair:
                p["hides"][q["id"]] = pair

    pieces_out = []
    for p in piece_entries:
        path = OUT / key / "outfits" / f"{p['id']}.gltf"
        export.piece(path, [skel] + p["meshes"])
        pieces_out.append({
            "id": p["id"], "slot": p["slot"], "layer": p["layer"], "label": p["label"],
            "file": rel(path), "hides": p["hides"],
            "covers_hair": "scalp" in p["hides"]["body"] or p["slot"] == "head",
            "coverage": {k: round(v, 3) for k, v in p["coverage"].items() if v > 0.05},
            "pushed_vertices": p["pushed"],
        })

    if "--review" in ARGS:
        review_body_type(key, skel, body, body_regions)
    bpy.ops.wm.save_as_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{key}.blend"))
    return entry, hair_entries, pieces_out


def review_body_type(key, skel, body, body_regions):
    from aldermoor_art import review
    REVIEW.mkdir(parents=True, exist_ok=True)
    for yaw in (0, 180):
        review.render(REVIEW / f"{key}-body-{yaw}.png", [body], yaw=yaw, size=(700, 900))


def main():
    frames, pelvis = reference_frames()
    catalog = {
        "version": 1,
        "regions": rg.REGIONS,
        "skin_tones": [{"id": n, "linear": c} for n, c in SKIN_TONES],
        "hair_colours": [{"id": n, "linear": c} for n, c in HAIR_COLOURS],
        "body_types": {},
        "hair": [],
        "pieces": [],
    }
    catalog["animations"] = build_animations(frames, pelvis)
    hair_index, piece_index = {}, {}
    for key, info in BODY_TYPES.items():
        entry, hair, pieces = build_body_type(key, info, frames)
        catalog["body_types"][key] = entry
        for hid, kind, label, path, moved in hair:
            h = hair_index.setdefault(hid, {"id": hid, "kind": kind, "label": label, "files": {}})
            h["files"][key] = path
        for p in pieces:
            q = piece_index.setdefault(p["id"], {k: v for k, v in p.items() if k not in ("file", "hides", "coverage", "pushed_vertices")} | {"files": {}, "hides": {}, "coverage": {}})
            q["files"][key] = p["file"]
            q["hides"][key] = p["hides"]
            q["coverage"][key] = p["coverage"]
    catalog["hair"] = list(hair_index.values())
    catalog["pieces"] = list(piece_index.values())
    # Keep pieces other builds own (build_armour.py marks its own with a source).
    out_path = sc.OUT / "catalog" / "humanoid.json"
    if out_path.exists():
        catalog["pieces"] += [p for p in json.loads(out_path.read_text())["pieces"] if p.get("source")]
    catalog["defaults"] = {
        "male": {"pieces": ["peasant.top", "peasant.trousers", "peasant.shoes"], "hair": "hair.simple_parted",
                 "brows": "brows.strong", "beard": "beard.full", "tone": "warm", "hair_colour": "chestnut"},
        "female": {"pieces": ["peasant.top", "peasant.trousers", "peasant.shoes"], "hair": "hair.buns",
                   "brows": "brows.fine", "beard": None, "tone": "olive", "hair_colour": "dark brown"},
    }
    for oid, outfit in OUTFITS.items():
        catalog.setdefault("texture_variant_labels", {}).update(outfit.get("variant_labels", {}))
        for base, alts in outfit["variants"].items():
            catalog.setdefault("texture_variants", {})[base] = alts
            for alt in alts:
                src = mt.find_image(alt, V / f"outfits/Textures/{outfit['label']}")
                mt.publish(src, TEX, 2048, "color")
    out = sc.OUT / "catalog" / "humanoid.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(catalog, indent=1))
    print("WROTE", out)


main()
