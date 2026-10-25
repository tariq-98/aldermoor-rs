//! Fixed scenes for the Perf workflow's sim gate (#31): `cargo run -p sim --release --example perf_gate [scene]`.
//!
//! Run under `valgrind --tool=callgrind --toggle-collect='*perf_gate::measured*'` to count only the
//! steps and snapshots. The sim is deterministic, so the count is the same on every run of the same
//! code, which makes it a fair comparison between two commits on a noisy shared runner. Each scene
//! also prints its wire size: postcard-encoded snapshot plus combat view, per player per second.

use glam::Vec2;
use protocol::{CombatView, Command, Snapshot, UnitId};
use sim::{Sim, Specialty};

const SCENES: [&str; 3] = ["wander", "fight", "dense"];

fn main() {
    let only = std::env::args().nth(1);
    for scene in SCENES.iter().filter(|s| only.as_deref().is_none_or(|o| o == **s)) {
        let (mut sim, viewer, steps) = setup(scene);
        let views = measured(&mut sim, viewer, steps);
        let bytes: usize = views.iter().map(|(s, c)| postcard::to_allocvec(s).unwrap().len() + postcard::to_allocvec(c).unwrap().len()).sum();
        let per_second = bytes as f64 / steps as f64 * sim::TICK_RATE as f64;
        println!("{scene}: {steps} steps, wire {:.0} B/s per player", per_second);
    }
}

/// A scene ready to measure, its viewer, and how many steps to measure.
fn setup(scene: &str) -> (Sim, UnitId, u32) {
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
    (sim, hero, steps)
}

fn run_circle(sim: &mut Sim, hero: UnitId, step: u32) {
    let a = step as f32 / 120.0;
    sim.command(hero, Command::Move { direction: Vec2::new(a.sin(), a.cos()), walk: false, face_yaw: None });
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
