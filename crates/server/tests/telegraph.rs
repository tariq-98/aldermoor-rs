//! Warnings on the server's clock (#197): with 150 ms of round trip, a roll pressed the moment
//! a warning's fill completes on the client's screen gets to the server in time to roll
//! through the blow. Drawn on the view's own (late) time instead, the same press is too late.

use glam::Vec2;
use protocol::{CombatView, Command, Event};
use sim::{Brain, Sim, Specialty, Stats, Transform};
use std::collections::VecDeque;

const STEP: f64 = 1. / 60.;
/// One way, client to server and back: 150 ms round trip.
const LATENCY: f64 = 0.075;

/// Plays the golem's warned quake at a hero over a link with [`LATENCY`] each way, pressing
/// roll when the fill completes as the client draws it (`ahead` says whether the client draws
/// warnings on the server's clock or on the view's). Returns the damage the hero took.
fn quake_with_latency(ahead: bool) -> f32 {
    let mut sim = Sim::new(7);
    let road = Vec2::new(5.0, 10.0);
    let hero = sim.spawn_hero("Hero", Specialty::Warrior, 1, road, 0.0);
    sim.get_mut::<Stats>(hero).unwrap().crit = 0.0;
    let golem = sim.spawn_mob("golem", road + Vec2::new(0.0, 3.0)).unwrap();
    let e = sim.entity(golem).unwrap();
    sim.world.entity_mut(e).remove::<Brain>();
    sim.get_mut::<Transform>(golem).unwrap().yaw = std::f32::consts::PI;
    sim.step();
    let ability = sim::content::catalog().ability_id("golem_quake").unwrap();
    assert!(sim::combat::abilities::use_ability(&mut sim.world, e, ability, Some(hero), None, true));

    // Views on their way to the client, and commands on their way to the server.
    let mut to_client: VecDeque<(f64, f64, CombatView)> = VecDeque::new();
    let mut to_server: VecDeque<(f64, Command)> = VecDeque::new();
    let mut seen: Option<(f64, CombatView)> = None;
    let mut pressed = false;
    let mut hurt = 0.0;
    for step in 0..240 {
        let now = step as f64 * STEP;
        while to_server.front().is_some_and(|(at, _)| *at <= now) {
            let (_, c) = to_server.pop_front().unwrap();
            sim.command(hero, c);
        }
        sim.step();
        for ev in sim.take_events() {
            if let Event::Damage { target, amount, .. } = ev
                && target == hero
            {
                hurt += amount;
            }
        }
        to_client.push_back((now + LATENCY, sim.time(), sim.combat_view(Some(hero))));
        while to_client.front().is_some_and(|(at, ..)| *at <= now) {
            let (_, t, v) = to_client.pop_front().unwrap();
            seen = Some((t, v));
        }
        // The client's frame: its clock knows the views arrive LATENCY late (its smoothed
        // offset), and QUIC knows the round trip.
        let Some((view_time, view)) = &seen else { continue };
        let server_now = (now - LATENCY) + LATENCY;
        let drawn = if ahead { protocol::telegraph_time(server_now, 2. * LATENCY) } else { *view_time };
        let fill = view
            .areas
            .iter()
            .filter(|a| a.warning)
            .map(|a| (a.elapsed as f64 + (drawn - view_time)) / a.duration as f64)
            .fold(0.0, f64::max);
        if !pressed && fill >= 1.0 {
            pressed = true;
            to_server.push_back((now + LATENCY, Command::Dodge { direction: Vec2::new(0.0, -1.0) }));
        }
    }
    assert!(pressed, "the warning never filled");
    hurt
}

#[test]
fn a_roll_pressed_as_the_warning_fills_on_screen_lands_in_time() {
    assert_eq!(quake_with_latency(true), 0.0, "rolled as the fill completed and was still hit");
    assert!(quake_with_latency(false) > 0.0, "drawn on the view's late time, the roll should be too late");
}
