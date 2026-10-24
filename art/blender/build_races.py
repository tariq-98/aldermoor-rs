"""Builds the races from the human bodies: elves, dwarves, foxfolk and goblins, on both frames.

    art/blender.sh -P art/blender/build_races.py [-- --review]

Needs build_humanoid.py's output. For each race and frame it deforms the body, eyes, every
outfit and every hairstyle by the race's bone-segment scales (aldermoor_art/races.py), adds race
features (pointed ears, a bigger nose, a dwarven beard; foxfolk's muzzle, ears, tail and fur,
aldermoor_art/fox.py), rebuilds the skeleton's joint positions
and exports everything under assets/humanoid/<race>_<frame>/. It records the new body types,
races and files in assets/catalog/humanoid.json. Run build_armour.py and build_back.py
afterwards; they fit plate and capes to every body type, races included.
"""

import json
import sys
from pathlib import Path

import bpy
from mathutils import Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import cloth, export, fox, races as rc, regions as rg, review, scene as sc  # noqa: E402

OUT = sc.OUT / "humanoid"
CAT_PATH = sc.OUT / "catalog" / "humanoid.json"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
# `--only=foxfolk` rebuilds just those races and keeps the others in the catalogue.
ONLY = next((a.split("=", 1)[1].split(",") for a in ARGS if a.startswith("--only=")), None)

FRAMES = {"male": "broad frame", "female": "slender frame"}
GOBLIN_TONES = [("goblin green", [0.10, 0.17, 0.05]), ("bog", [0.07, 0.11, 0.045]), ("ashen", [0.14, 0.15, 0.11]),
                ("moss", [0.13, 0.22, 0.07])]
# Fur: the coat, then the light fur (muzzle, throat, bib, inner ears, tail tip) and the dark
# (nose, ear backs, socks and gloves), all linear RGB. The body's markings mask places them.
FOX_TONES = [
    ("red fox", [0.40, 0.12, 0.028], [0.62, 0.55, 0.46], [0.022, 0.016, 0.012]),
    ("ember", [0.24, 0.05, 0.018], [0.55, 0.45, 0.36], [0.015, 0.01, 0.008]),
    ("sand", [0.48, 0.32, 0.15], [0.72, 0.64, 0.52], [0.2, 0.12, 0.06]),
    ("silver", [0.13, 0.13, 0.14], [0.62, 0.62, 0.63], [0.015, 0.015, 0.016]),
    ("arctic", [0.66, 0.66, 0.68], [0.78, 0.78, 0.8], [0.22, 0.22, 0.24]),
]
# The tail swings on springs, held up more than a cape hangs.
FOX_TAIL_SPRINGS = {"stiffness": 2.2, "drag": 0.3, "gravity": 1.6, "radius": 0.055}


def gl(v):
    return [round(v.x, 5), round(v.z, 5), round(-v.y, 5)]


def rel(p):
    return str(Path(p).relative_to(sc.OUT))


def dwarven_beard(beard):
    """Stretches a beard below the chin into a long, broad dwarven one."""
    me = beard.data
    mw = beard.matrix_world
    inv = mw.inverted()
    zs = [(mw @ v.co).z for v in me.vertices]
    lo, hi = min(zs), max(zs)
    chin = lo + 0.45 * (hi - lo)
    for v in me.vertices:
        p = mw @ v.co
        if p.z < chin:
            d = chin - p.z
            p = Vector((p.x * (1 + 0.6 * d / (chin - lo + 1e-6)), p.y - 0.25 * d, chin - d * 3.2))
            v.co = inv @ p
    me.update()


