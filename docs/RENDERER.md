# Native renderer and protocol-v4 client

The renderer consumes [ASSETS.md](ASSETS.md), [CONTRACT.md](CONTRACT.md) and the workshop
reference behaviour. Ownership follows PLAN decision 7: renderer/client/scripts here; all art,
placements, texture compression and look values come from Claude. Protocol is currently v4:
`Look.dyes` and `Look.variants` are connected. Foxfolk clothing/armour fits exist and are covered
by the asset crate's checks.

## Run and controls

```sh
scripts/run_client.sh
scripts/run_client.sh --fullscreen --size 5120x1440 --scale 0.65
```

Controls default to always-on action aiming (`controls::Scheme::Action`, like New World): the
pointer is locked and hidden, the mouse always turns the character, and a crosshair marks the
centre of the screen, where clicks target and aimed skills go. WASD runs and strafes relative to
it, and S backpedals. Hold Ctrl to get the pointer back for the HUD. `--scheme classic` is
WoW-style instead: W runs, S backpedals, A/D turn the character and camera, Q/E strafe (A/D strafe
too while the right button is held), left-drag looks around without turning, right-drag steers and
both buttons run forward. Drags there hide and hold the pointer; a press that moves under 4 px is a
click. In both, T or Num Lock toggles autorun. Shift walks, Space jumps, and
Alt (Option), V or mouse button 4 rolls the way the keys point. Action looks over the right shoulder. Tab cycles enemies; click selects, right-click starts auto-attacks.
1–9 and 0 send slots 0–9 with a point raycast onto `Valley::ground` under the cursor
(screen centre while mouse-looking). Hovering/selecting a ground skill previews its actual radius.
Hold F (or mouse button 5) to guard; ` swaps weapon sets; G revives a selected or nearby downed/defeated friend;
R respawns; Esc opens the controls menu. The camera eases: the wheel zoom glides, the eye snaps in front of walls and slides back out at
4 m/s, and the pivot follows slopes and jumps smoothly. Pitch goes from just below the horizon
(looking up from near the ground) to nearly straight down. In Classic the camera swings back
behind the character after a second of moving without the mouse.

Control settings live in `~/Library/Application Support/Aldermoor/controls.json`, or the file
given with `--controls FILE.json`. Every field is optional and a missing file means the defaults.
`--scheme` overrides the file's scheme.

```json
{ "scheme": "action", "sensitivity": 1.0, "invert_y": false, "fov_degrees": 52,
  "min_distance": 2, "max_distance": 24, "shoulder": 0.6, "swing_back": true }
```

Unknown fields are an error so typos show up; values outside sensible ranges are clamped.
`shoulder` left out means 0.6 m in Action and centred in Classic.

Every key is rebindable (`controls::Bindings`). The file's `bindings` maps a control to the inputs
for it, by winit key name (`KeyW`, `ShiftLeft`, `Digit1`, `Space`) or `Mouse3`..`Mouse5` for the
middle and thumb buttons. Controls left out keep their defaults:

```json
{ "bindings": { "jump": ["Space", "Mouse4"], "roll": ["AltLeft", "KeyV"] } }
```

The left and right mouse buttons stay fixed, because they click, look and steer. Esc opens the
controls menu in game. It has the scheme switch, every control with its keys (click a key to
unbind it, + then press a key or mouse button to add one), "Reset keys to defaults" and Quit.
Binding a key that another control already uses moves it over and says so; conflicts written
into the file by hand are listed in red. Every change saves the whole file straight away,
including a scheme given with `--scheme`.
The stock-widget HUD shows all ten named slots, costs, cooldowns and GCD
sweeps, health/endurance/resource, casts, auras, weapon-set recharge and target cast/defiance bars.
The HUD is composed after temporal AA at output resolution. Claude will supply its artwork,
icons, fonts and style. `--hud-font FILE` can load the supplied font without changing code.

```sh
# Asset review, independent of desktop capture permissions.
scripts/run_client.sh --offscreen --showcase --frames 180 \
  --size 1800x1100 --scale 0.8 --capture target/render-review/catalogue.png

# Close-up review: body IDs, e.g. male, female, elf_female, dwarf_male, goblin_male,
# or a creature ID, e.g. wolf.grey.
scripts/run_client.sh --offscreen --inspect wolf.grey --frames 120 \
  --size 1400x1100 --capture target/render-review/wolf.png

