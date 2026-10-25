# How the game and the renderer talk

For whoever builds `render` and `client`. The simulation side is done for milestones 1 and 2
(crates `world`, `sim`, `protocol` and `server`); how the fight works is in
[COMBAT.md](COMBAT.md).

## Conventions

- Metres, seconds, radians.
- +y is up and +z is south. A yaw of 0 faces +z: heading = `atan2(x, z)` and forward =
  `(sin yaw, 0, cos yaw)`. This matches both Swift codebases.

## The ground: `world::Valley`

Build terrain, water and props from this crate, so what's drawn always matches what you collide
with. It's a port of `aldermoor-wild/Sources/World.swift`, tested against values from that code.

- `world::height(x, z)`, `world::normal(x, z)`: terrain.
- `Valley::ground(x, z)`: the walkable surface, including the bridge deck.
- `world::river_z(x)`: the river's centre line, 12 m wide, with water at y = −0.78.
- `world::path_distance(x, z)`: distance to the nearest road, for ground blending.
- `valley.houses`, `valley.trees`: placements as position, yaw and scale. The windmill stands at
  (49, −84) and the shed at (−26, −48).
- `world::BOUNDS`: the map's edge, ±280 m.

Draw the buildings, trees, bridge and landmarks from `assets/catalog/environment.json`
(docs/ASSETS.md, "Scenery"): its `placements` map each house, tree and landmark `world` places
onto a prop, and list the props for decorative scatter (rocks, flowers, grass, bushes), which
can stay renderer-only, since nothing collides with it.

## Running the game: `server::Host`

```rust
let host = server::Host::start(seed, "Hero"); // the valley plus one player, stepping at 60 Hz
host.send(protocol::Command::Move { direction, walk, face_yaw });
let snap: Arc<Snapshot> = host.snapshot().unwrap(); // the latest tick
let fight: Arc<CombatView> = host.combat().unwrap(); // combat detail from the same tick
let events = host.take_events(); // everything since you last asked
```

Each `Snapshot` (protocol version 5) has:

- `units`: everyone within 160 m. For each one:
  - `body` (the simulation's body name: `"player"`, `"wolf"`, `"goblin"` or `"humanoid"`);
  - `look`, which says exactly what to draw (see "Art for each unit" below);
  - position, velocity, yaw, `pose` with `pose_time`;
  - health, name, level, `hostile` and target.
- `player`, the viewer's own numbers:
  - endurance (rolls and blocks spend it), the specialty's resource (`resource_kind`: rage,
    mana, focus, energy or essence) and its maximum;
  - `bar`: ten `Slot`s for keys 1–9 and 0, each with the ability's key (also its icon's name),
    name, seconds of cooldown left and its full recharge, cost, range, `ground` (aim it at the
    ground) with the `radius` of the circle it lands in, and `auto` (the basic attack). While downed, the bar holds the downed skills;
    `cooldowns` repeats the ten cooldowns;
  - `gcd` (seconds of global cooldown left), `cast` (the cast bar: ability, name, elapsed,
    duration, channel, interruptible), `auras` (key, name, stacks, seconds left, full
    duration, debuff);
  - `guarding`, `downed`, `weapon_set` (0 or 1) and `swap_ready` (seconds until a swap);
  - auto-attack, in-combat and can-respawn.
- `events`, this tick's happenings:
  - `Damage` (with `school`, and `periodic` for conditions ticking), `Heal`, and `Avoided` with
    why (evaded, blocked, parried, absorbed, immune, reflected, missed): show these as floating
    numbers and words;
  - `AbilityUsed`, `CastStarted`, `CastInterrupted`, `AuraApplied`, `AuraRemoved`;
  - `ProjectileLaunched` and `ProjectileEnded` (where it stopped, and on whom), `AreaImpact`
    (a warned blow landing: a burst of dust, frost or fire), `Combo`;
  - `Staggered` (thrown off balance by a parry), `Downed`, `Rallied`, `Revived`,
    `DefianceBroken`, `WeaponsSwapped`;
  - deaths, respawns, jumps, rolls and messages.

## Combat detail: `host.combat()`

A `CombatView` comes with every snapshot (same tick) and covers everything the player can see:

- `units`: for each unit, the `action` its body is doing (below) and seconds since it began
  (`action_time`), its cast bar, auras, resource, `guarding`, `downed`, and `defiance`
  (current and full) for elites and bosses, which shrug off crowd control until it breaks.
- `projectiles`: each arrow, bolt or thrown axe in flight: position, velocity, `visual` (an
  effect's name: `"arrow"`, `"fire_bolt"`, `"frost_lance"`, `"shadow_bolt"`, `"holy_bolt"`,
  `"arcane_missile"`, `"arcane_spark"`, `"axe"`) and school. Draw these: they're the real
  missiles, which can miss and can be turned back. A bow's `release` clip event is for timing
  the string and the sound.
