# Native-display validation on bf43200

M1 Air, built-in display at 2560×1600 native output, 1920×1200 internal (75%), custom TAA, one frame in flight, overlay off. The baseline is feb05ef; the current binary was rebuilt offline from bf43200. Commands, executable hashes, timestamps and sample coverage are in the JSON reports. No assets or look values were authored. The harness reports mode as windowed; the recorded client command includes --fullscreen.

| Run | GPU median / p95 | Completed work median / p95 | Presented mean fps / p95 interval |
|---|---:|---:|---:|
| Before: feb05ef | 34.801 / 49.266 ms | 47.042 / 83.120 ms | 18.963 fps / 99.996 ms |
| After: bf43200 | 34.120 / 57.763 ms | 44.623 / 79.386 ms | 20.380 fps / 83.330 ms |

**FAIL: both runs exceed the 12 ms GPU budget and miss 60 fps.** Each requests 720 frames with 120 warmup. The legacy baseline has 599 GPU samples; the profiler drains all 600 completed measured frames. Presentation has 585 and 591 samples respectively. These short sequential runs do not isolate counter cost or establish ten-minute thermal acceptance. The median difference is not evidence of a speedup, especially with the higher after-run p95.

## Counter overhead: incomplete

The native interleaved ABBA/BAAB test requested 12,120 total frames, including 120 warmup. It was stopped after more than 18 minutes because progress was very slow and no complete report had been produced. No native overhead estimate or confidence interval is available; the under-0.1 ms requirement remains unverified. The earlier offscreen estimate is also inconclusive (see offscreen-summary.json).

A one-second process sample at 21:45 UTC showed the main thread inside Renderer::encode, including allocation and draw encoding. This sample does not establish why the run slowed down or prove a deadlock. The diagnostic remains locally at /tmp/aldermoor-26-long-sample.txt. Only this task’s identified driver, harness and game processes were terminated.

## Conditions and restoration

An initial native attempt overlapped another Aldermoor client in aldermoor-worker-a. Those completed reports are retained with a -contended suffix and excluded from the table above. The user authorised stopping that client, but a fresh process check showed it had already exited; no other worker process was killed. The reported runs were repeated after that check. Other applications remained running, and system load was not controlled.

**Original display restored and verified:** 1440×900 logical / 2880×1800 Retina backing. The temporary validation setting was 1280×800 logical / 2560×1600 backing. The user explicitly approved CoreGraphics restoration after UI control timed out. The external display was not changed.

## Remaining validation and publication

The current-head F3 visual/keyboard check is unverified because UI control timed out. The older 5aaff69 check is historical evidence only. A Claude Mac check should capture the current F3 overlay visible and hidden, verify readable live values, intact scene/HUD, and menu key-binding capture priority. Codex creates no image files.

The exact bf43200 release build passed. Previously recorded validation includes 36 Rust and four Python tests; those were not rerun for this report-only update. GitHub status cannot be freshly verified because sandbox networking is blocked. These new reports have not been posted to PR #59; its prior restoration/blocker text is stale. Set network_access = true under [sandbox_workspace_write] in ~/.codex/config.toml, then refresh the remote branch before publishing without overwriting concurrent work.

Keep the PR draft and unmerged: counter overhead and current-head visual checks remain outstanding, and frame budgets fail. No new claim of physical GPU memory residency is made.
