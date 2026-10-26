//! A world of several regions, each its own deterministic `Sim` on its own thread (#168,
//! docs/SERVER.md "Regions").
//!
//! Every tick, each region in parallel takes what the others sent it last tick (units handed
//! over, harm aimed at its units, its neighbours' ghosts) and steps. Then the host collects what
//! each produced and routes it. A region's inputs are always in the same order (by unit ID, or
//! by sending region), so the same seeds and commands give the same world, however the threads
//! are scheduled.
//!
//! A region past its player cap opens a **layer** (#170): another copy of the same ground, its
//! own region with its own thread and block of IDs, where new arrivals go. The regions next to it
//! see the layer paired with theirs, and layers fold back into the one before them once both are
//! quiet. A ground that is never layered (the Wilderness) turns players back at its cap instead.

use crossbeam_channel::{unbounded, Receiver, Sender};
use protocol::{Command, Event, Snapshot, UnitId};
use sim::region::{Bounds, GhostState, Request, UnitRecord, BLOCK, GHOST_BAND, HANDOFF, REGIONS};
use sim::Sim;
use std::collections::{BTreeSet, HashMap};
use std::thread::JoinHandle;
use std::time::Instant;

/// What a region takes in at the start of a tick.
#[derive(Default)]
struct TickIn {
    arrivals: Vec<UnitRecord>,
    requests: Vec<Request>,
    ghosts: Vec<GhostState>,
    commands: Vec<(UnitId, Command)>,
    /// The layers around it changed, so each neighbour is sent every ghost whole again.
    regroup: bool,
}

/// What a region gives out at the end of one.
struct TickOut {
    /// Units handed over, and the ground each goes to.
    departures: Vec<(u32, UnitRecord)>,
    /// Ghosts for each neighbouring ground.
    ghosts: Vec<(u32, Vec<GhostState>)>,
    requests: Vec<Request>,
    /// What happened in the region this tick.
    events: Vec<Event>,
    /// Players living here once the departures have gone.
    players: usize,
    time: f64,
    step_ms: f64,
    exchange_ms: f64,
}

enum Job {
    Tick(TickIn),
    With(Box<dyn FnOnce(&mut Sim) + Send>),
}

struct Worker {
    id: u32,
    /// The region whose ground this is: its own ID, or the first copy's for a layer.
    ground: u32,
    /// 0 for the first copy of the ground, then 1, 2... for its layers.
    layer: u32,
    bounds: Bounds,
    players: usize,
    jobs: Sender<Job>,
    done: Receiver<TickOut>,
    thread: Option<JoinHandle<()>>,
}

/// How the last tick went.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealmStats {
    pub ticks: u64,
    /// The slowest region's step.
    pub step_ms: f64,
    /// The most a region spent on ghosts, handoffs and requests, before and after its step.
    pub exchange_ms: f64,
    /// The whole tick, waiting for every region and routing between them.
    pub tick_ms: f64,
    /// Units handed over and requests sent between regions.
    pub handoffs: usize,
    pub requests: usize,
    /// Ghosts published, all regions together.
    pub ghosts: usize,
    /// Regions running, layers included.
    pub regions: usize,
    /// Players turned back at a full ground that is never layered.
    pub turned_back: usize,
}

/// How a world opens layers when a ground passes its player cap.
pub struct Layering {
    /// Players a region holds before the next arrival goes to a layer.
    pub cap: usize,
    /// Grounds never layered (the Wilderness): at the cap, players are turned back at the border.
    pub unlayered: Vec<u32>,
    /// Makes a new layer of `ground` as region `id`, with its own monsters, the way the first
    /// copy was made (from [`Sim::region`] with that ID and the same bounds).
    pub open: Box<dyn FnMut(u32, u32) -> Sim + Send>,
    /// Ticks a closed layer's region ID waits before a new layer may take it again (#274), and
    /// with it the IDs of its block; [`REUSE_AFTER`] is a minute.
    pub reuse_after: u64,
}

/// A minute: long after anyone's client has forgotten the units it saw with those IDs (ten
/// seconds, `protocol::wire`), and after any blow in flight from them has landed.
pub const REUSE_AFTER: u64 = 3600;

impl Layering {
    /// No cap: every ground is one region.
    pub fn none() -> Layering {
        Layering { cap: usize::MAX, unlayered: Vec::new(), open: Box::new(|_, _| unreachable!("a world without layers opened one")), reuse_after: REUSE_AFTER }
    }
}