- `areas`: things on the ground:
  - warnings (`warning: true`): a blow about to land, filling in as `elapsed` reaches
    `duration`. A monster's warned blow is a circle, a cone (`Cone { range, angle }` along
    `yaw`) or a line (`Line { length, width }` along `yaw`); arrows and meteors falling on a spot
    are circles;
  - fields (`warning: false`): lasting areas, some of them combo fields (`field`: fire, ice,
    light, water, poison, lightning), and traps;
  - `hostile` says whether it threatens the viewer: draw those in red, friendly ones in a calm
    colour.

## Drawing between ticks

Snapshots arrive at 60 Hz, and the display may run at 120 Hz. Keep the last two snapshots and draw
at about one tick behind, interpolating positions and yaws by time. For a missed tick, extrapolate
along `velocity`. `pose_time` drives animation without guessing.

## Poses to animation

How long each pose lasts in the simulation (the clips themselves are listed per body below):

| Pose | Timing |
|---|---|
| Idle, Walk, Run | Walk below 3.2 m/s; scale playback by speed. Charging is Run |
| Jump, Fall | Rising, then falling. A skill's leap or vault is Jump |
| Dodge | A roll: 0.45 s, about 5 m |
| Attack | Holds 0.5 s after a blow or a release. A heavy blow's pose starts with its 0.4 s wind-up and runs on unbroken through the blow (0.9 s in all): time the clip so its strike lands when the wind-up ends (the cast bar gives the wind-up). Every attack clip marks its strike with a `hit` event, and spells and throws their release with `cast`; combos have one `hit` per blow |
| Cast | While casting or channelling |
| Hit | Holds 0.25 s; while thrown by a knockback; 0.5 s when staggered by a parry |
| Dead | Until respawn. The downed lie as the dead do: check `downed` in the combat view |

The combat view's `action` says which motion to play within the pose. Clips from `anims.glb`
and the held item's `clips` (docs/ASSETS.md):

| Action | Humanoid clip |
|---|---|
| Slash, Slash2 | The main-hand item's `attacks`, first and second (`Sword_Regular_A`, `B`; `GreatSword_Slash`, `Swing`) |
| Thrust | `Spear_Thrust` with a spear; otherwise the last of `attacks` (`Sword_Regular_C`, `Melee_Hook`) |
| Overhead | `GreatSword_Overhead` with a greatsword; otherwise `Sword_Overhead` (one blow, cut from `Sword_Heavy_Combo`) |
| Spin | `Sword_Regular_Combo` (a greatsword's `GreatSword_Swing`, looping while channelled) |
| Bash | The shield's `bash` (`Shield_OneShot`); bare hands `Punch_Jab` |
| Block | The item's `block` (`Sword_Block`, or the shield's `Idle_Shield_Loop`) on the upper body, held while `guarding` |
| Parry | `Sword_Block` from the start, quickly, then back to the guard |
| Shoot | `Bow_Shoot`; a wand's `Spell_Simple_Shoot`; a thrown axe's `OverhandThrow` |
| CastBolt | The item's `attacks` (`Staff_Cast`, `Spell_Simple_Shoot`) |
| CastArea | A staff's `slam` (`Staff_Slam`); otherwise `Spell_Simple_Shoot` |
| Channel | A staff's `channel` (`Staff_Channel_Loop`); otherwise `Spell_Simple_Idle_Loop` |
| CastSelf | `Spell_Simple_Enter`, then `Spell_Simple_Exit` |
| Kick | `Punch_Cross` |
| Leap | A sword's or shield's `dash` (`Sword_Dash`, `Shield_Dash`); otherwise `NinjaJump_Start` |
| Charge | The item's `dash`; otherwise `Sprint_Loop` |
| Roar | `Spell_Simple_Enter` for now (a shout of its own is on the art list) |
| Stagger | `Hit_Knockback`; `Idle_Shield_Break` if it was guarding a moment before (its guard broke) |
| Downed | `Death01`, held at its end; `LayToIdle` when `Rallied` |
| Revive | `Fixing_Kneeling`, looping |

Beasts: Bite and Claw play `attack`, Roar plays `special` (the howl), Leap plays `jump`.

## Art for each unit

Each unit's `look` (`protocol::Look`) names everything in the art
catalogue (docs/ASSETS.md):
- `art`: a body type in `assets/catalog/humanoid.json` (`"male"`, `"elf_female"`,
  `"goblin_male"`, `"skeleton"`…) or a beast in `assets/catalog/creatures.json`
  (`"wolf.grey"`).
- `tone`, `hair`, `hair_colour`, `brows`, `beard`: ids from the catalogue. `None` means none
  (hair and beard) or the body's default (tone and hair colour).