def build(race_id, race, frame, cat):
    bt = f"{race_id}_{frame}"
    bpy.ops.wm.open_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{frame}.blend"))
    skel = bpy.data.objects["skeleton"]
    body = bpy.data.objects["body"]
    eyes = bpy.data.objects["eyes"]
    outfits = [o for o in bpy.data.objects if o.type == "MESH" and o.get("piece")]
    # Hair comes back from the exported human files.
    hair = {}
    for h in cat["hair"]:
        if frame not in h["files"]:
            continue  # Race-only pieces (the dwarven beard) are derived below.
        objs = sc.import_gltf(sc.OUT / h["files"][frame])
        arm = sc.armature_of(objs)
        ms = sc.meshes_of(objs)
        for m in ms:
            sc.bind(m, skel)
        sc.remove([arm])
        hair[h["id"]] = ms
    meshes = [body, eyes] + outfits + [m for ms in hair.values() for m in ms]
    A, new_head = rc.transforms(skel, race)
    for m in meshes:
        rc.deform(m, A, 0.0)
    # Stand on the floor.
    dz = rc.ground_offset([body])
    for m in meshes:
        for v in m.data.vertices:
            v.co.z -= dz
        m.data.update()
    rc.rebuild_skeleton(skel, race, A, new_head, dz)
    head = skel.matrix_world @ skel.data.bones["Head"].head_local
    regions = rg.read(body)
    if race.get("ears"):
        rc.pointed_ears(body, regions, None, race["ears"], head)
    if race.get("nose"):
        rc.bigger_nose(body, regions, race["nose"], head)
    extra = {}
    body_parts = []
    if race.get("fox"):
        face = fox.Face(skel, body, eyes)
        fox.muzzle(face, [body] + outfits + [m for ms in hair.values() for m in ms])
        fox.fold_ears(body)
        parts = [fox.fox_ear(face, side) for side in (1, -1)]
        pts = fox.tail_points(skel, body)
        fox.add_tail_bones(skel, pts)
        parts.append(fox.tail_mesh(pts))
        fox.join_parts(body, parts)
        sc.bind(body, skel)
        _, marks = fox.paint(body, face, skel, OUT / "textures", f"fox_{frame}")
        body_parts = fox.details(face, skel, body)
        outfits += fox.dress(outfits, body, skel)
        fox.fox_eyes(eyes, OUT / "textures")
        extra["markings"] = f"humanoid/textures/{marks}"
        extra["springs"] = dict(FOX_TAIL_SPRINGS, chains=[fox.TAIL])
    extra_hair = []
    if race_id == "dwarf":
        src = hair["beard.full"][0]
        beard = src.copy()
        beard.data = src.data.copy()
        bpy.context.scene.collection.objects.link(beard)
        dwarven_beard(beard)
        beard.name = "beard.dwarven"
        hair["beard.dwarven"] = [beard]
        extra_hair.append("beard.dwarven")
    # Export: the body, every outfit piece and every hairstyle.
    files = {"body": rel(export.piece(OUT / bt / "body.gltf", [skel, body, eyes] + body_parts, sockets=True))}
    by_piece = {}
    for o in outfits:
        by_piece.setdefault(o["piece"], []).append(o)
    for pid, ms in by_piece.items():
        files[pid] = rel(export.piece(OUT / bt / "outfits" / f"{pid}.gltf", [skel] + ms))
    for hid, ms in hair.items():
        files[hid] = rel(export.piece(OUT / bt / "hair" / f"{hid}.gltf", [skel] + ms))
    # Hair stays out of the saved file: later builds (armour, capes) work from the body and clothes.
    sc.remove([m for ms in hair.values() for m in ms])
    bpy.ops.wm.save_as_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{bt}.blend"))
    base = cat["body_types"][frame]
    entry = {
        "label": f"{race['label']}, {FRAMES[frame]}",
        "race": race_id, "frame": frame,
        "file": files["body"],
        "pelvis": gl(skel.data.bones["pelvis"].head_local),
        "height": round(max((body.matrix_world @ v.co).z for v in body.data.vertices), 3),
        "skin_reference": base["skin_reference"],
        "sockets": base["sockets"],
        "colliders": cloth.colliders(skel, body, extra=[(b, nb, 0.012) for b, nb in zip(fox.TAIL, fox.TAIL[1:] + [None])]
                                     if race.get("fox") else ()),
        # Held and stowed items scale with the race (a goblin's shield is goblin-sized).
        "gear_scale": race.get("gear_scale", 1.0),
        **extra,
    }
    if "--review" in ARGS:
        review.render(sc.ROOT / "art" / "out" / "review" / "races" / f"{bt}-0.png", [body, eyes], yaw=20, size=(500, 800))
    return bt, entry, files, extra_hair


def main():
    cat = json.loads(CAT_PATH.read_text())
    # Start from the human-only catalogue each time (or drop just the races being rebuilt).
    for k in [k for k, v in cat["body_types"].items() if v.get("race") not in (None, "human")
              and (not ONLY or v.get("race") in ONLY)]:
        del cat["body_types"][k]
    for k, v in cat["body_types"].items():
        if k in FRAMES:
            v["race"], v["frame"] = "human", k
            v["label"] = f"Human, {FRAMES[k]}"
    cat["races"] = {"human": {"label": "Human", "frames": {f: f for f in FRAMES}},
                    **({k: v for k, v in cat.get("races", {}).items() if k not in ONLY} if ONLY else {})}
    tones = {t["id"] for t in cat["skin_tones"]}
    for tid, lin in GOBLIN_TONES:
        if tid not in tones:
            cat["skin_tones"].append({"id": tid, "linear": lin, "races": ["goblin"]})
    for tid, lin, light, dark in FOX_TONES:
        if tid not in tones:
            cat["skin_tones"].append({"id": tid, "linear": lin, "light": light, "dark": dark, "races": ["foxfolk"]})
    for race_id, race in rc.RACES.items():
        if ONLY and race_id not in ONLY:
            continue
        cat["races"][race_id] = {"label": race["label"], "frames": {}, "npc_only": bool(race.get("npc_only"))}
        for frame in FRAMES:
            bt, entry, files, extra = build(race_id, race, frame, cat)
            cat["body_types"][bt] = entry
            cat["races"][race_id]["frames"][frame] = bt
            for p in cat["pieces"]:
                if p.get("source"):
                    continue  # Armour is refitted per body type by build_armour.py.
                if p["id"] in files:
                    p["files"][bt] = files[p["id"]]
                    p["hides"][bt] = p["hides"][frame]
                    p.setdefault("coverage", {})[bt] = p.get("coverage", {}).get(frame, {})
            for h in cat["hair"]:
                if h["id"] in files:
                    h["files"][bt] = files[h["id"]]
            for hid in extra:
                h = next((x for x in cat["hair"] if x["id"] == hid), None)
                if h is None:
                    h = {"id": hid, "kind": "beard", "label": "Dwarven beard", "files": {}}
                    cat["hair"].append(h)
                h["files"][bt] = files[hid]
            d = dict(cat["defaults"][frame])
            if race_id == "dwarf":
                d["beard"] = "beard.dwarven" if frame == "male" else None
            if race_id == "goblin":
                d.update(tone="goblin green", hair="hair.buzzed", beard=None)
            if race_id == "elf":
                d.update(hair="hair.long", beard=None, tone="fair" if frame == "male" else "porcelain")
            if race_id == "foxfolk":
                d.update(hair="hair.buzzed" if frame == "male" else "hair.long", beard=None, brows=None, tone="red fox")
            cat["defaults"][bt] = d
            print("RACE", bt, entry["height"], "m tall")
    CAT_PATH.write_text(json.dumps(cat, indent=1))
    print("WROTE", CAT_PATH)


main()
