//! Numbers for the zone server design (#73, docs/SERVER.md):
//! `cargo run -p sim --release --example regions [region|instances|threads|all]`.
//!
//! - `region`: one busy region, 200 players and 2,000 monsters. Step time, the cost of finding
//!   each player's neighbours from a grid instead of scanning the world, and the cost of keeping
//!   a second of position history for lag compensation.
//! - `instances`: many small match sims in one process (10 players, 30 monsters each): how long
//!   one takes to create and tear down, how much memory it holds, and its step.
//! - `threads`: several busy regions stepping side by side, one thread each, against one alone.

use glam::{Vec2, Vec3};
use protocol::{Command, UnitId};
use sim::{Sim, Specialty, Transform, Unit};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Counts live heap bytes, so an instance's memory is what it allocated.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        LIVE.fetch_add(size, Ordering::Relaxed);
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.realloc(p, l, size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// How far a player's interest reaches in the design (docs/SERVER.md), and the grid cell for it.
const INTEREST: f32 = 60.0;
const CELL: f32 = 32.0;
/// Lag compensation keeps this many steps of history (one second at 60 Hz).
const HISTORY: usize = 60;

fn main() {
    let only = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    if only == "all" || only == "region" {
        region();
    }
    if only == "all" || only == "instances" {
        instances();
    }
    if only == "all" || only == "threads" {
        threads();
    }
}

/// A busy region: `players` heroes of every specialty milling about and fighting among `monsters`.
fn busy(players: usize, monsters: usize, seed: u64) -> (Sim, Vec<UnitId>) {
    let mut sim = Sim::brambleford(seed);
    sim.spawn_crowd(monsters);
    let heroes = (0..players)
        .map(|i| {
            let a = i as f32 * 2.399;
            let r = 8.0 + (i as f32).sqrt() * 6.0;
            let at = Vec2::new(5.0 + a.cos() * r, -30.0 + a.sin() * r);
            let id = sim.spawn_hero("Hero", Specialty::ALL[i % Specialty::ALL.len()], 4, at, a);
            sim.command(id, Command::SetAutoAttack(true));
            id
        })
        .collect();
    (sim, heroes)
}

/// Everyone steers in slow circles, each a little out of phase.
fn steer(sim: &mut Sim, heroes: &[UnitId], step: u32) {
    for (i, &id) in heroes.iter().enumerate() {
        let a = step as f32 / 120.0 + i as f32;
        sim.command(id, Command::Move { direction: Vec2::new(a.sin(), a.cos()), walk: false, sprint: false, face_yaw: None });
    }
}

fn stats(mut v: Vec<f64>) -> (f64, f64) {
    v.sort_by(f64::total_cmp);
    (v.iter().sum::<f64>() / v.len() as f64, v[v.len() * 99 / 100])
}

fn region() {
    let (mut sim, heroes) = busy(200, 2000, 42);
    for step in 0..60 {
        steer(&mut sim, &heroes, step);
        sim.step();
    }
    let mut steps = Vec::new();
    let mut grids = Vec::new();
    let mut interest = Vec::new();
    let mut seen = 0usize;
    let mut history = Vec::new();
    let mut ring: Vec<Vec<(UnitId, Vec3, f32)>> = (0..HISTORY).map(|_| Vec::with_capacity(2400)).collect();
    let mut cells: HashMap<(i32, i32), Vec<(UnitId, Vec2)>> = HashMap::new();
    let mut positions: Vec<(UnitId, Vec2)> = Vec::new();
    let mut q = sim.world.query::<(&Unit, &Transform)>();
    for step in 0..600u32 {
        steer(&mut sim, &heroes, 60 + step);
        let t0 = Instant::now();
        sim.step();
        steps.push(t0.elapsed().as_secs_f64() * 1000.0);

        // Lag compensation: record where everyone stood this step.
        let t0 = Instant::now();
        let slot = &mut ring[step as usize % HISTORY];
        slot.clear();
        slot.extend(q.iter(&sim.world).map(|(u, t)| (u.id, t.pos, t.yaw)));
        history.push(t0.elapsed().as_secs_f64() * 1000.0);

        // Interest: bucket units into cells once, then each player looks up the cells around it.
        let t0 = Instant::now();
        for c in cells.values_mut() {
            c.clear();
        }
        positions.clear();
        positions.extend(q.iter(&sim.world).map(|(u, t)| (u.id, Vec2::new(t.pos.x, t.pos.z))));
        for &(id, p) in &positions {
            cells.entry(key(p)).or_default().push((id, p));
        }
        grids.push(t0.elapsed().as_secs_f64() * 1000.0);
        let t0 = Instant::now();
        let mut near = Vec::new();
        for &h in &heroes {
            let Some(t) = sim.get::<Transform>(h) else { continue };
            let p = Vec2::new(t.pos.x, t.pos.z);
            near.clear();
            let (x0, z0) = key(p - Vec2::splat(INTEREST));
            let (x1, z1) = key(p + Vec2::splat(INTEREST));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    if let Some(c) = cells.get(&(x, z)) {
                        near.extend(c.iter().filter(|(_, o)| (*o - p).length_squared() <= INTEREST * INTEREST).map(|(id, _)| *id));
                    }
                }
            }
            near.sort_unstable();
            seen += near.len();
        }
        interest.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let (sm, sp) = stats(steps);
    let (gm, _) = stats(grids);
    let (im, ip) = stats(interest);
    let (hm, _) = stats(history);
    let ring_bytes: usize = ring.iter().map(|s| s.capacity() * std::mem::size_of::<(UnitId, Vec3, f32)>()).sum();
    println!("region, 200 players + 2,000 monsters:");
    println!("  step              mean {sm:.3} ms, p99 {sp:.3} ms");
    println!("  interest grid     {gm:.3} ms to bucket, then {im:.3} ms (p99 {ip:.3}) for all 200 players, {:.0} units each within {INTEREST} m", seen as f64 / 600.0 / 200.0);
    println!("  history           {hm:.3} ms a step to record, {:.1} MB for {HISTORY} steps", ring_bytes as f64 / 1e6);
    println!("  per player view   {:.4} ms", im / 200.0);
}