/// How often quiet layers are looked at for folding back.
const MERGE_EVERY: u64 = 60;

/// Several regions of one world, stepped together.
pub struct Realm {
    workers: Vec<Worker>,
    /// Each original ground and the regions near it, for the layers opened later.
    layout: Vec<(u32, Bounds)>,
    layering: Layering,
    /// The next region ID never used yet.
    next_id: u32,
    /// Region IDs of layers closed, oldest first, with the tick each closed: taken again once
    /// `layering.reuse_after` has passed and no unit from their block is left anywhere (#274).
    closed: std::collections::VecDeque<(u32, u64)>,
    tick: u64,
    /// Units living outside their own block's region, because they crossed.
    moved: HashMap<UnitId, u32>,
    next: Vec<TickIn>,
    /// Each region's events from the last tick.
    events: Vec<Vec<Event>>,
    stats: RealmStats,
}

impl Realm {
    /// Starts a thread for each region. Each sim comes from [`Sim::region`], with its own ID
    /// and ground, all at the same tick.
    pub fn new(sims: Vec<Sim>) -> Realm {
        Realm::layered(sims, Layering::none())
    }

    /// Like [`Realm::new`], opening layers as `layering` says.
    pub fn layered(sims: Vec<Sim>, layering: Layering) -> Realm {
        let layout: Vec<(u32, Bounds)> = sims.iter().map(|s| s.region_info()).map(|r| (r.id, r.bounds.expect("each region owns some ground"))).collect();
        let tick = sims.first().map_or(0, |s| s.tick());
        assert!(sims.iter().all(|s| s.tick() == tick), "regions step together from the same tick");
        let next_id = layout.iter().map(|&(id, _)| id + 1).max().unwrap_or(0);
        let mut realm = Realm {
            workers: Vec::new(),
            layout,
            layering,
            next_id,
            closed: Default::default(),
            tick,
            moved: HashMap::new(),
            next: Vec::new(),
            events: Vec::new(),
            stats: RealmStats::default(),
        };
        for mut sim in sims {
            let id = sim.region_info().id;
            let players = sim.players().len();
            realm.start(sim, id, 0, players);
        }
        realm.stats.regions = realm.workers.len();
        realm
    }

    /// Starts a region's thread.
    fn start(&mut self, sim: Sim, ground: u32, layer: u32, players: usize) -> usize {
        let id = sim.region_info().id;
        let bounds = self.layout.iter().find(|&&(g, _)| g == ground).expect("a ground of this world").1;
        // The grounds whose ground comes within a ghost's reach of this one's.
        let others: Vec<(u32, Bounds)> = self.layout.iter().copied().filter(|&(o, _)| o != ground).collect();
        let near: Vec<(u32, Bounds)> = others.iter().copied().filter(|(_, b)| gap(*b, bounds) <= GHOST_BAND).collect();
        let (jobs, job_rx) = unbounded();
        let (done_tx, done) = unbounded();
        let thread = std::thread::Builder::new()
            .name(format!("Aldermoor region {id}"))
            .spawn(move || work(sim, others, near, job_rx, done_tx))
            .expect("region thread starts");
        self.workers.push(Worker { id, ground, layer, bounds, players, jobs, done, thread: Some(thread) });
        self.next.push(TickIn::default());
        self.events.push(Vec::new());
        self.workers.len() - 1
    }

    /// The layers of `ground`, in order, the first copy first.
    pub fn layers(&self, ground: u32) -> Vec<u32> {
        self.workers.iter().filter(|w| w.ground == ground).map(|w| w.id).collect()
    }

    /// Players living in a region, as of the last tick and the arrivals since.
    pub fn players_in(&self, region: u32) -> usize {
        self.index(region).map_or(0, |i| self.workers[i].players)
    }

    /// The highest layer number of `ground`.
    fn last_layer(&self, ground: u32) -> u32 {
        self.workers.iter().filter(|w| w.ground == ground).map(|w| w.layer).max().unwrap_or(0)
    }

    /// Whether two regions of neighbouring grounds see each other: each layer goes with the
    /// same layer next door, and the last layer of the ground with fewer goes with all the rest.
    fn paired(&self, a: &Worker, b: &Worker) -> bool {
        a.layer.min(self.last_layer(b.ground)) == b.layer.min(self.last_layer(a.ground))
    }

