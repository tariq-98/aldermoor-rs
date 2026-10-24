"""Builds weapons, shields, bows, quivers and casters, and writes assets/catalog/gear.json.

    art/blender.sh -P art/blender/build_gear.py [-- --review]

Each item is a rigid glTF authored around its attachment point (docs/ASSETS.md, "Sockets").
`--review` renders product shots and the gear held and stowed on a character.
"""

import json
import math
import sys
from pathlib import Path

import bpy
from mathutils import Matrix

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import export, review, scene as sc, shapes as sh, weapons as wp  # noqa: E402

OUT = sc.OUT / "gear"
REVIEW = sc.ROOT / "art" / "out" / "review" / "gear"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []

# Rigid items. Clips name entries in the animation library; archery clips come later.
ITEMS = [
    dict(id="sword.arming", label="Arming sword", family="sword", slot="main_hand", build=wp.arming_sword,
         sheath=wp.arming_sword_sheath, held="socket_hand_r", stowed={"default": "socket_hip_l"},
         clips={"idle": "Sword_Idle", "attacks": ["Sword_Regular_A", "Sword_Regular_B", "Sword_Regular_C", "Sword_Regular_Combo", "Sword_Heavy_Combo"],
                "block": "Sword_Block", "dash": "Sword_Dash"}),
    dict(id="dagger.hunting", label="Hunting knife", family="dagger", slot="off_hand", build=wp.dagger,
         sheath=wp.dagger_sheath, held="socket_hand_l", stowed={"default": "socket_hip_r"},
         clips={"idle": "Sword_Idle", "attacks": ["Sword_Regular_A", "Melee_Hook"]}),
    dict(id="shield.round", label="Round shield", family="shield", slot="off_hand", build=wp.round_shield,
         held="socket_shield_l", stowed={"default": "socket_back_2", "backpack": "socket_pack", "cape": "socket_back_2"},
         clips={"idle": "Idle_Shield_Loop", "block": "Idle_Shield_Loop", "bash": "Shield_OneShot", "dash": "Shield_Dash"}),
    dict(id="bow.hunting", label="Hunting bow", family="bow", slot="two_hand", build=wp.hunting_bow,
         held="socket_hand_l", stowed={"default": "socket_back_1", "backpack": "socket_pack_side"}, clips={"idle": "Idle_Loop"}),
    # Ammunition has no drawn state: only where it rides, which depends on the back item.
    dict(id="quiver.leather", label="Leather quiver", family="quiver", slot="ammo", build=wp.quiver,
         stowed={"default": "socket_quiver", "backpack": "socket_quiver_hip"}, clips={}),
    dict(id="staff.oak", label="Oak staff", family="staff", slot="two_hand", build=wp.battle_staff,
         held="socket_hand_r", stowed={"default": "socket_back_1", "backpack": "socket_pack_side"},
         clips={"idle": "Spell_Simple_Idle_Loop", "attacks": ["Spell_Simple_Shoot"], "enter": "Spell_Simple_Enter", "exit": "Spell_Simple_Exit"}),
    dict(id="wand.yew", label="Yew wand", family="wand", slot="main_hand", build=wp.wand,
         held="socket_hand_r", stowed={"default": "socket_hip_r"},
         clips={"idle": "Spell_Simple_Idle_Loop", "attacks": ["Spell_Simple_Shoot"]}),
]


def build_item(item):
    sc.reset()
    parts = item["build"]()
    ob = sh.join(parts, item["id"])
    sh.uv_unwrap(ob)
    path = export.rigid(OUT / f"{item['id']}.gltf", [ob])
    entry = {k: v for k, v in item.items() if k not in ("build", "sheath")}
    entry["file"] = str(path.relative_to(sc.OUT))
    entry["triangles"] = sh.triangles([ob])
    if item.get("sheath"):
        sc.reset()
        sob = sh.join(item["sheath"](), f"{item['id']}.sheath")
        sh.uv_unwrap(sob)
        spath = export.rigid(OUT / f"{item['id']}.sheath.gltf", [sob])
        entry["sheath"] = str(spath.relative_to(sc.OUT))
    return entry


