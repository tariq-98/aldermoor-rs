//! The simulation host: one thread steps the world 60 times a second at a fixed rate, takes
//! commands from a channel, and publishes each player's own snapshot and combat view.
//!
//! [`Server`] is the world and its players. Players join either in-process ([`Host`], the
//! client's built-in server for solo play) or over the network ([`net`], what the headless
//! `server` binary serves). Both see exactly the same views.

pub mod bots;
pub mod net;
pub mod regions;

use crossbeam_channel::{bounded, unbounded, Receiver, Sender, TrySendError};
use protocol::{CombatView, Command, Snapshot, UnitId};
use sim::Sim;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const STEP: Duration = Duration::from_nanos(1_000_000_000 / sim::TICK_RATE as u64);

/// Network players get a view every third step, 20 a second (#34). Each gets its own third, so
/// the views to build are spread over every step.
pub const SEND_EVERY: u64 = 3;

/// At most this many threads encode one step's views...
pub const MAX_ENCODERS: usize = 4;
/// ...each given at least this many, so a handful of players doesn't pay for starting threads.
const PER_ENCODER: usize = 8;

/// `work` on `items` in up to `threads` runs at once (this thread one of them), each of at
/// least [`PER_ENCODER`] items, with their results in order: how the server encodes a step's
/// views (#292).
pub fn across<T: Send, R: Send>(items: &mut [T], threads: usize, work: impl Fn(&mut [T]) -> R + Sync) -> Vec<R> {
    let per = items.len().div_ceil(threads.max(1)).max(PER_ENCODER);
    if items.len() <= per {
        return vec![work(items)];
    }
    std::thread::scope(|s| {
        let mut chunks = items.chunks_mut(per);
        let first = chunks.next().unwrap();
        let others: Vec<_> = chunks.map(|c| s.spawn(|| work(c))).collect();
        let mut results = vec![work(first)];
        results.extend(others.into_iter().map(|h| h.join().expect("an encoder panicked")));
        results
    })
}

/// Where new players appear: the path into Brambleford, facing the village.
const SPAWN: glam::Vec2 = glam::Vec2::new(6.0, 28.0);

/// Numbers about how the host is keeping up.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostStats {
    pub ticks: u64,
    /// Mean time spent stepping the sim, over the last second.
    pub step_ms: f64,
    /// Mean time spent building, and for remote players encoding, every player's view.
    pub view_ms: f64,
    /// Steps dropped because the host fell more than a quarter-second behind.
    pub skipped: u64,
    /// Players connected now.
    pub players: usize,
    /// Bytes of views handed to network connections, since the start.
    pub bytes_out: u64,
}

/// Where one tick's time went, for load tests (`Server::take_tick_times`).
#[derive(Clone, Copy, Debug, Default)]
pub struct TickTime {
    /// Stepping the sim.
    pub step_ms: f64,
    /// Building in-process players' snapshots and combat views, and the frame network players'
    /// views are picked from.
    pub build_ms: f64,
    /// Picking network players' views out of the frame and encoding them.
    pub encode_ms: f64,
    /// Bytes of views encoded this tick.
    pub bytes: usize,
}

/// Tick times kept for `take_tick_times`: the last minute.
const KEPT_TICKS: usize = 60 * sim::TICK_RATE as usize;

/// One player's latest view, for an in-process viewer to read whenever it likes.
#[derive(Default)]
struct Latest {
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    combat: Mutex<Option<Arc<CombatView>>>,
    /// The newest snapshot's sim time and the instant its step was due. Steps are due at exact
    /// intervals, so a viewer can tell the sim time at any instant without arrival jitter.
    clock: Mutex<Option<(f64, Instant)>>,
    events: Mutex<Vec<protocol::Event>>,
}

/// Where a player's views go.
enum Viewer {
    Local(Arc<Latest>),
    /// Encoded `net::ServerMsg::View` frames for a connection's writer. A connection more than
    /// [`net::BACKLOG`] frames behind is dropped.
    Remote(Remote),
}

