//! Bots against the server over the real transport (#74):
//! `cargo run -p server --release --example load_test -- [--players 50,200,500] [--seconds 10] [--monsters 2000]`
//!
//! For each player count it starts a fresh valley with the monsters, joins that many bots
//! (`server::bots`), lets them settle for three seconds, then measures. It prints one Markdown
//! table row per count: tick rate kept, p50/p99 of the step, building views, encoding them and the
//! whole tick, and bytes out per player and in total.

use server::{bots, net, Server, TickTime};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let mut counts = vec![50, 200, 500];
    let mut seconds = 10.0;
    let mut monsters = 2000;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--players" => counts = value.split(',').map(|n| n.parse().expect("--players takes numbers")).collect(),
            "--seconds" => seconds = value.parse().expect("--seconds takes a number"),
            "--monsters" => monsters = value.parse().expect("--monsters takes a number"),
            _ => panic!("unknown argument {arg}"),
        }
    }
    println!("{monsters} monsters, {seconds} s measured per row, on {} cores\n", std::thread::available_parallelism().map_or(0, |n| n.get()));
    println!("| Players | Ticks/s | Step p50 / p99 ms | Build views p50 / p99 | Encode p50 / p99 | Tick p50 / p99 | Out per player | Out total |");
    println!("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    for players in counts {
        println!("{}", run(players, monsters, seconds));
    }
}

fn run(players: usize, monsters: usize, seconds: f64) -> String {
    let mut world = sim::Sim::brambleford(1);
    world.spawn_crowd(monsters);
    let mut server = Server::start(world);
    let listener = net::listen(&server, "127.0.0.1:0").expect("listens");
    let stop = Arc::new(AtomicBool::new(false));
    let bots: Vec<_> = (0..players)
        .map(|i| {
            let (addr, stop) = (listener.addr(), stop.clone());
            std::thread::Builder::new()
                .stack_size(256 << 10)
                .spawn(move || bots::run(addr, &format!("Bot {i}"), i as u64, stop))
                .expect("bot thread starts")
        })
        .collect();
    std::thread::sleep(Duration::from_secs(3));
    let _ = server.take_tick_times();
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let times = server.take_tick_times();
    let elapsed = start.elapsed().as_secs_f64();
    stop.store(true, Ordering::Relaxed);
    server.stop();
    let joined = bots.into_iter().filter_map(|b| b.join().ok()?.ok()).count();
    if joined < players {
        eprintln!("{} of {players} bots failed or were dropped for falling behind", players - joined);
    }
    let pick = |f: fn(&TickTime) -> f64| {
        let mut v: Vec<f64> = times.iter().map(f).collect();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v.get(((v.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
        format!("{:.2} / {:.2}", at(0.5), at(0.99))
    };
    let bytes: usize = times.iter().map(|t| t.bytes).sum();
    let per_second = bytes as f64 / elapsed;
    format!(
        "| {players} | {:.1} | {} | {} | {} | {} | {:.0} KB/s | {:.1} MB/s |",
        times.len() as f64 / elapsed,
        pick(|t| t.step_ms),
        pick(|t| t.build_ms),
        pick(|t| t.encode_ms),
        pick(|t| t.step_ms + t.build_ms + t.encode_ms),
        per_second / players as f64 / 1000.0,
        per_second / 1e6,
    )
}
