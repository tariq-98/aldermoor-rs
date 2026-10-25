# Roadmap

Eight phases, each a [milestone](https://github.com/tariq-98/aldermoor-rs/milestones) and a
`phase:N` label. Aldermoor is an MMORPG, built solo against the local server first: every system is
written server-side from day one so nothing is rewritten when the game goes online. Real networking
comes once Brambleford is fun, and the Wilderness and team modes follow because they need other
players. Whether Looks or Characters goes second is open in #23.

| Phase | Goal | Done when |
|---|---|---|
| 1. Feel and speed | Under 12 ms GPU on the M1 Air's own screen, camera v2, melee cones, movement, key remapping, CI | A ten-minute fight in the valley feels good and holds 60 fps on the laptop screen |
| 2. Looks | ASTC, scatter, post stack, day and night, spell effects, HUD art, crowd rendering, each priced in ms | A crowded market square still looks good and holds 60 fps on the laptop |
| 3. Characters and economy | Creator, specialty XP and perks, inventory, loot, shops, bank and trading, all server-side | Two players can level, trade and bank items, log out, and find everything still there |
| 4. Brambleford | Settlement, NPC routines, quests, one dungeon and the Barrow King, streamed world cells | Brambleford is fun to play alone, start to finish |
| 5. Online | Standalone Linux server over QUIC, network-ready snapshots, client prediction, accounts, bots, hosted test server | Two real players and 200 bots share Brambleford on one hosted server, and it still feels local at 100 ms ping |
| 6. Wilderness and team modes | Wilderness PvP and PvE in level bands with real stakes; no-stakes dungeons, capture the flag, arenas, parties, matchmaking | Players can risk real loot in the Wilderness, and two teams of five can queue for a full capture the flag match |
| 7. Living world | Gathering and crafting, the AI layer, companions, weather and world events | NPCs hold conversations and the world changes without players |
| 8. Scale and polish | Several world servers with region hand-over, anti-cheat and moderation, full audio, controllers, replay | Hundreds of players can be online at once across several servers |

## Budgets

Every feature is paid for against these, measured on the M1 Air's built-in display at native
resolution (decided on #36; the 5120×1440 ultrawide is best effort only):

- GPU: 12 ms per frame (60 fps). Over budget as of 2026-10-25, so phase 1 includes getting under it.
- Sim: 2 ms per step for 2,000 units.
