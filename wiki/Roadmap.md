# Roadmap

Six phases, each a [milestone](https://github.com/tariq-98/aldermoor-rs/milestones). The feel of
moving and fighting comes first, then the look, because those two decide whether this reads as
Fable or WoW. Whether Looks or Characters goes second is open in #23.

| Phase | Goal | Done when |
|---|---|---|
| 1. Feel | Client on the latest combat protocol, camera v2, combat visuals, under the frame budget, CI | A ten-minute fight in the valley feels good on keyboard and mouse at a steady 60 fps |
| 2. Looks | Scatter, AO, bloom and fog, day and night, real spell effects, each priced in ms | Captures hold up next to the reference shots, inside 12 ms GPU |
| 3. Characters | Creator, specialty XP and perks, inventory and loot, saves | Make a character, level two specialties, quit and come back |
| 4. Brambleford | Settlement and NPC routines, quests and dialogue, one dungeon and the Barrow King, first audio and menus | The first complete area is playable start to finish |
| 5. Living world | AI layer, companions, crafting, weather and events | NPCs hold a conversation and the valley changes without you |
| 6. Polish and co-op | Audio and music, controllers, co-op over QUIC | Two players clear the dungeon together |

## Budgets

Every feature is paid for against these, measured on the M1 Air's built-in display at native
resolution (decided on #36; the 5120×1440 ultrawide is best effort only):

- GPU: 12 ms per frame (60 fps). Over budget as of 2026-10-25, so phase 1 includes getting under it.
- Sim: 2 ms per step for 2,000 units.