# --- Review ---------------------------------------------------------------------------------

C = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))  # Blender to glTF axes.


def mount(objs, skel, socket):
    """Places imported item objects on a socket, exactly as the runtime does."""
    pb = skel.pose.bones[socket]
    m = skel.matrix_world @ pb.matrix @ C
    for o in objs:
        if o.parent is None:
            o.matrix_world = m @ o.matrix_world


def import_item(rel):
    objs = sc.import_gltf(sc.OUT / rel)
    return [o for o in objs if o.type == "MESH"]


def product_shots(entries):
    files = []
    for e in entries:
        sc.reset()
        objs = import_item(e["file"])
        if e.get("sheath"):
            for o in import_item(e["sheath"]):
                o.location.x += 0.12
                objs.append(o)
        bpy.context.view_layer.update()
        for yaw in (90, 0):
            f = REVIEW / f"item-{e['id']}-{yaw}.png"
            review.render(f, objs, yaw=yaw, size=(420, 640), pad=1.1)
            files.append(f)
    review.montage(REVIEW / "items.png", files, 6)


SCENES = [
    # Look, clip, frame, items in hand, items stowed.
    ("sword-idle", "Sword_Idle", 12, {"sword.arming": "held", "shield.round": "held"}),
    ("sword-swing", "Sword_Regular_A", 7, {"sword.arming": "held", "shield.round": "held"}),
    ("sword-block", "Sword_Block", 12, {"sword.arming": "held", "shield.round": "held"}),
    ("stowed", "Walk_Loop", 10, {"sword.arming": "stowed", "shield.round": "stowed", "quiver.leather": "held", "dagger.hunting": "stowed"}),
    ("bow", "Idle_Loop", 20, {"bow.hunting": "held", "quiver.leather": "held", "dagger.hunting": "stowed"}),
    ("staff-cast", "Spell_Simple_Shoot", 6, {"staff.oak": "held"}),
    ("wand-idle", "Spell_Simple_Idle_Loop", 20, {"wand.yew": "held", "dagger.hunting": "held"}),
    ("bow-stowed", "Jog_Fwd_Loop", 6, {"bow.hunting": "stowed", "quiver.leather": "held", "sword.arming": "stowed"}),
]


def on_character(entries):
    from aldermoor_art import looks
    by_id = {e["id"]: e for e in entries}
    files = []
    for name, clip, frame, gear in SCENES:
        look = dict(body="male", pieces=["ranger.top", "ranger.trousers", "ranger.boots"], hair="hair.simple_parted",
                    brows="brows.strong", beard="beard.full", tone="warm", hair_colour="chestnut", clip=clip, frame=frame)
        skel, objs = looks.assemble(look)
        for iid, state in gear.items():
            e = by_id[iid]
            socket = e.get("held", e["stowed"]["default"]) if state == "held" else e["stowed"]["default"]
            item = import_item(e["file"])
            mount(item, skel, socket)
            objs += item
            if e.get("sheath"):
                sheath = import_item(e["sheath"])
                mount(sheath, skel, e["stowed"]["default"])
                objs += sheath
        bpy.context.view_layer.update()
        for yaw in (25, 160):
            f = REVIEW / f"wear-{name}-{yaw}.png"
            review.render(f, objs, yaw=yaw, size=(560, 760), pad=1.05)
            files.append(f)
    review.montage(REVIEW / "worn.png", files, 4)


def main():
    entries = [build_item(it) for it in ITEMS]
    out = sc.OUT / "catalog" / "gear.json"
    # Keep items other scripts own (build_back.py's capes and packs).
    old = json.loads(out.read_text())["items"] if out.exists() else []
    ids = {e["id"] for e in entries}
    cat = {"version": 1, "items": entries + [e for e in old if e["id"] not in ids]}
    out.write_text(json.dumps(cat, indent=1))
    print("WROTE", out, sum(e["triangles"] for e in entries), "triangles")
    if "--review" in ARGS:
        REVIEW.mkdir(parents=True, exist_ok=True)
        product_shots(entries)
        on_character(entries)
        print("REVIEW", REVIEW)


main()