fn key(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn instances() {
    const N: usize = 50;
    // The land is built once and shared by every sim on the map (#167); time that on its own.
    let before = LIVE.load(Ordering::Relaxed);
    let t0 = Instant::now();
    let land = sim::brambleford_land();
    let land_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let land_bytes = LIVE.load(Ordering::Relaxed) - before;
    let before = LIVE.load(Ordering::Relaxed);
    let t0 = Instant::now();
    let mut matches: Vec<(Sim, Vec<UnitId>)> = (0..N).map(|i| busy(10, 30, i as u64)).collect();
    let create = t0.elapsed().as_secs_f64() * 1000.0 / N as f64;
    let held = (LIVE.load(Ordering::Relaxed) - before) as f64 / N as f64;
    let mut steps = Vec::new();
    for step in 0..300u32 {
        let t0 = Instant::now();
        for (sim, heroes) in &mut matches {
            steer(sim, heroes, step);
            sim.step();
        }
        if step >= 60 {
            steps.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let (sm, sp) = stats(steps);
    let t0 = Instant::now();
    matches.clear();
    let teardown = t0.elapsed().as_secs_f64() * 1000.0 / N as f64;
    drop(land);
    println!("instances, {N} matches of 10 players + 30 monsters on one thread:");
    println!("  shared land       {land_ms:.2} ms and {:.2} MB, once for all of them", land_bytes as f64 / 1e6);
    println!("  create            {create:.2} ms each, tear down {teardown:.3} ms");
    println!("  memory            {:.2} MB each", held / 1e6);
    println!("  step, all {N}      mean {sm:.3} ms, p99 {sp:.3} ms ({:.3} ms each)", sm / N as f64);
}

fn threads() {
    let alone = run_regions(1);
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let side_by_side = run_regions(cores);
    println!("threads, regions of 200 players + 2,000 monsters:");
    println!("  one alone         mean {:.3} ms, p99 {:.3} ms", alone.0, alone.1);
    println!("  {cores} side by side    mean {:.3} ms, p99 {:.3} ms", side_by_side.0, side_by_side.1);
}

/// Steps `n` regions at once, one thread each; the worst region's mean and p99.
fn run_regions(n: usize) -> (f64, f64) {
    let handles: Vec<_> = (0..n)
        .map(|i| {
            std::thread::spawn(move || {
                let (mut sim, heroes) = busy(200, 2000, 100 + i as u64);
                let mut steps = Vec::new();
                for step in 0..420u32 {
                    steer(&mut sim, &heroes, step);
                    let t0 = Instant::now();
                    sim.step();
                    if step >= 60 {
                        steps.push(t0.elapsed().as_secs_f64() * 1000.0);
                    }
                }
                stats(steps)
            })
        })
        .collect();
    handles.into_iter().map(|h| h.join().unwrap()).fold((0.0, 0.0), |a, b| (f64::max(a.0, b.0), f64::max(a.1, b.1)))
}