    /// Where a player arriving on `ground` goes: the first layer below its cap, opening a new one
    /// when they're all full, or nowhere when the ground is full and never layered.
    fn place(&mut self, ground: u32) -> Option<usize> {
        let cap = self.layering.cap;
        if let Some(i) = self.workers.iter().position(|w| w.ground == ground && w.players < cap) {
            return Some(i);
        }
        if self.layering.unlayered.contains(&ground) {
            return None;
        }
        // Out of IDs, the ground is full for now, as if it were never layered.
        let id = self.free_id()?;
        let mut sim = (self.layering.open)(ground, id);
        assert_eq!(sim.region_info().id, id, "a layer is made as the region it was asked for");
        sim.set_tick(self.tick);
        let layer = self.last_layer(ground) + 1;
        let players = sim.players().len();
        let i = self.start(sim, ground, layer, players);
        self.regroup();
        Some(i)
    }

    /// A region ID for a new layer: a closed layer's whose block is empty everywhere, after it
    /// has waited long enough, or else one never used. `None` when there are none.
    fn free_id(&mut self) -> Option<u32> {
        let ready = self.closed.iter().take_while(|&&(_, at)| self.tick - at >= self.layering.reuse_after).count();
        let regions: Vec<u32> = self.workers.iter().map(|w| w.id).collect();
        for n in 0..ready {
            let id = self.closed[n].0;
            if !regions.iter().any(|&r| self.with(r, move |sim| sim.holds_block(id))) {
                self.closed.remove(n);
                // Whatever's still noted about its old units is about nobody now.
                self.moved.retain(|unit, _| unit / BLOCK != id);
                return Some(id);
            }
        }
        if self.next_id < REGIONS {
            self.next_id += 1;
            return Some(self.next_id - 1);
        }
        None
    }

    /// Every region sends its neighbours whole ghosts next tick, because who pairs with whom
    /// has changed.
    fn regroup(&mut self) {
        for t in &mut self.next {
            t.regroup = true;
        }
    }

    fn index(&self, region: u32) -> Option<usize> {
        self.workers.iter().position(|w| w.id == region)
    }

    /// The ground `p` is on (its first copy's region).
    pub fn region_at(&self, p: glam::Vec2) -> Option<u32> {
        self.layout.iter().find(|(_, b)| b.contains(p)).map(|&(id, _)| id)
    }

    /// The region a unit lives in now.
    pub fn region_of(&self, id: UnitId) -> u32 {
        self.moved.get(&id).copied().unwrap_or(id / BLOCK)
    }

