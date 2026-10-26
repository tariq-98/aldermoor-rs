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
| Layers: 600 players in a town capped at 200, so three layers of 1,000 monsters each, beside open country (`cargo run -p server --release --example layers`, four regions on four cores) | 2.29 ms mean, 4.86 ms p99 tick; 1.43 ms mean, 3.38 ms p99 slowest step; 0.28 ms ghosts and handoffs |
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

### Over QUIC

Linux server in an arm64 container on the M1 Air (8 cores), release build, 26 October 2026,
`cargo run -p server --release --example load_test -- --players 1,50,200 --monsters 2000`
(bots over QUIC on loopback; "out" is view payload before QUIC's own headers):

| Players | Ticks/s | Tick p50 / p99 ms | Out per player | Out total |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 60.0 | 3.25 / 8.06 | 13 KB/s | 0.0 MB/s |
| 50 | 60.0 | 5.73 / 22.01 | 24 KB/s | 1.2 MB/s |
| 200 | 60.0 | 8.84 / 14.97 | 68 KB/s | 13.6 MB/s |

The Mac client against that server (`--connect`, the smoke route with its wolf fight, a few wolves
nearby): 186 KB sent by the server's network interface over a 45 s run including start-up, all
QUIC headers, acks and keep-alives counted, so about 6 KB/s while connected.

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
- **Layers** (#170): when a region passes its player cap (a capital on launch night), the world
  opens another copy of its ground, a layer, and new arrivals go there. A layer is a region like
  any other: its own thread, its own monsters, its own region ID and block of unit IDs, so a
  unit's ID never changes while it's in the world. Unit IDs leave room for 256 regions, so a
  closed layer's ID is taken again by a later layer (#274), but only once a minute has passed
  (long after clients forget units they saw, ten seconds) and no unit from its block is left
  in any region. With none free, a full ground turns new arrivals back, as the Wilderness does. Players in different layers of a ground don't see
  each other. Next door, layer *n* pairs with the neighbour's layer *n*, and the last layer of
  the ground with fewer pairs with all the rest, so open country beside a layered town sees every
  layer's side of the border. Crossing over, a player goes to the layer paired with the one they
  left while it has room, so people walking in together stay together, and otherwise to the
  first layer below the cap. Once a second the host looks at each ground's last layer, and when
  it and the one before it hold no more than half a cap together, its players move into the one
  before where they stand and it closes. Groups and companies aren't built yet; when they are,
  placing a player should go to a layer with their group in it first.
- **The Wilderness is never layered**, so a fight with real death stakes can't be dodged by
  changing layer. At its cap a player stepping over its border is put back just inside the
  ground they came from, and joining inside it is refused, until someone leaves.

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
seconds (#85). `--crowd 2000` adds wandering monsters to load it; `--seed` picks the world;
`--regions 2` or `4` runs it as a realm of regions (#208).

- **Transport:** QUIC over UDP (`quinn`, #86), with the same postcard messages as before.
  A client opens one reliable stream and says `Hello` with its name and `protocol::VERSION`, gets
  `Welcome` (or `Refused` for another version), then sends `Command`s and `Ack`s on it, each a
  4-byte little-endian length then postcard. Views go the other way unreliably, a `View` every
  third step (20 a second, each player on their own third of the steps): one datagram when it
  fits, which is nearly always, else a one-way stream of its own (the first view, a crowd
  arriving). A lost or late view costs only its own update, because the next is a delta against
  the last view the client acknowledged, not the last one sent. One small runtime (two threads)
  carries every connection in a process.
- **Certificates:** the server's identity is a self-signed certificate for `aldermoor` and its
  private key, made the first time (`--cert aldermoor.der`, key beside it as `aldermoor.key`,
  readable only by its owner). Players get the `.der` file, never the key, and the client trusts
  exactly it with ordinary TLS checks (`--server-cert`); a server with another certificate is
  refused. A certificate from a real authority waits for accounts and a host name (#88).
- **Playing on a server:** `client --connect HOST:PORT --server-cert aldermoor.der --name NAME`
  plays the same as solo, through the same `server::Host` interface (`Host::connect`). Over the
  network the host's clock is the views' arrival, so it carries the network's jitter.
- **Views on the wire** (`protocol::wire`, #34): a unit's kind, body, name and look go once, in an
  intro, when it comes into view or changes (and are remembered for ten seconds after it leaves,
  so a unit wandering along the edge of view doesn't send them again). After that a view carries
  only what changed since the last view the client acknowledged: per unit, one byte saying which
  parts follow, then the position as centimetres from where the base's velocity was taking it,
  the change in velocity (to 5 cm/s) and in yaw (to 1/65536 of a turn), health to a tenth. Timers (cooldowns, auras, casts, poses) go as the tick they end or
  began, so they don't change while they run. The player's own state goes in five parts that
  each go only when they change. Units further from the player go out less often (#278): every
  view they change within 30 m, every second view out to 60 m, every fourth to 100 m and every
  eighth beyond, taking turns by ID so each view carries a share. One that's off its turn stays as
  the last view had it, so a late ack never makes it jump back. `server::net::Client` rebuilds whole `Snapshot`s and
  `CombatView`s (`protocol::wire::Decoder`), so bots and later the client see what they saw
  before. Each step that some player is due a view, the server puts everyone into one
  `protocol::wire::Frame` (`Sim::net_frame`), quantised once, and each player's encoder picks
  their view out of it (`Sim::encode_view`, #286): nothing is built twice, no unit's strings are
  copied unless its look or name changed, and an unchanged unit's look and combat detail are known
  by a version number. `cargo run -p server --release --example wire_bench` times one player's
  views both ways and checks they're the same.
- **Players:** joining with a name already in the world takes that character back; a player who
  leaves stays standing where they were (until accounts, #88). A connection more than two
  seconds of views behind is dropped.
- **Solo play** is the same server in-process: the client's `server::Host` joins it as a local
  player, and others could join it over the network.
- **Measured** (cloud Linux box, release, 2,000 wandering monsters, 25 October 2026): the step
  takes 1.1 to 1.4 ms, inside the 2 ms budget. Before #34 each player's view cost about 1.1 ms
  to build and encode and was about 10 MB/s, a full snapshot every step. With wire views at 20 Hz
  one player among the 2,000 monsters (1,037 in view) costs 0.28 ms to build, 0.18 ms to encode
  and 0.24 ms to decode a view, at 166 bytes a view (3.2 KB/s) standing still. Picked from the
  step's frame (#286), a view costs 0.1 ms, and the frame 0.6 ms a step however many players
  share it; 116 bytes a view.

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
falls only because the server manages fewer ticks a second. That table is the before. With wire
views at 20 Hz (#34; same box, 10 s a row, 25 October 2026):

| Players | Ticks/s | Step p50 / p99 ms | Build views p50 / p99 | Encode p50 / p99 | Tick p50 / p99 | Out per player | Out total |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 51.5 | 0.94 / 1.63 | 8.92 / 16.12 | 8.73 / 13.03 | 18.68 / 28.95 | 30 KB/s | 1.5 MB/s |
| 200 | 9.1 | 1.40 / 2.17 | 53.90 / 83.94 | 54.88 / 72.01 | 109.05 / 155.71 | 29 KB/s | 5.8 MB/s |
| 500 | 2.9 | 1.43 / 7.51 | 152.24 / 244.58 | 177.48 / 560.19 | 371.72 / 762.91 | 27 KB/s | 13.7 MB/s |

Out per player is now about 30 KB/s whatever the tick rate (a hundred times less at 200 players),
and 50 players nearly keep 60 ticks a second. Sending far units less often (#278) took out per
player to about 20 KB/s (19, 21 and 21 KB/s at 50, 200 and 500 bots; ticks a second unchanged).

What's left is CPU per view, not finding who's in it. Scanning the whole valley's 3,000 units
costs a snapshot 0.008 ms and a combat view 0.015 ms (a viewer with nobody near), so taking views
from the spatial grid would save almost nothing at region sizes and isn't done. The cost is in the
units actually in view: with 1,000 of them, about 0.3 ms to build the snapshot and combat view
and 0.2 ms to encode, on cores the bots share here.

Building views straight into the wire form from one shared frame a step (#286; same box, 10 s a
row, 26 October 2026):

| Players | Ticks/s | Step p50 / p99 ms | Build views p50 / p99 | Encode p50 / p99 | Tick p50 / p99 | Out per player | Out total |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 50 | 60.0 | 1.08 / 2.66 | 1.18 / 3.36 | 2.68 / 7.05 | 5.12 / 10.65 | 23 KB/s | 1.1 MB/s |
| 200 | 36.7 | 1.46 / 5.39 | 7.78 / 18.82 | 16.62 / 26.56 | 26.65 / 38.64 | 42 KB/s | 8.3 MB/s |
| 500 | 9.0 | 1.83 / 4.88 | 22.13 / 64.12 | 80.55 / 136.16 | 107.85 / 161.29 | 64 KB/s | 31.8 MB/s |

50 players keep the full 60 ticks a second and 200 four times the rate before. Out per player
grows with the tick rate the server keeps (it's about the same per tick). "Build views" is now the
frame (about 1 ms) and handing each view to its connection's thread; here those threads and the
200 bots' own share the server's 4 cores, so the next measure wants the bots on another machine.

Since #292 each player gets only the events about units in their view (or, for those with a place
and no unit, within sight), and a step's views are encoded across up to `server::MAX_ENCODERS`
threads (the cores left beside the sim's). The load test can't show that, because the bots already
fill this box: 200 of them kept 37 ticks a second with 3.4 of its 4 cores busy. Without bots,
`cargo run -p server --release --example crowd_bench` steps the perf gate's `players` scene and
encodes each player's view every third step, as the server does (same box, 26 October 2026):

| Players | Encoding on | Step ms | Frame ms | Views ms | Tick ms | Out per player |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 200 | 1 thread | 1.26 | 0.91 | 14.69 | 16.87 | 65 KB/s |
| 200 | 3 threads | 1.10 | 0.74 | 6.00 | 7.84 | 65 KB/s |
| 500 | 1 thread | 1.98 | 1.38 | 46.61 | 49.97 | 115 KB/s |
| 500 | 3 threads | 1.78 | 1.17 | 19.76 | 22.71 | 115 KB/s |

So 200 players fighting in one area fit a 60 Hz tick with room to spare, and about 400 would on
these 4 cores. Here everyone is in everyone's view, so it's the worst case: about 0.22 ms a view.
The `players` scene in
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
- **Serving a realm** (#208): `Server::start_realm` (and `server --regions 2` or `4`, which cut
  the valley into halves or quarters) runs a `Realm` behind the same `Host` and network players
  as one sim. A player joins the region at the spawn point; their commands go to the region they
  live in and their views come from it, across borders, each region's views built on its own
  thread from its own frame.
- **Ghosts in the combat view:** a ghost carries its action, cast, auras and defiance as its
  region published them, aged to the viewer's step, and its side, so a unit that changes side
  changes for its neighbours too. With about 2,000 ghosts a tick in the realm example, that
  takes ghosts and handoffs from 0.21 to 0.31 ms mean a region (0.42 to 0.73 ms p99; Linux in
  a container on the M1 Air, 26 October 2026, main and this change ABAB).
- **Fighting across a border:** a ghost carries its player's specialty, so their blows earn
  their share of a kill where it lands. When the monster dies its region pays each share where
  the player lives, at their own level there (`Landing::KillXp`). A blow landing across the
  border also tells the attacker's own region it was in a fight (`Landing::Struck`), so they
  stay in combat (and PvP combat) and remember whom they hit.
- **Quiet regions sleep:** a region with no players inside, no player ghosts at its border and
  nothing arriving, requested or commanded steps once a second and only moves its clock on in
  between (`Sim::idle`), so it's on its neighbours' tick when someone comes. Its monsters wander
  and heal at a sixtieth of the pace meanwhile. `RealmStats::asleep` counts them.

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
