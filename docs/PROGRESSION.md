# Progression

How specialties level, agreed with Chippy on 25 October 2026 (#66). The rules are in
`crates/sim/src/progress.rs`; the numbers to tune are in `crates/sim/src/content/progression.rs`.

## Levels

- Every specialty has its own level, 1 to **50**, and its own XP toward the next. There is no
  overall character level.
- The **equipped** specialty's level is the one that fights: it sets health, power and armour
  (`stats::for_specialty`, 6% a level compounding, so level 50 is about 17 times level 1). It is
  also the level other players and monsters see.
- Switching specialty (`Sim::switch_specialty`) works only out of combat, not while down,
  defeated or casting. It brings that specialty's resource, default weapons, skills and level.
  Health keeps its share of the maximum. Full loadouts with gear and perks come later (DESIGN.md).
- Levelling up the equipped specialty applies at once, again keeping health's share. Each level
  sends a `LevelUp` event and a message.

## Fight XP

- A monster pays XP only when it **dies**. One that gets away, resets, or never dies (a training
  dummy) pays nothing, so there's nothing to farm.
- Each monster keeps a tally of the fight (`Credit`): for every player, the damage they dealt
  it (not counting overkill, but counting what a shield soaked), the damage it dealt them, and
  the healing they gave anyone it's fighting. Each amount is noted against the specialty the
  player had equipped. A summon's share counts for its owner.
- When it dies, **every** player with at least a tenth of the fight (`SHARE_FLOOR`) gets its
  full XP, split between the specialties they used in proportion to how much each did. Players
  don't share one pool, so bringing a friend doesn't halve your XP, but a single tap on someone
  else's kill earns nothing (#201, decided 2026-10-25).
- What a kill pays is `KILL_XP` for the monster's level, times 4 for an elite and 15 for a boss.
  Monsters more than 5 levels below the specialty pay less, falling to nothing 10 levels below.
- Summoned monsters and other players pay nothing (Wilderness kill rewards are #76).

## Quest XP

Quests grant a pool of XP (`Sim::grant_quest_xp`) that the player gives to any specialty they
like, in any amounts, with `Command::AssignXp { specialty, amount }` ("warrior", "archer",
"mage", "necromancer", "rogue", "healer"). Asking for more than is in the pool does nothing.

## The curve

`XP_TO_NEXT` grows 12% a level (250 XP for level 2, 58,000 for level 50, about 540,000 in all)
and `KILL_XP` 9% a level (40 at level 1, 2,700 at level 50). So a level takes about 6 even kills
at level 1 and about 23 at level 49, before quests. How long 50 should take is still to tune.

## What the player sees

The player's own state in each snapshot carries the equipped `specialty`, its `level`, `xp`
into the level and `xp_next` (0 at the cap), `unassigned_xp`, and every specialty's level in
`specialties`. Protocol version 11. The client shows the equipped specialty's XP bar under the
player frame, a banner on each level up, and a Specialties screen (K by default) that lists
every specialty's level and gives quest XP to one.
