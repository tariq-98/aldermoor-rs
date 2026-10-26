//! What one network player's views cost (#34): `cargo run -p server --release --example wire_bench`.
//!
//! A player stands among the load test's 2,000 monsters and gets a view every third step, as the
//! server sends them. Prints the time to build, encode and decode a view, and its size on the
//! wire by part, after the first (which introduces everyone in view).

use protocol::wire::{Decoder, Encoder};
use std::time::Instant;

fn main() {
    let mut sim = sim::Sim::brambleford(1);
    sim.spawn_crowd(2000);
    let hero = sim.spawn_player("Hero", glam::Vec2::new(6.0, 28.0), 0.0);
    for _ in 0..120 {
        sim.step();
    }
    let (mut encoder, mut decoder) = (Encoder::new(), Decoder::new());
    let first = encoder.encode(&sim.snapshot(Some(hero)), &sim.combat_view(Some(hero)));
    println!("{} units in view; the first view is {} bytes", sim.snapshot(Some(hero)).units.len(), postcard::to_stdvec(&first).unwrap().len());
    encoder.ack(first.tick);
    decoder.decode(first).unwrap();
    let (mut build, mut encode, mut decode) = (0.0, 0.0, 0.0);
    // Intros, units, the player, combat detail, projectiles and areas, and everything.
    let mut bytes = [0usize; 6];
    let (mut views, mut events) = (0, Vec::new());
    for step in 1..=600 {
        sim.step();
        events.extend(sim.take_events());
        if step % 3 != 0 {
            continue;
        }
        let t0 = Instant::now();
        let mut snapshot = sim.snapshot(Some(hero));
        snapshot.events = std::mem::take(&mut events);
        let combat = sim.combat_view(Some(hero));
        let t1 = Instant::now();
        let view = encoder.encode(&snapshot, &combat);
        let t2 = Instant::now();
        bytes[0] += size(&view.intros);
        bytes[1] += view.units.len();
        bytes[2] += size(&view.player);
        bytes[3] += size(&view.combat.units);
        bytes[4] += size(&view.combat.projectiles) + size(&view.combat.areas);
        bytes[5] += size(&view);
        let tick = view.tick;
        let t3 = Instant::now();
        decoder.decode(view).unwrap();
        decode += t3.elapsed().as_secs_f64();
        encoder.ack(tick);
        build += (t1 - t0).as_secs_f64();
        encode += (t2 - t1).as_secs_f64();
        views += 1;
    }
    let ms = |s: f64| s / views as f64 * 1000.0;
    let b = |i: usize| bytes[i] / views;
    println!("build {:.3} ms, encode {:.3} ms, decode {:.3} ms a view", ms(build), ms(encode), ms(decode));
    println!("{} bytes a view ({:.1} KB/s at 20 Hz): intros {}, units {}, player {}, combat {}, projectiles and areas {}", b(5), b(5) as f64 * 20.0 / 1024.0, b(0), b(1), b(2), b(3), b(4));
}

/// Postcard size.
fn size<T: serde::Serialize>(v: &T) -> usize {
    postcard::to_stdvec(v).unwrap().len()
}
