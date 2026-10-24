"""Builds skinned back items per body type: capes on spring-bone chains and backpacks.

    art/blender.sh -P art/blender/build_back.py [-- --review]

Needs art/out/humanoid-<body type>.blend from build_humanoid.py. Adds the items to
assets/catalog/gear.json.
"""

import json
import sys
from pathlib import Path

import bmesh
import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import cloth, export, garments as gm, metal, regions as rg, review, scene as sc, shapes as sh  # noqa: E402

OUT = sc.OUT / "gear"
REVIEW = sc.ROOT / "art" / "out" / "review" / "back"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
HUMANOID = json.loads((sc.OUT / "catalog" / "humanoid.json").read_text())

CAPES = [
    dict(id="cape.traveller", label="Traveller's cape", cloth="wool_red", trim="wool_red_trim", metal="brass", length=0.42),
    dict(id="cape.ranger", label="Ranger's cloak", cloth="wool_green", trim="leather", metal="iron", length=0.30),
]
TEX = OUT / "textures"


def cape_material(style):
    """Woven wool with a trim round the hem, sides and collar, and a dye mask: the same for every
    body type, since every cape shares one grid of UVs (u across, v from hem to collar)."""
    sid = style["id"].replace(".", "_")
    base = list(sh.PALETTE[style["cloth"]][0])
    trim = list(sh.PALETTE[style["trim"]][0])
    path = TEX / sid
    if not path.with_suffix(".jpg").exists():
        TEX.mkdir(parents=True, exist_ok=True)
        gm.cloth_texture(path, 1024, base, trim, [(0.0, 0.03), (0.975, 1.0)], weave=0.1, seed=7, ubands=[(0.0, 0.018), (0.982, 1.0)])
    img = bpy.data.images.load(str(path.with_suffix(".jpg")), check_existing=True)
    m = gm.cloth_material(f"cloth.{style['id']}", img)
    info = {"dye": str(Path(f"{path}_dye.png").relative_to(sc.OUT)),
            "dye_channels": [{"channel": "r", "name": "cloth", "reference": base},
                             {"channel": "g", "name": "trim", "reference": trim}]}
    return m, info
PACKS = [
    dict(id="backpack.travel", label="Travel pack", cloth="leather_tan", leather="leather", roll="wool_blue"),
]


def open_body(bt):
    bpy.ops.wm.open_mainfile(filepath=str(sc.ROOT / "art" / "out" / f"humanoid-{bt}.blend"))
    skel = bpy.data.objects["skeleton"]
    body = bpy.data.objects["body"]
    return skel, body


def envelope(body):
    """The body without its head, plus every shirt and pair of trousers it can wear."""
    head = {rg.INDEX[r] for r in ("scalp", "face", "ears")}
    trunk = body.copy()
    trunk.data = body.data.copy()
    bpy.context.scene.collection.objects.link(trunk)
    regions = rg.read(trunk)
    bm = bmesh.new()
    bm.from_mesh(trunk.data)
    bm.faces.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[bm.faces[i] for i, r in enumerate(regions) if r in head], context="FACES")
    bm.to_mesh(trunk.data)
    bm.free()
    clothes = [o for o in bpy.data.objects if o.type == "MESH" and o.get("slot") in ("torso_under", "legs_under")]
    env = cloth.Envelope([trunk] + clothes, trunk)
    return env, trunk


def clothes_of():
    return [o for o in bpy.data.objects if o.type == "MESH" and o.get("slot") in ("torso_under", "legs_under")]


def merge_catalog(entries):
    path = sc.OUT / "catalog" / "gear.json"
    cat = json.loads(path.read_text()) if path.exists() else {"version": 1, "items": []}
    ids = {e["id"] for e in entries}
    cat["items"] = [e for e in cat["items"] if e["id"] not in ids] + entries
    path.write_text(json.dumps(cat, indent=1))