    /// Runs `f` on a region's sim, on its thread, between ticks.
    pub fn with<R: Send + 'static>(&self, region: u32, f: impl FnOnce(&mut Sim) -> R + Send + 'static) -> R {
        let w = &self.workers[self.index(region).expect("a region of this world")];
        let (tx, rx) = crossbeam_channel::bounded(1);
        w.jobs
            .send(Job::With(Box::new(move |sim| {
                let _ = tx.send(f(sim));
            })))
            .expect("region thread running");
        rx.recv().expect("region thread running")
    }

    /// Runs `f` on the sim where a unit lives now.
    pub fn with_unit<R: Send + 'static>(&self, id: UnitId, f: impl FnOnce(&mut Sim, UnitId) -> R + Send + 'static) -> R {
        self.with(self.region_of(id), move |sim| f(sim, id))
    }

    /// Adds a player wherever `at` is, in the first layer there with room. `None` off the map,
    /// or on a full ground that is never layered.
    pub fn spawn_player(&mut self, name: &str, at: glam::Vec2) -> Option<UnitId> {
        let i = self.place(self.region_at(at)?)?;
        self.workers[i].players += 1;
        let name = name.to_string();
        Some(self.with(self.workers[i].id, move |sim| sim.spawn_player(&name, at, 0.0)))
    }

    /// Queues a command for the region where its unit lives.
    pub fn command(&mut self, from: UnitId, c: Command) {
        if let Some(i) = self.index(self.region_of(from)) {
            self.next[i].commands.push((from, c));
        }
    }

    /// The world as `viewer` sees it, from the region it's in: that region's units and its
    /// neighbours' ghosts.
    pub fn snapshot(&self, viewer: UnitId) -> Snapshot {
        let mut snapshot = self.with_unit(viewer, |sim, id| sim.snapshot(Some(id)));
        snapshot.events = self.index(self.region_of(viewer)).map(|i| self.events[i].clone()).unwrap_or_default();
        snapshot
    }

    /// What happened in a region last tick.
    pub fn events(&self, region: u32) -> &[Event] {
        self.index(region).map_or(&[], |i| &self.events[i])
    }

    pub fn stats(&self) -> RealmStats {
        self.stats
    }

    /// Steps every region once, side by side, then hands on what crosses between them.
    pub fn step(&mut self) {
        let start = Instant::now();
        let inputs = std::mem::replace(&mut self.next, self.workers.iter().map(|_| TickIn::default()).collect());
        for (w, input) in self.workers.iter().zip(inputs) {
            w.jobs.send(Job::Tick(input)).expect("region thread running");
        }
        let outs: Vec<TickOut> = self.workers.iter().map(|w| w.done.recv().expect("region thread running")).collect();
        self.tick += 1;
        let mut stats = RealmStats { ticks: self.stats.ticks + 1, ..Default::default() };
        let mut crossing = Vec::new();
        // In region order, so every region's inbox is in the same order each run. Ghosts all go
        // first, paired as the layers stood for this step.
        for (n, out) in outs.into_iter().enumerate() {
            self.events[n] = out.events;
            self.workers[n].players = out.players;
            stats.step_ms = stats.step_ms.max(out.step_ms);
            stats.exchange_ms = stats.exchange_ms.max(out.exchange_ms);
            for (to, ghosts) in out.ghosts {
                stats.ghosts += ghosts.len();
                let (from, mut last) = (&self.workers[n], None);
                for (i, w) in self.workers.iter().enumerate() {
                    if w.ground == to && self.paired(from, w) {
                        // Shared between the layers that pair with it; the last takes the list.
                        if let Some(j) = last.replace(i) {
                            self.next[j].ghosts.extend(ghosts.iter().cloned());
                        }
                    }
                }
                if let Some(j) = last {
                    self.next[j].ghosts.extend(ghosts);
                }
            }
            crossing.push((self.workers[n].id, out.departures, out.requests, out.time));
        }
        for (from, departures, requests, time) in crossing {
            for (to, record) in departures {
                let Some(back) = self.index(from) else { continue };
                let Some(i) = self.arrive(back, to, record.player.is_some()) else {
                    // Turned back at a full ground: put back just inside the ground it left.
                    stats.turned_back += 1;
                    self.next[back].arrivals.push(turn_back(record, self.workers[back].bounds));
                    continue;
                };
                stats.handoffs += 1;
                let id = self.workers[i].id;
                // For the step it takes to arrive, the region it left still sees it.
                if let Some(g) = record.ghost(id, time) {
                    self.next[back].ghosts.push(g);
                }
                self.moved_to(record.id, id);
                if record.player.is_some() {
                    self.workers[i].players += 1;
                }
                self.next[i].arrivals.push(record);
            }
            for r in requests {
                stats.requests += 1;
                // To wherever the target lives now, which may not be where its ghost said.
                if let Some(i) = self.index(self.region_of(r.target)) {
                    self.next[i].requests.push(r);
                }
            }
        }
        if self.tick.is_multiple_of(MERGE_EVERY) {
            self.merge_quiet();
        }
        stats.regions = self.workers.len();
        stats.tick_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.stats = stats;
    }

    /// Which region of `ground` a unit leaving region `from` goes to: the layer paired with the
    /// one it leaves when that has room (so a party crossing together stays together), or else
    /// wherever [`place`](Realm::place) puts it. Anything but a player just follows the pairing.
    fn arrive(&mut self, from: usize, ground: u32, player: bool) -> Option<usize> {
        let paired = (0..self.workers.len()).find(|&i| self.workers[i].ground == ground && self.paired(&self.workers[from], &self.workers[i]))?;
        if !player || self.workers[paired].players < self.layering.cap {
            return Some(paired);
        }
        self.place(ground)
    }

    fn moved_to(&mut self, id: UnitId, region: u32) {
        if region == id / BLOCK {
            self.moved.remove(&id);
        } else {
            self.moved.insert(id, region);
        }
    }

    /// Folds the last layer of each ground into the one before it, once the two together hold
    /// no more than half a cap of players. Its players move over where they stand; its
    /// monsters go with it.
    fn merge_quiet(&mut self) {
        let cap = self.layering.cap;
        let grounds: Vec<u32> = self.layout.iter().map(|&(g, _)| g).collect();
        for ground in grounds {
            let last = self.last_layer(ground);
            if last == 0 {
                continue;
            }
            let at = |realm: &Realm, layer: u32| realm.workers.iter().position(|w| w.ground == ground && w.layer == layer).expect("layers are numbered in a row");
            let (l, p) = (at(self, last), at(self, last - 1));
            if self.workers[l].players + self.workers[p].players > cap / 2 {
                continue;
            }
            let (gone, into) = (self.workers[l].id, self.workers[p].id);
            let records: Vec<UnitRecord> = self.with(gone, |sim| sim.players().into_iter().filter_map(|id| sim.export(id)).collect());
            let pending = std::mem::take(&mut self.next[l]);
            for r in records.iter().chain(&pending.arrivals) {
                self.moved_to(r.id, into);
            }
            self.workers[p].players += records.len() + pending.arrivals.iter().filter(|r| r.player.is_some()).count();
            self.next[p].arrivals.extend(records);
            self.next[p].arrivals.extend(pending.arrivals);
            self.moved.retain(|_, r| *r != gone);
            self.closed.push_back((gone, self.tick));
            let mut w = self.workers.remove(l);
            self.next.remove(l);
            self.events.remove(l);
            // Closing the job channel ends the thread's loop.
            w.jobs = unbounded().0;
            if let Some(t) = w.thread.take() {
                let _ = t.join();
            }
            for r in pending.requests {
                if let Some(i) = self.index(self.region_of(r.target)) {
                    self.next[i].requests.push(r);
                }
            }
            self.regroup();
        }
    }
}

