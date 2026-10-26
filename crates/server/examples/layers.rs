//! A capital on launch night (#170): `cargo run -p server --release --example layers`.
//!
//! 600 players join a town capped at 200, so it opens two layers, each its own region with its
//! own 1,000 monsters. The players run circles that take some of them over the border into the
//! open country next door, which pairs with all three. Prints the tick, the slowest region's step,
//! and the players in each region (budget: docs/SERVER.md "Machines").

use glam::Vec2;
use protocol::Command;
use server::regions::{Layering, Realm};
use sim::region::Bounds;
use sim::{brambleford_land, Sim};

const BORDER: f32 = 5.0;
const CAP: usize = 200;

fn main() {
    let land = brambleford_land();
    let far = 1000.0;
    let town = Bounds::new(Vec2::new(-far, -far), Vec2::new(BORDER, far));
    let country = Bounds::new(Vec2::new(BORDER, -far), Vec2::new(far, far));
    let monsters = |sim: &mut Sim, toward: Vec2| sim.spawn_crowd_in(1000, toward, Vec2::splat(48.0));
    let mut a = Sim::region(land.clone(), 60, 0, town);
    monsters(&mut a, Vec2::new(-60.0, -30.0));
    let mut b = Sim::region(land.clone(), 61, 1, country);
    monsters(&mut b, Vec2::new(70.0, -30.0));
    let open = Box::new(move |_ground: u32, id: u32| {
        let mut sim = Sim::region(land.clone(), 60 + id as u64, id, town);
        monsters(&mut sim, Vec2::new(-60.0, -30.0));
        sim
    });
    let mut realm = Realm::layered(vec![a, b], Layering { cap: CAP, unlayered: Vec::new(), open, reuse_after: server::regions::REUSE_AFTER });
    let heroes: Vec<(u32, f32)> = (0..CAP * 3)
        .map(|n| {
            let a = n as f32 * 2.399;
            let at = Vec2::new(-15.0, -30.0) + Vec2::new(a.cos(), a.sin()) * (n as f32 * 0.03);
            let id = realm.spawn_player("Hero", at).expect("the town has room in some layer");
            realm.command(id, Command::SetAutoAttack(true));
            (id, a)
        })
        .collect();
    let (mut ticks, mut steps, mut exchanges) = (Vec::new(), Vec::new(), Vec::new());
    let (mut handoffs, mut ghosts) = (0, 0);
    let seconds = 20;
    for tick in 0..seconds * 60 {
        for &(id, a) in &heroes {
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
        ghosts += s.ghosts;
    }
    let report = |name: &str, v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        println!("  {name:<18} mean {mean:.3} ms, p99 {:.3} ms", v[v.len() * 99 / 100]);
    };
    let regions: Vec<u32> = realm.layers(0).into_iter().chain(realm.layers(1)).collect();
    println!("{} players in a town capped at {CAP}, beside open country, 1,000 monsters a region:", heroes.len());
    report("tick", &mut ticks);
    report("slowest step", &mut steps);
    report("ghosts and handoffs", &mut exchanges);
    println!("  town layers       {:?}", realm.layers(0));
    println!("  players           {:?} in regions {regions:?}", regions.iter().map(|&r| realm.players_in(r)).collect::<Vec<_>>());
    println!("  ghosts            {} a tick, all regions together", ghosts / (seconds as usize * 60));
    println!("  crossings         {:.1} handoffs a second", handoffs as f64 / seconds as f64);
}
