# Gathering and crafting

Agreed with Chippy on 25 October 2026 (#68). The tables are `crates/content/crafts/*.ron`
(checked on load by `content::crafts`); the rules are in `crates/sim/src/crafting.rs`.

## Skills

| Skill | Kind | Gathers or makes |
|---|---|---|
| Mining | Gathering | Copper and iron ore (garnets, rarely) |
| Woodcutting | Gathering | Oak and yew logs |
| Skinning | Gathering | Pelts from dead wolves and dire wolves |
| Herbalism | Gathering | Marigold, flax and silverleaf |
| Fishing | Gathering | River trout (and the odd message in a bottle) |
| Smithing | Craft | Ingots at the forge; blades, maces and tools at the anvil |
| Leatherworking | Craft | Leather, bows and jerkins at the tanning rack |
| Tailoring | Craft | Linen and padded coats at the loom |
| Enchanting | Craft | Staves, wands and focus orbs at the altar |
| Cooking | Craft | Grilled trout at the fire |
| Alchemy | Craft | Healing draughts at the alchemy table |

- Every skill levels on its own through use, from 1 to **50**, on the specialties' XP curve
  (PROGRESSION.md). Each swing or item pays its XP. Work more than 5 levels below your skill pays
  less, and nothing 10 below, as with kills.
- Nodes and recipes ask for a skill level (iron and yew take 10, a sword Smithing 5).

## Gathering

- **Nodes** (ore veins, trees, herb patches, fishing spots) give a few swings, then run out and
  grow back after a few minutes (`uses`, `regrow`). The better nodes stand where it's
  dangerous: iron and silverleaf are among the goblins on the eastern ridge, and yews nearby.
  Nodes don't tick. One that ran out notes when it's back, so idle nodes cost nothing.
- **Carcasses:** a dead wolf or dire wolf can be skinned once, where it fell, straight after the
  fight. Other work waits until you're out of combat.
- **Tools** are required: a pickaxe, hatchet, skinning knife, sickle or rod in the pack. The
  fastest one you have the level for is used, and swings take the node's time over its
  `speed` (an iron pickaxe, at Mining 10, swings 1.4 times as fast). The general store sells
  the copper ones, and smiths can make them.
- `Command::Gather { node }` (within reach, 3 m, or 9 m for a fishing spot from the bank) or
  `Command::Skin { target }` starts swinging. It goes on until the node runs out, the pack
  fills, you move, anything hits you (or you hit it) or you start casting. `Command::StopWork` ends it.
- **The sweet spot:** each swing has a window from 65% to 85% of the way through
  (`SWEET_SPOT`). `Command::Strike` inside it gives one more of the yield and a chance at the
  node's rare find. A strike outside it, or none, gives the normal yield. Only the first strike
  each swing counts, so mashing doesn't help.
- Yields roll on their own sequence from the world's seed, so gathering doesn't shift the
  fight's rolls, and the same seed and commands gather the same.

## Crafting

- Stations stand in Brambleford: the forge and anvil by the smithy, the alchemy table by the
  store, the loom, tanning rack, altar and cooking fire by the houses. They are placeholders
  until the art lands (#145).
- `Command::Craft { recipe, count }`, within 4 m of the right kind of station, makes one after
  another. Each item takes the recipe's seconds, takes its inputs from the pack and pays XP. It
  stops when the inputs run out, the pack is full, or you move away.
- The chains: mine copper → smelt ingots → smith a dagger, pickaxe or sword (with an oak log for
  the handle); skin wolves → tan leather → a jerkin or (with oak) a bow; pick flax → linen → a
  padded coat; pick marigolds → a healing draught; catch trout → cook it.
- Crafted gear isn't fine yet, and food and potions can't be used yet (#205).

## What the player sees

The player's own state carries `skills` (level, XP and the next level's XP for all 11, in
`content::crafts::Skill` order), `work` (the swing or item under way, with the sweet spot in
seconds and whether this swing's strike landed), the `nodes` within 40 m (ready or growing
back), and the `station` you stand at with each recipe and how many you could make now.
`Event::SkillUp` marks a level. Protocol version 8. The client side (the swing bar, strike key,
crafting window and skills panel) is #206.
