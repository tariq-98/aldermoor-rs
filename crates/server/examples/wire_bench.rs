//! What one network player's views cost (#34, #286): `cargo run -p server --release --example wire_bench`.
//!
//! A player stands among the load test's 2,000 monsters and gets a view every third step, as the
//! server sends them. Prints the time to build, encode and decode a view, and its size on the
//! wire by part, after the first (which introduces everyone in view): first from a whole
//! snapshot and combat view, then picked out of the step's shared frame, as the server does.
//! The two must give the same views.

use protocol::wire::{Decoder, Encoder};
use std::time::Instant;

fn main() {
    let mut sim = sim::Sim::brambleford(1);
    sim.spawn_crowd(2000);
    let hero = sim.spawn_player("Hero", glam::Vec2::new(6.0, 28.0), 0.0);
    for _ in 0..120 {
        sim.step();
    }
    let (mut encoder, mut decoder, mut direct) = (Encoder::new(), Decoder::new(), Encoder::new());
    let mut frame = sim.net_frame(None);
    let mut snapshot = sim.snapshot(Some(hero));
    snapshot.events.clear();
    let first = encoder.encode(&snapshot, &sim.combat_view(Some(hero)));
    same(&first, &sim.encode_view(&frame, hero, &mut direct, &[]), 0);
    println!("{} units in view of {}; the first view is {} bytes", sim.snapshot(Some(hero)).units.len(), frame.wire.len(), postcard::to_stdvec(&first).unwrap().len());
    encoder.ack(first.tick);
    direct.ack(first.tick);
    decoder.decode(first).unwrap();
    let (mut build, mut encode, mut decode, mut framing, mut picking) = (0.0, 0.0, 0.0, 0.0, 0.0);
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
        frame = sim.net_frame(Some(&frame));
        let t3 = Instant::now();
        let picked = sim.encode_view(&frame, hero, &mut direct, &snapshot.events);
        let t4 = Instant::now();
        same(&view, &picked, step);
        bytes[0] += size(&view.intros);
        bytes[1] += view.units.len();
        bytes[2] += size(&view.player);
        bytes[3] += size(&view.combat.units);
        bytes[4] += size(&view.combat.projectiles) + size(&view.combat.areas);
        bytes[5] += size(&view);
        let tick = view.tick;
        let t5 = Instant::now();
        decoder.decode(view).unwrap();
        decode += t5.elapsed().as_secs_f64();
        encoder.ack(tick);
        direct.ack(tick);
        build += (t1 - t0).as_secs_f64();
        encode += (t2 - t1).as_secs_f64();
        framing += (t3 - t2).as_secs_f64();
        picking += (t4 - t3).as_secs_f64();
        views += 1;
    }
    let ms = |s: f64| s / views as f64 * 1000.0;
    let b = |i: usize| bytes[i] / views;
    println!("from a snapshot: build {:.3} ms, encode {:.3} ms, decode {:.3} ms a view", ms(build), ms(encode), ms(decode));
    println!("from the frame: {:.3} ms a step for the frame (shared by every player), {:.3} ms a view", ms(framing), ms(picking));
    println!("{} bytes a view ({:.1} KB/s at 20 Hz): intros {}, units {}, player {}, combat {}, projectiles and areas {}", b(5), b(5) as f64 * 20.0 / 1024.0, b(0), b(1), b(2), b(3), b(4));
}

fn same(a: &protocol::wire::WireView, b: &protocol::wire::WireView, step: u32) {
    let parts = [
        ("head", (a.tick, a.time, a.base) == (b.tick, b.time, b.base)),
        ("intros", a.intros == b.intros),
        ("units", a.units == b.units),
        ("gone", a.gone == b.gone),
        ("player", a.player == b.player),
        ("combat units", a.combat.units == b.combat.units),
        ("combat gone", a.combat.gone == b.combat.gone),
        ("projectiles", a.combat.projectiles == b.combat.projectiles),
        ("areas", a.combat.areas == b.combat.areas && a.combat.areas_gone == b.combat.areas_gone),
        ("events", a.events == b.events),
    ];
    let differ: Vec<&str> = parts.iter().filter(|p| !p.1).map(|p| p.0).collect();
    assert!(differ.is_empty(), "the frame's view of step {step} differs in {differ:?} ({} against {} intros, {} against {} combat units)", a.intros.len(), b.intros.len(), a.combat.units.len(), b.combat.units.len());
}

/// Postcard size.
fn size<T: serde::Serialize>(v: &T) -> usize {
    postcard::to_stdvec(v).unwrap().len()
}
