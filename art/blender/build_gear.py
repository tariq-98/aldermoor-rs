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
from aldermoor_art import export, review, scene as sc, shapes as sh, surface, weapons as wp  # noqa: E402
from aldermoor_art.holds import BOW_DRAW, OFF_HAND  # noqa: E402

OUT = sc.OUT / "gear"
REVIEW = sc.ROOT / "art" / "out" / "review" / "gear"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []

# Rigid items. `clips` name entries in the animation library (build_clips.py): `idle` while drawn,
# `attacks` in turn, `ready` and `stow` to take the item out and put it away. Two-handed items
# say where the off hand holds them (`grips`), and back items how far along them a hand may take
# them when drawing (`grab_range`, metres along the item's up axis). docs/ASSETS.md has the rest.
SWORD = {"idle": "Sword_Idle", "attacks": ["Sword_Regular_A", "Sword_Regular_B", "Sword_Regular_C", "Sword_Regular_Combo",
                                           "Sword_Heavy_Combo"], "block": "Sword_Block", "dash": "Sword_Dash",
         "ready": "Draw_Hip", "stow": "Sheathe_Hip"}
STAFF = {"idle": "Staff_Idle_Loop", "attacks": ["Staff_Cast"], "channel": "Staff_Channel_Loop", "slam": "Staff_Slam",
         "ready": "Draw_Pole", "stow": "Sheathe_Pole"}
BOW = {"idle": "Bow_Idle_Loop", "attacks": ["Bow_Shoot"], "draw": "Bow_Draw", "aim": "Bow_Aim_Loop", "release": "Bow_Release",
       "ready": "Draw_Bow", "stow": "Sheathe_Bow"}
POLE_STOWED = {"default": "socket_back_pole", "backpack": "socket_pack_side_r"}
BOW_STOWED = {"default": "socket_back_bow", "backpack": "socket_pack_side"}
SHIELD_STOWED = {"default": "socket_back_shield", "backpack": "socket_pack", "cape": "socket_back_shield"}


def off_hand(family):
    """The off hand's socket frame on an item, in the item's glTF space (up +Y, front +Z)."""
    s, t, (lo, hi) = OFF_HAND[family]
    return {"at": [0.0, s, 0.0], "up": [0.0, 1.0, 0.0], "front": [round(math.sin(t), 5), 0.0, round(math.cos(t), 5)],
            "range": [lo, hi]}


