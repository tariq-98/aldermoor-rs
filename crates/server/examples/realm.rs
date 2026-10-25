//! A world of four regions stepping side by side (#168):
//! `cargo run -p server --release --example realm`.
//!
//! The valley is cut into quarters around the middle of its crowd. Each quarter has 500
//! monsters and 50 players, and the players run circles through the middle, so they keep
//! crossing borders and fighting across them. Prints the tick, the slowest region's step, and
//! what ghosts, handoffs and requests cost on top (budget 0.5 ms a region tick, docs/SERVER.md).

use glam::Vec2;
use protocol::Command;
use server::regions::Realm;
use sim::region::Bounds;
use sim::{brambleford_land, Sim, Specialty};

const MIDDLE: Vec2 = Vec2::new(5.0, -30.0);

fn main() {
    let land = brambleford_land();
    let far = 1000.0;
    let quarters = [
        Bounds::new(Vec2::new(-far, -far), MIDDLE),
        Bounds::new(Vec2::new(MIDDLE.x, -far), Vec2::new(far, MIDDLE.y)),
        Bounds::new(Vec2::new(-far, MIDDLE.y), Vec2::new(MIDDLE.x, far)),
        Bounds::new(MIDDLE, Vec2::new(far, far)),
    ];
    let mut heroes = Vec::new();
    let sims = quarters
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let mut sim = Sim::region(land.clone(), 40 + i as u64, i as u32, b);
            // Monsters fill the quarter's part of the valley, 100 m out from the middle.
            let toward = (b.min.max(MIDDLE - 100.0) + b.max.min(MIDDLE + 100.0)) * 0.5;
            sim.spawn_crowd_in(500, toward, Vec2::splat(48.0));
            // Players start 10 m from the middle, so their circles cross the borders.
            let start = (b.min.max(MIDDLE - 20.0) + b.max.min(MIDDLE + 20.0)) * 0.5;
            for n in 0..50 {
                let a = n as f32 * 2.399;
                let at = start + Vec2::new(a.cos(), a.sin()) * (n as f32 * 0.1);
                let id = sim.spawn_hero("Hero", Specialty::ALL[n % Specialty::ALL.len()], 4, at, a);
                sim.command(id, Command::SetAutoAttack(true));
                heroes.push((id, a));
            }
            sim
        })
        .collect();
    let mut realm = Realm::new(sims);
    let (mut ticks, mut steps, mut exchanges) = (Vec::new(), Vec::new(), Vec::new());
    let (mut handoffs, mut requests, mut ghosts) = (0, 0, 0);
    let seconds = 20;
    for tick in 0..seconds * 60 {
        for &(id, a) in &heroes {
            // Circles through the middle, each its own way round.
            let t = tick as f32 / 120.0 + a;
            realm.command(id, Command::Move { direction: Vec2::new(t.cos(), t.sin()), walk: false, sprint: false, face_yaw: None });
        }
        realm.step();
        let s = realm.stats();
        if tick >= 60 {
            ticks.push(s.tick_ms);
            steps.push(s.step_ms);
            exchanges.push(s.exchange_ms);
        }
        handoffs += s.handoffs;
        requests += s.requests;
        ghosts += s.ghosts;
    }
    let report = |name: &str, v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        println!("  {name:<18} mean {mean:.3} ms, p99 {:.3} ms", v[v.len() * 99 / 100]);
    };
    println!("4 regions of 50 players + 500 monsters, one thread each:");
    report("tick", &mut ticks);
    report("slowest step", &mut steps);
    report("ghosts and handoffs", &mut exchanges);
    println!("  ghosts            {} a tick, all regions together", ghosts / (seconds as usize * 60));
    println!("  crossings         {:.1} handoffs and {:.1} requests a second", handoffs as f64 / seconds as f64, requests as f64 / seconds as f64);
}
