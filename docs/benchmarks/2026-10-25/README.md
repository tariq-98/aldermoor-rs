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
