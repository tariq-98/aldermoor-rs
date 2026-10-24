#!/bin/zsh
# Rebuilds every asset in dependency order. Add --review for review renders.
set -euo pipefail
ROOT=${0:A:h:h}
B=$ROOT/art/blender.sh
S=$ROOT/art/blender
R=()
[[ "${1:-}" == "--review" ]] && R=(-- --review)
step() { echo "== $1"; $B -P $S/$1 "${R[@]}" 2>&1 | grep -E 'WROTE|BUILT|RACE|CHECK|Error|Traceback' || true; }
step build_humanoid.py   # bodies, hair, outfits, clips, catalogue
step build_races.py      # elves, dwarves and goblins from the human bodies
step build_creatures.py  # skeleton warriors, the Barrow King and stone golems
step build_gear.py       # rigid weapons, shields, bows, casters and tools
step build_back.py       # capes and packs for every body type
step build_armour.py     # plate for every body type