- `pieces`: clothing and armour ids. Their layers decide what covers what.
- `gear`: (slot, item id) pairs from `assets/catalog/gear.json`.
- `dyes`: (piece or back item id, main dye, trim dye) for dyed pieces and capes, dye ids from
  the catalogue's `dyes` (`None` keeps that channel's painted colour); `variants`: (piece id,
  colourway texture) for pieces in another colourway. A piece has one or the other.
- `drawn`: weapons on their `held` sockets (true), or on their `stowed` sockets, chosen by
  the back item's `back_kind`. When it changes, play the main-hand item's `clips.ready` or
  `clips.stow` over the legs; the item changes hands at that clip's `grab` or `stow` event
  (docs/ASSETS.md, "Clips, curves and events").

Today the player is a farmhand: a warrior with an arming sword and a round shield, and a
greatsword to swap to (`gear` follows the weapon set in hand). Goblins wear the Ranger jerkin
dyed rust and walnut, with a hunting knife and round shield (archers carry a hunting bow), and
wolves are `wolf.grey`. Monsters for later zones are in the content tables already: the dire
wolf (`wolf.dire`), skeletons, the stone golem and the Barrow King. `drawn` turns on during a
fight or behind a raised guard. Looks are sent every tick for now; the network version will send them only
when they change. `tools/workshop/` assembles a look from these names exactly as the game
should (docs/ASSETS.md, "The reference implementation").

Humanoid clips by pose, from `anims.glb`:

| Pose | Clip |
|---|---|
| Idle | `Idle_Loop`; with weapons drawn, the main-hand item's `clips.idle` (`Sword_Idle`, `Bow_Idle_Loop`, `Staff_Idle_Loop`, `Spear_Idle_Loop`, `GreatSword_Idle_Loop`); a piece with an `idle` (the robes' `Idle_Robe_Loop`) replaces `Idle_Loop` |
| Walk | `Walk_Loop`; with weapons drawn, the item's idle on the upper body over it |
| Run | `Jog_Fwd_Loop`; `Sprint_Loop` above 7 m/s of ground speed (ignore the vertical part: a normal run is 6 m/s, and slopes add to the full speed), for swiftness and charges (the same layering when drawn) |
| Jump, Fall | `Jump_Start`, `Jump_Loop`, `Jump_Land` |
| Dodge | `Roll` |
| Attack | The main-hand item's `clips.attacks`, in turn (e.g. `Sword_Regular_A`, `B`, `C`; `Bow_Shoot`; `Spear_Thrust`, `Spear_Sweep`; `GreatSword_Slash`, `Swing`, `Overhead`) |
| Cast | A staff's `clips.attacks` (`Staff_Cast`); otherwise `Spell_Simple_Shoot` |
| Hit | `Hit_Chest` |
| Dead | `Death01` |

Every clip's `curves` apply on every frame and its `events` fire as they pass: the off hand
stays on two-handed items, bowstrings follow the drawing hand, and `Bow_Shoot` looses its arrow
at its `release` event (spawn the projectile then). The rules, with a reference implementation,
are in docs/ASSETS.md, "Clips, curves and events".

Beasts name their clips by role in `clips` (idle, walk, run, attack, hit, death, jump, special).
They have levels of detail like everything else (`lod_path` names the files), and their clips
always carry the jaw closed (the bind pose's mouth is a little open), so draw them animated.

## Input the client sends

- **Movement keys:** a world-space direction, sent as `Move` whenever it changes. In the default
  Classic scheme the client keeps the character's heading (A/D turn it, the right mouse button
  sets it to the camera's yaw) and always sends it as `face_yaw`, so S backpedals and Q/E strafe.
  In the Action scheme WASD points from the camera's yaw, and `face_yaw` goes only while the
  right mouse button is held (mouse-look strafing).
- **Space:** `Jump`.
- **Dodge key:** `Dodge` (a zero direction rolls backwards).
- **Tab or clicking a unit:** `SetTarget`. Right-clicking an enemy: `SetTarget` plus
  `SetAutoAttack(true)`.
- **1–9 and 0:** `UseSlot` 0–9. Send `ground` with the point under the cursor (or the centre of
  the screen with mouse-look): ground skills land there, and straight-flying shots fly at it, so
  a player can lead a moving target. Show slots with `ground` as a circle of the skill's radius
  following the cursor.
- **Guard:** hold a key (`F`, or mouse button 5; Q and E strafe): `Guard(true)` on press,
  `Guard(false)` on release.
- **Swap weapons:** `` ` `` sends `SwapWeapons`.
- **Interact on a downed or defeated friend:** `Revive { target }`; it takes a few seconds of
  standing still.
- **When defeated:** `Respawn`.

## Budgets on the M1 Air at 5120×1440

- **Simulation:** 0.72 ms a step on average (1.3 ms at the 99th percentile) with 2,000 monsters
  and the fight running (`cargo run -p sim --release --example bench 2000 dense`), against a
  2 ms budget. It runs on its own thread, so the main thread is free for the renderer.
- **Target:** a steady 60 fps, with the GPU under 12 ms.
