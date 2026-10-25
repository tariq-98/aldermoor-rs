//! A world of several regions, each its own deterministic `Sim` on its own thread (#168,
//! docs/SERVER.md "Regions").
//!
//! Every tick, each region in parallel takes what the others sent it last tick (units handed
//! over, harm aimed at its units, its neighbours' ghosts) and steps. Then the host collects what
//! each produced and routes it. A region's inputs are always in the same order (by unit ID, or
//! by sending region), so the same seeds and commands give the same world, however the threads
//! are scheduled.

use crossbeam_channel::{unbounded, Receiver, Sender};
use protocol::{Command, Event, Snapshot, UnitId};
use sim::region::{Bounds, GhostState, Request, UnitRecord, BLOCK, GHOST_BAND};
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
}

/// What a region gives out at the end of one.
struct TickOut {
    /// Units handed over, and the region each goes to.
    departures: Vec<(u32, UnitRecord)>,
    /// Ghosts for each neighbour.
    ghosts: Vec<(u32, Vec<GhostState>)>,
    requests: Vec<Request>,
    /// What happened in the region this tick.
    events: Vec<Event>,
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
    bounds: Bounds,
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
}

/// Several regions of one world, stepped together.
pub struct Realm {
    workers: Vec<Worker>,
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
        let layout: Vec<(u32, Bounds)> = sims.iter().map(|s| s.region_info()).map(|r| (r.id, r.bounds.expect("each region owns some ground"))).collect();
        let tick = sims.first().map(|s| s.tick());
        assert!(sims.iter().all(|s| Some(s.tick()) == tick), "regions step together from the same tick");
        let workers = sims
            .into_iter()
            .enumerate()
            .map(|(i, sim)| {
                let (id, bounds) = layout[i];
                // The regions whose ground comes within a ghost's reach of this one's.
                let others: Vec<(u32, Bounds)> = layout.iter().copied().filter(|&(o, _)| o != id).collect();
                let near: Vec<(u32, Bounds)> = others.iter().copied().filter(|(_, b)| gap(*b, bounds) <= GHOST_BAND).collect();
                let (jobs, job_rx) = unbounded();
                let (done_tx, done) = unbounded();
                let thread = std::thread::Builder::new()
                    .name(format!("Aldermoor region {id}"))
                    .spawn(move || work(sim, others, near, job_rx, done_tx))
                    .expect("region thread starts");
                Worker { id, bounds, jobs, done, thread: Some(thread) }
            })
            .collect::<Vec<_>>();
        let next = workers.iter().map(|_| TickIn::default()).collect();
        let events = workers.iter().map(|_| Vec::new()).collect();
        Realm { workers, moved: HashMap::new(), next, events, stats: RealmStats::default() }
    }

    fn index(&self, region: u32) -> Option<usize> {
        self.workers.iter().position(|w| w.id == region)
    }

    /// The region whose ground `p` is on.
    pub fn region_at(&self, p: glam::Vec2) -> Option<u32> {
        self.workers.iter().find(|w| w.bounds.contains(p)).map(|w| w.id)
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

    /// Adds a player wherever `at` is.
    pub fn spawn_player(&mut self, name: &str, at: glam::Vec2) -> Option<UnitId> {
        let region = self.region_at(at)?;
        let name = name.to_string();
        Some(self.with(region, move |sim| sim.spawn_player(&name, at, 0.0)))
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
        let mut stats = RealmStats { ticks: self.stats.ticks + 1, ..Default::default() };
        // In region order, so every region's inbox is in the same order each run.
        for (n, out) in outs.into_iter().enumerate() {
            let from = self.workers[n].id;
            self.events[n] = out.events;
            stats.step_ms = stats.step_ms.max(out.step_ms);
            stats.exchange_ms = stats.exchange_ms.max(out.exchange_ms);
            for (to, ghosts) in out.ghosts {
                stats.ghosts += ghosts.len();
                if let Some(i) = self.index(to) {
                    self.next[i].ghosts.extend(ghosts);
                }
            }
            for (to, record) in out.departures {
                let Some(i) = self.index(to) else { continue };
                stats.handoffs += 1;
                // For the step it takes to arrive, the region it left still sees it.
                if let (Some(g), Some(back)) = (record.ghost(to, out.time), self.index(from)) {
                    self.next[back].ghosts.push(g);
                }
                if to == record.id / BLOCK {
                    self.moved.remove(&record.id);
                } else {
                    self.moved.insert(record.id, to);
                }
                self.next[i].arrivals.push(record);
            }
            for r in out.requests {
                stats.requests += 1;
                // To wherever the target lives now, which may not be where its ghost said.
                if let Some(i) = self.index(self.region_of(r.target)) {
                    self.next[i].requests.push(r);
                }
            }
        }
        stats.tick_ms = start.elapsed().as_secs_f64() * 1000.0;
        self.stats = stats;
    }
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
        let requests = sim.take_requests();
        let events = sim.take_events();
        let exchange_ms = start.elapsed().as_secs_f64() * 1000.0 - step_ms;
        let out = TickOut { departures, ghosts, requests, events, time: sim.time(), step_ms, exchange_ms };
        if done.send(out).is_err() {
            return;
        }
    }
}
