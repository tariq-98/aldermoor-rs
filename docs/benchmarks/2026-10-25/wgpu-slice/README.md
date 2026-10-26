# wgpu valley slice: initial M1 comparison (#95)

The current slice **does not pass the adoption gate**. Keep the shipped Metal renderer and do
not start #96 on this evidence. This is a result for this implementation, not a conclusion
that wgpu cannot meet the budget. Visual parity remains unverified, so these are preliminary
performance diagnostics rather than a completed equivalent-renderer acceptance test.

Source commit: `da2627dd8e19fa050da95c9f1f2d818e39493207`, based on `f630bca`.
Standalone commands and limitations: [slice README](../../../../crates/render/examples/wgpu_slice/README.md).

## Conditions and results

Local M1 Air; 2560x1600 output, 75% scale, one frame in flight. Both paths used the same supplied
catalogues, fixed river-review camera, rigid scenery, three 1024px shadow cascades, split scene
passes, TAA and composite. No characters or HUD. Each release run used 600 frames with 120
warmup. Order: Metal / wgpu / wgpu / Metal. All runs submitted 434 draws per frame. The shared
GPU lock covered all four runs, and the task's Linux build was paused throughout.

| Run | GPU median ms | GPU p95 ms | CPU encode median ms | CPU encode p95 ms | Peak RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1-metal | 9.665 | 10.087 | 0.455 | 0.528 | 461.4 |
| 2-wgpu | 14.904 | 15.625 | 2.958 | 4.109 | 323.8 |
| 3-wgpu | 14.185 | 14.587 | 4.959 | 5.585 | 325.3 |
| 4-metal | 9.838 | 10.492 | 1.982 | 3.841 | 458.5 |

The wgpu p95 exceeds the 12 ms GPU budget in both runs and is well outside the within-5%
comparison gate. Native Metal is below 12 ms in these short runs. This does not establish the
budget after ten minutes or on-screen 60 fps. CPU encode includes preparation and submission;
encode p95 is below 6 ms in these samples, but this harness excludes the rest of the client.
The 2 ms sim-step budget was not measured; the simulation is unchanged.
CPU timings drifted substantially in the repeated Metal run, so do not interpret the CPU ratio
as a stable intrinsic backend overhead. Peak process RSS is reported, but it does not verify the combined 2 GB GPU/client footprint: GPU
allocations and macOS footprint were not collected. No 200-character workload or
launch-to-first-frame measurement has been made.

## Reported pass spans

Median milliseconds, in ABBA order:

| Pass | Metal 1 | wgpu 2 | wgpu 3 | Metal 4 |
| --- | ---: | ---: | ---: | ---: |
| shadow_0 | 0.524 | 0.608 | 0.305 | 0.552 |
| shadow_1 | 1.563 | 1.425 | 1.206 | 1.246 |
| shadow_2 | 1.434 | 1.647 | 1.494 | 1.522 |
| sky | 0.631 | 0.422 | 0.328 | 0.617 |
| opaque | 4.847 | 7.007 | 6.692 | 4.933 |
| water_transparent | 4.991 | 7.313 | 7.049 | 5.067 |
| taa | 7.216 | 12.093 | 11.686 | 7.305 |
| composite | 7.445 | 12.375 | 11.918 | 7.536 |

These hardware timestamp spans overlap: their sum exceeds the whole-frame GPU duration. Do not
add them or assign an upstream delay entirely to the later pass. In particular, the larger TAA
span does not by itself prove that the TAA shader accounts for that entire difference. Total GPU
time clearly fails the comparison gate regardless of that limitation.

## Validation and remaining gates

- All three WGSL files passed Naga parsing and validation on the Mac.
- Scoped clippy passed with style warnings; no asset or shipping renderer files were changed.
- Both backends completed a 3-frame, 320x200 debug draw check on the M1, each under the GPU lock.
  wgpu API validation was enabled for its draw check. No images were created by Codex.
- Linux build and seven tests passed in an amd64 Debian container on the local ARM Mac VM.
  The lavapipe draw check timed out after 300 seconds with empty stdout/stderr; drawing on Linux
  is **unverified**. This emulated software setup supplies no performance evidence. The owned
  container was stopped and removed after the timeout. Native Linux CI is still required.
- GitHub Actions rejected every job before any steps ran due to account payments/spending limits
  (#232). This is not a code-test result; required CI is not green.
- Latest-head Mac-worker captures and visual acceptance remain outstanding. Inspect terrain
  normals, shadows, foliage cut-outs, sky, reflections, water and TAA with matching camera/frame.
- Launch, a draw-count stress, steady-state behaviour and an accepted visual match remain gates.

The raw per-frame JSON and `summary.json` are retained here. Peak RSS was measured with macOS
`/usr/bin/time -l` around each process. No billing settings, merge rules, assets or look values
were changed. PR #231 remains draft and must not merge while its required checks are missing.
