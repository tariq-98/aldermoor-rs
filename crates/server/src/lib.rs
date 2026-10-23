//! The simulation host: one thread steps the world 60 times a second at a fixed rate,
//! takes commands from a channel, and publishes the latest snapshot for each viewer.
//!
//! In-process today (the client and server share a process); the same shape serves a network
//! session later, with `protocol` messages on the wire.

use crossbeam_channel::{unbounded, Receiver, Sender};
use protocol::{Command, Snapshot, UnitId};
use sim::Sim;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const STEP: Duration = Duration::from_nanos(1_000_000_000 / sim::TICK_RATE as u64);

/// Numbers about how the host is keeping up.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostStats {
    pub ticks: u64,
    /// Mean time spent stepping, over the last second.
    pub step_ms: f64,
    /// Steps dropped because the host fell more than a quarter-second behind.
    pub skipped: u64,
}

struct Shared {
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    events: Mutex<Vec<protocol::Event>>,
    stats: Mutex<HostStats>,
    running: AtomicBool,
}

/// A running simulation.
pub struct Host {
    commands: Sender<(UnitId, Command)>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    pub player: UnitId,
}

impl Host {
    /// Starts the Brambleford valley with one player and returns once the first snapshot exists.
    pub fn start(seed: u64, player_name: &str) -> Host {
        let (tx, rx) = unbounded();
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(None),
            events: Mutex::new(Vec::new()),
            stats: Mutex::new(HostStats::default()),
            running: AtomicBool::new(true),
        });
        let mut world = Sim::brambleford(seed);
        let player = world.spawn_player(player_name, glam::Vec2::new(6.0, 28.0), std::f32::consts::PI);
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("Aldermoor simulation".into())
            .spawn(move || run(world, player, rx, s))
            .expect("simulation thread starts");
        let host = Host { commands: tx, shared, thread: Some(thread), player };
        while host.snapshot().is_none() {
            std::thread::sleep(Duration::from_millis(1));
        }
        host
    }

    /// Queues a command from the local player.
    pub fn send(&self, c: Command) {
        let _ = self.commands.send((self.player, c));
    }

    /// The most recent snapshot.
    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.shared.snapshot.lock().unwrap().clone()
    }

    /// Events since the last call, in order (snapshots only carry their own tick's events).
    pub fn take_events(&self) -> Vec<protocol::Event> {
        std::mem::take(&mut *self.shared.events.lock().unwrap())
    }

    pub fn stats(&self) -> HostStats {
        *self.shared.stats.lock().unwrap()
    }

    pub fn stop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(mut world: Sim, player: UnitId, commands: Receiver<(UnitId, Command)>, shared: Arc<Shared>) {
    let mut next = Instant::now();
    let mut window = (0u32, 0.0f64);
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
        for (id, c) in commands.try_iter() {
            world.command(id, c);
        }
        let t0 = Instant::now();
        world.step();
        let snapshot = world.snapshot(Some(player));
        let spent = t0.elapsed().as_secs_f64() * 1000.0;
        shared.events.lock().unwrap().extend(snapshot.events.iter().cloned());
        *shared.snapshot.lock().unwrap() = Some(Arc::new(snapshot));
        window = (window.0 + 1, window.1 + spent);
        {
            let mut stats = shared.stats.lock().unwrap();
            stats.ticks = world.tick();
            if window.0 == sim::TICK_RATE {
                stats.step_ms = window.1 / window.0 as f64;
                window = (0, 0.0);
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
        host.send(Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, face_yaw: None });
        std::thread::sleep(Duration::from_millis(1000));
        let now = host.snapshot().unwrap();
        let ticks = now.tick - start.tick;
        assert!((54..=66).contains(&ticks), "{ticks} ticks in a second");
        let moved = (now.unit(host.player).unwrap().position - p0).length();
        assert!(moved > 4.0, "moved {moved} m");
        host.stop();
    }
}