# Live host integration: movement, jump, dodge and combat events are required to pass.
scripts/run_client.sh --offscreen --smoke --frames 660 \
  --capture target/render-review/combat.png
```

`--assets PATH` selects a catalogue root. Its `catalog/look.json` and `catalog/surfaces.json`
load automatically when present; `--look-settings` and `--surface-assets` override them. Explicit
missing/invalid files are errors. Missing optional catalogues retain neutral engine defaults.
Review animation advances at 60 Hz; normal play uses
snapshot pose and pose time. Fullscreen selects the monitor closest to `--size`; macOS may
otherwise use the laptop panel even with an ultrawide attached. Capture reads our output texture.

## Rendering and asset behaviour

- Direct Metal, HDR single-sample scene and motion vectors; TAA with camera, skin, morph and
  wind motion, depth rejection and YCoCg variance clipping. Motion uses unjittered matrices;
  current colour is unjittered in resolve and history uses nine-tap Catmull–Rom reconstruction.
  Camera cuts and resizing reset history; an individual appearance/weapon change does not. `--aa none` and `--aa msaa` are diagnostic comparisons; TAA is
  the default. Internal resolution is `--scale` times the output dimensions.
- Scene and cascade frustum culling, 64 m terrain chunks, screen-height whole-file LODs using
  `assets::lod_path`: full above 300 output pixels, LOD 1 above 100, otherwise LOD 2, with 10%
  hysteresis. Missing optional LOD files fall back to the full file. Creature textures and
  animation clips swap with the entire creature file; models are preloaded before play. Catalogue
  extension JSON is cached once per asset cache. Each character caches its visited LOD assemblies
  and swaps their geometry/clip data while preserving the live rig, blends, spring particles,
  equipment transitions and combat playback state.
- Three 1024² shadow cascades, 15/50/150 m splits, quantised extents and light-space texel
  snapping. Alpha masks and hidden body regions apply in shadow rendering too. Transparent
  surfaces do not cast solid shadows. Opaque geometry sorts front-to-back; blended geometry
  sorts back-to-front and does not write depth.
- GLB/glTF geometry, vertex colours, colour/normal/metallic-roughness/occlusion/emissive maps,
  alpha modes and `KHR_materials_emissive_strength`. The golem's emissive maps/strength and
  separate glowing meshes are consumed unchanged. Double-sided normal orientation is handled.
- Each skin uses its own inverse binds, matched to body bones by name; extra cape bones attach
  to their named parents. Region coverage, layered outfits, sockets, held/stowed weapons,
  race scaling, bow morphs, reference clips, draw/stow events, IK and cape chains are retained.
  Beasts start with idle applied before their first draw, including every wolf LOD; no open-jaw
  bind-pose frame is displayed.
- Material-name tinting preserves untinted materials, including brass beard rings. Dyes follow
  the workshop's linear-space mask rule, independently for R cloth and G trim:
  `out = mix(out, base * dye / max(reference, 0.01), mask)`. Variant maps replace only registered
  outfit colourways. Capes use their own dye channels; masks/maps are shared across instances.
- Scenery comes only from `environment.json` placements: world house/tree transforms mapped to
  the supplied families, the supplied bridge, landmarks and dressing. `_WIND` uses the reference
  sway formula; `parts.spin` animates sails around the supplied pivot/axis/RPM. No decorative
  scatter is generated. Building footprints/heights provide expanded camera collision boxes.
- Terrain, river, sky and lighting use Claude’s supplied catalogues by default. Terrain layers
  are triplanar, weighted by the geometric normal, with the supplied horizontal layer-control
  map retaining road/river placement. The side projections stand upright (u along the face, v
  down it), so a layer's rows lie level on cliffs. Normal-map slopes are blended in world
  space; flat maps preserve the geometric normal. Texture gradients remain explicit when unused
  projections are skipped. Derivative tangent frames use the requested negative-v bitangent for image-top
  green normals; explicitly supplied mesh tangents retain their authored handedness.
- Shared GPU textures and numeric mesh identities reduce duplicate uploads. Decoded image bytes
  are released after upload. Native ASTC KTX2 loading is implemented; actual compressed asset
  validation awaits Claude's build. PNG/JPEG, including embedded GLB images, remain the fallback.
- Repeated opaque rigid meshes are instanced in both scene and shadow passes; each instance
  retains current/previous transforms and its supplied material. Transparent objects remain
  individually sorted.
- Two frames in flight is the measured client default. `--in-flight 1..3` is diagnostic.
  Shared render targets/history are ordered on one Metal command queue; CPU bone/draw/HUD buffers
  are fenced before reuse. Skin palettes are shared across a skin's primitives. Bone and draw
  rings grow to the frame's requirement, including packed instance constants, instead of imposing
  a fixed 4 MB ceiling. The 60-armoured-character test submits real Metal draws for three frames.
- Composite and HUD rendering target the drawable directly. Captures/offscreen rendering keep
  the output texture; `--output-blit` retains the old path for comparisons.
- Springs and arm IK update only the affected node and descendants. A full solve once per
  animation pose is retained. Character membership and snapshot interpolation use ID maps/sets.
- `--fps 60` paces the client. `--benchmark --in-flight 1` fences every completed frame.
  With two or three in flight, CPU encoding overlaps GPU execution: the reported FRAME value is
  **CPU encoding plus slot wait**, not end-to-end latency. GPU timings are from completed command
  buffers; PRESENT values are native drawable presentation times. Never compare the two FRAME
  definitions as a latency improvement.
- `--aa metalfx` enables `MTLFXTemporalScaler` when supported, with custom TAA fallback on
  unsupported devices/allocation failure. Its HDR output replaces both custom resolve and the
  internal-resolution bilinear upscale. It is opt-in because it missed the M1 GPU budget badly.
  The benchmark refuses to label a fallback run as MetalFX. Input usage flags come from the scaler;
  our current-minus-previous UV vectors are scaled by negative input dimensions, and jitter is
  supplied separately in pixel space, with 19 jitter phases at 65% and a negative texture mip bias. See [Apple's motion-vector convention](https://developer.apple.com/documentation/metalfx/mtlfxtemporalscalerbase/motionvectorscalex).
- Materials receive sky-driven ambient specular. A supplied sky is GGX-prefiltered once into
  roughness mips, with an analytic split-sum BRDF approximation. Without a sky texture the term
  uses the supplied sky/ambient values as a sky/ground hemisphere. Metal no longer receives the
  incorrect diffuse-only ambient term. This derives lighting data; it creates no art assets.

## Inputs for Claude's materials and look

`--look-settings FILE` reads JSON with optional fields and rejects unknown fields/non-finite
values. Fields are `sun_direction` (direction toward the sun), `sun_colour`, `sun_intensity`,
`ambient`, `sky`, `fog_colour`, `fog_density`, `exposure`, `grade` (three RGB rows), `tone_map`
(`linear` or `aces`), and `wind_strength`. Defaults are neutral: white unit sun straight overhead,
grey ambient/sky, no fog, unit exposure, identity grade, linear output; wind strength 1 matches
workshop sway. These are engine defaults, not an authored visual preset. Colour values are linear.

`--surface-assets FILE` reads a JSON object with optional `terrain`, `water` and `sky` members.
Paths are relative to `--assets`. Omitted members retain neutral placeholders. No sample art or
look preset is generated here. Data definitions live in `crates/render/src/surfaces.rs`:

| Member | Required supplied fields |
|---|---|
| terrain | `layers` (1–4 entries: `colour`, `normal`, `uv_metres`, `roughness`), `weights` map (RGBA channels in layer order), `bounds` `[min_x,min_z,max_x,max_z]` |
| water | `normal`, `uv_metres`, `normal_strength`, `flow` (world UV drift in metres/second), `deep_colour`, `shallow_colour`, `absorption` (RGB per metre), `roughness`, `opacity`, `reflection_strength`, `ior` |
| sky | Equirectangular `texture`, `rotation_radians`, `intensity` |

Terrain layer arrays must share dimensions, format and mip count within each colour/normal
array. Weights/normal maps are linear. Colour and sky textures are sRGB. Implemented features
are triplanar normal/colour/roughness layer blending, scrolling water normals, depth absorption,
Fresnel sky reflection and an equirectangular sky. Claude’s delivered inputs now exercise all
three surface types and the sky radiance prefilter. Foam, refractive scene sampling and atmosphere art are not fabricated here.

## Earlier baseline measurements (24 October 2026)

```sh
cargo test -p render -p client
cargo test -p assets
cargo clippy -p render -p client --all-targets --no-deps -- -D warnings
```

Asset-backed tests cover bind matrices, body-region coverage, event timing, beasts/whole-file
LODs, textured wolves, dye references, shared cape masks, untinted beard brass, variant scope,
wind data and exact placement counts. Unit tests cover frusta, LOD hysteresis, stable cascade
snapping, KTX2 bounds/layout and camera collision. The smoke run requires authoritative movement,
jump, dodge and damage events. Metal errors are checked at frame-slot reuse, capture and shutdown.

The earlier six-character scene with supplied scenery and HUD completed 90 frames with Metal
API validation enabled at 1800×1100 output / 1440×880 internal, without GPU validation errors.
The renderer/client suites pass 19 checks; the separate asset suite passes all 10 checks,
including foxfolk fits. Scoped Clippy passes with warnings denied. Clippy without `--no-deps`
currently encounters existing lints in Claude-owned `world` and `assets`; those files are
unchanged by this work.
The final release smoke run completed 780 frames: 49.41 m of authoritative movement, jump,
dodge and damage events, and a wolf kill. Local captures are
`target/render-review/catalogue-instanced.png` and `target/render-review/combat-current.png`.

Budgets and the reference setting (the Air's own panel at 75% render scale) are in
[PERFORMANCE.md](PERFORMANCE.md); the measurements below predate that choice and use the ultrawide.

Benchmark method:

```sh
cargo build --release -p client
python3 scripts/benchmark_renderer.py --windowed --fullscreen --scale 0.65 \
  --frames 3720 --warmup 120 --output target/render-review/fullscreen.json