/// A unit that crossed onto a full ground, put back just inside the edge of `bounds`.
fn turn_back(mut record: UnitRecord, bounds: Bounds) -> UnitRecord {
    if let Some(t) = &mut record.transform {
        let inside = HANDOFF.min((bounds.max - bounds.min).min_element() / 2.0) * 0.25;
        t.pos.x = t.pos.x.clamp(bounds.min.x + inside, bounds.max.x - inside);
        t.pos.z = t.pos.z.clamp(bounds.min.y + inside, bounds.max.y - inside);
    }
    record
}

impl Drop for Realm {
    fn drop(&mut self) {
        for w in &mut self.workers {
            // Closing the job channel ends the thread's loop.
            let (dead, _) = unbounded();
            w.jobs = dead;
        }
        for w in &mut self.workers {
            if let Some(t) = w.thread.take() {
                let _ = t.join();
            }
        }
    }
}

/// How far apart two pieces of ground are, 0 when they touch.
fn gap(a: Bounds, b: Bounds) -> f32 {
    (a.min - b.max).max(b.min - a.max).max(glam::Vec2::ZERO).length()
}

fn work(mut sim: Sim, others: Vec<(u32, Bounds)>, near: Vec<(u32, Bounds)>, jobs: Receiver<Job>, done: Sender<TickOut>) {
    // What each neighbour was sent last tick, which it needn't be sent the look of again.
    let mut known: Vec<BTreeSet<UnitId>> = near.iter().map(|_| BTreeSet::new()).collect();
    while let Ok(job) = jobs.recv() {
        let input = match job {
            Job::With(f) => {
                f(&mut sim);
                continue;
            }
            Job::Tick(input) => input,
        };
        let start = Instant::now();
        if input.regroup {
            known.iter_mut().for_each(BTreeSet::clear);
        }
        sim.import(input.arrivals);
        sim.apply_requests(input.requests);
        sim.set_ghosts(input.ghosts);
        for (id, c) in input.commands {
            sim.command(id, c);
        }
        let stepping = Instant::now();
        sim.step();
        let step_ms = stepping.elapsed().as_secs_f64() * 1000.0;
        // Ghosts first, so a unit about to leave is still published as this region's.
        let ghosts: Vec<(u32, Vec<GhostState>)> = near.iter().zip(&known).map(|(&(r, b), k)| (r, sim.ghosts_near(b, k))).collect();
        for ((_, list), k) in ghosts.iter().zip(&mut known) {
            k.clear();
            k.extend(list.iter().map(|g| g.id));
        }
        let departures = sim.departures(&others).into_iter().filter_map(|(id, to)| sim.export(id).map(|r| (to, r))).collect();
        let players = sim.players().len();
        let requests = sim.take_requests();
        let events = sim.take_events();
        let exchange_ms = start.elapsed().as_secs_f64() * 1000.0 - step_ms;
        let out = TickOut { departures, ghosts, requests, events, players, time: sim.time(), step_ms, exchange_ms };
        if done.send(out).is_err() {
            return;
        }
    }
}
