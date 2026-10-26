# Tuned wgpu slice against native Metal on the M1 (#317)

**Tuned wgpu is still about 24% slower than Metal on the M1, so the Mac stays on Metal.** The best
wgpu setting (trusted shaders, one scene pass) took 13.0 ms GPU median against 10.5 ms for Metal
in the same layout. That is over the 12 ms budget and well outside the 5% gate. Trusted shader
modules recover about 2 ms of the original 4.6 ms gap. The rest of the gap is in the scene, TAA
and composite stretch; the shadow passes cost the same on both.

Source: the slice at `a5cbc0c` (PR #320). Each JSON here is one run's full report, with every
measured frame.

## Conditions

M1 Air, 2560x1600 output at 75% scale, one frame in flight, 600 frames after 120 warm-up frames,
464 draws a frame. All 12 runs went back to back under one GPU lock (13:39 to 13:43 UTC,
26 October), in the ABBA order of the file numbers. Other agents' builds were running on the Mac
during the window (builds don't take the GPU lock), which likely explains outlier run 07.

## Results

Median and p95 in ms, averaged over each setting's two runs (run 07 is shown on its own).

| Setting | GPU median | GPU p95 | CPU encode median | vs Metal, same layout |
| --- | ---: | ---: | ---: | ---: |
| Metal, one scene pass (06, 12) | 10.50 | 12.23 | 1.39 | baseline |
| Metal, split scene passes (01, 11) | 11.01 | 12.85 | 1.16 | baseline |
| wgpu default checks, split (02, 10) | 15.56 | 17.04 | 2.94 | +41% |
| wgpu trusted, split (03, 09) | 13.59 | 15.37 | 2.70 | +23% |
| wgpu trusted, one pass (04, 08) | 12.98 | 14.76 | 2.47 | +24% |
| wgpu trusted, one pass, bundles (05) | 12.94 | 14.73 | 2.76 | +23% |
| same, repeat (07, outlier: every pass slower) | 15.80 | 17.94 | 2.52 | |

- **Trusted shaders** (`create_shader_module_trusted` with `ShaderRuntimeChecks::unchecked()`)
  save 2.0 ms. That confirms naga's default bounds checks and loop guards caused over 40% of the
  #95 gap.
- **One scene pass** saves about 0.6 ms on wgpu and 0.5 ms on Metal, so it doesn't change the
  ratio.
- **Render bundles** don't change GPU time. They cut wgpu's encode time in the first half of the
  runs (2.5 to 2.8 ms against 3.2 to 3.3 ms), but wgpu still encodes about twice what Metal does.
- **Where the rest goes.** Pass spans are running totals from the start of the scene pass on both
  backends, so only their differences mean anything. With one scene pass, the scene ends about
  1.1 ms later on wgpu (7.0 against 5.9 ms), and TAA takes another 1.2 ms more (3.1 against
  1.9 ms between scene end and TAA end). Shadows match within 0.1 ms.

## Visual check

Mac captures at frame 130 (PR #320 comment) match Metal on terrain, shadows, foliage cut-outs,
water, fog and TAA: full-frame PSNR 48.3 dB, SSIM 0.994. The one mismatch was the emissive lantern
panes. The slice's default emissive map was black where the Metal renderer's is white; glTF
multiplies the emissive factor by the map, so materials with a factor and no map lost their glow.
That is fixed in the slice and checked on software Vulkan, where only the lantern pixels change.

## What's left

- The remaining 24% is probably naga's generated MSL against hand-written MSL (its loops become
  `while (true)` with flags, and its samplers are arguments where ours are `constexpr`), or
  wgpu-hal's binding and hazard tracking. Feeding `shaders.metal` through wgpu's MSL passthrough
  (`Features::PASSTHROUGH_SHADERS`) would tell the two apart. It isn't needed for the PC backend
  (#96), which goes ahead on wgpu with trusted modules either way.
- The CPU encode cost (about 2x Metal) matters for crowds on PC. #96 should keep render bundles
  for static scenery and measure a 200-character scene once PC hardware is chosen (#94).
