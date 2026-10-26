//! A world split into regions (#168): crossing a border, fighting across one, monsters that
//! stay on their own ground, and the same world every run.

use glam::Vec2;
use protocol::Command;
use server::regions::Realm;
use sim::combat::{auras, source};
use sim::region::{Bounds, BLOCK, HANDOFF};
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
    // A pounce can land it nearly 4 m over; walking back from a standstill takes about a second.
    assert!(west_of_border < 70, "the wolf spent {west_of_border} steps past its border");
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

mod layers {
    use super::*;
    use server::regions::Layering;

    const CAP: usize = 4;

    /// The west as a town layered at `CAP` (or never, when `wild`), the east open country.
    fn layered(wild: bool) -> Realm {
        let (a, b) = two_regions(21);
        let land = brambleford_land();
        let open = Box::new(move |ground: u32, id: u32| {
            assert_eq!(ground, 0, "only the town fills up");
            Sim::region(land.clone(), 100 + id as u64, id, west())
        });
        Realm::layered(vec![a, b], Layering { cap: CAP, unlayered: if wild { vec![0] } else { Vec::new() }, open, reuse_after: server::regions::REUSE_AFTER })
    }

    #[test]
    fn players_past_the_cap_go_to_a_new_layer() {
        let mut realm = layered(false);
        let ids: Vec<u32> = (0..CAP * 2 + 1).map(|n| realm.spawn_player("Hero", Vec2::new(-20.0, n as f32)).unwrap()).collect();
        let layers = realm.layers(0);
        assert_eq!(layers.len(), 3);
        assert_eq!(layers.iter().map(|&l| realm.players_in(l)).collect::<Vec<_>>(), vec![CAP, CAP, 1]);
        assert_eq!(realm.layers(1), vec![1], "the empty east was layered too");
        // Each lives in its own layer, with IDs from that layer's block.
        assert_eq!(realm.region_of(ids[CAP]), layers[1]);
        assert_eq!(ids[CAP] / BLOCK, layers[1]);
        run(&mut realm, 0.5);
        assert_eq!(realm.stats().regions, 4);
        assert!(realm.snapshot(ids[CAP * 2]).player.is_some_and(|p| p.unit == ids[CAP * 2]));
        // Layers don't see each other.
        let seen = realm.snapshot(ids[0]).units;
        assert!(seen.iter().any(|u| u.id == ids[1]) && !seen.iter().any(|u| u.id == ids[CAP]), "a layer sees into another");
    }

    #[test]
    fn walking_into_a_full_town_opens_a_layer_and_layers_see_their_side_of_the_border() {
        let mut realm = layered(false);
        for n in 0..CAP {
            realm.spawn_player("Local", Vec2::new(-40.0, n as f32 * 2.0)).unwrap();
        }
        let east = realm.with(1, |sim| (0..2).map(|n| sim.spawn_player("Visitor", Vec2::new(BORDER + 2.0, 10.0 + n as f32), 0.0)).collect::<Vec<_>>());
        for &v in &east {
            walk(&mut realm, v, -Vec2::X);
        }
        run(&mut realm, 2.0);
        let town = realm.layers(0);
        assert_eq!(town.len(), 2, "the visitors didn't open a layer");
        assert!(east.iter().all(|&v| realm.region_of(v) == town[1]), "the visitors aren't in the new layer together");
        // The east, with one copy, pairs with both, so the visitors still see who's across it.
        let watcher = realm.with(1, |sim| sim.spawn_player("Watcher", Vec2::new(BORDER + 3.0, 12.0), 0.0));
        run(&mut realm, 0.1);
        assert!(realm.snapshot(east[0]).units.iter().any(|u| u.id == watcher), "the layer can't see across the border");
        assert!(realm.snapshot(watcher).units.iter().any(|u| u.id == east[0]), "the border can't see into the layer");
    }

    #[test]
    fn quiet_layers_fold_back_into_the_first() {
        let mut realm = layered(false);
        let ids: Vec<u32> = (0..CAP + 1).map(|n| realm.spawn_player("Hero", Vec2::new(-20.0, n as f32)).unwrap()).collect();
        let town = realm.layers(0);
        assert_eq!(town.len(), 2);
        realm.with_unit(ids[CAP], |sim, id| sim.get_mut::<Inventory>(id).unwrap().coins = 41);
        run(&mut realm, 1.5);
        assert_eq!(town.len(), 2, "full layers merged");
        // Three leave the first copy, so the two layers together hold half a cap.
        for &id in &ids[..3] {
            realm.with(town[0], move |sim| sim.export(id));
        }
        run(&mut realm, 1.5);
        assert_eq!(realm.layers(0), vec![0], "the quiet layer is still open");
        assert_eq!(realm.layers(1), vec![1]);
        let last = ids[CAP];
        assert_eq!(realm.region_of(last), 0);
        assert_eq!(realm.with_unit(last, |sim, id| sim.get::<Inventory>(id).unwrap().coins), 41, "its player lost something on the way");
        assert_eq!(realm.players_in(0), 2);
        assert_eq!(realm.stats().regions, 2);
        // It plays on in the first copy.
        walk(&mut realm, last, Vec2::Y);
        let y = realm.with_unit(last, |sim, id| sim.get::<sim::Transform>(id).unwrap().pos.z);
        run(&mut realm, 1.0);
        assert!(realm.with_unit(last, |sim, id| sim.get::<sim::Transform>(id).unwrap().pos.z) > y + 3.0);
    }

