# Items, loot and shops

Agreed with Chippy on 25 October 2026 (#71). The tables are `crates/content/items/*.ron`
(checked on load by `content::items`); the rules are in `crates/sim/src/loot.rs`.

## Items

- **Fixed stats.** An item always has the same stats (`ItemDef::stats`). Better gear comes from
  higher material tiers, named items from bosses and quests, and crafting upgrades. No random
  affixes.
- **Fine versions.** Weapons and armour can drop (and later be crafted) as a **fine** version:
  +10% to every stat (`FINE_BONUS`), always exactly that, and worth three times as much
  (`FINE_VALUE`). Materials, food and junk are never fine. Fine and plain never share a stack.
- Gear isn't applied to fights yet; equipment slots come with the rest of #17.

## Inventory

- 28 slots (`SLOTS`). Gear takes one slot each; materials, food and junk stack (100 ore, 50
  hides, 20 bread, as each item says). Picking up or buying tops up existing stacks first, then
  fills empty slots.
- Coins are carried apart from the slots.

## Food and potions

- `Command::UseItem { slot }` eats or drinks one of the consumable in that slot. Its `effect` in
  items.ron names an aura in the combat tables (common.ron), which goes on the player. Nothing is
  used up if the aura doesn't take, and the dead or downed use nothing.
- Food (tagged `food`) is only eaten out of combat. Its buff lasts into the next fight: Bread
  gives Fed (a slow heal for 60 s), Grilled Trout gives Well Fed (a slow heal and +5% damage for
  180 s). Out of combat, health comes back quickly anyway, so food is for before a fight.
- Potions (tagged `potion`) work in combat and share one cooldown, `POTION_COOLDOWN` (60 s). The
  Healing Draught heals over 8 s. The player's state shows `potion_ready`, the seconds left.
- The tables are checked on load: every consumable has an effect that is a real aura and is
  tagged food or potion, and nothing else has an effect.

## Loot

- A monster that dies rolls its loot table (`loot.ron`): coins in a range, then each line on its
  own chance and count. Gear that drops is fine with the table's `fine_chance` (5% unless a
  table says otherwise; the golem 15%, the Barrow King 25%). Summons drop nothing.
- Loot rolls use their own random sequence from the world's seed, so the same seed and commands
  give the same drops, and loot doesn't shift the fight's own rolls.
- **Personal loot:** every player who earned the kill's XP (a tenth of the fight or more, see
  PROGRESSION.md) gets their own roll of the table, in their own **pile** where the monster
  fell. Only they see it and only they can take it (`Command::Loot`, within 3 m), so there's no
  race for drops and no ninja looting. It's gone after 3 minutes. What doesn't fit in your pack
  stays in the pile.
- The Wilderness keeps its own rule: a player who dies there drops everything for anyone to
  take (#76).

## Shops

- Shops sell what they stock at the item's value, always in stock, to a player standing within
  6 m of the counter (`Command::Buy`). Nothing happens if you can't pay or carry it all.
- Shops buy at 40% of value (`SELL_SHARE`), fine gear at 40% of its higher value
  (`Command::Sell`). Some only buy certain goods (the smith buys metal and ore).
- **Regional prices:** each town pays more for the goods it wants (`demand`, by tag).
  Brambleford pays half again for hides. Later towns want other things (ore in a smithing town).
- No player trading or market while the game is solo; that comes with co-op (phase 6).
- Counters stand by the village houses as placeholders until the shopkeepers and their buildings
  exist (#125, #128).

## Coin sinks

Training, perk swaps, recipes, appearance changes and later housing (DESIGN.md). None are built
yet. Losing some coins in a recoverable pouch on death (DESIGN.md) is its own follow-up.

## What the player sees

The player's own state carries `coins`, every `inventory` slot (with what the shop you stand at
pays for each), the `loot` piles nearby you may take, and the `shop` you stand at with its wares
and prices. Protocol version 7 (8 with gathering and crafting, CRAFTING.md; 9 with `UseItem` and `potion_ready`). The loot window, bags and shop screen are client work (#179).
