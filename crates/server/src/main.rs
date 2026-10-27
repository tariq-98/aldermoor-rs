//! The headless server: the Brambleford valley stepping at 60 Hz with no window or GPU, serving
//! players over the network (`server::net`).
//!
//! `cargo run -p server --release -- [--bind 0.0.0.0:7878] [--cert aldermoor.der] [--seed 1] [--crowd 0] [--accounts aldermoor.db]`
//!
//! Players connect over QUIC (UDP). `--cert FILE` is the server's certificate, made the first
//! time with its private key beside it (`FILE` with a `.key` extension, readable only by its
//! owner). Give players the certificate, never the key: the client's `--server-cert` trusts
//! exactly it. `--regions 2` or `4` splits the valley into halves or quarters, each its own
//! region on its own thread (#208); players cross between them as they walk. `--crowd N` adds N
//! wandering monsters (shared out among the regions), to see how the step holds up. It prints
//! where it listens and where the certificate is, then how it's keeping up every ten seconds.
//!
//! `--accounts FILE` keeps accounts and characters in that SQLite file (#88): players log in
//! instead of just giving a name, and their characters are saved every ten seconds and when
//! they leave, so a restart loses at most the last ten seconds.

use std::time::Duration;

fn main() {
    let mut bind = "127.0.0.1:7878".to_string();
    let mut cert = std::path::PathBuf::from("aldermoor.der");
    let mut seed = 1u64;
    let mut crowd = 0usize;
    let mut regions = 1usize;
    let mut accounts: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage(&format!("{arg} needs a value")));
        match arg.as_str() {
            "--bind" => bind = value(),
            "--cert" => cert = value().into(),
            "--seed" => seed = value().parse().unwrap_or_else(|_| usage("--seed takes a number")),
            "--crowd" => crowd = value().parse().unwrap_or_else(|_| usage("--crowd takes a number")),
            "--accounts" => accounts = Some(value().into()),
            "--regions" => regions = value().parse().ok().filter(|n| [1, 2, 4].contains(n)).unwrap_or_else(|| usage("--regions takes 1, 2 or 4")),
            _ => usage(&format!("unknown argument {arg}")),
        }
    }
    let server = if regions == 1 {
        let mut world = sim::Sim::brambleford(seed);
        world.spawn_crowd(crowd);
        server::Server::start(world)
    } else {
        server::Server::start_realm(realm(regions, seed, crowd))
    };
    let identity = server::net::Identity::load_or_create(&cert).unwrap_or_else(|e| {
        eprintln!("can't read or make the certificate {}: {e}", cert.display());
        std::process::exit(1)
    });
    let store = accounts.map(|path| {
        std::sync::Arc::new(server::accounts::Store::open(&path).unwrap_or_else(|e| {
            eprintln!("can't open the account database {}: {e}", path.display());
            std::process::exit(1)
        }))
    });
    let listener = server::net::listen_with(&server, &bind, identity, store.clone()).unwrap_or_else(|e| {
        eprintln!("can't listen on {bind}: {e}");
        std::process::exit(1)
    });
    // On stdout, for scripts and tests that start the server on port 0.
    println!("listening on {}", listener.addr());
    println!("certificate {}", cert.display());
    loop {
        std::thread::sleep(Duration::from_secs(10));
        if let Some(store) = &store {
            for r in server.records() {
                // Only characters someone logged in to belong to an account.
                if let Some(account) = store.owner(&r.name)
                    && let Err(e) = store.save(&account, &r)
                {
                    eprintln!("couldn't save {}: {e}", r.name);
                }
            }
        }
        let s = server.stats();
        eprintln!("tick {}: step {:.2} ms, views {:.2} ms, {} players, {} steps skipped", s.ticks, s.step_ms, s.view_ms, s.players, s.skipped);
    }
}

/// The valley cut into halves (west and east) or quarters around the middle of its crowd, each
/// region with its share of the monsters.
fn realm(n: usize, seed: u64, crowd: usize) -> server::regions::Realm {
    use glam::Vec2;
    use sim::region::Bounds;
    const MIDDLE: Vec2 = Vec2::new(5.0, -30.0);
    let far = 1000.0;
    let parts: Vec<Bounds> = if n == 2 {
        vec![Bounds::new(Vec2::splat(-far), Vec2::new(MIDDLE.x, far)), Bounds::new(Vec2::new(MIDDLE.x, -far), Vec2::splat(far))]
    } else {
        vec![
            Bounds::new(Vec2::splat(-far), MIDDLE),
            Bounds::new(Vec2::new(MIDDLE.x, -far), Vec2::new(far, MIDDLE.y)),
            Bounds::new(Vec2::new(-far, MIDDLE.y), Vec2::new(MIDDLE.x, far)),
            Bounds::new(MIDDLE, Vec2::splat(far)),
        ]
    };
    let land = sim::brambleford_land();
    let sims = parts
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            let mut sim = sim::Sim::region(land.clone(), seed + i as u64, i as u32, b);
            // Its share of the crowd, in its part of the valley within 100 m of the middle.
            let (lo, hi) = (b.min.max(MIDDLE - 100.0), b.max.min(MIDDLE + 100.0));
            sim.spawn_crowd_in(crowd / n, (lo + hi) * 0.5, (hi - lo) * 0.5);
            sim
        })
        .collect();
    server::regions::Realm::new(sims)
}

fn usage(why: &str) -> ! {
    eprintln!("{why}\nusage: server [--bind ADDR] [--cert FILE] [--seed N] [--crowd N] [--regions 1|2|4] [--accounts FILE]");
    std::process::exit(2)
}