    /// A town layered at two players, with one living there, reusing closed layers' IDs after
    /// `reuse_after` ticks.
    fn crowded(reuse_after: u64) -> Realm {
        let (a, b) = two_regions(31);
        let land = brambleford_land();
        let open = Box::new(move |_: u32, id: u32| Sim::region(land.clone(), 100 + id as u64, id, west()));
        let mut realm = Realm::layered(vec![a, b], Layering { cap: 2, unlayered: Vec::new(), open, reuse_after });
        realm.spawn_player("Resident", Vec2::new(-20.0, 0.0)).unwrap();
        realm
    }

    /// Two players fill the town and open a layer, then leave, and the layer folds back. The
    /// layer's region ID.
    fn open_and_close(realm: &mut Realm) -> u32 {
        let visitor = realm.spawn_player("Visitor", Vec2::new(-20.0, 2.0)).unwrap();
        let id = realm.spawn_player("Passing", Vec2::new(-20.0, 4.0)).expect("a layer opens");
        let layer = realm.region_of(id);
        assert_ne!(layer, 0);
        realm.with(0, move |sim| sim.export(visitor));
        realm.with(layer, move |sim| sim.export(id));
        run(realm, 1.0);
        assert_eq!(realm.layers(0), vec![0], "the empty layer stayed open");
        layer
    }

    #[test]
    fn closed_layers_ids_come_back_into_use() {
        // More layers over a life than there are region IDs (#274).
        let mut realm = crowded(0);
        let ids: std::collections::BTreeSet<u32> = (0..sim::region::REGIONS + 20).map(|_| open_and_close(&mut realm)).collect();
        assert!(ids.len() < 4, "layers took {} IDs", ids.len());
    }

    #[test]
    fn a_closed_layers_id_waits_before_it_comes_back() {
        let mut realm = crowded(120);
        let first = open_and_close(&mut realm);
        // A second after: still too soon.
        assert_ne!(open_and_close(&mut realm), first);
        run(&mut realm, 2.0);
        assert_eq!(open_and_close(&mut realm), first);
    }

    #[test]
    fn a_full_wilderness_turns_players_back_at_its_border() {
        let mut realm = layered(true);
        for n in 0..CAP {
            realm.spawn_player("Inside", Vec2::new(-40.0, n as f32 * 2.0)).unwrap();
        }
        assert_eq!(realm.spawn_player("Late", Vec2::new(-40.0, 20.0)), None, "joined a full Wilderness");
        let outside = realm.with(1, |sim| sim.spawn_player("Outside", Vec2::new(BORDER + 2.0, 10.0), 0.0));
        walk(&mut realm, outside, -Vec2::X);
        let mut turned = 0;
        for _ in 0..3 * 60 {
            realm.step();
            turned += realm.stats().turned_back;
            assert_eq!(realm.region_of(outside), 1, "walked into a full Wilderness");
        }
        assert!(turned > 0);
        assert_eq!(realm.layers(0), vec![0], "the Wilderness was layered");
        let x = realm.with(1, move |sim| sim.get::<sim::Transform>(outside).unwrap().pos.x);
        assert!((BORDER - HANDOFF..BORDER + 1.0).contains(&x), "turned back to {x}");
        // Once someone leaves, it lets them in.
        let one = realm.with(0, |sim| sim.players()[0]);
        realm.with(0, move |sim| sim.export(one));
        run(&mut realm, 2.0);
        assert_eq!(realm.region_of(outside), 0);
    }
}

#[test]
fn a_served_realm_follows_its_players_across_a_border() {
    let (a, b) = two_regions(5);
    let server = server::Server::start_realm(Realm::new(vec![a, b]));
    let listener = server::net::listen(&server, "127.0.0.1:0").unwrap();
    // The spawn point is east of the border; both players walk west across it.
    let mut host = server::Host::join(server, "Walker");
    let mut remote = server::net::Client::connect(listener.addr(), listener.certificate(), "Runner").unwrap();
    let (first, _) = remote.next_view().unwrap();
    assert!(first.unit(host.player).is_some_and(|u| u.name == "Walker"), "players at the spawn see each other");
    let start = host.snapshot().unwrap();
    assert!(start.unit(host.player).unwrap().position.x > BORDER);
    let west = Command::Move { direction: -Vec2::X, walk: false, sprint: false, face_yaw: None };
    host.send(west.clone());
    remote.send(west).unwrap();
    let mut last = first;
    for _ in 0..60 {
        last = remote.next_view().unwrap().0;
    }
    let here = host.snapshot().unwrap();
    assert!(here.tick >= start.tick + 150, "the realm kept stepping: {} to {}", start.tick, here.tick);
    let walker = here.unit(host.player).expect("the walker still sees itself");
    assert!(walker.position.x < BORDER - HANDOFF, "the walker is at x {}", walker.position.x);
    assert!(host.combat().unwrap().units.iter().any(|u| u.id == host.player));
    let runner = last.unit(remote.player).expect("the runner still sees itself");
    assert!(runner.position.x < BORDER - HANDOFF, "the runner is at x {}", runner.position.x);
    assert!(last.unit(host.player).is_some(), "and still sees the walker beside it");
    host.stop();
}

