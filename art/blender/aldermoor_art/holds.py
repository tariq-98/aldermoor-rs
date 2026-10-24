"""How items sit in the hands, shared by the clip authoring (build_clips.py) and the gear
catalogue (build_gear.py), so a clip's two-handed grips match its item's `grips` entry."""

import math

# The off hand on two-handed items: where it holds (metres along the item from the main hand's
# grip), its turn about the item's axis from the main hand's socket (radians), and how far along
# the item it may slide (a spear runs through the front hand as it's thrust).
OFF_HAND = {
    "staff": (0.28, 0.0, (0.1, 0.55)),
    "spear": (0.45, math.radians(-40), (0.1, 0.9)),
    "greatsword": (-0.10, 0.0, (-0.16, -0.06)),
}
# Bows: where the arrow sits on the string (above the grip's centre), and how far back the
# string comes at full draw on the male frame.
NOCK_Z = 0.02
BOW_DRAW = 0.60