# Without desktop access, GPU/work timing only:
python3 scripts/benchmark_renderer.py --scale 0.65 --frames 7320 --warmup 120 \
  --output target/render-review/offscreen.json
```

The harness verifies actual output size/AA mode, reports actual internal size, GPU median/p95,
completed CPU+GPU frame-work median/p95 and frame intervals. A windowed pass additionally needs
actual drawable presentation samples, mean at least 59.5 fps and p95 at most 17.2 ms. The
completed-work budgets are GPU p95 ≤12 ms and frame-work p95 ≤16.667 ms. Offscreen timing can
pass the work budgets but cannot prove steady display presentation or sustained thermal behaviour.

Release, Apple M1 (7 GPU cores, 16 GB), TAA, one frame in flight, three 1024² cascades. The scene
includes the six-character catalogue fixture, Claude's actual scenery, dyes and the HUD; about
398k visible scene triangles. No other applications were stopped or system settings changed.

| Output | Internal scene | GPU median / p95 | Frame-work median / p95 | Measured frames | Work budgets |
|---|---|---:|---:|---:|---|
| 5120×1440 | 3584×1008 (70%), before instancing | 11.849 / 13.137 ms | 16.585 / 18.195 ms | 599 | Fail |
| 5120×1440 | 3584×1008 (70%), instanced | 11.186 / 11.525 ms | 15.396 / 16.813 ms | 599 | GPU pass; frame fail |
| 5120×1440 | 3328×936 (65%), instanced | **10.491 / 11.639 ms** | **14.751 / 16.262 ms** | **7,199** | **Pass** |

The 65% run lasted about 122 seconds, discarding the first 120 frames. Completed-frame interval
mean was 59.58 fps, interval p95 18.717 ms; those intervals include client pacing/scheduling and
are distinct from frame work. Draw calls fell from 1,406 to 973 in the ultrawide scene while
visible scene triangles stayed unchanged. Raw local report:
`target/render-review/catalogue-instanced-65-long.json`. Use 65% as the measured ultrawide
setting pending compressed assets; the normal small-window default remains 80%.

**Earlier handoff: display acceptance remained open:** the Mac was locked during that check; native
presentation times and physical input validation were unavailable. These offscreen results
meet the work budgets but do not establish steady onscreen 60 fps or longer thermal behaviour.
The Odyssey G95SC is connected at 5120×1440/120 Hz. A previous fullscreen attempt used the
laptop panel (2880×1800 backing size); it is rejected by the dimension guard, and monitor
selection is now explicit.

Historical comparison, before supplied scenery/HUD: 4x MSAA at 5120×1440 / 4096×1152 internal
measured 12.63 ms GPU median and 13.52 ms p95. This is not the current scene or a final budget pass.

## Protocol v4 integration (25 October 2026)

Each frame reads `host.combat()` alongside the snapshot; only matching tick pairs replace the
client's current pair. Combat units are indexed by ID. The Action-to-clip table is applied with
upper-body guard/parry (respecting the supplied loop flag and combat clock), horizontal-speed sprint selection, held Death01 for downed and LayToIdle
on rally/revive. Heavy swings retain their cast wind-up across the release and remap the clip
so its `hit` marker lands on that boundary, then recover over 0.5 s. Every attack clip carries
`hit` markers (vendor clips included), and Overhead plays `Sword_Overhead`, the heavy combo's
last blow cut on its own, so no clip needs a playback range of its own.
`cargo run -p render --example inspect_strikes -- <clip>` prints a clip's markers and its
blade tip's path, to check the markers against the renderer's own skeleton solve.

Temporary combat overlays project the sim's live projectile positions/velocity and circle,
cone and yaw-aligned line areas, with warning progress, hostile red and friendly blue, field
labels, cast/defiance bars, auras, guard and downed status. They use stock lines, fills and text,
not substitute art. These diagnostic overlays currently draw above scene occluders; supplied
world-space effect/decal assets should replace them. Clip events never spawn duplicate missiles.
Damage/Heal/Avoided events produce bounded, expiring floating text with critical/periodic labels.

Protocol v5's Slot carries a ground skill's `radius` (0 for every other skill), which the
local aim preview draws, so a remote client needs nothing beyond the wire. Guard release is
sent even if egui consumes the release, and on focus loss. Revive only selects a nearby friendly downed/defeated unit.

## Current validation and comparison method

```sh
cargo test -p render -p client -p server --locked
cargo clippy -p render -p client --all-targets --no-deps --locked -- -D warnings
cargo build --release -p client --locked
python3 scripts/benchmark_renderer.py --windowed --fullscreen --scale 0.65 \
  --in-flight 2 --frames 3720 --warmup 120 --output target/render-review/v4-display-long.json
