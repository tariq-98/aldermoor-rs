# Renderer measurements, 25 October 2026

Apple M1, 7 GPU cores, 16 GB. Every report requests 5120×1440 output and 65% scale
(3328×936 internal), paced at 60 fps. These are sequential runs, not isolated-machine
experiments. No other applications were stopped. GPU command-buffer durations can overlap
when multiple frames are in flight. See [RENDERER.md](../../RENDERER.md) for interpretation.

- `display-*`: native drawable presentation on the unlocked Odyssey G95SC at 120 Hz,
  using the monitor-selecting borderless window. Short runs use 480 frames / 120 warmup;
  `display-long` uses 3720 / 120. The long run failed; it must not be omitted in favour
  of the short two-frame presentation pass.
- `taa-final-cost`, `metalfx-final-cost`: final release, 480 / 120, one frame in flight,
  offscreen, API validation disabled. MetalFX has corrected jitter offsets, 19 jitter
  phases at 65%, and a texture mip bias. The custom TAA uses nine-tap Catmull–Rom.
- `*-validation`: same final paths with Metal API validation and matched final-frame
  captures. Validation overhead means these are not the cost comparison.
- `live-*`: live host scene (seven units at start), indexed/partial skeleton updates
  versus `--legacy-cpu`. This small, noisy comparison did not demonstrate a speedup.
- `look-*`: six-character showcase, one weapon swap per 60 frames; compare retained
  object history with `--reset-on-look`. Check the `history_resets` count.

The display/CPU/history runs precede the final MetalFX-only jitter/phase/mip-bias correction;
custom TAA's numerical path is unchanged (its texture mip bias is zero). A final guard-clock
correction affects guarding units only; the benchmark showcase has none. Captures remain local
in `target/render-review/v4-{taa,metalfx}-final.png` and `v4-aa-comparison.png`.

`FRAME` means completed CPU+GPU work with one frame in flight, and CPU encoding plus slot
wait with two. `PRESENT` uses native presentation timestamps and is the display evidence.
The harness requires at least 95% of measured frames to have presentation samples. Offscreen
throughput is not display acceptance. Missing optional art uses neutral engine defaults.

## Supplied surface integration

`supplied-surfaces-before-skip` and `supplied-surfaces-final` use Claude's delivered surface/look
catalogues and material-packed kit. Both are offscreen, one frame in flight, 480 / 120 frames,
API validation disabled. The latter skips terrain layers whose weight is exactly zero.
Static ground and limestone capture regions matched pixel for pixel with the skip enabled.
Both reports fail the work budgets; no new display-acceptance claim follows from them.
The harness now records resolved look/surface paths to distinguish these from earlier neutral
benchmarks. The fixed-camera river/limestone review is:

```sh
cargo run --release -p render --example review_surfaces --locked
```

## Reference setting re-baseline (#36)

`reference-offscreen.json` is the first run at the budget reference chosen on #36: the Air's own
2560×1600 panel at 75% (1920×1200 internal), TAA, one frame in flight, offscreen, 3720 / 120
frames, main at 53be557 (per-pass timers from #26 included). The Mac was shared with other agents;
the run held the GPU lock but CPU load was still settling in its first minute, so read
`gpu_by_minute`:

| Window | GPU median / p95 | Frames |
| --- | ---: | ---: |
| Whole run | 14.6 / 20.4 ms | 3,600 |
| Minute 1, load settled | **13.0 / 14.8 ms** | 493 |

An earlier run on 351a797 with a quiet machine read 13.2 / 17.5 ms. Either way the reference
setting is over the 12 ms GPU budget, by roughly 1 to 3 ms, before effects (#13) are added. Draw
calls 724, about 332k visible triangles, 1.14 GB allocated. Per-pass elapsed medians: three shadow
cascades 0.8 + 1.6 + 2.1 ms, combined scene 8.5 ms (6.4 ms fragment), TAA fragment 2.8 ms.
Stage intervals overlap, so they don't add up to the total.

No on-screen (`--windowed --fullscreen`) number is recorded. Two attempts on the panel read about
20 fps with GPU medians of 24 and 40 ms while other agents were building and running Codex
computer use; those are void, not a result. The display run belongs with #9's soak once the Mac
can be left to one agent.
