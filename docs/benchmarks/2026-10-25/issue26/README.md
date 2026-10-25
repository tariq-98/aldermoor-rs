# Issue #26: per-pass counter validation

## Latest reference-setting measurements

M1 Air (7 GPU cores, 16 GB), AC power, 2560×1600 output, 1920×1200 internal,
75% render scale, custom TAA, one frame in flight. All four runs are **offscreen**;
API validation and the F3 overlay are off. Existing supplied assets and look values
are unchanged. Commands, executable hashes and start times are recorded in each JSON.

The baseline is `feb05ef`; the profiler implementation is `a1352e9` (before this
report-only commit). Claude subsequently merged the same main into the PR as `2a98092`;
its only runtime differences from the measured build are F3 event priority and menu/overlay
UI draw order. Both are inactive during these overlay-off, non-interactive benchmarks.
Both use the committed catalogue from the isolated validation
checkout. A missing terrain texture stopped an earlier attempt in the shared checkout;
that failed attempt is not a measurement and no missing asset was authored or replaced.

| Run | GPU median / p95 | Completed work median / p95 | GPU samples |
|---|---:|---:|---:|
| Before (`baseline-feb-offscreen.json`) | 23.139 / 34.307 ms | 29.418 / 41.237 ms | 599 |
| After (`after-feb-offscreen.json`) | 22.782 / 34.952 ms | 28.717 / 42.348 ms | 600 |
| Split diagnostic (`split-feb-offscreen.json`) | 12.756 / 13.634 ms | 16.972 / 18.399 ms | 360 |

Before/after use 720 total frames with 120 warmup; the legacy logger omits the final
frame while the new collector drains it. The diagnostic run uses 480/120 frames.
System load varied substantially. The split run's lower time is not evidence of a
speedup: it occurred later and uses different encoder boundaries. All runs exceed
the 12 ms GPU p95 budget. These short runs do not establish the ten-minute thermal
acceptance required for steady-state budgets in `docs/PERFORMANCE.md`.

## Counter overhead: inconclusive

`overhead-feb-offscreen.json` contains 3,600 measured frames (3,720 total), 1,800 ON
and 1,800 OFF, with the overlay off. Sampling alternates in 900 ABBA/BAAB four-frame
blocks, balanced across TAA jitter phases. Buffers remain allocated in both groups,
with no extra queue drain or scene split at the switches.

- ON median / p95: **19.907 / 33.373 ms**.
- OFF median / p95: **19.524 / 33.928 ms**.
- Mean paired ON-minus-OFF block delta: **−0.00187 ms** (noise, not a speedup).
- Descriptive 95% interval using 30 batches of 30 blocks: **−0.30497 to +0.30124 ms**.

The interval is too wide to establish the **under 0.1 ms** requirement at this setting.
Retain the signed differences rather than selecting a favourable subset. CPU sampling
cost is not measured independently. Reports include elapsed-minute buckets, complete
coverage for expected passes, draw calls, visible triangles and allocation bytes.

## Validation and remaining work

- 36 Rust tests pass (11 client, 13 render unit, 12 render integration), including
  counter slot reuse, resize, final drain, disable/re-enable, absent samples, supplied
  surface pixel equality with split profiling, and the 60-character GPU case.
- Release build, scoped fmt, macOS CI-equivalent clippy and four Python harness tests pass.
- The prior head `5aaff69` was visually checked on the unlocked Mac at a 1600×1000 window:
  F3 displayed readable live counters and a second F3 hid them. Scene and HUD remained
  intact. The current merge preserves the newer menu/controls and their key-binding capture
  priority; its on-screen check has not yet been repeated.
- Hosted macOS CI has a paravirtual GPU without stage counters. Tests explicitly verify
  unavailable timestamps and continued rendering there; physical-M1 tests exercise counters.

**Native-display checks and display restoration are pending.** The user approved a temporary
switch from 1440×900 logical to 1280×800 logical. The change succeeded: macOS reports
2560×1600 physical pixels and 1280×800 logical at 60 Hz. The Mac then locked, and the
computer-control service paused automatic unlocking after detecting physical input.
It requires a manual unlock before on-screen validation or restoration to 1440×900.
No native presentation measurements were obtained. The user has been asked to unlock.

After unlock: repeat F3 on the current build, measure on-display overhead at the native
panel setting, and restore the original display mode. Keep the PR draft until the required
checks pass. Codex creates no capture files; a Claude Mac check can capture the visible
F3 panel and its hidden state for the PR.

## Interpretation and earlier evidence

Default scene encoders remain combined. `--gpu-profile-scene` splits sky, opaque and
water/transparency for diagnosis and changes tile load/store costs. Pass intervals can
overlap and must not be summed. MetalFX internal encoders are not instrumented. Memory
is `MTLDevice.currentAllocatedSize` (allocations), not measured physical residency.

The `*-080-native.json` files are earlier **offscreen** results at 100% internal scale,
against base `08011c1`; their filename means native-sized output, not on-display capture.
They do not prove performance of the latest revision or the 75% reference setting.
Earlier rejected fullscreen/windowed attempts and the Metal-validation overlay log
are retained as historical diagnostics, not successful native-display acceptance.
