//! Fixed scenes for the Perf workflow's sim gate (#31): `cargo run -p sim --release --example perf_gate [scene]`.
//!
//! Run under `valgrind --tool=callgrind --toggle-collect='*perf_gate::measured*'` to count only the
//! steps and snapshots. The sim is deterministic, so the count is the same on every run of the same
//! code, which makes it a fair comparison between two commits on a noisy shared runner. Each scene
//! also prints its wire size: views as the server sends them (`protocol::wire`, every third step,
//! deltas against the last), per player per second.
//! The `players` scene gives 200 players a view each step, as the server does (#74).

use glam::Vec2;
use protocol::wire::Encoder;
use protocol::{CombatView, Command, Event, Snapshot, UnitId};
use sim::{Sim, Specialty};

const SCENES: [&str; 4] = ["wander", "fight", "dense", "players"];

/// The server sends a view every third step (`server::SEND_EVERY`).
const SEND_EVERY: usize = 3;

/// Bytes on the wire for one view, with the events since the last one sent.
fn wire(encoder: &mut Encoder, snapshot: &Snapshot, combat: &CombatView, events: Vec<Event>) -> usize {
    let snapshot = Snapshot { events, ..snapshot.clone() };
    let view = encoder.encode(&snapshot, combat);
    encoder.ack(view.tick);
    postcard::to_allocvec(&view).unwrap().len()
}

fn main() {
    let only = std::env::args().nth(1);
    for scene in SCENES.iter().filter(|s| only.as_deref().is_none_or(|o| o == **s)) {
        if *scene == "players" {
            players();
            continue;
        }
        let (mut sim, viewer, steps, mut encoder) = setup(scene);
        let views = measured(&mut sim, viewer, steps);
        let (mut bytes, mut events) = (0, Vec::new());
        for (i, (s, c)) in views.iter().enumerate() {
            events.extend(s.events.iter().cloned());
            if i % SEND_EVERY == SEND_EVERY - 1 {
                bytes += wire(&mut encoder, s, c, std::mem::take(&mut events));
            }
        }
        let per_second = bytes as f64 / steps as f64 * sim::TICK_RATE as f64;
        println!("{scene}: {steps} steps, wire {:.0} B/s per player", per_second);
    }
}

/// A scene ready to measure, its viewer, how many steps to measure, and the viewer's wire encoder
/// with a view already sent.
fn setup(scene: &str) -> (Sim, UnitId, u32, Encoder) {
    let mut sim = Sim::brambleford(42);
    let steps = match scene {
        // 2,000 monsters wandering the valley while the hero runs circles through them.
        "wander" => {
            sim.spawn_crowd(2000);
            300
        }
        // 300 monsters and 12 heroes of every specialty fighting.
        "fight" => {
            sim.spawn_crowd_in(300, Vec2::new(0.0, -40.0), Vec2::new(60.0, 60.0));
            for i in 0..12 {
                let id = sim.spawn_hero("Hero", Specialty::ALL[i % 6], 4, Vec2::new(-30.0 + i as f32 * 5.0, -40.0), 0.0);
                sim.command(id, Command::SetAutoAttack(true));
            }
            300
        }
        // 5,000 monsters packed within 100 m of the hero: the worst case for a step.
        "dense" => {
            sim.spawn_crowd_in(5000, Vec2::new(5.0, 10.0), Vec2::splat(100.0));
            120
        }
        _ => panic!("unknown scene {scene}; scenes are {SCENES:?}"),
    };
    let hero = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), 0.0);
    // Let fights start and crowds settle before measuring.
    for step in 0..60 {
        run_circle(&mut sim, hero, step);
        sim.step();
        let _ = sim.snapshot(Some(hero));
    }
    let mut encoder = Encoder::new();
    wire(&mut encoder, &sim.snapshot(Some(hero)), &sim.combat_view(Some(hero)), Vec::new());
    (sim, hero, steps, encoder)
}

