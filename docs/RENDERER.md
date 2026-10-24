# Native renderer: milestone 1

The renderer consumes [ASSETS.md](ASSETS.md), [CONTRACT.md](CONTRACT.md) and the workshop
reference behaviour. Ownership follows PLAN decision 7: renderer/client/scripts here; all art,
placements, texture compression and look values come from Claude. Protocol is currently v3:
`Look.dyes` and `Look.variants` are connected. Foxfolk clothing/armour fits exist and are covered
by the asset crate's checks.

## Run and controls

```sh
scripts/run_client.sh
scripts/run_client.sh --fullscreen --size 5120x1440 --scale 0.65
```

WASD moves relative to the camera; Shift walks, Space jumps, V rolls, right-drag rotates and
fixes facing for strafing. Tab cycles enemies; click selects, right-click starts auto-attacks.
1/2 use the simulation's current abilities; R respawns; Esc exits. The plain stock-widget HUD
shows health, endurance, target health and ability cooldowns, with clickable ability/target/
auto-attack/dodge/respawn controls. A cast-bar consumer is reserved, but v3 supplies no cast data.
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

`--assets PATH` selects a catalogue root. Review animation advances at 60 Hz; normal play uses
snapshot pose and pose time. Fullscreen selects the monitor closest to `--size`; macOS may
otherwise use the laptop panel even with an ultrawide attached. Capture reads our output texture.

## Rendering and asset behaviour

- Direct Metal, HDR single-sample scene and motion vectors; TAA with camera, skin, morph and
  wind motion, depth rejection and YCoCg variance clipping. Camera cuts, appearance changes
  and resizing reset history. `--aa none` and `--aa msaa` are diagnostic comparisons; TAA is
  the default. Internal resolution is `--scale` times the output dimensions.
- Scene and cascade frustum culling, 64 m terrain chunks, screen-height whole-file LODs using
  `assets::lod_path`: full above 300 output pixels, LOD 1 above 100, otherwise LOD 2, with 10%
  hysteresis. Missing optional LOD files fall back to the full file. Creature textures and
  animation clips swap with the entire creature file; models are preloaded before play.
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
- The old procedural houses/trees and painted ground have been removed. Terrain, river and sky
  remain plain neutral placeholders until Claude supplies their materials and look values.
- Shared GPU textures and numeric mesh identities reduce duplicate uploads. Decoded image bytes
  are released after upload. Native ASTC KTX2 loading is implemented; actual compressed asset
  validation awaits Claude's build. PNG/JPEG, including embedded GLB images, remain the fallback.
- Repeated opaque rigid meshes are instanced in both scene and shadow passes; each instance
  retains current/previous transforms and its supplied material. Transparent objects remain
  individually sorted.
- One frame in flight is the default, limiting temporal-history latency on M1. `--in-flight 1..3`
  is diagnostic. `--fps 60` paces the client; `--benchmark` also fences completed GPU work so CPU
  submission speed cannot be mistaken for full frame time.

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
are normal/colour/roughness layer blending, scrolling water normals, depth absorption, Fresnel
sky reflection and an equirectangular sky. Their asset-dependent visual acceptance awaits the
supplied inputs. Foam, refractive scene sampling and atmosphere art are not fabricated here.

## Verification and measurements (24 October 2026)

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

The current six-character scene with supplied scenery and HUD completed 90 frames with Metal
API validation enabled at 1800×1100 output / 1440×880 internal, without GPU validation errors.
The renderer/client suites pass 19 checks; the separate asset suite passes all 10 checks,
including foxfolk fits. Scoped Clippy passes with warnings denied. Clippy without `--no-deps`
currently encounters existing lints in Claude-owned `world` and `assets`; those files are
unchanged by this work.
The final release smoke run completed 780 frames: 49.41 m of authoritative movement, jump,
dodge and damage events, and a wolf kill. Local captures are
`target/render-review/catalogue-instanced.png` and `target/render-review/combat-current.png`.

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

**Display acceptance remains open:** the Mac was locked during the display check; native
presentation times and physical input validation were unavailable. These offscreen results
meet the work budgets but do not establish steady onscreen 60 fps or longer thermal behaviour.
The Odyssey G95SC is connected at 5120×1440/120 Hz. A previous fullscreen attempt used the
laptop panel (2880×1800 backing size); it is rejected by the dimension guard, and monitor
selection is now explicit.

Historical comparison, before supplied scenery/HUD: 4x MSAA at 5120×1440 / 4096×1152 internal
measured 12.63 ms GPU median and 13.52 ms p95. This is not the current scene or a final budget pass.

Protocol v4 projectile/area/aura/cast/effect rendering is deliberately deferred until Claude's
contract and effect assets arrive. The functional cast-bar input is reserved. No snapshot fields,
combat art or effect timings have been invented.

Integration note: Cargo regenerates the root lockfile for the new renderer/client dependencies.
The root `Cargo.lock` and the pre-existing README changes are outside this ownership scope and
are excluded from this commit. Claude needs to include the refreshed lockfile in the shared
integration before using `cargo --locked` in CI. The required checks here use the working-tree
lockfile.

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
- **Terrain:** Claude-authored albedo/normal layers, UV scale per layer, layer-weight/control
  maps and their world bounds, plus any roughness/ORM data. Ground topology continues to use
  `world::height`; the temporary surface is uniformly neutral.
- **River:** normal maps, flow direction/speed, absorption/scattering colours, roughness,
  opacity/depth/fresnel settings and any foam mask/texture with placement/scale data. The
  river stays uniformly neutral until these arrive.
- **Sky and look:** sky texture or model/material and the sun direction/colour/intensity,
  ambient lighting, fog, exposure and grading settings. Renderer defaults are neutral and
  settings are data driven; no visual preset is authored here.
- **Dressing/scatter:** explicit placements for rocks, grass, flowers, bushes and reeds. The
  catalogue currently names candidates in `scatter`; the renderer will not choose locations.
- **HUD:** frame/bar art, ability icons, fonts and layout/style values. Functional controls
  use plain temporary styling until supplied.
- **Protocol v4 effects (reserved):** projectile/trail/particle/decal meshes, textures and
  their visual/timing parameters. Rendering is deferred until the new snapshot contract and
  Claude's effect assets arrive.
