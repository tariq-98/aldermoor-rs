//! The Wilderness (#76): bands with level ceilings, fights between players only inside it, a
//! death there dropping everything, and richer loot deeper in.

use glam::{Vec2, Vec3};
use protocol::{Command, Event};
use sim::wilderness::{Band, Wilderness};
use sim::{Credit, Inventory, Loot, Sim, Specialty, Stats, Transform, Unit, Vitals};

/// The north road, clear ground far from the valley's monsters.
const ROAD: Vec2 = Vec2::new(5.0, 10.0);

/// Two bands along the road going north (-z): band 1 (levels up to 10) from z = 6 to 0, band 2
/// (no limit) from z = 0 to -20. South of z = 6 is outside the Wilderness.
fn wild_road(seed: u64) -> Sim {
    let mut sim = Sim::new(seed);
    sim.world.insert_resource(Wilderness {
        bands: vec![
            Band { min: Vec2::new(-20.0, 0.0), max: Vec2::new(30.0, 6.0), ceiling: Some(10), tier: 1 },
            Band { min: Vec2::new(-20.0, -20.0), max: Vec2::new(30.0, 0.0), ceiling: None, tier: 3 },
        ],
    });
    sim
}

fn hero(sim: &mut Sim, name: &str, level: u8, at: Vec2) -> u32 {
    let id = sim.spawn_hero(name, Specialty::Warrior, level, at, 0.0);
    sim.get_mut::<Stats>(id).unwrap().crit = 0.0;
    id
}

fn put(sim: &mut Sim, id: u32, at: Vec2) {
    let y = sim.land().ground(at.x, at.y);
    sim.get_mut::<Transform>(id).unwrap().pos = Vec3::new(at.x, y, at.y);
}