struct Remote {
    frames: Sender<Vec<u8>>,
    /// One more than the last view the player said it has (0 for none yet), from its connection.
    acked: Arc<std::sync::atomic::AtomicU64>,
    encoder: protocol::wire::Encoder,
    /// Which of each [`SEND_EVERY`] steps it gets its view on.
    phase: u64,
}

enum Control {
    /// Adds a player (or, for a name already in the world, hands that character back) and
    /// replies with its unit.
    Join { name: String, viewer: Viewer, reply: Sender<UnitId> },
}

struct Shared {
    stats: Mutex<HostStats>,
    times: Mutex<Vec<TickTime>>,
    running: AtomicBool,
}

/// A running world on its own thread.
pub struct Server {
    commands: Sender<(UnitId, Command)>,
    control: Sender<Control>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Starts stepping `world`, with nobody in it yet.
    pub fn start(world: Sim) -> Server {
        let (commands, command_rx) = unbounded();
        let (control, control_rx) = unbounded();
        let shared = Arc::new(Shared { stats: Mutex::new(HostStats::default()), times: Mutex::new(Vec::new()), running: AtomicBool::new(true) });
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("Aldermoor simulation".into())
            .spawn(move || run(world, command_rx, control_rx, s))
            .expect("simulation thread starts");
        Server { commands, control, shared, thread: Some(thread) }
    }

    fn join(&self, name: &str, viewer: Viewer) -> Option<UnitId> {
        let (reply, answer) = bounded(1);
        self.control.send(Control::Join { name: name.to_string(), viewer, reply }).ok()?;
        answer.recv().ok()
    }

    /// A handle for queueing commands from any thread.
    pub fn commands(&self) -> Sender<(UnitId, Command)> {
        self.commands.clone()
    }

    pub fn stats(&self) -> HostStats {
        *self.shared.stats.lock().unwrap()
    }

    /// Each tick's timings since the last call (at most the last minute's), oldest first.
    pub fn take_tick_times(&self) -> Vec<TickTime> {
        std::mem::take(&mut *self.shared.times.lock().unwrap())
    }

