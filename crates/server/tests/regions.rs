//! A world split into regions (#168): crossing a border, fighting across one, monsters that
//! stay on their own ground, and the same world every run.

use glam::Vec2;
use protocol::Command;
use server::regions::Realm;
use sim::combat::{auras, source};
use sim::region::{Bounds, BLOCK};
use sim::{brambleford_land, Brain, Inventory, Progress, Sim, Specialty, Vitals};

/// The border runs north–south at x = 5, across the open north road.
const BORDER: f32 = 5.0;

fn west() -> Bounds {
    Bounds::new(Vec2::new(-300.0, -300.0), Vec2::new(BORDER, 300.0))
}

fn east() -> Bounds {
    Bounds::new(Vec2::new(BORDER, -300.0), Vec2::new(300.0, 300.0))
}

fn two_regions(seed: u64) -> (Sim, Sim) {
    let land = brambleford_land();
    (Sim::region(land.clone(), seed, 0, west()), Sim::region(land, seed + 1, 1, east()))
}

fn run(realm: &mut Realm, seconds: f32) {
    for _ in 0..(seconds * 60.0).round() as u32 {
        realm.step();
    }
}

fn walk(realm: &mut Realm, id: u32, direction: Vec2) {
    realm.command(id, Command::Move { direction, walk: false, sprint: false, face_yaw: None });
}

#[test]
fn a_player_walks_across_a_border_and_keeps_everything() {
    let (mut a, b) = two_regions(3);
    let hero = a.spawn_hero("Hero", Specialty::Mage, 7, Vec2::new(-2.0, 10.0), 0.0);
    a.grant_quest_xp(hero, 30);
    a.get_mut::<Inventory>(hero).unwrap().coins = 77;
    let e = a.entity(hero).unwrap();
    let src = source(&a.world, e, None).unwrap();
    assert!(auras::apply_key(&mut a.world, "revive_sickness", e, &src));
    let mut realm = Realm::new(vec![a, b]);
    assert_eq!(realm.region_of(hero), 0);

    walk(&mut realm, hero, Vec2::X);
    run(&mut realm, 3.0);
    walk(&mut realm, hero, Vec2::ZERO);
    run(&mut realm, 0.1);
    assert_eq!(realm.region_of(hero), 1, "still in the west after walking well past the border");
    let (level, unassigned, coins, sick, x) = realm.with_unit(hero, |sim, id| {
        let e = sim.entity(id).unwrap();
        (
            sim.get::<Progress>(id).unwrap().level(Specialty::Mage),
            sim.get::<Progress>(id).unwrap().unassigned,
            sim.get::<Inventory>(id).unwrap().coins,
            auras::has(&sim.world, e, "revive_sickness"),
            sim.get::<sim::Transform>(id).unwrap().pos.x,
        )
    });
    assert!(x > BORDER + 4.0);
    assert_eq!((level, unassigned, coins), (7, 30, 77));
    assert!(sick, "an aura was lost on the way");
    // The west no longer has it, except as a ghost.
    assert!(realm.with(0, move |sim| sim.get::<Progress>(hero).is_none() && sim.entity(hero).is_some()));
    // Its own snapshot comes from the east now.
    assert!(realm.snapshot(hero).player.is_some_and(|p| p.unit == hero));
}

#[test]
fn a_blow_across_the_border_lands_in_the_other_region() {
    let (mut a, mut b) = two_regions(5);
    let hero = a.spawn_hero("Hero", Specialty::Warrior, 10, Vec2::new(BORDER - 1.0, 10.0), std::f32::consts::FRAC_PI_2);
    let skeleton = b.spawn_mob("skeleton", Vec2::new(BORDER + 1.0, 10.0)).unwrap();
    let e = b.entity(skeleton).unwrap();
    b.world.entity_mut(e).remove::<Brain>();
    assert_eq!(skeleton / BLOCK, 1);
    let mut realm = Realm::new(vec![a, b]);
    run(&mut realm, 0.1); // Each side sees the other's ghost.
    assert!(realm.snapshot(hero).units.iter().any(|u| u.id == skeleton), "the skeleton across the border isn't in view");

    realm.command(hero, Command::SetTarget(Some(skeleton)));
    realm.command(hero, Command::UseSlot { slot: 0, ground: None });
    realm.command(hero, Command::SetAutoAttack(false));
    run(&mut realm, 1.0);
    let v = realm.with(1, move |sim| *sim.get::<Vitals>(skeleton).unwrap());
    assert!(v.health < v.max_health, "the blow didn't land across the border");
    assert_eq!(realm.region_of(skeleton), 1);
    assert!(realm.stats().ticks > 0);
}

#[test]
fn monsters_turn_back_at_their_border() {
    let (mut a, b) = two_regions(9);
    let hero = a.spawn_player("Hero", Vec2::new(BORDER - 3.0, 10.0), 0.0);
    let mut realm = Realm::new(vec![a, b]);
    let wolf = realm.with(1, |sim| sim.spawn_mob("wolf", Vec2::new(BORDER + 6.0, 10.0)).unwrap());
    let mut west_of_border = 0;
    for _ in 0..8 * 60 {
        realm.step();
        assert_eq!(realm.region_of(wolf), 1, "the wolf crossed into the west");
        let x = realm.with(1, move |sim| sim.get::<sim::Transform>(wolf).unwrap().pos.x);
        if x < BORDER {
            west_of_border += 1;
        }
    }
    assert!(west_of_border < 60, "the wolf spent {west_of_border} steps past its border");
    assert_eq!(realm.region_of(hero), 0);
}

#[test]
fn the_same_seeds_and_commands_give_the_same_world() {
    fn world() -> Vec<String> {
        let (mut a, mut b) = two_regions(11);
        a.spawn_crowd_in(60, Vec2::new(-20.0, 10.0), Vec2::new(20.0, 30.0));
        b.spawn_crowd_in(60, Vec2::new(25.0, 10.0), Vec2::new(20.0, 30.0));
        let hero = a.spawn_hero("Hero", Specialty::Warrior, 10, Vec2::new(-2.0, 10.0), 0.0);
        let mut realm = Realm::new(vec![a, b]);
        let mut seen = Vec::new();
        let (mut handoffs, mut requests) = (0, 0);
        for step in 0..6 * 60 {
            // East through both crowds, weaving.
            let a = step as f32 / 90.0;
            walk(&mut realm, hero, Vec2::new(1.0, a.sin() * 0.5));
            if step % 30 == 0 {
                realm.command(hero, Command::UseSlot { slot: 0, ground: None });
            }
            realm.step();
            handoffs += realm.stats().handoffs;
            requests += realm.stats().requests;
            if step % 60 == 59 {
                for r in 0..2 {
                    seen.push(format!("{:?}", realm.with(r, |sim| sim.snapshot(None).units)));
                }
            }
        }
        assert_eq!(realm.region_of(hero), 1, "the hero never crossed");
        assert!(handoffs > 0 && requests > 0, "nothing crossed the border ({handoffs} handoffs, {requests} requests)");
        seen.push(format!("{handoffs} {requests}"));
        seen
    }
    let (first, second) = (world(), world());
    assert!(first == second, "two runs of the same world came out differently");
}
