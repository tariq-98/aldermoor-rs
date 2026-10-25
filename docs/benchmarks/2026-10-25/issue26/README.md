# Issue #26: per-pass counter validation

Measured on the Apple M1 Air (7 GPU cores, 16 GB). Output and internal scene are
2560×1600, scale 1, custom TAA, one frame in flight. Cost runs have Metal API validation
disabled and the F3 overlay off. All use the existing supplied catalogue and unchanged look
values. Commands, executable SHA-256 hashes and run start times are in the JSON files.

The baseline executable was built from `eed4a4f`; the implementation is `7b8bab7`.
Both include the same river change from #48. The baseline used a sparse source checkout and
read the existing main-checkout assets. No assets or look settings were authored or edited.

## Before and after

| Run | GPU median / p95 | Frame work median / p95 | Measured GPU frames |
|---|---:|---:|---:|
| Before instrumentation (`baseline-eed-native.json`) | 15.708 / 42.551 ms | 20.568 / 47.599 ms | 599 |
| After, counters enabled (`after-eed-native.json`) | 15.492 / 16.129 ms | 19.955 / 20.769 ms | 600 |
| Split-scene diagnostic (`split-native.json`) | 15.837 / 16.330 ms | 21.011 / 21.808 ms | 360 |

The before/after runs use 720 frames and discard 120 warmup frames. The old logger omits
the last frame; the new collector drains all completions and filters by submitted frame ID.
The diagnostic run uses 480/120 frames. The full scene **fails** the 12 ms GPU budget.
Background load varied, with other development sessions active. The lower p95 after the
change is not evidence of a renderer speedup. Use the interleaved comparison below for
counter overhead, not the difference between these sequential run medians.

## Counter-only overhead

`overhead-balanced-native.json` contains 3,600 measured frames (3,720 total, 120 warmup)
and 900 four-frame blocks. OFF/ON/ON/OFF and ON/OFF/OFF/ON blocks are balanced across TAA
jitter phases; no extra queue drain, buffer allocation or scene split occurs at the switches.
One frame in flight prevents neighbouring frames from overlapping on the GPU. Both groups
use the same binary and preallocated counter buffers. The overlay stays off.

- Sampling ON: median **15.8229 ms**, p95 **16.5470 ms**, 1,800 samples.
- Sampling OFF: median **15.8086 ms**, p95 **16.6288 ms**, 1,800 samples.
- Mean of paired block differences, ON minus OFF: **+0.00456 ms**.
- Median paired difference: **−0.00483 ms**; this does not imply a speedup.
- Grouping consecutive blocks into 30 batches of 30 blocks gives a descriptive 95% t
  interval of **−0.02471 to +0.03384 ms** for the mean difference (29 degrees of freedom).
  Both the point estimate and upper bound are below **0.1 ms GPU** in this offscreen run.
  This is not a guarantee for every frame or other hardware, and does not measure CPU
  sampling cost independently. Raw signed block differences are retained in the report.

The run spans two elapsed-minute buckets after warmup: minute 0 has 2,812 samples and
minute 1 has 788. Buckets use elapsed submission time, not an assumed 60 fps.

## Pass details

Every expected pass had timestamp coverage for every sampled frame. The diagnostic split
reports median fragment-stage intervals of 0.626 ms sky, 7.077 ms opaque, and 0.037 ms
water/transparency. These are diagnostic intervals with extra tile load/store boundaries,
not the unperturbed costs of the default combined scene. Raw reports also contain each
stage's p95, vertex intervals, and vertex-start-to-fragment-end elapsed intervals. Stages
can overlap: adding pass elapsed intervals does not give whole-frame GPU time.

The reports include draw calls, visible scene triangles and Metal allocation bytes. Peak
allocation in the overhead run is 1,230,127,104 bytes. This is **not measured physical GPU
residency**. MetalFX's private encoders are not individually instrumented; its separate TAA
entry remains unavailable when that mode is selected.

## Validation and outstanding checks

- `MTL_DEBUG_LAYER=1 cargo test -p render -p client --locked`: **36 passed** (11 client,
  13 renderer unit, 12 renderer integration), including the 60-character GPU stress case.
- New GPU tests cover slot reuse, final draining, resize, counter disable/re-enable, absent
  HUD/TAA samples, and pixel-for-pixel equality of the supplied surfaces with split profiling.
  Pixel comparison is in memory; it creates no image files.
- Scoped rustfmt and clippy pass. The CI-equivalent clippy invocation also completes;
  existing dependency warnings remain outside this change.
- Four Python harness tests cover completed-frame reporting, absent timestamps, unavailable
  counters and partial coverage during the overhead check.
- The release client was built and the overlay was exercised separately with Metal validation.

**Native on-display acceptance is not verified.** The session reports that the Mac is locked,
so there are no native presentation samples or physical F3/visual acceptance. Fullscreen under
the existing scaled desktop renders 2880×1800; a windowed request was clamped to 2560×1378.
Both were correctly rejected by the dimension guard. The rejected logs are retained as
`fullscreen-rejected.txt` (initial exploratory run) and `windowed-rejected.txt` (rebased run);
neither is included in the native-size comparisons.

Remaining: unlock the Mac, use a native-mode built-in display so the actual drawable is
2560×1600, rerun the native on-screen comparison and F3 interaction, and verify the sub-0.1 ms
counter overhead there. Unsupported-device fallback is implemented but was not exercised on
a second GPU. This PR remains draft with these limits stated.