def main():
    entries = {}
    for bt, info in HUMANOID["body_types"].items():
        if info.get("dress") is False:
            continue  # Golems and other creatures that wear nothing.
        for style in CAPES:
            skel, body = open_body(bt)
            env, trunk = envelope(body)
            parts, chains = cloth.cape(skel, env, style)
            mat, dye = cape_material(style)
            parts[0].data.materials.clear()
            parts[0].data.materials.append(mat)
            metal.work(parts, [trunk] + clothes_of(), keep_uvs=[parts[0]])
            path = export.piece(OUT / bt / f"{style['id']}.gltf", [skel] + parts)
            e = entries.setdefault(style["id"], {
                "id": style["id"], "label": style["label"], "family": "cape", "slot": "back", "back_kind": "cape",
                "layer": 2.8, "files": {}, "hides": {},
                "springs": {"chains": chains, "stiffness": 0.35, "drag": 0.25, "gravity": 9.81, "radius": 0.035},
                **dye,
            })
            e["files"][bt] = str(path.relative_to(sc.OUT))
            e["hides"][bt] = {"body": []}
            if "--review" in ARGS:
                review_item(bt, style["id"], skel, parts)
        for style in PACKS:
            skel, body = open_body(bt)
            env, trunk = envelope(body)
            parts = cloth.backpack(skel, env, style)
            metal.work(parts, [trunk] + clothes_of())
            path = export.piece(OUT / bt / f"{style['id']}.gltf", [skel] + parts)
            e = entries.setdefault(style["id"], {
                "id": style["id"], "label": style["label"], "family": "backpack", "slot": "back", "back_kind": "backpack",
                "layer": 2.9, "files": {}, "hides": {},
            })
            e["files"][bt] = str(path.relative_to(sc.OUT))
            e["hides"][bt] = {"body": []}
            if "--review" in ARGS:
                review_item(bt, style["id"], skel, parts)
    merge_catalog(list(entries.values()))
    print("WROTE", ", ".join(entries))


def review_item(bt, iid, skel, parts):
    """Rest-pose fit on the body with a shirt and trousers, and a few library poses."""
    REVIEW.mkdir(parents=True, exist_ok=True)
    keep = set(parts) | {bpy.data.objects["body"], bpy.data.objects["eyes"]}
    for o in bpy.data.objects:
        if o.type == "MESH":
            show = o in keep or o.get("piece") in ("ranger.top", "ranger.trousers", "ranger.boots")
            o.hide_render = not show
    from aldermoor_art import looks
    shown = [o for o in bpy.data.objects if o.type == "MESH" and not o.hide_render]
    # Hide covered body regions the way the game does.
    hidden = looks.hidden_for("body", bt, ["ranger.top", "ranger.trousers", "ranger.boots"])
    looks.drop_regions(bpy.data.objects["body"], hidden)
    files = []
    for clip, frame in (("rest", 0), ("Idle_Loop", 20), ("Walk_Loop", 10), ("Sword_Regular_A", 7)):
        if clip != "rest":
            if clip not in bpy.data.actions:
                lib = sc.import_gltf(sc.OUT / HUMANOID["animations"]["file"])
                sc.remove(lib)
            act = bpy.data.actions[clip]
            skel.animation_data_create()
            skel.animation_data.action = act
            if hasattr(skel.animation_data, "action_slot") and act.slots:
                skel.animation_data.action_slot = act.slots[0]
            bpy.context.scene.frame_set(frame)
        chains = [[b.name for b in skel.data.bones if b.name.startswith(f"cape_{c}_")] for c in range(8)]
        chains = [sorted(c, key=lambda n: int(n.rsplit("_", 1)[1])) for c in chains if c]
        if chains:
            for c in chains:
                for n in c:
                    skel.pose.bones[n].matrix_basis.identity()
            cloth.settle(skel, chains, HUMANOID["body_types"][bt]["colliders"])
        for yaw in (30, 150):
            f = REVIEW / f"{bt}-{iid}-{clip}-{yaw}.png"
            review.render(f, shown, yaw=yaw, size=(520, 700))
            files.append(f)
    review.montage(REVIEW / f"{bt}-{iid}.png", files, 4)


main()
