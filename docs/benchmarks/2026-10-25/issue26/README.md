# Issue #26: per-pass counter validation

Measured on the Apple M1 Air (7 GPU cores, 16 GB). Output and internal scene are
2560×1600, scale 1, custom TAA, one frame in flight. Cost runs have Metal API validation
disabled and the F3 overlay off. All use the existing supplied catalogue and unchanged look
values. Commands, executable SHA-256 hashes and run start times are in the JSON files.

The baseline executable was built from `08011c1`; the implementation is `4fbc9ad`.
Both include the merged controls from #52 and supplied limestone textures from #54. The baseline used a sparse source checkout and
read the existing main-checkout assets. No assets or look settings were authored or edited.

## Before and after

| Run | GPU median / p95 | Frame work median / p95 | Measured GPU frames |
|---|---:|---:|---:|
| Before instrumentation (`baseline-080-native.json`) | 15.349 / 15.666 ms | 20.370 / 20.889 ms | 599 |
| After, counters enabled (`after-080-native.json`) | 17.502 / 22.043 ms | 22.495 / 27.500 ms | 600 |
| Split-scene diagnostic (`split-080-native.json`) | 15.942 / 16.653 ms | 20.677 / 21.561 ms | 360 |

The before/after runs use 720 frames and discard 120 warmup frames. The old logger omits
the last frame; the new collector drains all completions and filters by submitted frame ID.
The diagnostic run uses 480/120 frames. The full scene **fails** the 12 ms GPU budget.
Background load varied, with other development sessions active. The slower sequential after
run is not an isolated measurement of counter cost. Use the interleaved comparison below for
counter overhead, not the difference between these sequential run medians.

## Counter-only overhead

`overhead-080-native.json` contains 3,600 measured frames (3,720 total, 120 warmup)
and 900 four-frame blocks. OFF/ON/ON/OFF and ON/OFF/OFF/ON blocks are balanced across TAA
jitter phases; no extra queue drain, buffer allocation or scene split occurs at the switches.
One frame in flight prevents neighbouring frames from overlapping on the GPU. Both groups
use the same binary and preallocated counter buffers. The overlay stays off.

- Sampling ON: median **15.5785 ms**, p95 **21.3316 ms**, 1,800 samples.
- Sampling OFF: median **15.5681 ms**, p95 **21.3672 ms**, 1,800 samples.
- Mean of paired block differences, ON minus OFF: **−0.00712 ms** (noise, not evidence of a speedup).
- Median paired difference: **+0.00564 ms**.
- Grouping consecutive blocks into 30 batches of 30 blocks gives a descriptive 95% t
  interval of **−0.07234 to +0.05810 ms** for the mean difference (29 degrees of freedom).
  Both the point estimate and upper bound are below **0.1 ms GPU** in this offscreen run.
  This is not a guarantee for every frame or other hardware, and does not measure CPU
  sampling cost independently. Raw signed block differences are retained in the report.

The run spans two elapsed-minute buckets after warmup: minute 0 has 2,770 samples and
minute 1 has 830. Buckets use elapsed submission time, not an assumed 60 fps.

## Pass details

Every expected pass had timestamp coverage for every sampled frame. The diagnostic split
reports median fragment-stage intervals of 0.630 ms sky, 7.101 ms opaque, and 0.037 ms
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
`fullscreen-rejected.txt` and `windowed-rejected.txt` (earlier display-mode attempts);
neither is included in the native-size comparisons.

Remaining: unlock the Mac, use a native-mode built-in display so the actual drawable is
2560×1600, rerun the native on-screen comparison and F3 interaction, and verify the sub-0.1 ms
counter overhead there. GitHub's macOS runner exposes an Apple paravirtual GPU without stage-boundary counters.
The GPU tests verify continued rendering and unavailable data on that device; full timestamp
and split-pass assertions still require the physical M1 and ran locally. This PR remains
draft with the native-display limits stated.
