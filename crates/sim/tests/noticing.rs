//! Monsters notice players from shorter range as the players outlevel them, and not at all from
//! 10 levels up (#199).

use glam::Vec2;
use protocol::Command;
use sim::{notice_radius, Brain, Mode, Sim, Specialty};

/// Open road, away from the valley's own monsters.
const ROAD: Vec2 = Vec2::new(5.0, 10.0);

/// A wolf at `level` and a hero of `hero_level` `distance` metres from it. Returns whether the
/// wolf took up the chase.
fn noticed(level: u8, hero_level: u8, distance: f32) -> bool {
    let mut sim = Sim::new(4);
    let wolf = sim.spawn_mob_at_level("wolf", ROAD, level).unwrap();
    sim.spawn_hero("Hero", Specialty::Warrior, hero_level, ROAD + Vec2::new(distance, 0.0), 0.0);
    sim.run_for(2.0);
    sim.get::<Brain>(wolf).unwrap().mode == Mode::Chase
}

fn base() -> f32 {
    let mut sim = Sim::new(4);
    let wolf = sim.spawn_mob("wolf", ROAD).unwrap();
    sim.get::<Brain>(wolf).unwrap().aggro_radius
}

#[test]
fn the_notice_radius_shrinks_and_grows_by_a_metre_a_level() {
    assert_eq!(notice_radius(12.0, 5, 5), Some(12.0));
    assert_eq!(notice_radius(12.0, 5, 8), Some(9.0));
    assert_eq!(notice_radius(12.0, 5, 2), Some(15.0));
    // From 4 m to twice its own.
    assert_eq!(notice_radius(12.0, 5, 14), Some(4.0));
    assert_eq!(notice_radius(12.0, 30, 1), Some(24.0));
    // Ten levels up, never.
    assert_eq!(notice_radius(12.0, 5, 15), None);
    assert_eq!(notice_radius(12.0, 5, 40), None);
}

#[test]
fn a_monster_notices_its_own_level_at_its_own_radius() {
    let r = base();
    assert!(noticed(5, 5, r - 1.0));
    assert!(!noticed(5, 5, r + 1.5));
}

#[test]
fn a_player_above_it_has_to_come_closer() {
    let r = base();
    assert!(!noticed(5, 10, r - 1.0), "a player five levels up was noticed from the full radius");
    assert!(noticed(5, 10, r - 6.0));
}

#[test]
fn a_player_below_it_is_noticed_from_further() {
    let r = base();
    assert!(noticed(10, 5, r + 4.0), "a player five levels down wasn't noticed 4 m past the radius");
    assert!(!noticed(10, 5, r + 6.5));
}

#[test]
fn ten_levels_up_it_leaves_you_alone_until_you_start_it() {
    let mut sim = Sim::new(4);
    let wolf = sim.spawn_mob_at_level("wolf", ROAD, 2).unwrap();
    let hero = sim.spawn_hero("Hero", Specialty::Warrior, 12, ROAD + Vec2::new(2.0, 0.0), std::f32::consts::PI * 1.5);
    sim.run_for(2.0);
    assert_eq!(sim.get::<Brain>(wolf).unwrap().mode, Mode::Idle, "it picked a fight ten levels up");
    sim.command(hero, Command::SetTarget(Some(wolf)));
    sim.command(hero, Command::UseSlot { slot: 0, ground: None });
    sim.run_for(1.0);
    assert_eq!(sim.get::<Brain>(wolf).unwrap().mode, Mode::Chase, "it didn't fight back");
}