fn run_circle(sim: &mut Sim, hero: UnitId, step: u32) {
    let a = step as f32 / 120.0;
    sim.command(hero, Command::Move { direction: Vec2::new(a.sin(), a.cos()), walk: false, sprint: false, face_yaw: None });
}

/// The measured part: `steps` steps, each followed by the viewer's snapshot and combat view, as
/// the host does. Encoding them for the wire happens afterwards, outside the count.
#[inline(never)]
fn measured(sim: &mut Sim, viewer: UnitId, steps: u32) -> Vec<(Snapshot, CombatView)> {
    let mut views = Vec::with_capacity(steps as usize);
    for step in 0..steps {
        run_circle(sim, viewer, 60 + step);
        sim.step();
        views.push((sim.snapshot(Some(viewer)), sim.combat_view(Some(viewer))));
    }
    views
}

/// 200 players of every specialty among 2,000 monsters, each needing their own view every step:
/// the MMO load (#74). Few steps, because a step here costs about a hundred times the others.
fn players() {
    const PLAYERS: usize = 200;
    const STEPS: u32 = 10;
    let mut sim = Sim::brambleford(42);
    sim.spawn_crowd(2000);
    let ids: Vec<UnitId> = (0..PLAYERS)
        .map(|i| {
            let at = Vec2::new(-60.0 + (i % 20) as f32 * 6.0, -70.0 + (i / 20) as f32 * 6.0);
            let id = sim.spawn_hero("Bot", Specialty::ALL[i % 6], 4, at, 0.0);
            sim.command(id, Command::SetAutoAttack(true));
            id
        })
        .collect();
    for step in 0..20 {
        move_players(&mut sim, &ids, step);
        sim.step();
        let _ = sim.take_events();
    }
    let mut encoders: Vec<Encoder> = ids
        .iter()
        .map(|&id| {
            let mut e = Encoder::new();
            wire(&mut e, &sim.snapshot(Some(id)), &sim.combat_view(Some(id)), Vec::new());
            e
        })
        .collect();
    // One measured call a step, encoding outside it, so 2,000 views aren't held at once. Each
    // player gets a view on its own third of the steps, as the server sends them.
    let (mut bytes, mut recent) = (0, std::collections::VecDeque::new());
    for step in 0..STEPS {
        move_players(&mut sim, &ids, 20 + step);
        let views = measured_players(&mut sim, &ids);
        recent.push_back(views.first().map(|(s, _)| s.events.clone()).unwrap_or_default());
        if recent.len() > SEND_EVERY {
            recent.pop_front();
        }
        for (i, (s, c)) in views.iter().enumerate() {
            if (step as usize + i) % SEND_EVERY == 0 {
                bytes += wire(&mut encoders[i], s, c, recent.iter().flatten().cloned().collect());
            }
        }
    }
    let per_second = bytes as f64 / (STEPS as usize * PLAYERS) as f64 * sim::TICK_RATE as f64;
    println!("players: {STEPS} steps, wire {per_second:.0} B/s per player");
}

/// Every player walks its own circle, so they spread, meet monsters and fight.
fn move_players(sim: &mut Sim, ids: &[UnitId], step: u32) {
    for (i, &id) in ids.iter().enumerate() {
        let a = step as f32 / 120.0 + i as f32 * 0.7;
        sim.command(id, Command::Move { direction: Vec2::new(a.sin(), a.cos()), walk: false, sprint: false, face_yaw: None });
    }
}

/// The measured part of the players scene: a step, then every player's snapshot and combat
/// view, with the step's events given to all of them as the server does.
#[inline(never)]
fn measured_players(sim: &mut Sim, ids: &[UnitId]) -> Vec<(Snapshot, CombatView)> {
    sim.step();
    let events = sim.take_events();
    ids.iter()
        .map(|&id| {
            let mut snapshot = sim.snapshot(Some(id));
            snapshot.events = events.clone();
            (snapshot, sim.combat_view(Some(id)))
        })
        .collect()
}
