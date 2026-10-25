//! Lag compensation: a player's shot at another player is tested where the shooter saw them
//! (#169).

use std::f32::consts::PI;

use glam::Vec2;
use protocol::{Command, Event};
use sim::{Brain, Faction, Motion, Outbox, Sim, Specialty, Stats, Transform, Unit};

const ROAD: Vec2 = Vec2::new(5.0, 10.0);
/// Slot 0 of the archer's bow.
const ARROW_SPEED: f32 = 55.0;

/// An archer and a target 12 m up the road running across their aim: another player turned
/// hostile, or a goblin. Steps until the target is up to speed, recording where it stood each
/// tick. Then the archer looses an arrow at where the target was `behind` ticks ago, led by its
/// speed as the archer saw it, saying (if `says`) that's the tick they're viewing. Returns
/// whether it struck.
fn shoot(player: bool, behind: u64, says: bool) -> bool {
    let mut sim = Sim::new(7);
    let archer = sim.spawn_hero("Archer", Specialty::Archer, 1, ROAD, 0.0);
    sim.get_mut::<Stats>(archer).unwrap().crit = 0.0;
    let at = ROAD + Vec2::new(-4.0, 12.0);
    let target = if player {
        let id = sim.spawn_hero("Runner", Specialty::Warrior, 1, at, PI);
        sim.get_mut::<Unit>(id).unwrap().faction = Faction::Hostile;
        id
    } else {
        let id = sim.spawn_mob("goblin", at).unwrap();
        let e = sim.entity(id).unwrap();
        sim.world.entity_mut(e).remove::<Brain>();
        id
    };
    let mut seen = Vec::new();
    for _ in 0..60 {
        if player {
            sim.command(target, Command::Move { direction: Vec2::X, walk: false, sprint: false, face_yaw: Some(PI) });
        } else {
            sim.get_mut::<Motion>(target).unwrap().intent.direction = Vec2::X;
        }
        sim.step();
        seen.push((sim.tick(), sim.get::<Transform>(target).unwrap().pos));
    }
    let now = sim.tick();
    let viewed = now - behind;
    let pos = |tick: u64| seen.iter().find(|(t, _)| *t == tick).unwrap().1;
    let was = pos(viewed);
    let velocity = (pos(viewed) - pos(viewed - 10)) * 6.0;
    let from = sim.get::<Transform>(archer).unwrap().pos;
    let ground = was + velocity * ((was - from).length() / ARROW_SPEED);
    sim.command(archer, Command::SetTarget(Some(target)));
    if says {
        // It arrives with the shot, a step later, so the view is a step further behind by then.
        sim.command(archer, Command::Viewing { tick: viewed });
    }
    sim.command(archer, Command::UseSlot { slot: 0, ground: Some(ground) });
    sim.command(archer, Command::SetAutoAttack(false));
    let mut hit = false;
    for _ in 0..48 {
        if player {
            sim.command(target, Command::Move { direction: Vec2::X, walk: false, sprint: false, face_yaw: Some(PI) });
        } else {
            sim.get_mut::<Motion>(target).unwrap().intent.direction = Vec2::X;
        }
        sim.step();
        let events = std::mem::take(&mut sim.world.resource_mut::<Outbox>().0);
        hit |= events.iter().any(|e| matches!(e, Event::Damage { target: t, source, .. } if *t == target && *source == archer));
    }
    hit
}

#[test]
fn a_shot_at_where_a_player_was_100_ms_ago_hits() {
    assert!(shoot(true, 6, true), "the rewound shot should land");
    assert!(!shoot(true, 6, false), "without the viewed tick the same aim is behind the runner");
    assert!(shoot(true, 0, false), "a shot at where they are now hits without any rewind");
}

#[test]
fn a_view_400_ms_behind_is_only_rewound_250_ms() {
    assert!(shoot(true, 14, true), "250 ms back is still rewound");
    assert!(!shoot(true, 24, true), "400 ms back is tested at 250 ms, so it misses");
}

#[test]
fn monsters_are_never_rewound() {
    assert!(!shoot(false, 14, true), "a goblin isn't where the archer saw it");
    assert!(shoot(false, 0, true), "a goblin is hit where it is now");
}
