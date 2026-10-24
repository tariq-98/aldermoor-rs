"""Renders review sheets from the exported humanoid assets, the way the game assembles them.

    art/blender.sh -P art/blender/review_humanoid.py [-- look-name ...]

Loads assets/catalog/humanoid.json and the glTF files, hides covered regions, tints skin and
hair, poses the skeleton with a clip from anims.glb and renders each look from two angles into
art/out/review/looks/.
"""

import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aldermoor_art import looks, review, scene as sc  # noqa: E402

OUT = sc.ROOT / "art" / "out" / "review" / "looks"
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []

LOOKS = {
    "farmhand": dict(body="male", pieces=["peasant.top", "peasant.trousers", "peasant.shoes"],
                     hair="hair.simple_parted", brows="brows.strong", beard="beard.full",
                     tone="tan", hair_colour="dark brown", clip="Idle_Loop", frame=20),
    "ranger": dict(body="male", pieces=["ranger.top", "ranger.trousers", "ranger.boots", "ranger.hood", "ranger.pauldron"],
                   hair="hair.long", brows="brows.strong", tone="fair", hair_colour="chestnut",
                   clip="Sword_Idle", frame=12),
    "villager": dict(body="female", pieces=["peasant.top", "peasant.trousers", "peasant.shoes"],
                     hair="hair.buns", brows="brows.fine", tone="brown", hair_colour="black",
                     clip="Walk_Loop", frame=8),
    "scout": dict(body="female", pieces=["ranger.top", "ranger.trousers", "ranger.boots", "ranger.pauldron"],
                  hair="hair.long", brows="brows.fine", tone="porcelain", hair_colour="copper",
                  clip="Spell_Simple_Idle_Loop", frame=20),
    "mixed": dict(body="male", pieces=["ranger.top", "peasant.trousers", "ranger.boots"],
                  hair="hair.buzzed", brows="brows.strong", tone="deep", hair_colour="black",
                  clip="Jog_Fwd_Loop", frame=6),
    "swapped": dict(body="female", pieces=["peasant.top", "ranger.trousers", "peasant.shoes", "ranger.hood"],
                    hair="hair.simple_parted", brows="brows.fine", tone="olive", hair_colour="honey",
                    clip="Sword_Regular_A", frame=6),
}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    names = ARGS or list(LOOKS)
    for name in names:
        _, objs = looks.assemble(LOOKS[name])
        for yaw in (25, 200):
            review.render(OUT / f"{name}-{yaw}.png", objs, yaw=yaw, size=(640, 860))
        print("LOOK", name)


main()