```

The Metal tests require actual GPU access (the desktop sandbox hides the device). Tests include
60 full-detail armoured characters, growing rings, shared palettes, LOD state preservation,
action mappings, partial-skeleton/full-solve equality, horizontal sprint threshold, ground-ray
hits, area yaw/fill, all avoidance labels, HUD clicks with ground targets and zero static-scene
motion across Halton jitter. The neutral sky fallback also exercises every prefilter mip on GPU.
The final held-guard regression also passes; non-looping guards hold across locomotion clock changes.
The new MetalFX dependency and root Cargo.lock are committed together.

Diagnostic comparisons are explicit: `--legacy-cpu` restores full-skeleton turns and linear unit
lookups; `--look-churn` swaps the first showcase character's weapon set every 60 frames;
`--reset-on-look` restores the previous global-history-reset behaviour for that comparison.
`--live` on the benchmark harness selects the live host scene (including snapshot interpolation).
These flags are not gameplay defaults. The harness requires presentation samples for at least
95% of measured frames; offscreen results never establish display acceptance.

### Results

The 29 renderer/client/server checks passed (client 7, renderer unit 9, renderer asset-backed
12, server 1), including the real 60-character GPU stress test, with Metal API validation.
Scoped Clippy and the locked release build passed. After the final MetalFX shader correction,
the static-motion/prefilter GPU regression passed again, and both final 480-frame AA capture
runs completed with Metal API validation. The live smoke completed 780 frames, 50.01 m of
authoritative movement and 80 events; it required jump, dodge, damage and weapon swap, and
exercised the 0.4 s heavy wind-up. Captures show all ten HUD slot positions (empty slots remain
empty). Physical keyboard/mouse interaction and authored effect/lighting appearance are not
claimed as human acceptance.

All following runs use 5120×1440 output / 3328×936 internal (65%) on the M1 7-core GPU, 16 GB.
Raw reports and conditions are committed under [benchmarks/2026-10-25](benchmarks/2026-10-25/README.md).
Short runs discard 120 of 480 frames; the sustained display run discards 120 of 3720.

| Comparison | GPU median / p95 (ms) | FRAME median / p95 (ms) | Native presentation |
|---|---:|---:|---|
| Custom TAA, final, offscreen, 1 in flight | 11.139 / 11.921 | 15.911 / 16.827 completed work | Not measured |
| MetalFX temporal, final, offscreen, 1 in flight | 22.717 / 24.233 | 28.062 / 30.279 completed work | Not measured |
| Custom TAA, drawable direct, 1 in flight | 12.305 / 13.323 | 16.663 / 17.953 completed work | 58.443 fps; p95 16.668 ms |
| Custom TAA, drawable direct, 2 in flight | 11.857 / 12.587 | 4.296 / 8.820 CPU + slot wait | 59.910 fps; p95 16.668 ms |
| Custom TAA, output + blit, 2 in flight | 16.430 / 28.087 | 13.435 / 24.296 CPU + slot wait | 31.231 fps; p95 33.337 ms |
| **Sustained G95SC, custom TAA, direct, 2 in flight** | **36.251 / 43.944** | **24.062 / 25.127 CPU + slot wait** | **45.678 fps; p95 33.337 ms — FAIL** |

The G95SC was unlocked and selected at its actual 5120×1440/120 Hz mode. The sustained run
recorded 3,596 native presentation intervals; both work and presentation budgets failed.
The short two-frame run passed the presentation threshold but missed the 12 ms GPU-p95
budget. Two frames in flight remains the client default on that short A/B evidence; it is
not a claim that the sustained 60 fps target is met. Direct drawable composition removes a
copy, but variable system load means the whole measured blit difference cannot be attributed
to that copy. Later aggregate system sampling showed zero idle CPU and a 20.29 five-minute
load average; `pmset` reported no recorded thermal warning. No other applications/settings
were changed, and this evidence does not establish thermal throttling or isolate its cause.

Final matched capture crops (`target/render-review/v4-aa-comparison.png`, custom TAA above,
MetalFX below) show sharper fences, roofs and foliage with MetalFX. Its corrected pixel-space
jitter removes the checker pattern seen in the first integration. Its GPU median is about
2.04× custom TAA here, so custom TAA remains default and MetalFX is opt-in with fallback.
Both cost runs disable API validation; validation captures are separate reports. The custom
path narrowly misses the completed-frame p95 budget despite passing the GPU budget.

| CPU/history diagnostic, 2 in flight, offscreen | GPU median / p95 (ms) | CPU + slot wait median / p95 (ms) | History resets |
|---|---:|---:|---:|
| Live, indexed lookups + affected-chain updates | 15.807 / 25.985 | 3.896 / 6.346 | 1 |
| Live, linear lookups + whole-skeleton turns | 13.383 / 21.415 | 3.667 / 4.496 | 1 |
| Weapon swaps, retain object history | 20.301 / 27.502 | 8.789 / 19.529 | 1 |
| Weapon swaps, restore old global reset | 21.485 / 29.353 | 11.921 / 25.625 | 8 |

The small live scene does **not** demonstrate a CPU speedup; the structural scaling change is
backed by equivalence checks, including 199 reordered/missing units and affected-chain matrices.
The weapon-swap comparison verifies that swaps no longer reset the entire history; its timing
is also subject to system load. Missing vendor strike markers and supplied surface/effect art
remain explicit integration requests below.

Those earlier measurements preceded the supplied terrain/river/sky/look files. The integration
below supersedes the earlier neutral-surface captures; no replacement assets were made.

## Supplied valley integration (25 October 2026)

Claude’s `b843117` surface/look commit and `7d27043` material-packing follow-up were available
on the local `look` branch, while fetched `origin/main` still ended at `c40f44e`. They were merged
into this checkout without modifying the supplied assets. All three “Asks of the renderer” in
ASSETS.md are implemented: optional catalogue defaults with explicit flag precedence, triplanar
terrain, and the negative-v derivative bitangent. The renderer does not choose new look values.

Validation: 40 checks pass (assets 11, client 8, render unit 9, render asset-backed 12), including
the 60-armoured-character GPU draw after the material-packing change. The new default-selection
check covers both catalogues, explicit overrides and absent catalogues. The 180-frame client
capture at 5120×1440/65% loads both default files and completes with Metal API validation.
Scoped Clippy passes. Review captures are local in `target/render-review/supplied-surfaces.png`;
`cargo run --release -p render --example review_surfaces --locked` also captures the river and
limestone ledge from fixed cameras using only supplied scenery/materials.

The final shader skips exactly-zero material weights as well as negligible axis projections.
A second 180-frame Metal-validated capture (`supplied-surfaces-final.png`) has pixel-identical
static ground and limestone regions to the first, while avoiding unused layer reads. The fixed
river/ledge camera reviews and a 60-frame MetalFX capture at 1800×1100/65% also completed
with Metal API validation. The locked release build
passes; source assets and look values remain unchanged.

| Supplied scene, 5120×1440/65%, custom TAA, 1 in flight | GPU median / p95 | Completed work median / p95 |
|---|---:|---:|
| Before zero-weight layer skip | 20.073 / 22.120 ms | 25.373 / 28.348 ms |
| Final | **16.500 / 18.378 ms** | **21.482 / 23.747 ms** |

Both runs use 480 frames / 120 warmup, offscreen with API validation disabled. The final run
misses the 12 ms GPU and 16.667 ms completed-work p95 budgets; it is not display acceptance.
These measurements include the new material-packed kit, terrain, river, sky and lighting, so
they cannot isolate triplanar cost against the earlier neutral scene. Sequential runs still
have system-load noise. Raw reports are `supplied-surfaces-before-skip.json` and
`supplied-surfaces-final.json` under [benchmarks/2026-10-25](benchmarks/2026-10-25/README.md).

## Combat effects pass (25 October 2026)

`crate::fx` is what the client hands over each frame (`Renderer::effects`): quads (camera-facing
sprites, beams between two points, planes), decals and effect-mesh instances, all unlit with
premultiplied alpha. The art is uploaded once with `Renderer::load_effects`: the sprite atlas,
the tiles as an array texture, colour ramps (32-bit float, read and blended by hand) and the
effect meshes. Shaders are `crates/render/src/fx.metal`, compiled as their own library; the
Metal side is `crates/render/src/metal/fx_pass.rs`. Neither part adds a render pass:

- **Decals** are drawn at the end of the scene pass as back faces of their boxes with depth
  testing off. Each pixel reads the scene's linear depth from the motion target's w by
  programmable blending, rebuilds its world position, and keeps only surfaces facing up the
  box, so marks lie on slopes and steps but not on the legs of whoever stands in them. They are
  jittered like the scene and resolved by temporal AA with it, and leave motion vectors alone.
- **Quads and meshes** are drawn at the end of the composite pass, straight onto the output
  after exposure, grade and ACES (each tone-mapped as it's drawn), with soft-particle fades
  against the scene depth. They never enter TAA history, so fast sparks and trails aren't
  smeared or dimmed. Meshes go first, then quads farthest first, in one instanced draw.

The MSAA comparison mode draws no effects (they need single-sample depth). In the staged fight
(`--effects-review`), at 2560×1600 output / 1920×1200 internal on the M1, effects added
1.29 ms GPU median (11.21 → 12.50 ms) and 1.13 ms p95, against about 25 ms for #49's lit
effects; two ABAB runs of 480 frames, one in flight, offscreen.

## Asset requests

All art and look decisions belong to Claude. The renderer will consume supplied data; it will
not generate substitute scenery, textures, scatter, effects, icons or visual presets.

- **Compressed textures:** KTX2 with native ASTC 6×6 blocks, complete offline mip chains,
  no Basis/UASTC payload and no supercompression. Use sRGB for colour/emissive maps and linear
  UNORM for normal/ORM/dye/control maps. External glTF images may have a same-stem `.ktx2`
  sibling. For embedded GLB images, please supply an asset mapping from `file.glb#imageN` to
  the corresponding KTX2 path. Existing image decoding remains the fallback until delivered.
  Mapping format: `catalog/textures.json`, `{"textures":{"creatures/wolf.grey.glb#image0":"packed/wolf-grey-colour.ktx2"}}`.
  Map source paths relative to the asset root, use orientation `rd`, and provide full mip chains.
- **Delivered terrain, river, sky and look:** `catalog/surfaces.json` and `catalog/look.json`
  are consumed unchanged. Any later foam or scene-refraction work still needs supplied masks
  and look parameters; current water uses the delivered normal/absorption/Fresnel inputs.
- **Dressing/scatter:** explicit placements for rocks, grass, flowers, bushes and reeds. The
  catalogue currently names candidates in `scatter`; the renderer will not choose locations.
- **HUD:** frame/bar art, ability icons, fonts and layout/style values. Functional controls
  use plain temporary styling until supplied.
- **Shout:** a dedicated roar/shout clip, as already listed in CONTRACT.md.
