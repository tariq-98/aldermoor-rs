//! Views on the wire (#34): what the decoder rebuilds matches what the sim saw, to within the
//! quantising, however late the acks come, and the views are small.

use glam::Vec2;
use protocol::wire::{Decoder, Encoder, WireError};
use protocol::{CombatView, Command, PlayerState, Snapshot, UnitState};
use sim::{Sim, Specialty};

/// A hero fighting through a crowd, so units move, die, cast and wear auras.
fn fight(seed: u64) -> (Sim, u32) {
    let mut sim = Sim::brambleford(seed);
    sim.spawn_crowd_in(60, Vec2::new(6.0, 10.0), Vec2::splat(15.0));
    let hero = sim.spawn_hero("Hero", Specialty::Warrior, 10, Vec2::new(6.0, 20.0), 0.0);
    sim.command(hero, Command::SetAutoAttack(true));
    (sim, hero)
}

fn act(sim: &mut Sim, hero: u32, step: u64) {
    let a = step as f32 / 200.0;
    sim.command(hero, Command::Move { direction: Vec2::new(a.cos(), a.sin()), walk: false, sprint: false, face_yaw: None });
    // The hero looks up and down: others see its aim pitch (#244).
    if step % 15 == 0 {
        sim.command(hero, Command::Aim { pitch: ((step / 15) % 200) as i8 - 100 });
    }
    if step % 40 == 0 {
        sim.command(hero, Command::UseSlot { slot: (step / 40 % 5) as u8, ground: None });
    }
}

fn near(a: f32, b: f32, by: f32, what: &str) {
    assert!((a - b).abs() <= by || a == b, "{what}: {a} against {b}");
}

/// Whether `b` is `a` to within the quantising, `ago` seconds later.
fn unit_close(a: &UnitState, b: &UnitState, ago: f32) -> Result<(), String> {
    let close = |x: f32, y: f32, by: f32, what: &str| if (x - y).abs() <= by || x == y { Ok(()) } else { Err(format!("unit {} {what}: {x} against {y}", a.id)) };
    if (a.id, a.kind, &a.body, &a.name, a.level, a.pose, a.traversal, a.aim_pitch) != (b.id, b.kind, &b.body, &b.name, b.level, b.pose, b.traversal, b.aim_pitch) {
        return Err(format!("unit {}: {:?} against {:?}", a.id, (a.level, a.pose, a.traversal), (b.level, b.pose, b.traversal)));
    }
    if (a.max_health, a.hostile, a.target, &*a.look) != (b.max_health, b.hostile, b.target, &*b.look) {
        return Err(format!("unit {}: target, health or look", a.id));
    }
    for i in 0..3 {
        close(a.position[i], b.position[i], 0.0051, "position")?;
        close(a.velocity[i], b.velocity[i], 0.026, "velocity")?;
    }
    let turn = (a.yaw - b.yaw).rem_euclid(std::f32::consts::TAU);
    close(turn.min(std::f32::consts::TAU - turn), 0.0, 1e-4, "yaw")?;
    close(a.pose_time + ago, b.pose_time, 0.004, "pose time")?;
    close(a.health, b.health, 0.051, "health")
}

/// Units near the player come back as they were sent; further out, as one of the last few views
/// sent had them (they go out every second, fourth or eighth view).
fn same_unit(a: &UnitState, b: &UnitState, me: Option<glam::Vec3>, history: &[(Snapshot, CombatView)]) {
    if me.is_none_or(|m| m.distance(a.position) < 29.0) {
        unit_close(a, b, 0.0).unwrap();
        return;
    }
    let now = history.last().unwrap().0.time;
    let tries: Vec<String> = history
        .iter()
        .rev()
        .take(8)
        // Its look (an intro) goes at once; the rest waits its turn.
        .filter_map(|(s, _)| s.unit(a.id).map(|old| unit_close(&UnitState { look: a.look.clone(), ..old.clone() }, b, (now - s.time) as f32)))
        .map(|r| r.err().unwrap_or_default())
        .collect();
    assert!(tries.iter().any(String::is_empty), "far unit {} isn't as any recent view had it: {tries:?}", a.id);
}

