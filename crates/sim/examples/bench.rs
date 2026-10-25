//! Step time with a crowded valley: `cargo run -p sim --release --example bench [units]`.
//!
//! The milestone 1 budget is 2 ms a step for 2,000 active units (an eighth of a 60 Hz frame).

use glam::Vec2;
use protocol::Command;
use sim::Sim;
use std::time::Instant;

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(2000);
    // "dense" packs the crowd within 100 m of the hero, like the Swift comparison run.
    let dense = std::env::args().any(|a| a == "dense");
    let mut sim = Sim::brambleford(42);
    let hero = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), 0.0);
    if dense {
        sim.spawn_crowd_in(n, Vec2::new(5.0, 10.0), Vec2::splat(100.0));
    } else {
        sim.spawn_crowd(n);
    }
    // Run the hero in circles through the crowd so monsters keep noticing and chasing.
    let mut steps = Vec::with_capacity(1200);
    let mut snapshots = Vec::with_capacity(1200);
    for step in 0..1260u32 {
        let a = step as f32 / 120.0;
        sim.command(hero, Command::Move { direction: Vec2::new(a.sin(), a.cos()), walk: false, sprint: false, face_yaw: None });
        let t0 = Instant::now();
        sim.step();
        let t1 = Instant::now();
        let snap = sim.snapshot(Some(hero));
        let t2 = Instant::now();
        if step >= 60 {
            steps.push((t1 - t0).as_secs_f64() * 1000.0);
            snapshots.push(((t2 - t1).as_secs_f64() * 1000.0, snap.units.len()));
        }
    }
    steps.sort_by(f64::total_cmp);
    let mean = steps.iter().sum::<f64>() / steps.len() as f64;
    let p99 = steps[steps.len() * 99 / 100];
    let max = steps[steps.len() - 1];
    let snap_mean = snapshots.iter().map(|s| s.0).sum::<f64>() / snapshots.len() as f64;
    let seen = snapshots.iter().map(|s| s.1).sum::<usize>() / snapshots.len();
    println!(
        "{} units{}: step mean {mean:.3} ms, p99 {p99:.3} ms, max {max:.3} ms; snapshot {snap_mean:.3} ms ({seen} units in view)",
        n + 6,
        if dense { " (dense)" } else { "" }
    );
}
