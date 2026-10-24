# Aldermoor in Rust: rebuild plan

Agreed with Tariq on 23 October 2026. This replaces the two Swift codebases (`~/code/aldermoor` on
branch `v2`, and `~/code/aldermoor-wild`). Both stay frozen as reference.

## Why Rust

- **Predictable speed.** No hidden reference counting or allocations, and safe multithreading for
  the simulation.
- **Data-oriented design.** An entity-component system (ECS) handles thousands of NPCs and monsters
  efficiently.
- **A portable server.** The same simulation runs as a server on Mac or cheap Linux machines when
  multiplayer arrives.

**What Rust won't do on its own:** at 5120×1440 on the M1 Air, the GPU is the main limit, so
rendering technique still decides the frame rate. That's why the engine talks to Metal directly
(decision 2).

## Decisions

1. **Rust everywhere** (engine, game, server, AI), with a small Swift add-on only where Apple
   offers no other interface, mainly the on-device language model. Metal, MetalFX, game
   controllers, AVFoundation and VideoToolbox are reached from Rust through the `objc2` crates.
2. **Our own engine on Metal**, not Bevy or another engine, so we can use MetalFX upscaling, Apple
   GPU tile memory and Metal 4 features. Proven crates where they fit: `bevy_ecs` as a standalone
   ECS, Rapier for physics, `gltf` for models.
3. **Platforms:** the game client runs on Apple-silicon Macs. The server builds for Mac and Linux.
4. **A fresh repo** (`aldermoor-rs`), ported by feature, not line by line. The Swift game's 169
   simulation checks become the acceptance tests for the Rust simulation.
5. **The new character design**, not a port of the four classes and talent trees. The whole
   design interview is settled; see [DESIGN.md](DESIGN.md). In short:
   - Races (humans, elves, dwarves, foxfolk), with every race open to every specialty.
   - Specialties (warrior, archer, mage, necromancer, rogue, healer) level through use,
     RuneScape-style, with hybrids and loadouts.
   - Visible, correctly fitted gear with dyes; factions and guilds.
   - Solo and offline first.
6. **Prove it first.** Milestone 1 is a slice that justifies the switch. It is measured against the
   Swift build.
7. **Split of work:** one repo.
   - Claude: simulation, gameplay, AI, server, and since 24 October **the art itself**: characters,
     gear, creatures, animation and the asset pipeline. See [ASSETS.md](ASSETS.md).
   - Codex: renderer and shaders. It reads the asset contract in ASSETS.md and draws what it
     describes. It doesn't design or create assets (Tariq, 24 October): models, textures,
     materials, animations, effects, terrain, water and sky materials, scenery placement, HUD
     art, icons and look settings all come from Claude, and so does texture compression.
   - Shared: the snapshot contract between simulation and renderer, plus automated checks (tests
     and a frame-time benchmark).
8. **Disk:** at least 30–40 GB free before heavy building starts. Rust build folders grow to tens
   of GB, and the Mac had 12 GB free on 23 October.

## Defaults chosen without asking (change any)

| Area | Default | Why |
|---|---|---|
| Window and events | `winit` | Mature; we keep raw access to the `CAMetalLayer` |
| HUD and menus | `egui`, drawn by our own Metal painter | Fast to build, easy to theme. Revisit for final polish |
| Maths | `glam` | SIMD-friendly, the Rust standard |
| Simulation timing | Fixed 60 Hz steps, seeded RNG, no wall clock | Same rules as the Swift simulation; replays and multiplayer depend on it |
| Content data | RON files, checked at load | Readable, typed, diffable |
| Networking (later) | `quinn` (QUIC) with `postcard` messages | Encrypted, fast, works over the internet |
| AI | `tokio` and `reqwest`; the `claude` CLI on the subscription with Haiku; Jev via the AI Gateway; on-device model through the Swift add-on | Ports the Swift AI layer, including the Haiku defaults |
| Audio | `kira` first; PHASE through the Swift add-on if we need Apple's 3D audio | `kira` covers mixing and streaming today |
| Models and textures | glTF in, packed into our own format at build time; ASTC textures | ASTC is what Apple GPUs read natively |

## Crates

| Crate | What it holds | Owner |
|---|---|---|
| `sim` | The deterministic world: ECS, movement, combat, specialties, gear, quests, NPC behaviour. No I/O | Claude |
| `content` | Data tables for races, specialties, abilities, items, monsters, quests and dialogue, plus checks | Claude |
| `protocol` | Commands and snapshots between server and client | Shared |
| `server` | Runs `sim` at 60 Hz on its own thread; saves; dispatches AI requests | Claude |
| `ai` | Providers, router, budget and features: talking NPCs, persuasion, moods, companions, events, quest judging | Claude |
| `render` | Metal renderer: frame graph, lighting, shadows, water, foliage, characters, MetalFX or our own temporal anti-aliasing (TAA) | Codex |
| `assets` | Build-time tools: glTF to engine format, texture compression, catalogue checks. The Blender pipeline itself is `art/blender/` | Claude |
| `client` | The app: window, input, camera, interpolation, HUD, audio | Shared |
| `apple` | Swift add-on built as a static library behind a C interface: on-device model, and later Game Center | Claude |

## Milestones

1. **Performance slice.** The Rust simulation at 60 Hz, and the Metal renderer drawing the
   Brambleford valley with the rebuilt player. WASD movement, the camera and one fight.
   - **Targets:** a steady 60 fps at 5120×1440 on the M1 Air, GPU under 12 ms, simulation under
     2 ms per step with 2,000 active units.
   - **Comparison:** recorded against the Swift build.
2. **Combat core.** Abilities, auras, projectiles, area attacks, threat, crowd control with
   diminishing returns, defiance bars, downed state and revives. The Swift checks are the bar to
   pass.
   - **Built 24 October 2026:** see [COMBAT.md](COMBAT.md), with guarding, parrying, aimed shots
     and the duel checks for the balance target. The combat tables live in `sim::content` for
     now, and move to the `content` crate when Cargo.lock is free to change.
3. **Characters.** Races, customisation, specialties and their training, gear and loadouts, and
   correctly fitted equipment.
4. **World and story.** The Brambleford zone, quests, dialogue, NPC routines, day and night,
   weather, and world events.
5. **AI layer.** Talking NPCs, persuasion, moods, quest judging and event writing, on Haiku through
   the subscription, Jev through the AI Gateway, and the on-device model.
6. **Party and danger.** Companions with orders, banter, tactics and approval; factions; bosses and
   dungeons.
7. **Sound and polish.** Audio, instant replay, Game Mode and DualSense extras.
8. **Multiplayer.** Co-op first, then the shared world.

## Carried over from the Swift work

- The design documents: `aldermoor/docs/DESIGN-v2.md` and `aldermoor/docs/AI-FEATURES.md`.
- The AI prompts, guards and fallbacks.
- The art: the Blender sources, the rebuilt player (`aldermoor-wild/Art/CharacterRebuild`) and its
  credits (CC BY 4.0 for Blender Studio's Rain).
- The behaviour the Swift tests pin down: combat numbers, quest flows, events, AI fallbacks.

## Still to decide

- **Talent trees (settled 24 October):** replaced by specialty perks, with combat where player
  skill decides close fights. See DESIGN.md, "Progression and player skill".
- **The world:** settled as deliberately designed regions (DESIGN.md), starting with the
  Brambleford valley. The generated continent is retired.