/// Timers come back to the tick; bars to the point.
fn same_player(a: &PlayerState, b: &PlayerState) {
    let tick = 1.0 / 60.0 + 1e-3;
    near(a.endurance, b.endurance, 0.51, "endurance");
    near(a.resource, b.resource, 0.51, "resource");
    near(a.gcd, b.gcd, tick, "gcd");
    near(a.swap_ready, b.swap_ready, tick, "swap");
    near(a.potion_ready, b.potion_ready, tick, "potion");
    for (x, y) in a.cooldowns.iter().zip(&b.cooldowns) {
        near(*x, *y, tick, "cooldown");
    }
    for (x, y) in a.bar.iter().zip(&b.bar) {
        near(x.cooldown, y.cooldown, tick, "slot cooldown");
    }
    for (x, y) in a.auras.iter().zip(&b.auras) {
        near(x.remaining, y.remaining, tick, "aura");
    }
    let mut b = b.clone();
    // What's left must match exactly.
    b.endurance = a.endurance;
    b.resource = a.resource;
    b.gcd = a.gcd;
    b.swap_ready = a.swap_ready;
    b.potion_ready = a.potion_ready;
    b.cooldowns = a.cooldowns.clone();
    for (x, y) in a.bar.iter().zip(&mut b.bar) {
        y.cooldown = x.cooldown;
    }
    for (x, y) in a.auras.iter().zip(&mut b.auras) {
        y.remaining = x.remaining;
    }
    if let (Some(x), Some(y)) = (&a.cast, &mut b.cast) {
        near(x.elapsed, y.elapsed, tick, "cast");
        y.elapsed = x.elapsed;
    }
    if let (Some(x), Some(y)) = (&a.work, &mut b.work) {
        near(x.elapsed, y.elapsed, tick, "work");
        y.elapsed = x.elapsed;
    }
    assert_eq!(*a, b);
}

fn same_combat(a: &CombatView, b: &CombatView) {
    let tick = 1.0 / 60.0 + 1e-3;
    assert_eq!((a.tick, a.units.len(), &a.projectiles, a.areas.len()), (b.tick, b.units.len(), &b.projectiles, b.areas.len()));
    for (x, y) in a.units.iter().zip(&b.units) {
        near(x.action_time, y.action_time, tick, "action time");
        near(x.resource, y.resource, 0.51, "unit resource");
        let mut y = y.clone();
        y.action_time = x.action_time;
        y.resource = x.resource;
        for (p, q) in x.auras.iter().zip(&mut y.auras) {
            near(p.remaining, q.remaining, tick, "unit aura");
            q.remaining = p.remaining;
        }
        if let (Some(p), Some(q)) = (&x.cast, &mut y.cast) {
            near(p.elapsed, q.elapsed, tick, "unit cast");
            q.elapsed = p.elapsed;
        }
        assert_eq!(*x, y);
    }
    for (x, y) in a.areas.iter().zip(&b.areas) {
        near(x.elapsed, y.elapsed, tick, "area");
        for i in 0..3 {
            near(x.center[i], y.center[i], 0.051, "area centre");
        }
        assert_eq!(*x, protocol::AreaView { elapsed: x.elapsed, center: x.center, ..y.clone() });
    }
}

/// `history` ends with what was sent; `got` is what came back.
fn same(history: &[(Snapshot, CombatView)], got: &(Snapshot, CombatView)) {
    let sent = history.last().unwrap();
    let (a, b) = (&sent.0, &got.0);
    assert_eq!((a.tick, a.time, &a.events), (b.tick, b.time, &b.events));
    assert_eq!(a.units.iter().map(|u| u.id).collect::<Vec<_>>(), b.units.iter().map(|u| u.id).collect::<Vec<_>>(), "different units in view");
    let me = a.player.as_ref().and_then(|p| a.unit(p.unit)).map(|u| u.position);
    for (x, y) in a.units.iter().zip(&b.units) {
        same_unit(x, y, me, history);
    }
    match (&a.player, &b.player) {
        (Some(x), Some(y)) => same_player(x, y),
        (None, None) => {}
        _ => panic!("the player state came and went"),
    }
    same_combat(&sent.1, &got.1);
}

/// Steps `seconds`, sending a view every third step; the client acks only every `ack_every`th.
/// `check` gets the views sent so far (the last few, newest last), what came back, and its size.
fn play(seconds: u64, ack_every: u64, mut check: impl FnMut(&[(Snapshot, CombatView)], &(Snapshot, CombatView), usize)) {
    let (mut sim, hero) = fight(3);
    let (mut encoder, mut decoder) = (Encoder::new(), Decoder::new());
    let (mut events, mut history) = (Vec::new(), Vec::new());
    for step in 0..seconds * 60 {
        act(&mut sim, hero, step);
        sim.step();
        events.extend(sim.take_events());
        if step % 3 != 0 {
            continue;
        }
        let mut snapshot = sim.snapshot(Some(hero));
        snapshot.events = std::mem::take(&mut events);
        let combat = sim.combat_view(Some(hero));
        let view = encoder.encode(&snapshot, &combat);
        let size = postcard::to_stdvec(&view).unwrap().len();
        let tick = view.tick;
        let got = decoder.decode(view).unwrap();
        history.push((snapshot, combat));
        if history.len() > 12 {
            history.remove(0);
        }
        check(&history, &got, size);
        if step / 3 % ack_every == 0 {
            encoder.ack(tick);
        }
    }
}

#[test]
fn views_come_back_as_they_were_sent() {
    let (mut fought, mut cast) = (false, false);
    play(10, 1, |history, got, _| {
        same(history, got);
        let sent = history.last().unwrap();
        fought |= sent.0.events.iter().any(|e| matches!(e, protocol::Event::Damage { .. }));
        cast |= sent.1.units.iter().any(|u| !u.auras.is_empty());
    });
    assert!(fought && cast, "the fight never got going");
}