ITEMS = [
    dict(id="sword.arming", label="Arming sword", family="sword", slot="main_hand", build=wp.arming_sword,
         sheath=wp.arming_sword_sheath, held="socket_hand_r", stowed={"default": "socket_hip_l"}, clips=SWORD),
    dict(id="dagger.hunting", label="Hunting knife", family="dagger", slot="off_hand", build=wp.dagger,
         sheath=wp.dagger_sheath, held="socket_hand_l", stowed={"default": "socket_hip_r"},
         clips={"idle": "Sword_Idle", "attacks": ["Sword_Regular_A", "Melee_Hook"], "ready": "Draw_Hip_L", "stow": "Sheathe_Hip_L"}),
    dict(id="shield.round", label="Round shield", family="shield", slot="off_hand", build=wp.round_shield,
         held="socket_shield_l", stowed=SHIELD_STOWED,
         clips={"idle": "Idle_Shield_Loop", "block": "Idle_Shield_Loop", "bash": "Shield_OneShot", "dash": "Shield_Dash"}),
    dict(id="bow.hunting", label="Hunting bow", family="bow", slot="two_hand", build=wp.hunting_bow, post="bow",
         held="socket_hand_l", stowed=BOW_STOWED, clips=BOW, grab_range=[-0.7, 0.7]),
    # Ammunition has no drawn state: only where it rides, which depends on the back item.
    dict(id="quiver.leather", label="Leather quiver", family="quiver", slot="ammo", build=wp.quiver,
         stowed={"default": "socket_quiver", "backpack": "socket_quiver_hip"}, clips={}),
    dict(id="staff.oak", label="Oak staff", family="staff", slot="two_hand", build=wp.battle_staff,
         held="socket_hand_r", stowed=POLE_STOWED, clips=STAFF, grips={"off_hand": off_hand("staff")}, grab_range=[-1.0, 0.6]),
    dict(id="wand.yew", label="Yew wand", family="wand", slot="main_hand", build=wp.wand,
         held="socket_hand_r", stowed={"default": "socket_hip_r"},
         clips={"idle": "Spell_Simple_Idle_Loop", "attacks": ["Spell_Simple_Shoot"], "ready": "Draw_Side", "stow": "Sheathe_Side"}),
    dict(id="axe.bearded", label="Bearded axe", family="axe", slot="main_hand", build=wp.hand_axe,
         held="socket_hand_r", stowed={"default": "socket_hip_l"},
         clips={"idle": "Sword_Idle", "attacks": ["Sword_Regular_A", "Sword_Regular_B", "Melee_Hook"], "work": "TreeChopping_Loop",
                "ready": "Draw_Hip", "stow": "Sheathe_Hip"}),
    dict(id="mace.flanged", label="Flanged mace", family="mace", slot="main_hand", build=wp.mace,
         held="socket_hand_r", stowed={"default": "socket_hip_l"},
         clips={"idle": "Sword_Idle", "attacks": ["Sword_Regular_A", "Sword_Regular_C"], "ready": "Draw_Hip", "stow": "Sheathe_Hip"}),
    dict(id="spear.ash", label="Ash spear", family="spear", slot="main_hand", build=wp.spear,
         held="socket_hand_r", stowed=POLE_STOWED,
         clips={"idle": "Spear_Idle_Loop", "attacks": ["Spear_Thrust", "Spear_Sweep"], "ready": "Draw_Pole", "stow": "Sheathe_Pole"},
         grips={"off_hand": off_hand("spear")}, grab_range=[-1.1, 1.0]),
    dict(id="sword.great", label="Greatsword", family="greatsword", slot="two_hand", build=wp.greatsword,
         held="socket_hand_r", stowed={"default": "socket_back_1", "backpack": "socket_pack_blade"},
         clips={"idle": "GreatSword_Idle_Loop", "attacks": ["GreatSword_Slash", "GreatSword_Swing", "GreatSword_Overhead"],
                "ready": "Draw_Back", "stow": "Sheathe_Back"},
         grips={"off_hand": off_hand("greatsword")}, grab_range=[-0.15, 0.05]),
    dict(id="shield.kite", label="Kite shield", family="shield", slot="off_hand", build=wp.kite_shield,
         held="socket_shield_l", stowed=SHIELD_STOWED,
         clips={"idle": "Idle_Shield_Loop", "block": "Idle_Shield_Loop", "bash": "Shield_OneShot"}),
    dict(id="bow.long", label="Longbow", family="bow", slot="two_hand", build=wp.longbow, post="bow",
         held="socket_hand_l", stowed=BOW_STOWED, clips=BOW, grab_range=[-0.85, 0.85]),
    dict(id="staff.necro", label="Gravecaller's staff", family="staff", slot="two_hand", build=wp.necro_staff,
         held="socket_hand_r", stowed=POLE_STOWED, clips=STAFF, grips={"off_hand": off_hand("staff")}, grab_range=[-1.0, 0.6]),
    dict(id="staff.healer", label="Sunward staff", family="staff", slot="two_hand", build=wp.healer_staff,
         held="socket_hand_r", stowed=POLE_STOWED, clips=STAFF, grips={"off_hand": off_hand("staff")}, grab_range=[-1.0, 0.6]),
    dict(id="book.spells", label="Spellbook", family="book", slot="off_hand", build=wp.spellbook,
         held="socket_hand_l", stowed={"default": "socket_hip_r"},
         clips={"idle": "Spell_Simple_Idle_Loop", "ready": "Draw_Hip_L", "stow": "Sheathe_Hip_L"}),
    dict(id="orb.focus", label="Focus orb", family="focus", slot="off_hand", build=wp.focus_orb,
         held="socket_hand_l", stowed={"default": "socket_hip_r"},
         clips={"idle": "Spell_Simple_Idle_Loop", "ready": "Draw_Hip_L", "stow": "Sheathe_Hip_L"}),
    # Work tools for NPC routines and the crafting skills.
    dict(id="tool.hammer", label="Smith's hammer", family="tool", slot="main_hand", build=wp.smith_hammer,
         held="socket_hand_r", stowed={"default": "socket_hip_r"},
         clips={"work": "Fixing_Kneeling", "ready": "Draw_Side", "stow": "Sheathe_Side"}),
    dict(id="tool.hoe", label="Hoe", family="tool", slot="two_hand", build=wp.hoe,
         held="socket_hand_r", stowed=POLE_STOWED, clips={"work": "Farm_Harvest", "ready": "Draw_Pole", "stow": "Sheathe_Pole"},
         grab_range=[-0.5, 0.9]),
    dict(id="tool.watering_can", label="Watering can", family="tool", slot="off_hand", build=wp.watering_can,
         held="socket_hand_r", stowed={"default": "socket_hip_r"},
         clips={"work": "Farm_Watering", "ready": "Draw_Side", "stow": "Sheathe_Side"}),
    dict(id="tool.lantern", label="Lantern", family="tool", slot="off_hand", build=wp.lantern,
         held="socket_hand_l", stowed={"default": "socket_hip_r"},
         clips={"idle": "Idle_Lantern_Loop", "ready": "Draw_Hip_L", "stow": "Sheathe_Hip_L"}),
]


TEX = OUT / "textures"