#[test]
fn ghosts_bring_their_combat_detail_and_side_across() {
    let (mut a, mut b) = two_regions(6);
    let hero = a.spawn_hero("Hero", Specialty::Mage, 7, Vec2::new(BORDER - 1.0, 10.0), 0.0);
    let e = a.entity(hero).unwrap();
    let src = source(&a.world, e, None).unwrap();
    assert!(auras::apply_key(&mut a.world, "revive_sickness", e, &src));
    let skeleton = b.spawn_mob("skeleton", Vec2::new(BORDER + 1.0, 10.0)).unwrap();
    let se = b.entity(skeleton).unwrap();
    b.world.entity_mut(se).remove::<Brain>();
    let mut realm = Realm::new(vec![a, b]);
    run(&mut realm, 0.1);
    // From the skeleton's side of the border, the hero is a ghost with its aura.
    let seen = realm.combat_view(skeleton);
    let ghost = seen.units.iter().find(|u| u.id == hero).expect("the hero's ghost is in the combat view");
    assert!(ghost.auras.iter().any(|a| a.key == "revive_sickness"), "{:?}", ghost.auras);
    assert!(seen.units.windows(2).all(|w| w[0].id < w[1].id), "combat units stay in ID order");
    let hostile = |realm: &Realm| realm.snapshot(skeleton).units.iter().find(|u| u.id == hero).map(|u| u.hostile);
    assert_eq!(hostile(&realm), Some(false));
    // The hero turns on its own side (charmed, say): the ghost changes side with it.
    realm.with(0, move |sim| sim.get_mut::<sim::Unit>(hero).unwrap().faction = sim::Faction::Hostile);
    run(&mut realm, 0.1);
    assert_eq!(hostile(&realm), Some(true), "the ghost kept its old side");
}

#[test]
fn a_kill_across_the_border_pays_and_puts_the_hero_in_combat() {
    let (mut a, mut b) = two_regions(8);
    let hero = a.spawn_hero("Hero", Specialty::Warrior, 3, Vec2::new(BORDER - 1.0, 10.0), std::f32::consts::FRAC_PI_2);
    let skeleton = b.spawn_mob("skeleton", Vec2::new(BORDER + 1.0, 10.0)).unwrap();
    // One good blow fells it.
    b.get_mut::<Vitals>(skeleton).unwrap().health = 1.0;
    let mut realm = Realm::new(vec![a, b]);
    run(&mut realm, 0.1);
    let xp = |realm: &Realm| realm.with(0, move |sim| sim.get::<Progress>(hero).unwrap().xp[Specialty::Warrior.index()]);
    let before = xp(&realm);
    let fought = |realm: &Realm| realm.with(0, move |sim| sim.get::<Vitals>(hero).unwrap().last_combat);
    let calm = fought(&realm);

    realm.command(hero, Command::SetTarget(Some(skeleton)));
    realm.command(hero, Command::UseSlot { slot: 0, ground: None });
    run(&mut realm, 1.0);
    assert!(realm.with(1, move |sim| sim.get::<Vitals>(skeleton).is_none_or(|v| v.dead)), "the skeleton didn't die");
    assert!(xp(&realm) > before, "no XP for a kill across the border");
    assert!(fought(&realm) > calm, "the hero's own region didn't see it fight");
}

#[test]
fn a_region_with_nobody_near_sleeps_and_wakes_when_someone_comes() {
    let (mut a, b) = two_regions(10);
    let hero = a.spawn_player("Hero", Vec2::new(BORDER - 80.0, 10.0), 0.0);
    let mut realm = Realm::new(vec![a, b]);
    // Not on a whole second, when a sleeping region takes its one step.
    run(&mut realm, 1.25);
    assert_eq!(realm.stats().asleep, 1, "the empty east should be asleep");
    assert_eq!(realm.with(1, |sim| sim.tick()), realm.tick(), "a sleeping region keeps time");
    walk(&mut realm, hero, Vec2::X);
    run(&mut realm, 14.0);
    assert_eq!(realm.region_of(hero), 1, "the hero crossed");
    assert_eq!(realm.stats().asleep, 0, "the east woke when the hero came");
}
