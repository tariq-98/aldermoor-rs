# How the game and the renderer talk

For whoever builds `render` and `client`. The simulation side is done for milestone 1 (crates
`world`, `sim`, `protocol` and `server`).

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

The renderer's decorative scatter (rocks, flowers, grass) can stay renderer-only, since nothing
collides with it.

## Running the game: `server::Host`

```rust
let host = server::Host::start(seed, "Hero"); // the valley plus one player, stepping at 60 Hz
host.send(protocol::Command::Move { direction, walk, face_yaw });
let snap: Arc<Snapshot> = host.snapshot().unwrap(); // the latest tick
let events = host.take_events(); // everything since you last asked
```

Each `Snapshot` has:

- `units`: everyone within 160 m. For each one:
  - `body` (the simulation's body name: `"player"`, `"wolf"` or `"goblin"` so far);
  - `look`, which says exactly what to draw (see "Art for each unit" below);
  - position, velocity, yaw, `pose` with `pose_time`;
  - health, name, level, `hostile` and target.
- `player`: endurance, the bar's cooldowns, auto-attack, in-combat and can-respawn.
- `events`: damage (0 means evaded), deaths, respawns, jumps, dodges and messages.

## Drawing between ticks

Snapshots arrive at 60 Hz, and the display may run at 120 Hz. Keep the last two snapshots and draw
at about one tick behind, interpolating positions and yaws by time. For a missed tick, extrapolate
along `velocity`. `pose_time` drives animation without guessing.

## Poses to animation

How long each pose lasts in the simulation (the clips themselves are listed per body below):

| Pose | Timing |
|---|---|
| Idle, Walk, Run | Walk below 3.2 m/s; scale playback by speed |
| Jump, Fall | Rising, then falling |
| Dodge | 0.45 s, about 5 m |
| Attack | Holds 0.5 s |
| Hit | Holds 0.25 s |
| Dead | Until respawn |

## Art for each unit

Each unit's `look` (`protocol::Look`, protocol version 2) names everything in the art
catalogue (docs/ASSETS.md):
- `art`: a body type in `assets/catalog/humanoid.json` (`"male"`, `"elf_female"`,
  `"goblin_male"`, `"skeleton"`…) or a beast in `assets/catalog/creatures.json`
  (`"wolf.grey"`).
- `tone`, `hair`, `hair_colour`, `brows`, `beard`: ids from the catalogue. `None` means none
  (hair and beard) or the body's default (tone and hair colour).
- `pieces`: clothing and armour ids. Their layers decide what covers what.
- `gear`: (slot, item id) pairs from `assets/catalog/gear.json`.
- `drawn`: weapons on their `held` sockets (true), or on their `stowed` sockets, chosen by
  the back item's `back_kind`. When it changes, play the main-hand item's `clips.ready` or
  `clips.stow` over the legs; the item changes hands at that clip's `grab` or `stow` event
  (docs/ASSETS.md, "Clips, curves and events").

Today the player is a farmhand with an arming sword and a round shield. Goblins wear the Ranger
jerkin with a hunting knife and round shield, and wolves are `wolf.grey`. `drawn` turns on
during a fight. Looks are sent every tick for now; the network version will send them only
when they change. `tools/workshop/` assembles a look from these names exactly as the game
should (docs/ASSETS.md, "The reference implementation").

Humanoid clips by pose, from `anims.glb`:

| Pose | Clip |
|---|---|
| Idle | `Idle_Loop`; with weapons drawn, the main-hand item's `clips.idle` (`Sword_Idle`, `Bow_Idle_Loop`, `Staff_Idle_Loop`, `Spear_Idle_Loop`, `GreatSword_Idle_Loop`); a piece with an `idle` (the robes' `Idle_Robe_Loop`) replaces `Idle_Loop` |
| Walk | `Walk_Loop`; with weapons drawn, the item's idle on the upper body over it |
| Run | `Jog_Fwd_Loop`; `Sprint_Loop` above 6 m/s (the same layering when drawn) |
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

## Input the client sends

- **WASD:** a world-space direction from the camera's yaw, sent as `Move` whenever it changes.
- **Holding the right mouse button:** also sends `face_yaw` (mouse-look strafing).
- **Space:** `Jump`.
- **Dodge key:** `Dodge` (a zero direction rolls backwards).
- **Tab or clicking a unit:** `SetTarget`. Right-clicking an enemy: `SetTarget` plus
  `SetAutoAttack(true)`.
- **1 and 2:** `UseSlot` (Strike and Sweep).
- **When defeated:** `Respawn`.

## Budgets on the M1 Air at 5120×1440

- **Simulation:** 0.36 ms a step with 2,000 monsters (`cargo run -p sim --release --example bench
  2000 dense`). It runs on its own thread, so the main thread is free for the renderer.
- **Target:** a steady 60 fps, with the GPU under 12 ms.