def texture_size(ob):
    """1K for long items, 512 for the rest (docs/ASSETS.md budgets 1K for gear)."""
    return 1024 if max(ob.dimensions) > 0.9 else 512


def build_item(item):
    sc.reset()
    parts = item["build"]()
    ob = sh.join(parts, item["id"])
    sh.uv_unwrap(ob)
    # Bake before the bow's morphs fold its arrow away.
    surface.bake(ob, TEX, item["id"], texture_size(ob))
    entry = {k: v for k, v in item.items() if k not in ("build", "sheath", "post")}
    if item.get("post") == "bow":
        rest, pull = wp.bow_morphs(ob, BOW_DRAW)
        # Item glTF space: x, z, -y of Blender's.
        entry["string"] = {"rest": [round(rest.x, 5), round(rest.z, 5), round(-rest.y, 5)],
                           "pull": [round(pull.x, 5), round(pull.z, 5), round(-pull.y, 5)]}
    path = export.rigid(OUT / f"{item['id']}.gltf", [ob])
    entry["file"] = str(path.relative_to(sc.OUT))
    entry["triangles"] = sh.triangles([ob])
    if item.get("sheath"):
        sc.reset()
        sob = sh.join(item["sheath"](), f"{item['id']}.sheath")
        sh.uv_unwrap(sob)
        surface.bake(sob, TEX, f"{item['id']}.sheath", 512)
        spath = export.rigid(OUT / f"{item['id']}.sheath.gltf", [sob])
        entry["sheath"] = str(spath.relative_to(sc.OUT))
    return entry


# --- Review ---------------------------------------------------------------------------------

C = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))  # Blender to glTF axes.


def mount(objs, socket_matrix):
    """Places imported item objects on a socket's world matrix, exactly as the runtime does."""
    m = socket_matrix @ C
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
    ("bow", "Bow_Aim_Loop", 0, {"bow.hunting": "held", "quiver.leather": "held", "dagger.hunting": "stowed"}),
    ("staff-cast", "Staff_Cast", 13, {"staff.oak": "held"}),
    ("wand-idle", "Spell_Simple_Idle_Loop", 20, {"wand.yew": "held", "dagger.hunting": "held"}),
    ("bow-stowed", "Jog_Fwd_Loop", 6, {"bow.hunting": "stowed", "quiver.leather": "held", "sword.arming": "stowed"}),
    ("axe", "Sword_Regular_A", 7, {"axe.bearded": "held", "shield.kite": "held"}),
    ("mace", "Sword_Idle", 12, {"mace.flanged": "held", "shield.kite": "held"}),
    ("spear", "Spear_Idle_Loop", 0, {"spear.ash": "held"}),
    ("greatsword", "GreatSword_Idle_Loop", 0, {"sword.great": "held"}),
    ("necro", "Staff_Idle_Loop", 0, {"staff.necro": "held", "book.spells": "held"}),
    ("healer", "Staff_Channel_Loop", 0, {"staff.healer": "held", "orb.focus": "held"}),
    ("smith", "Fixing_Kneeling", 40, {"tool.hammer": "held"}),
    ("lantern", "Idle_Lantern_Loop", 20, {"tool.lantern": "held"}),
    ("water", "Farm_Watering", 40, {"tool.watering_can": "held"}),
    ("harvest", "Farm_Harvest", 30, {"tool.hoe": "held"}),
    ("back-two-handers", "Walk_Loop", 10, {"sword.great": "stowed", "shield.kite": "stowed"}),
    ("back-poles", "Walk_Loop", 10, {"staff.oak": "stowed", "shield.round": "stowed"}),
    ("back-bow", "Walk_Loop", 10, {"bow.long": "stowed", "quiver.leather": "held"}),
]


def on_character(entries):
    from aldermoor_art import looks
    by_id = {e["id"]: e for e in entries}
    offsets = looks.socket_offsets("male")
    files = []
    for name, clip, frame, gear in SCENES:
        look = dict(body="male", pieces=["ranger.top", "ranger.trousers", "ranger.boots"], hair="hair.simple_parted",
                    brows="brows.strong", beard="beard.full", tone="warm", hair_colour="chestnut", clip=clip, frame=frame)
        skel, objs = looks.assemble(look)
        for iid, state in gear.items():
            e = by_id[iid]
            socket = e.get("held", e["stowed"]["default"]) if state == "held" else e["stowed"]["default"]
            item = import_item(e["file"])
            mount(item, looks.socket_world(skel, offsets, socket))
            if e.get("string") and state == "held":
                for o in item:
                    if o.data.shape_keys:
                        o.data.shape_keys.key_blocks["arrow"].value = 1.0
                        o.data.shape_keys.key_blocks["draw"].value = 1.0
            objs += item
            if e.get("sheath"):
                sheath = import_item(e["sheath"])
                mount(sheath, looks.socket_world(skel, offsets, e["stowed"]["default"]))
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
