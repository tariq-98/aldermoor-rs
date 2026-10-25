#!/bin/zsh
# Rebuilds every asset in dependency order. Add --review for review renders.
set -euo pipefail
ROOT=${0:A:h:h}
B=$ROOT/art/blender.sh
S=$ROOT/art/blender
R=()
[[ "${1:-}" == "--review" ]] && R=(-- --review)
step() { echo "== $1"; $B -P $S/$1 "${R[@]}" 2>&1 | grep -E 'WROTE|BUILT|RACE|CLIP|CHECK|WARN|Error|Traceback' || true; }
step build_humanoid.py   # bodies, hair, outfits, catalogue
step build_clips.py      # the clip library: vendor clips plus our own
step build_races.py      # elves, dwarves and goblins from the human bodies
step build_creatures.py  # skeleton warriors, the Barrow King and stone golems
step build_gear.py       # rigid weapons, shields, bows, casters and tools
step build_back.py       # capes and packs for every body type
step build_armour.py     # plate for every body type
step build_garments.py   # robes, tunic, coat, apron and hats for every body type
step build_beasts.py     # wolves on their own rig
echo "== ground fields"      # world's ground, trees, houses and masonry, for the environment and world steps
(cd $ROOT && cargo run -q -p world --release --example ground_fields -- $ROOT/art/out/world)
step build_environment.py # buildings, trees, rocks, plants, village props, field walls and the ruin
step build_effects.py    # combat effects: sprite atlas, tiling strips and effect meshes
step build_world.py      # the valley's ground, river and sky, and its light