fn told(sim: &mut Sim, id: u32) -> Vec<String> {
    let s = sim.snapshot(Some(id));
    s.events
        .into_iter()
        .filter_map(|e| match e {
            Event::Message { text } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn a_strong_player_fights_at_the_bands_ceiling_and_at_their_own_level_again_outside() {
    let mut sim = wild_road(7);
    let id = hero(&mut sim, "Hero", 20, ROAD);
    sim.step();
    let full = sim.get::<Vitals>(id).unwrap().max_health;
    let power = sim.get::<Stats>(id).unwrap().power;
    // Into band 1 (levels up to 10).
    put(&mut sim, id, Vec2::new(5.0, 3.0));
    sim.step();
    assert!(told(&mut sim, id).iter().any(|t| t.contains("band 1") && t.contains("levels up to 10")));
    assert_eq!(sim.get::<Unit>(id).unwrap().level, 10);
    let scaled = sim.get::<Vitals>(id).unwrap().max_health;
    assert!((scaled / full - sim::stats::scale(10) / sim::stats::scale(20)).abs() < 1e-3, "{scaled} of {full}");
    assert!(sim.get::<Stats>(id).unwrap().power < power);
    // Deeper, with no limit: full strength.
    put(&mut sim, id, Vec2::new(5.0, -5.0));
    sim.step();
    assert!(told(&mut sim, id).iter().any(|t| t.starts_with("Deeper into the Wilderness: band 2")));
    assert_eq!(sim.get::<Unit>(id).unwrap().level, 20);
    // Back out of the Wilderness.
    put(&mut sim, id, Vec2::new(5.0, 3.0));
    sim.step();
    put(&mut sim, id, ROAD);
    sim.step();
    assert!(told(&mut sim, id).iter().any(|t| t == "You leave the Wilderness."));
    assert_eq!(sim.get::<Unit>(id).unwrap().level, 20);
    assert!((sim.get::<Vitals>(id).unwrap().max_health - full).abs() < 1e-3);
}

#[test]
fn a_player_below_the_ceiling_keeps_their_level() {
    let mut sim = wild_road(7);
    let id = hero(&mut sim, "Hero", 6, Vec2::new(5.0, 3.0));
    sim.step();
    assert_eq!(sim.get::<Unit>(id).unwrap().level, 6);
}

/// Has `a` swing at `b` with the basic attack for a couple of seconds; how much `b` lost.
fn swing(sim: &mut Sim, a: u32, b: u32) -> f32 {
    let before = sim.get::<Vitals>(b).unwrap().health;
    sim.command(a, Command::SetTarget(Some(b)));
    for _ in 0..120 {
        sim.command(a, Command::UseSlot { slot: 0, ground: None });
        sim.step();
    }
    before - sim.get::<Vitals>(b).unwrap().health
}

#[test]
fn players_can_fight_each_other_in_the_wilderness_and_nowhere_else() {
    // Side by side on the road, outside: blows don't land.
    let mut sim = wild_road(7);
    let a = hero(&mut sim, "A", 10, ROAD);
    let b = hero(&mut sim, "B", 10, ROAD + Vec2::new(0.0, -1.5));
    sim.get_mut::<Transform>(a).unwrap().yaw = std::f32::consts::PI;
    sim.step();
    assert_eq!(swing(&mut sim, a, b), 0.0, "a blow landed outside the Wilderness");

    // The same two inside band 1: they do.
    let mut sim = wild_road(7);
    let a = hero(&mut sim, "A", 10, Vec2::new(5.0, 4.5));
    let b = hero(&mut sim, "B", 10, Vec2::new(5.0, 3.0));
    sim.get_mut::<Transform>(a).unwrap().yaw = std::f32::consts::PI;
    sim.step();
    assert!(swing(&mut sim, a, b) > 0.0, "no blow landed in the Wilderness");

    // One inside and one just outside the edge: still not.
    let mut sim = wild_road(7);
    let a = hero(&mut sim, "A", 10, Vec2::new(5.0, 7.2));
    let b = hero(&mut sim, "B", 10, Vec2::new(5.0, 5.8));
    sim.get_mut::<Transform>(a).unwrap().yaw = std::f32::consts::PI;
    sim.step();
    assert_eq!(swing(&mut sim, a, b), 0.0, "a blow from outside landed on someone inside");
}

#[test]
fn dying_in_the_wilderness_drops_everything_for_the_killer() {
    let mut sim = wild_road(7);
    let killer = hero(&mut sim, "Killer", 10, Vec2::new(5.0, 4.0));
    let victim = hero(&mut sim, "Victim", 10, Vec2::new(5.0, 3.0));
    {
        let eco = content::items::economy();
        let mut bag = sim.get_mut::<Inventory>(victim).unwrap();
        bag.coins = 250;
        bag.add(eco.item("wolf_pelt").unwrap(), 3, false);
    }
    sim.step();
    let at = sim.get::<Transform>(victim).unwrap().pos;
    let e = sim.entity(victim).unwrap();
    sim::combat::life::die(&mut sim.world, e, Some(killer));
    assert_eq!(*sim.get::<Inventory>(victim).unwrap(), Inventory::default(), "the victim kept something");
    let pile = sim.world.resource::<Loot>().piles.iter().find(|p| (p.at - at).length() < 0.1).cloned().expect("no pile");
    assert_eq!(pile.owner, Some(killer));
    assert_eq!(pile.coins, 250);
    assert!(pile.items.iter().any(|s| s.item == "wolf_pelt" && s.count == 3));

    // Outside, a death keeps everything.
    let mut sim = wild_road(7);
    let victim = hero(&mut sim, "Victim", 10, ROAD);
    sim.get_mut::<Inventory>(victim).unwrap().coins = 250;
    sim.step();
    let e = sim.entity(victim).unwrap();
    sim::combat::life::die(&mut sim.world, e, None);
    assert_eq!(sim.get::<Inventory>(victim).unwrap().coins, 250);
}

#[test]
fn monsters_drop_more_deeper_in() {
    // The same wolf, felled many times at the edge and in band 2 (tier 3): coins add up faster
    // deeper in.
    let coins = |at: Vec2| {
        let mut total = 0;
        for seed in 0..40 {
            let mut sim = wild_road(seed);
            let p = hero(&mut sim, "Hero", 10, at + Vec2::new(1.5, 0.0));
            let w = sim.spawn_mob("wolf", at).unwrap();
            let e = sim.entity(w).unwrap();
            sim.get_mut::<Credit>(w).unwrap().0.push((p, Specialty::Warrior, 100.0));
            sim::combat::life::die(&mut sim.world, e, Some(p));
            total += sim.world.resource::<Loot>().piles.iter().map(|q| q.coins).sum::<u32>();
        }
        total
    };
    let edge = coins(ROAD);
    let deep = coins(Vec2::new(5.0, -10.0));
    assert!(deep as f32 > edge as f32 * 2.0, "deep {deep}, edge {edge}");
}
