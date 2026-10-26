//! What a crowd of network players costs the server, without bots sharing its cores (#292):
//! `cargo run -p server --release --example crowd_bench -- [--players 200] [--seconds 5]`.
//!
//! The perf gate's `players` scene: heroes of every specialty walking circles through 2,000
//! monsters and fighting them, each getting a view every third step as the server sends them.
//! Steps the same scene twice, encoding the due views on one thread and then across
//! [`server::MAX_ENCODERS`] or the cores left beside the sim's, and prints where a step's time
//! goes and how many players that would leave room for at 60 steps a second.

use glam::Vec2;
use protocol::wire::Encoder;
use protocol::{Command, Event, UnitId};
use sim::{Sim, Specialty};
use std::time::Instant;

fn main() {
    let (mut players, mut seconds) = (200, 5.0);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--players" => players = value.parse().expect("--players takes a number"),
            "--seconds" => seconds = value.parse().expect("--seconds takes a number"),
            _ => panic!("unknown argument {arg}"),
        }
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let threads = cores.saturating_sub(1).clamp(1, server::MAX_ENCODERS);
    println!("{players} players among 2,000 monsters, {seconds} s, on {cores} cores\n");
    println!("| Encoding on | Step ms | Frame ms | Views ms | Tick ms | Out per player |");
    println!("| --- | ---: | ---: | ---: | ---: | ---: |");
    for t in [1, threads] {
        let r = run(players, (seconds * 60.0) as u32, t);
        let tick = r.step + r.frame + r.views;
        println!("| {t} thread{} | {:.2} | {:.2} | {:.2} | {:.2} | {:.0} KB/s |", if t == 1 { "" } else { "s" }, r.step, r.frame, r.views, tick, r.bytes / 1024.0);
    }
}

struct Run {
    step: f64,
    frame: f64,
    views: f64,
    /// Bytes a second per player.
    bytes: f64,
}

fn run(players: usize, steps: u32, threads: usize) -> Run {
    let mut sim = Sim::brambleford(42);
    sim.spawn_crowd(2000);
    let ids: Vec<UnitId> = (0..players)
        .map(|i| {
            let at = Vec2::new(-60.0 + (i % 20) as f32 * 6.0, -70.0 + (i / 20) as f32 * 6.0);
            let id = sim.spawn_hero("Bot", Specialty::ALL[i % 6], 4, at, 0.0);
            sim.command(id, Command::SetAutoAttack(true));
            id
        })
        .collect();
    for step in 0..20 {
        walk(&mut sim, &ids, step);
        sim.step();
        let _ = sim.take_events();
    }
    let mut frame = sim.net_frame(None);
    let mut viewers: Vec<(UnitId, Encoder)> = ids
        .iter()
        .map(|&id| {
            let mut e = Encoder::new();
            let first = sim.encode_view(&frame, id, &mut e, &[]);
            e.ack(first.tick);
            (id, e)
        })
        .collect();
    let (mut step_s, mut frame_s, mut views_s, mut bytes) = (0.0, 0.0, 0.0, 0);
    let mut recent: std::collections::VecDeque<Vec<Event>> = std::collections::VecDeque::new();
    for step in 0..steps {
        walk(&mut sim, &ids, 20 + step);
        let t0 = Instant::now();
        sim.step();
        let t1 = Instant::now();
        recent.push_back(sim.take_events());
        if recent.len() > server::SEND_EVERY as usize {
            recent.pop_front();
        }
        let since: Vec<Event> = recent.iter().flatten().cloned().collect();
        frame = sim.net_frame(Some(&frame));
        let t2 = Instant::now();
        let mut due: Vec<&mut (UnitId, Encoder)> = viewers.iter_mut().enumerate().filter(|(i, _)| (*i as u64 + step as u64).is_multiple_of(server::SEND_EVERY)).map(|(_, v)| v).collect();
        let sim = &sim;
        let frame = &frame;
        let out: Vec<usize> = server::across(&mut due, threads, |chunk| {
            chunk
                .iter_mut()
                .map(|(id, encoder)| {
                    let view = sim.encode_view(frame, *id, encoder, &since);
                    encoder.ack(view.tick);
                    server::net::encode(&server::net::ServerMsg::View(view)).len()
                })
                .sum()
        });
        let t3 = Instant::now();
        bytes += out.iter().sum::<usize>();
        step_s += (t1 - t0).as_secs_f64();
        frame_s += (t2 - t1).as_secs_f64();
        views_s += (t3 - t2).as_secs_f64();
    }
    let ms = |s: f64| s / steps as f64 * 1000.0;
    Run { step: ms(step_s), frame: ms(frame_s), views: ms(views_s), bytes: bytes as f64 / steps as f64 * sim::TICK_RATE as f64 / players as f64 }
}

/// Every player walks its own circle, so they spread, meet monsters and fight.
fn walk(sim: &mut Sim, ids: &[UnitId], step: u32) {
    for (i, &id) in ids.iter().enumerate() {
        let a = step as f32 / 120.0 + i as f32 * 0.7;
        sim.command(id, Command::Move { direction: Vec2::new(a.sin(), a.cos()), walk: false, sprint: false, face_yaw: None });
    }
}