#[test]
fn late_and_missing_acks_only_make_bigger_deltas() {
    play(6, 4, |history, got, _| same(history, got));
}

#[test]
fn timers_survive_the_turn_of_a_minute() {
    // 3,600 steps is a minute: every running timer is sent again then, and still comes back right.
    play(62, 2, |history, got, _| same(history, got));
}

#[test]
fn deltas_are_a_small_part_of_whole_views() {
    let (mut whole, mut wire, mut views) = (0, 0, 0);
    play(10, 1, |history, _, size| {
        let sent = history.last().unwrap();
        whole += postcard::to_stdvec(&sent.0).unwrap().len() + postcard::to_stdvec(&sent.1).unwrap().len();
        wire += size;
        views += 1;
    });
    let per_second = wire as f64 / views as f64 * 20.0;
    println!("{views} views: whole {whole} bytes, on the wire {wire} ({per_second:.0} bytes a second at 20 Hz)");
    assert!(wire * 10 < whole, "deltas are {wire} bytes against {whole} whole");
}

#[test]
fn a_view_against_a_base_the_client_lost_is_refused() {
    let (mut sim, hero) = fight(4);
    let mut encoder = Encoder::new();
    sim.step();
    let first = encoder.encode(&sim.snapshot(Some(hero)), &sim.combat_view(Some(hero)));
    encoder.ack(first.tick);
    sim.step();
    let second = encoder.encode(&sim.snapshot(Some(hero)), &sim.combat_view(Some(hero)));
    assert_eq!(second.base, Some(first.tick));
    assert!(matches!(Decoder::new().decode(second), Err(WireError::UnknownBase(_))));
    // Acking a view it never sent changes nothing.
    encoder.ack(999_999);
    sim.step();
    assert_eq!(encoder.encode(&sim.snapshot(Some(hero)), &sim.combat_view(Some(hero))).base, Some(first.tick));
}


#[test]
fn views_from_the_frame_match_views_from_snapshots() {
    // The server picks views out of a frame a step (#286): the player must end up with what their
    // own snapshot and combat view would have given them, view after view, with late acks.
    let (mut sim, hero) = fight(5);
    let (mut whole, mut picked) = (Encoder::new(), Encoder::new());
    let (mut from_whole, mut from_frame) = (Decoder::new(), Decoder::new());
    let (mut frame, mut events, mut sizes) = (None, Vec::new(), (0, 0));
    for step in 0..600u64 {
        act(&mut sim, hero, step);
        sim.step();
        events.extend(sim.take_events());
        // A frame every step, as the server builds them, though this player's view is every third.
        frame = Some(sim.net_frame(frame.as_ref()));
        if step % 3 != 0 {
            continue;
        }
        let mut snapshot = sim.snapshot(Some(hero));
        snapshot.events = std::mem::take(&mut events);
        let a = whole.encode(&snapshot, &sim.combat_view(Some(hero)));
        let b = sim.encode_view(frame.as_ref().unwrap(), hero, &mut picked, &snapshot.events);
        sizes.0 += postcard::to_stdvec(&a).unwrap().len();
        sizes.1 += postcard::to_stdvec(&b).unwrap().len();
        let tick = a.tick;
        assert_eq!(from_whole.decode(a).unwrap(), from_frame.decode(b).unwrap(), "view {tick}");
        if step / 3 % 2 == 0 {
            whole.ack(tick);
            picked.ack(tick);
        }
    }
    // Detail that changed and changed back between views may go again from the frame, no more.
    assert!(sizes.1 <= sizes.0 + sizes.0 / 20, "{} bytes from the frame against {}", sizes.1, sizes.0);
}

#[test]
fn players_get_only_the_events_they_can_see() {
    // A fight at one end of the valley, and a player too far away to see it (#292).
    let (mut sim, hero) = fight(6);
    let far = sim.spawn_player("Far", Vec2::new(6.0, -160.0), 0.0);
    let frame = sim.net_frame(None);
    let (mut near_view, mut far_view) = (Encoder::new(), Encoder::new());
    let _ = sim.encode_view(&frame, hero, &mut near_view, &[]);
    let _ = sim.encode_view(&frame, far, &mut far_view, &[]);
    let (mut frame, mut fought) = (frame, false);
    for step in 0..300 {
        act(&mut sim, hero, step);
        sim.step();
        let events = sim.take_events();
        frame = sim.net_frame(Some(&frame));
        let (a, b) = (sim.encode_view(&frame, hero, &mut near_view, &events), sim.encode_view(&frame, far, &mut far_view, &events));
        // The fighter sees all of it.
        assert_eq!(a.events, events);
        fought |= events.iter().any(|e| matches!(e, protocol::Event::Damage { .. }));
        // The other sees nothing but what's about themselves, or messages.
        for e in &b.events {
            let about = e.units();
            assert!(about.contains(&Some(far)) || (about == [None, None] && e.at().is_none()), "{e:?} reached a player 180 m away");
        }
    }
    assert!(fought, "the fight never got going");
}