    pub fn stop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One player's view of a world: the client's built-in server for solo play, or a server
/// across the network (`--connect`). Both look the same to the client.
pub struct Host {
    backend: Backend,
    latest: Arc<Latest>,
    pub player: UnitId,
}

enum Backend {
    Local(Server),
    Remote(Link),
}

/// A network player's connection, read on a thread of its own into the same [`Latest`] an
/// in-process player has.
struct Link {
    commands: net::Commands,
    closer: net::Closer,
    received: Arc<std::sync::atomic::AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Host {
    /// Starts the Brambleford valley with one player and returns once the first snapshot exists.
    pub fn start(seed: u64, player_name: &str) -> Host {
        Host::join(Server::start(Sim::brambleford(seed)), player_name)
    }

    /// Joins `server` as an in-process player and returns once its first snapshot exists.
    pub fn join(server: Server, player_name: &str) -> Host {
        let latest = Arc::new(Latest::default());
        let player = server.join(player_name, Viewer::Local(latest.clone())).expect("the simulation is running");
        let host = Host { backend: Backend::Local(server), latest, player };
        while host.snapshot().is_none() {
            std::thread::sleep(Duration::from_millis(1));
        }
        host
    }

    /// Joins the server at `addr` (whose certificate is `certificate`) over the network, and
    /// returns once its first view has arrived.
    pub fn connect(addr: impl std::net::ToSocketAddrs, certificate: &net::CertificateDer<'_>, player_name: &str) -> std::io::Result<Host> {
        let mut client = net::Client::connect(addr, certificate, player_name)?;
        let latest = Arc::new(Latest::default());
        let (player, commands, closer, received) = (client.player, client.commands(), client.closer(), client.received());
        let into = latest.clone();
        let thread = std::thread::Builder::new().name("Aldermoor connection".into()).spawn(move || loop {
            match client.next_view() {
                Ok((snapshot, combat)) => {
                    into.events.lock().unwrap().extend(snapshot.events.iter().cloned());
                    *into.combat.lock().unwrap() = Some(Arc::new(combat));
                    // Arrival stands in for the server's schedule, so this clock has the
                    // network's jitter in it.
                    *into.clock.lock().unwrap() = Some((snapshot.time, Instant::now()));
                    *into.snapshot.lock().unwrap() = Some(Arc::new(snapshot));
                }
                Err(e) => {
                    eprintln!("disconnected: {e}");
                    break;
                }
            }
        })?;
        let host = Host { backend: Backend::Remote(Link { commands, closer, received, thread: Some(thread) }), latest, player };
        let start = Instant::now();
        while host.snapshot().is_none() {
            if start.elapsed() > Duration::from_secs(10) {
                return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no view from the server"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(host)
    }

    /// The world this player is in, for others to join (`net::listen`); `None` over the
    /// network.
    pub fn server(&self) -> Option<&Server> {
        match &self.backend {
            Backend::Local(server) => Some(server),
            Backend::Remote(_) => None,
        }
    }

    /// Queues a command from the local player.
    pub fn send(&self, c: Command) {
        match &self.backend {
            Backend::Local(server) => {
                let _ = server.commands.send((self.player, c));
            }
            Backend::Remote(link) => {
                let _ = link.commands.send(c);
            }
        }
    }

    /// The most recent snapshot.
    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.latest.snapshot.lock().unwrap().clone()
    }

    /// Combat detail for everything the player can see, from the same step as `snapshot`:
    /// actions, casts, auras, projectiles, warnings and fields.
    pub fn combat(&self) -> Option<Arc<CombatView>> {
        self.latest.combat.lock().unwrap().clone()
    }

    /// The newest snapshot's sim time, and the instant its step was due on the host's
    /// schedule. Sim time at any other instant is that time plus the instants' difference.
    pub fn clock(&self) -> Option<(f64, Instant)> {
        *self.latest.clock.lock().unwrap()
    }

    /// Events since the last call, in order (snapshots only carry their own tick's events).
    pub fn take_events(&self) -> Vec<protocol::Event> {
        std::mem::take(&mut *self.latest.events.lock().unwrap())
    }

    /// The world's stats in-process. Over the network only `ticks` (the newest view's) and
    /// `bytes_out` (views received) are known.
    pub fn stats(&self) -> HostStats {
        match &self.backend {
            Backend::Local(server) => server.stats(),
            Backend::Remote(link) => HostStats {
                ticks: self.snapshot().map_or(0, |s| s.tick),
                players: 1,
                bytes_out: link.received.load(Ordering::Relaxed),
                ..HostStats::default()
            },
        }
    }

    pub fn stop(&mut self) {
        match &mut self.backend {
            Backend::Local(server) => server.stop(),
            Backend::Remote(link) => {
                link.closer.close();
                if let Some(t) = link.thread.take() {
                    let _ = t.join();
                }
            }
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(mut world: Sim, commands: Receiver<(UnitId, Command)>, control: Receiver<Control>, shared: Arc<Shared>) {
    let mut viewers: Vec<(UnitId, Viewer)> = Vec::new();
    // Characters stay in the world when their player leaves, and joining with the same name
    // takes them back (until accounts, #88).
    let mut names: HashMap<String, UnitId> = HashMap::new();
    let mut next = Instant::now();
    let mut window = (0u32, 0.0f64, 0.0f64);
    // The last few steps' events, for network players who get a view every few steps.
    let mut recent: std::collections::VecDeque<Vec<protocol::Event>> = std::collections::VecDeque::new();
    let mut joined = 0u64;
    // The last frame built, to lend the next what hasn't changed.
    let mut frame: Option<sim::NetFrame> = None;
    // Threads encoding views at once, this one among them: the cores left beside the sim's.
    let encoders = std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1)).clamp(1, MAX_ENCODERS);
    while shared.running.load(Ordering::Acquire) {
        let now = Instant::now();
        if now < next {
            std::thread::sleep(next - now);
            continue;
        }
        // Too far behind (the Mac slept, a debugger paused us): drop the backlog instead of racing.
        if now - next > Duration::from_millis(250) {
            shared.stats.lock().unwrap().skipped += ((now - next).as_nanos() / STEP.as_nanos()) as u64;
            next = now;
        }
        for Control::Join { name, mut viewer, reply } in control.try_iter() {
            let id = match names.get(&name) {
                Some(&id) => id,
                None => {
                    let at = SPAWN + glam::Vec2::new((names.len() % 8) as f32 * 1.5, (names.len() / 8 % 8) as f32 * 1.5);
                    let id = world.spawn_player(&name, at, std::f32::consts::PI);
                    names.insert(name, id);
                    id
                }
            };
            if let Viewer::Remote(r) = &mut viewer {
                r.phase = joined % SEND_EVERY;
            }
            joined += 1;
            // A second connection for the same character replaces the first.
            viewers.retain(|(v, _)| *v != id);
            viewers.push((id, viewer));
            let _ = reply.send(id);
        }
        for (id, c) in commands.try_iter() {
            world.command(id, c);
        }
        let t0 = Instant::now();
        world.step();
        let t1 = Instant::now();
        let events = world.take_events();
        recent.push_back(events.clone());
        while recent.len() > SEND_EVERY as usize {
            recent.pop_front();
        }
        let tick = world.tick();
        let mut gone = Vec::new();
        let (mut encoding, mut bytes) = (Duration::ZERO, 0);
        // Everyone, once, for the network players whose turn it is (#286).
        let due = viewers.iter().any(|(_, v)| matches!(v, Viewer::Remote(r) if tick % SEND_EVERY == r.phase));
        let since: Vec<protocol::Event> = if due { recent.iter().flatten().cloned().collect() } else { Vec::new() };
        if due {
            frame = Some(world.net_frame(frame.as_ref()));
        }
        for (id, viewer) in &viewers {
            if let Viewer::Local(latest) = viewer {
                let mut snapshot = world.snapshot(Some(*id));
                snapshot.events = events.clone();
                let combat = world.combat_view(Some(*id));
                latest.events.lock().unwrap().extend(events.iter().cloned());
                // The combat view first, so whoever sees a snapshot finds its combat detail beside it.
                *latest.combat.lock().unwrap() = Some(Arc::new(combat));
                *latest.clock.lock().unwrap() = Some((snapshot.time, next));
                *latest.snapshot.lock().unwrap() = Some(Arc::new(snapshot));
            }
        }
        if let Some(frame) = frame.as_ref().filter(|_| due) {
            let t = Instant::now();
            let mut sending: Vec<(UnitId, &mut Remote)> = viewers
                .iter_mut()
                .filter_map(|(id, v)| match v {
                    Viewer::Remote(r) if tick % SEND_EVERY == r.phase => Some((*id, r)),
                    _ => None,
                })
                .collect();
            let world = &world;
            // Views of one frame don't depend on each other, so a crowd's go across cores (#292).
            let send = |chunk: &mut [(UnitId, &mut Remote)]| {
                let (mut dropped, mut bytes) = (Vec::new(), 0);
                for (id, remote) in chunk {
                    let acked = remote.acked.load(Ordering::Acquire);
                    if acked > 0 {
                        remote.encoder.ack(acked - 1);
                    }
                    let view = world.encode_view(frame, *id, &mut remote.encoder, &since);
                    let out = net::encode(&net::ServerMsg::View(view));
                    bytes += out.len();
                    if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = remote.frames.try_send(out) {
                        dropped.push(*id);
                    }
                }
                (dropped, bytes)
            };
            let results = across(&mut sending, encoders, send);
            for (dropped, b) in results {
                gone.extend(dropped);
                bytes += b;
            }
            encoding = t.elapsed();
        }
        viewers.retain(|(id, _)| !gone.contains(id));
        // Whoever left stops where they stand.
        for id in gone {
            world.command(id, Command::Move { direction: glam::Vec2::ZERO, walk: false, sprint: false, face_yaw: None });
            world.command(id, Command::SetAutoAttack(false));
        }
        let (step, view) = ((t1 - t0).as_secs_f64() * 1000.0, t1.elapsed().as_secs_f64() * 1000.0);
        window = (window.0 + 1, window.1 + step, window.2 + view);
        {
            let encode_ms = encoding.as_secs_f64() * 1000.0;
            let mut times = shared.times.lock().unwrap();
            if times.len() >= KEPT_TICKS {
                times.drain(..KEPT_TICKS / 2);
            }
            times.push(TickTime { step_ms: step, build_ms: view - encode_ms, encode_ms, bytes });
        }
        {
            let mut stats = shared.stats.lock().unwrap();
            stats.ticks = world.tick();
            stats.players = viewers.len();
            stats.bytes_out += bytes as u64;
            if window.0 == sim::TICK_RATE {
                stats.step_ms = window.1 / window.0 as f64;
                stats.view_ms = window.2 / window.0 as f64;
                window = (0, 0.0, 0.0);
            }
        }
        next += STEP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    #[test]
    fn steps_at_sixty_a_second_and_takes_commands() {
        let mut host = Host::start(1, "Hero");
        let start = host.snapshot().unwrap();
        let p0 = start.unit(host.player).unwrap().position;
        host.send(Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, sprint: false, face_yaw: None });
        std::thread::sleep(Duration::from_millis(1000));
        let now = host.snapshot().unwrap();
        let ticks = now.tick - start.tick;
        assert!((54..=66).contains(&ticks), "{ticks} ticks in a second");
        let moved = (now.unit(host.player).unwrap().position - p0).length();
        assert!(moved > 4.0, "moved {moved} m");
        let combat = host.combat().expect("a combat view beside the snapshot");
        assert!(combat.units.iter().any(|u| u.id == host.player));
        assert_eq!(host.stats().players, 1);
        host.stop();
    }

    #[test]
    fn a_remote_player_sees_the_local_one() {
        let host = Host::start(2, "Local");
        let listener = net::listen(host.server().unwrap(), "127.0.0.1:0").unwrap();
        let mut remote = net::Client::connect(listener.addr(), listener.certificate(), "Remote").unwrap();
        assert_ne!(remote.player, host.player);
        let (snapshot, combat) = remote.next_view().unwrap();
        assert!(snapshot.unit(host.player).is_some_and(|u| u.name == "Local"));
        assert!(snapshot.player.is_some());
        assert!(combat.units.iter().any(|u| u.id == remote.player));
        let local = host.snapshot().unwrap();
        assert!(local.unit(remote.player).is_some_and(|u| u.name == "Remote"));
    }

    #[test]
    fn bots_join_and_find_a_fight() {
        let mut world = Sim::brambleford(4);
        world.spawn_crowd_in(40, SPAWN, Vec2::splat(15.0));
        let server = Server::start(world);
        let listener = net::listen(&server, "127.0.0.1:0").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let bots: Vec<_> = (0..4)
            .map(|i| {
                let (addr, cert, stop) = (listener.addr(), listener.certificate().clone(), stop.clone());
                std::thread::spawn(move || bots::run(addr, &cert, &format!("Bot {i}"), i, stop).unwrap())
            })
            .collect();
        std::thread::sleep(Duration::from_secs(2));
        stop.store(true, Ordering::Relaxed);
        let reports: Vec<_> = bots.into_iter().map(|b| b.join().unwrap()).collect();
        assert!(reports.iter().all(|r| r.views > 30 && r.bytes > 0), "{reports:?}");
        assert!(reports.iter().any(|r| r.fighting > 0), "nobody found a fight: {reports:?}");
        let times = server.take_tick_times();
        assert!(times.len() > 60 && times.iter().any(|t| t.bytes > 0 && t.encode_ms > 0.0));
        assert!(server.stats().bytes_out > 0);
    }

    #[test]
    fn the_same_name_takes_its_character_back() {
        let host = Host::start(3, "Local");
        let listener = net::listen(host.server().unwrap(), "127.0.0.1:0").unwrap();
        let first = net::Client::connect(listener.addr(), listener.certificate(), "Wanderer").unwrap().player;
        let again = net::Client::connect(listener.addr(), listener.certificate(), "Wanderer").unwrap().player;
        assert_eq!(first, again);
    }
}
