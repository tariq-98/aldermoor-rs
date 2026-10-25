# Zone servers

How one Aldermoor world holds 2,000 players, 200 of them in one busy area, plus instanced team
modes. This is the design from the spike in #73; the budgets it sets are in
[PERFORMANCE.md](PERFORMANCE.md#mmo-scale). Snapshot size and the wire format are #34.

## Measured

Cloud x86 (4 vCPU Xeon at 2.1 GHz, shared), release build, 25 October 2026:
`cargo run -p sim --release --example regions`. The M1 is about 1.5 times faster on the sim bench
(#58), so these are on the safe side for a Mac host and about right for a cheap Linux one.

| What | Result |
| --- | --- |
| Region step, 200 players fighting among 2,000 monsters | 1.34 ms mean, 2.28 ms p99 |
| Same, four regions side by side, one thread each, all four cores busy | 1.62 ms mean, 3.62 ms p99 (worst region) |
| Interest: bucket every unit into a 32 m grid | 0.065 ms a step |
| Interest: every one of the 200 players finds its neighbours within 60 m | 0.91 ms for all, 0.0046 ms each, 229 units each |
| Lag compensation: record everyone's position and facing | 0.015 ms a step, 2.9 MB for one second |
| Match instance (10 players, 30 monsters): create | 0.24 ms on the shared land (12.7 ms before #167, which built the land each time) |
| Match instance: memory | 0.24 MB (0.63 MB before #167) |
| The shared land, built once per map | about 10–12 ms and 0.39 MB |
| Match instance: tear down | 0.09 ms |
| Match instance: step | 0.023 ms (50 of them: 1.14 ms mean, 1.94 ms p99 on one thread) |

What this says:

- **The step is not the problem.** A full busy region steps in about a tenth of its 16.7 ms tick,
  and stays well inside it with every core loaded. One region per thread is the right unit.
- **Finding who a player can see is cheap with a grid**, 0.005 ms a player against the 0.02 ms
  budget. Today's 0.8 ms per view is the full-world scan and building full `UnitState`s every
  tick, which #34 replaces.
- **Instances are cheap to make and run** now that every sim on a map shares one copy of its
  land (`Sim::on`, #167). Before that, each instance spent 12 ms building its own.
- **History for lag compensation costs almost nothing** in time; its memory is per region, not
  per player.

## Regions

The world is split into **regions**: designed areas such as Brambleford, the river valley north
of it, each Wilderness depth, and so on (DESIGN.md's "deliberately designed regions"). Each
region is one `Sim` with its own land, its own seeded random sequence and its own thread, stepped
at 60 Hz exactly as the single sim is today. A region is the unit of ownership: every unit lives
in exactly one region at a time, and only that region changes it.

- **Size:** a region is drawn so a normal busy evening stays under **200 players and 2,000
  monsters**, and up to about 300 players for a world event. Borders go at natural chokepoints
  (bridges, passes, gates, the river) where few fights happen, not through the middle of a town
  or a battlefield.
- **Unit IDs** stay globally unique across the world: each region hands out IDs from its own
  block (`region << 24 | n`, 16 million each), so an ID never changes when a unit crosses over and
  clients never re-learn who is who.
- **Ghosts:** each region publishes a read-only copy of the units within 32 m of its borders to
  its neighbours at the start of each step. A neighbour can see ghosts (they go into snapshots) and
  target them, but never changes them.
- **Crossing:** a unit hands over when its centre is 4 m past a border (the gap stops a unit
  that strolls along the line from flapping). At the end of a step the old region removes it and
  sends its whole state (components, auras, cooldowns, threat) to the new one, which adds it at the
  start of its next step: one step, 16.7 ms, in which the unit is a ghost on both sides. Handoffs
  are applied in unit ID order, so each region stays deterministic given its inbox.
- **Fighting across a border:** an attack on a ghost resolves in the attacker's region, and the
  damage, heal or aura is sent to the owning region as a request, applied at its next step. One
  step late, invisible at 20–30 Hz sends. Monsters do not chase past their region's border; they
  turn back, the way they give up a chase today.
- **Quiet regions sleep.** With no players inside or in their ghost band, a region steps once a
  second (wandering, respawns) instead of 60 times (#33).
- **Layers:** when a region passes its player cap (a capital on launch night), the world opens a
  second copy of it, a layer, and new arrivals go there; groups and companies are kept together.
  Layers merge back when both are quiet. The Wilderness is never layered, so a fight with real
  death stakes can't be dodged by changing layer; instead its entrances stop letting players in
  at its cap (#170).

### Machines

A region needs one core of its own. Per tick, at 200 players and 2,000 monsters:

| Part | Budget | Measured or estimated |
| --- | --- | --- |
| Step | 4 ms p99 | 2.3 ms p99 alone, 3.6 ms with every core busy |
| Interest (grid and 200 lookups, a third of the players each tick at 20 Hz) | 0.5 ms | 0.065 + 0.3 ms |
| Encode and send views (a third of the players each tick) | 3 ms | not built (#34) |
| Ghosts, handoffs and cross-border requests | 0.5 ms | 0.46 ms mean, 1.5 ms p99 with about 500 ghosts each way (#168) |
| Lag compensation history | 0.1 ms | 0.015 ms |
| **Total, against the 16.7 ms tick** | **8 ms**, half the tick spare | |

Sends run at 20 Hz while the sim runs at 60, and each tick builds views for a third of the
players, so the view cost is spread rather than all landing on one tick.

One server process runs a world: one thread per awake region, a small pool of threads stepping
match instances, and the network threads (QUIC, #86) that take commands and hand out encoded
views. On an 8-vCPU Linux host, one core goes to networking and the OS, which leaves seven: about
**six busy regions plus an instance pool**. 2,000 players spread over a world mostly sit in a few
busy regions and many quiet ones, so one such host should carry one world; the bot load test (#74)
proves it. A world that outgrows one host moves whole regions to a second process; the handoff
messages above are already the shape that goes over a socket.

## The Wilderness bands

The Wilderness is a set of regions north of the valley, split by depth into **bands** with level
ceilings (1–10, 11–20, …, the deepest without one) (#76). Bands are a property of where you stand,
worked out by the region's sim from position, not a region border: one Wilderness region can hold
several bands, and fights cross band lines.

Everything is enforced on the server, in the sim. The client only shows it.

- **One rule decides who may harm whom:** a `can_harm(attacker, target)` check, called wherever
  harm is applied (direct hits, projectiles landing, ground areas ticking, auras applied), not only
  when a target is picked, so a fireball thrown from band 3 into band 1 is judged where it lands.
  Two players may harm each other only when both stand in the Wilderness at that moment. Outside
  it, players never harm players (except inside team modes, below).
- **Scaling, not refusal, protects low levels:** a player above a band's ceiling is scaled down to
  it on entering (health, power, armour; skills and perks kept). So inside band 1–10 everyone
  fights at level 10 or below, and a level-8 there only ever meets level-10 strength. Scaling
  follows the band the unit stands in, re-checked every step, with a 3 second grace when crossing
  to a deeper band so a fight on the line doesn't flicker between strengths.
- **Combat holds the scaling:** a player in combat who steps back into a shallower band is scaled
  down immediately; one who steps deeper keeps the shallower ceiling until out of combat, so
  nobody can stand on the line, hit at full strength and step back.
- **Death drops** (everything, for the killer to loot) only happen in the Wilderness and are
  decided by the region that owns the dead unit. A death caused across a region border is applied
  by the owner when the damage request arrives, so the loot pile is always in one region.

None of this adds a system: `can_harm` is a comparison in code that already runs, and scaling is
a stat multiplier set when the band changes.

## Match instances

Dungeons, capture the flag and other team modes (#77) run as **match instances**: small,
short-lived sims in the same server process as the world.

- **Made per match, thrown away after.** An instance is a `Sim` on a map (the Barrow, a capture
  the flag map) with a `Mode` resource that holds teams, scores, the timer and respawn points.
  Measured: 0.24 ms to make on the map's shared land (`Sim::on`, #167), 0.1 ms to tear down,
  0.24 MB each.
- **Teams:** players in a match get a team faction, so `can_harm` lets opposing teams fight with
  no death stakes. Defeat respawns you at your team's spawn point after a few seconds; nothing
  drops and nothing is lost.
- **Many per thread:** one pool thread steps its whole list of instances each tick. At 0.023 ms an
  instance, one thread could step several hundred; cap it at 100 a thread so a boss fight with
  every effect going never misses the tick.
- **Entering and leaving** is the same handoff as crossing a region border: the world region
  removes the player and sends their state to the instance, and back again at the end, to the spot
  they left from.
- **The same determinism holds**, so a match can be replayed from its seed and inbox for disputes
  and debugging.

## PvP hit checks and lag compensation

A player sees others where they were about 100 ms ago: the view is interpolated between snapshots
(#87), on top of their ping. Without help, a well-aimed shot misses on the server.

- **History:** each player keeps the last second (60 steps) of where they stood in a ring buffer
  (`sim::Trail`, #169). Only players are ever rewound, so monsters keep none, and the record costs
  about 0.3% of the players scene's instructions. (Measured for every unit in #73: 0.015 ms a step
  and 2.9 MB at 2,200 units.) Facing isn't rewound: guards and blocks face where they face now.
- **Rewind:** the client sends `Command::Viewing { tick }` with each batch of commands, the tick
  its view is showing; the server keeps how many steps behind that was when it arrived. When a player's aimed shot,
  cone or ground attack is checked **against another player**, the target is checked where it
  stood at that tick, up to **250 ms** back; older than that (a bad connection) uses 250 ms. The
  rewind is only for the hit test; the target never moves back.
- **Monsters aren't rewound.** The server's monsters are only ever where the server says, and PvE
  is forgiving enough at these pings.
- **What it costs:** a lookup into the ring per player-against-player hit test, nothing else. The
  known trade-off is the one every shooter makes: someone who just ducked behind a wall can still
  be hit by a shot fired before they got there. Capping the rewind at 250 ms keeps that to a
  quarter-second.
- **Warned blows keep their rules:** telegraphed attacks and dodges resolve on the server's own
  time, as today, so dodging a warned blow is never undone by someone else's lag.

## Running a server today

`cargo run -p server --release -- --bind 0.0.0.0:7878` runs the valley headless on Linux (or a
Mac): no window, GPU or Metal, stepping at 60 Hz and printing step and view times every ten
seconds (#85). `--crowd 2000` adds wandering monsters to load it; `--seed` picks the world.

- **Transport:** plain TCP, each message a 4-byte little-endian length then postcard
  (`server::net`). A client says `Hello` with its name and `protocol::VERSION`, gets `Welcome`
  (or `Refused` for another version), sends `Command`s and gets a `View` (snapshot and combat
  view) every step. QUIC (#86) replaces the transport and keeps the messages.
- **Players:** joining with a name already in the world takes that character back; a player who
  leaves stays standing where they were (until accounts, #88). A connection more than two
  seconds of views behind is dropped.
- **Solo play** is the same server in-process: the client's `server::Host` joins it as a local
  player, and others could join it over the network.
- **Measured** (cloud Linux box, release, 2,000 wandering monsters, 25 October 2026): the step
  takes 1.1 to 1.4 ms, inside the 2 ms budget. Each player's view costs about 1.1 ms to build
  and encode and is about 10 MB/s, because every view is a full snapshot of everything in range
  every step. Views from the interest grid, deltas and a 20 to 30 Hz send rate (#34) are what
  bring that down before more than a handful of players join.

## Load test

`cargo run -p server --release --example load_test -- --players 50,200,500` joins that many bots
(`server::bots`) over the real transport into a valley with 2,000 monsters. The bots wander, then
chase, target and fight the nearest monster, use skills and respawn. It prints p50/p99 of the step,
building views, encoding them and the whole tick, and bytes out (#74). Measured 25 October 2026 on
a 4-core cloud Linux box, with the bots' own reader threads competing for the same cores:

| Players | Ticks/s | Step p50 / p99 ms | Build views p50 / p99 | Encode p50 / p99 | Tick p50 / p99 | Out per player | Out total |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 16.8 | 1.24 / 2.11 | 32.05 / 45.58 | 24.10 / 34.06 | 57.35 / 81.43 | 2994 KB/s | 149.7 MB/s |
| 200 | 3.3 | 1.45 / 2.81 | 166.57 / 215.97 | 121.22 / 142.96 | 291.75 / 360.73 | 699 KB/s | 139.7 MB/s |
| 500 | 1.2 | 1.59 / 5.46 | 482.86 / 591.31 | 381.46 / 429.91 | 869.01 / 1026.68 | 340 KB/s | 170.1 MB/s |

Since #189 a snapshot shares each unit's look instead of copying it, which took building views at
200 players to 91.58 / 111.34 ms (5.4 ticks a second), with the same bytes on the wire.

The step itself stays under 2 ms even at 500 players; the ticks are slow because of the views.
A full snapshot and combat view for every player, every tick, is nearly all of it. Out per player
falls only because the server manages fewer ticks a second. #34 (views from the interest grid,
deltas, 20 to 30 Hz) is the fix, and this table is the before. The `players` scene in
`crates/sim/examples/perf_gate.rs` (200 players and 2,000 monsters, a view each per step) is in
the CI instruction-count gate, so per-player view costs can't creep back up unnoticed.

## The region host today

`server::regions::Realm` runs a world of regions, each a `Sim::region` on its own thread
(`sim::region` is a region's side; #168). Each tick every region, side by side, takes in the units
handed to it, the requests aimed at its units and its neighbours' ghosts, then steps; the host
then routes what each produced, always in region order. A region sends its neighbours a ghost's
name, body and look only when they don't have it yet; after that, only what moves.

- `cargo test -p server --test regions`: a player walks across a border and keeps their level,
  XP, coins and auras; a blow across a border lands in the other region; a wolf turns back at
  its border; two runs with the same seeds and commands give the same worlds.
- `cargo run -p server --release --example realm`: four regions of 50 players and 500 monsters,
  their crowds packed against the borders, so about 500 ghosts go each way every tick. Cloud
  Linux, 25 October 2026: tick 1.8 ms mean, slowest region step 0.75 ms, ghosts and handoffs
  0.46 ms mean and 1.5 ms p99 a region, about 19 handoffs a second.
- Not yet (#208): the headless server and solo host still run one region; the combat view
  leaves ghosts out; a hit across a border doesn't build the attacker's rage or count toward
  their XP, because the attacker is only a ghost where it lands.

## Follow-up work

- #167 (done): each map's land is shared between sims.
- #168 (done): the region host, several region sims on threads in one process, with global unit
  ID blocks, ghosts and border handoff (below).
- #208: serving a multi-region world over the network, ghosts in the combat view, attacker-side
  credit for hits across a border, and sleeping quiet regions.
- #169: lag compensation, a position history and rewound PvP hit tests.
- #170: region layers and region player caps.
- Already filed: #34 (views from the grid, deltas, 20–30 Hz), #33 (sleeping regions, no
  whole-world scans), #76 (Wilderness bands, with `can_harm` and scaling above), #77 (team modes on
  match instances), #85 (headless server), #86 (QUIC), #74 (bot load test).
