//! The headless server: the Brambleford valley stepping at 60 Hz with no window or GPU, serving
//! players over the network (`server::net`).
//!
//! `cargo run -p server --release -- [--bind 0.0.0.0:7878] [--cert aldermoor.der] [--seed 1] [--crowd 0]`
//!
//! Players connect over QUIC (UDP). `--cert FILE` is the server's certificate, made the first
//! time with its private key beside it (`FILE` with a `.key` extension, readable only by its
//! owner). Give players the certificate, never the key: the client's `--server-cert` trusts
//! exactly it. `--crowd N` adds N wandering monsters, to see how the step holds up. It prints
//! where it listens and where the certificate is, then how it's keeping up every ten seconds.

use std::time::Duration;

fn main() {
    let mut bind = "127.0.0.1:7878".to_string();
    let mut cert = std::path::PathBuf::from("aldermoor.der");
    let mut seed = 1u64;
    let mut crowd = 0usize;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| usage(&format!("{arg} needs a value")));
        match arg.as_str() {
            "--bind" => bind = value(),
            "--cert" => cert = value().into(),
            "--seed" => seed = value().parse().unwrap_or_else(|_| usage("--seed takes a number")),
            "--crowd" => crowd = value().parse().unwrap_or_else(|_| usage("--crowd takes a number")),
            _ => usage(&format!("unknown argument {arg}")),
        }
    }
    let mut world = sim::Sim::brambleford(seed);
    world.spawn_crowd(crowd);
    let server = server::Server::start(world);
    let identity = server::net::Identity::load_or_create(&cert).unwrap_or_else(|e| {
        eprintln!("can't read or make the certificate {}: {e}", cert.display());
        std::process::exit(1)
    });
    let listener = server::net::listen_as(&server, &bind, identity).unwrap_or_else(|e| {
        eprintln!("can't listen on {bind}: {e}");
        std::process::exit(1)
    });
    // On stdout, for scripts and tests that start the server on port 0.
    println!("listening on {}", listener.addr());
    println!("certificate {}", cert.display());
    loop {
        std::thread::sleep(Duration::from_secs(10));
        let s = server.stats();
        eprintln!("tick {}: step {:.2} ms, views {:.2} ms, {} players, {} steps skipped", s.ticks, s.step_ms, s.view_ms, s.players, s.skipped);
    }
}

fn usage(why: &str) -> ! {
    eprintln!("{why}\nusage: server [--bind ADDR] [--cert FILE] [--seed N] [--crowd N]");
    std::process::exit(2)
}
