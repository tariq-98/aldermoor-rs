# Decisions log

Choices that shape the game, newest first. Open questions are issues labelled
[`status:needs-decision`](https://github.com/tariq-98/aldermoor-rs/issues?q=is%3Aopen+label%3Astatus%3Aneeds-decision).
When one is answered, add it here and close the issue.

| Date | Decision | Why | Issue |
|---|---|---|---|
| 2026-10-25 | Team modes (dungeons, capture the flag, more) with no death stakes; respawn at team spawn points | Competitive and co-op play without risk | #77 |
| 2026-10-25 | A level-banded wilderness with PvP and PvE: players above a band's ceiling are scaled down to it; deeper bands have better loot; dying there drops everything, equipped gear included | Real stakes, while low levels who stay in their band only meet their own strength | #76 |
| 2026-10-25 | Aldermoor is an MMORPG: large worlds, many players. Built solo against the local server first, every system server-authoritative; online once the first area is fun | Same end state, a fun first area sooner, hosting paid only when worth it. Replaces "solo and offline first" | |
| 2026-10-25 | Loot has fixed stats (better tiers, named rares, crafting upgrades) with a small chance of a "fine" version; NPC shops with set, region-dependent prices; player trading waits for co-op; coins go on training, perks, recipes, appearance and housing | Fits fixed item identities and keeps crafted gear competitive | #71 |
| 2026-10-25 | Quests: a handful of big handmade story quests per area plus small jobs from notice boards and townsfolk; quests are linear with no branching world outcomes | Big quests carry the story and best rewards; no branches keeps content cost down | #70 |
| 2026-10-25 | Gathering: nodes deplete and regrow in minutes, better ones in dangerous places; tools required; each swing has an optional timing sweet spot for bonus or rare yield, no penalty for missing | Works both actively and passively | #68 |
| 2026-10-25 | Gathering skills: mining, woodcutting, skinning, herbalism and fishing; crafts gain cooking and alchemy alongside smithing, tailoring, leatherworking and enchanting. Each levels through use to 50 | Every craft needs a source of materials; RuneScape-style freedom | #68 |
| 2026-10-25 | Specialty XP: an enemy's death pays XP split between the specialties whose skills were used on it; quests give XP the player assigns to any specialty | Feels like levelling through use without training-dummy farming | #66 |
| 2026-10-25 | Specialties cap at level 50, and the equipped specialty's level is the one that counts in combat (no overall character level) | Power compounds 6% a level, so 99 would be ~300x a level-1 fighter; 50 is ~17x | #66 |
| 2026-10-25 | Action controls are the default: crosshair always on, mouse always steers, shots go to the crosshair, Tab locks a target optionally. Classic (WoW-style) stays as a setting | Chippy wants aiming to matter; Classic already built in PR #52 | #62, #22 |
| 2026-10-25 | Space jumps, Shift sprints, roll has its own key (Alt and mouse thumb) in the movement direction; double-tap roll is an off-by-default setting | Double-tap misfires while strafing | #62 |
| 2026-10-25 | Melee hits a cone in front with a gentle pull toward the crosshair | Positioning matters without whiffing by a few degrees | #64 |
| 2026-10-25 | Movement adds out-of-combat sprint, climbing ledges to chest height, swimming (no fighting in deep water) and light fall damage; free climbing and mounts later | Makes the valley's cliffs, ruins and river part of play | #65 |
| 2026-10-25 | All keys can be remapped in game | Chippy's request | #63 |
| 2026-10-25 | Agents merge their own PRs once tested and working, without asking | Chippy doesn't want to be the bottleneck; AGENTS.md lists what "tested and working" means | |
| 2026-10-25 | Codex never creates or edits assets or works in Blender | Assets and look come from Claude; Codex sticks to renderer code. Enforced by the Codex guard check on `codex/` branches | |
| 2026-10-25 | 60 fps / 12 ms GPU is measured on the M1 Air's built-in display at native resolution; 5120×1440 is best effort | Chippy set the ultrawide aside; effects and scatter stay in scope, priced against this target. Render scale for the budget is still open | #36 |
| 2026-10-25 | Classic (WoW-style) is the default camera and movement scheme | Matches the WoW and Guild Wars feel; action camera stays as an option | #22 (guard key still open) |
| 2026-10-25 | MetalFX stays opt-in, not the main performance lever | Measured at about 2x the cost of the custom TAA on the M1 (22.7 vs 11.1 ms) | #10 |
| 2026-10-25 | GitHub issues are the source of truth for agent work | Lets several agents work in parallel without clashing | #24 |
| 2026-10-24 | Claude makes all art; Codex owns the renderer and doesn't invent look values | Set in docs/PLAN.md | |
