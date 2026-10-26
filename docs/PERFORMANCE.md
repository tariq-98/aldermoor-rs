# Performance budgets

What each part of the game may spend. A feature that needs more takes it from another line of
this table, or the table changes in the same PR with the reason written down. Issues labelled
`perf` report before and after numbers against these budgets (AGENTS.md).

## Reference machine and setting

- **Machine:** the MacBook Air M1 (7-core GPU, 16 GB), on mains power, other apps left as they are.
- **Output:** the built-in 2560×1600 panel. **Render scale 75%** (1920×1200 internal, about
  2.3 MP), TAA upscaling to the panel. Decided on #36: the ultrawide (5120×1440) is best effort.
- **Steady state, not a cold start:** a budget holds after 10 minutes of play. The 25 October
  offscreen soak (#6) showed no thermal slowdown over 15 minutes, but re-check on long runs.

## Budgets

| Area | Budget | Last measured |
| --- | --- | --- |
| Whole frame on screen | 60 fps mean, 17.2 ms p95 presented | not yet at the reference setting |
| GPU total | 12 ms p95 | 13.0 ms median, 14.8 ms p95 offscreen at the reference setting, settled minute (#36) |
| GPU: shadows (3 cascades) | 1.5 ms | about 4.5 ms for the three cascades, elapsed median (#36) |
| GPU: opaque scene, terrain included | 5.5 ms, terrain at most 2 ms | #26 |
| GPU: sky, water and transparents | 1 ms | #26 |
| GPU: TAA or upscaler | 1 ms | #26 |
| GPU: AO, bloom and fog (#13) | 1.5 ms together, at half resolution | not built |
| GPU: HUD | 0.5 ms | #26 |
| GPU: spare | 1.5 ms | |
| Client CPU, main thread | 6 ms a frame | frame median 22.7 ms vs GPU 14.5 ms with one frame in flight (#6) |
| Sim step | 2 ms p99 at 2,000 active units | 0.52 ms mean, 0.77 ms p99 on the M1 (#58); in the cloud 0.55 ms mean, 1.05 ms p99, and 1.3 ms mean, 2.5–3.3 ms p99 with 5,000 packed within 100 m (#33) |
| Server, each extra player | 0.1 ms a tick for their snapshot | 0.16 ms at 2,000 units on the M1 (#58) |
| Network, down | 10 KB/s per player typical, 64 KB/s in a big fight | wire views at 20 Hz, far units less often (#34, #278): 2.3 KB/s standing among 1,000 units in view, 16 KB/s in the perf gate's fight, 15 KB/s running through 2,000 |
| GPU and client memory | 2 GB footprint | 1.34 GB steady, 1.9 GB peak while loading (#6) |
| Launch to playable | 5 s | 4.1 s to first frame and exit (#6) |

The GPU split is a starting point. When per-pass timers land (#26), replace it with measured
numbers and move the spare to wherever it is needed.

## MMO scale

Aldermoor is an MMORPG (decided 25 October 2026), so the server and the network get budgets of
their own. Working sizes, until Chippy picks: **2,000 players in a world, 200 in one busy area**
(a town, a Wilderness fight). Team modes (dungeons, capture the flag) run as separate small
instances and fit inside these numbers.

| Area | Budget | Last measured |
| --- | --- | --- |
| Server region tick (step, interest, sends) | 16.7 ms at 200 players and 2,000 monsters | 1.14 ms step (#33) plus 92 ms building views at 200 bots (#189, load test) |
| Server, each player's view | 0.02 ms at a 20–30 Hz send rate | 0.1 ms with 1,000 units in view, picked from a frame a step shared by every player (0.6 ms a step at 2,000 units); was 0.3 ms to build and 0.2 ms to encode (#286) |
| Network, down, MMO | 20 KB/s per player typical, 64 KB/s in a 100v100 fight | 23 KB/s per bot at 50 bots and 60 ticks a second in the load test, 42 KB/s at 200; the perf gate's `players` scene (200 players and 2,000 monsters, all in view) 61 KB/s, measured over 5 s (#278, #286) |
| Server out, one region | 4 MB/s at 200 players | 2.8 GB/s at 200 players (#73) |
| Client: crowd of 200 characters | 4 ms GPU and 3 ms CPU together, inside the frame budgets above | not built (#75) |

The view cost is the hard one: it has to drop about 40 times. That takes an interest grid,
appearance sent once, deltas against what the client already has, quantised fields and a
20–30 Hz send rate (#34, #73). A bot load test (#74) checks the whole region tick.

## How each is measured

- **GPU and frame:** `scripts/benchmark_renderer.py` on the reference machine, offscreen for
  work budgets, `--windowed --fullscreen` for presentation. At the reference setting:

  ```sh
  cargo build --release -p client --locked
  python3 scripts/benchmark_renderer.py --size 2560x1600 --scale 0.75 --in-flight 1 \
    --frames 3720 --warmup 120 --output target/render-review/reference.json
  ```

  Save reports worth keeping under `docs/benchmarks/<date>/`. Offscreen numbers never prove
  on-screen 60 fps.
- **Sim step:** `cargo run --release -p sim --example bench 2000` for timing, and the Perf
  workflow's instruction-count gate (#31) for changes between commits. The sim is deterministic,
  so instruction counts are stable where timings on shared runners are not.
- **Server per player:** the `snapshot` time printed by the same bench.
- **Network:** postcard-encoded `Snapshot` plus `CombatView` bytes per player per second at the
  60 Hz tick (#31, #34).
- **Memory:** `footprint -p <pid>` on the running client, peak and steady.
- **Launch:** wall clock from starting the release binary to its first presented frame.
- **MMO server and network:** `cargo run -p sim --release --example regions` times a busy region
  (200 players, 2,000 monsters), finding each player's neighbours from a grid, lag compensation
  history, match instances and regions side by side; the design and its numbers are in
  [SERVER.md](SERVER.md) (#73). The bot load test (#74) reports the whole region tick and bytes
  per player once it exists.

## Changing a budget

Change this file in the PR that needs it, say why in the PR body, and record the decision in
`wiki/Decisions-log.md`.
