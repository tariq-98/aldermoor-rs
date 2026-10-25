//! Loot, inventories and shops (#71).

use content::items::{economy, SELL_SHARE};
use glam::{Vec2, Vec3};
use protocol::Command;
use sim::{roll, Brain, Credit, Inventory, Loot, Rng, Sim, Specialty, Transform, Unit, Vitals, PILE_LASTS, SLOTS};

fn wolf(sim: &Sim) -> u32 {
    let mut q = sim.world.try_query::<(&Unit, &Brain)>().unwrap();
    q.iter(&sim.world).filter(|(u, _)| u.body == "wolf").map(|(u, _)| u.id).min().unwrap()
}

/// Fights the first wolf until it dies. Returns it.
fn kill_a_wolf(sim: &mut Sim, hero: u32) -> u32 {
    let w = wolf(sim);
    sim.command(hero, Command::SetTarget(Some(w)));
    sim.command(hero, Command::SetAutoAttack(true));
    for _ in 0..(20 * 60) {
        let (h, p) = (sim.get::<Transform>(hero).unwrap().pos, sim.get::<Transform>(w).unwrap().pos);
        let to = Vec2::new(p.x - h.x, p.z - h.z);
        let dir = if to.length() > 1.8 { to.normalize() } else { Vec2::ZERO };
        sim.command(hero, Command::Move { direction: dir, walk: false, sprint: false, face_yaw: None });
        sim.command(hero, Command::UseSlot { slot: 0, ground: None });
        sim.step();
        if sim.get::<Vitals>(w).unwrap().dead {
            return w;
        }
    }
    panic!("the wolf survived");
}

fn inv(sim: &Sim, id: u32) -> Inventory {
    sim.get::<Inventory>(id).unwrap().clone()
}

#[test]
fn drops_roll_the_same_from_the_same_seed() {
    let golem = economy().loot_for("golem").unwrap();
    let rolls = |seed| {
        let mut rng = Rng::new(seed);
        (0..200).map(|_| roll(&mut rng, golem, Vec3::ZERO, None, 0.0)).map(|p| (p.coins, p.items)).collect::<Vec<_>>()
    };
    assert_eq!(rolls(7), rolls(7));
    assert_ne!(rolls(7), rolls(8));
}

#[test]
fn rare_drops_are_rare_and_some_gear_comes_out_fine() {
    let golem = economy().loot_for("golem").unwrap();
    let mut rng = Rng::new(42);
    let (mut cores, mut maces, mut fine) = (0, 0, 0);
    for _ in 0..4000 {
        let p = roll(&mut rng, golem, Vec3::ZERO, None, 0.0);
        assert!((20..=40).contains(&p.coins));
        for s in &p.items {
            match s.item {
                "golem_core" => {
                    cores += 1;
                    assert!(!s.fine, "materials are never fine");
                }
                "mace.flanged" => {
                    maces += 1;
                    fine += s.fine as u32;
                }
                _ => {}
            }
        }
    }
    assert_eq!(cores, 4000, "a sure drop always drops");
    assert!((500..700).contains(&maces), "15% mace: {maces} of 4000");
    let share = fine as f32 / maces as f32;
    assert!((0.10..0.20).contains(&share), "15% of maces fine: {share}");
}

#[test]
fn a_kill_leaves_a_pile_for_whoever_earned_it() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    let other = sim.spawn_player("Other", Vec2::new(-40.0, 21.0), 0.0);
    sim.run_for(1.0);
    let w = kill_a_wolf(&mut sim, hero);
    let at = sim.get::<Transform>(w).unwrap().pos;
    let pile = sim.world.resource::<Loot>().piles.iter().find(|p| (p.at - at).length() < 0.1 && p.owner == Some(hero)).cloned();
    let pile = pile.expect("this wolf leaves the hero something");
    let me = sim.snapshot(Some(hero)).player.unwrap();
    assert!(me.loot.iter().any(|l| l.id == pile.id));
    // Someone else can't see it or take it, even standing on it.
    sim.get_mut::<Transform>(other).unwrap().pos = at;
    sim.step();
    assert!(sim.snapshot(Some(other)).player.unwrap().loot.iter().all(|l| l.id != pile.id));
    sim.command(other, Command::Loot { pile: pile.id });
    sim.step();
    assert_eq!(inv(&sim, other), Inventory::default());
    // The one who earned it can, from beside it.
    sim.get_mut::<Transform>(hero).unwrap().pos = at + Vec3::new(1.0, 0.0, 0.0);
    sim.command(hero, Command::Loot { pile: pile.id });
    sim.step();
    let mine = inv(&sim, hero);
    assert_eq!(mine.coins, pile.coins);
    for s in &pile.items {
        assert_eq!(mine.count(s.item), s.count as u32);
    }
    assert!(sim.world.resource::<Loot>().piles.iter().all(|p| p.id != pile.id), "the pile is gone");
}

#[test]
fn a_pile_stays_its_owners_alone_then_rots() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    let other = sim.spawn_player("Other", Vec2::new(30.0, 40.0), 0.0);
    let golem = economy().loot_for("golem").unwrap();
    let at = sim.get::<Transform>(other).unwrap().pos;
    let mut rng = Rng::new(1);
    let mut pile = roll(&mut rng, golem, at, Some(hero), sim.time());
    pile.id = 99;
    sim.world.resource_mut::<Loot>().piles.push(pile.clone());
    sim.run_for(90.0);
    sim.get_mut::<Transform>(other).unwrap().pos = at;
    sim.command(other, Command::Loot { pile: 99 });
    sim.step();
    assert_eq!(inv(&sim, other).count("golem_core"), 0, "someone else took a personal pile");
    // A pile nobody takes is gone in the end.
    sim.run_for((PILE_LASTS - 90.0) as f32 + 1.0);
    assert!(sim.world.resource::<Loot>().piles.iter().all(|p| p.id != 99));
}

#[test]
fn everyone_with_a_fair_share_gets_their_own_roll_and_a_tap_gets_nothing() {
    let mut sim = Sim::brambleford(3);
    let a = sim.spawn_player("A", Vec2::new(-38.0, 21.0), 0.0);
    let b = sim.spawn_player("B", Vec2::new(-37.0, 21.0), 0.0);
    let tap = sim.spawn_player("Tap", Vec2::new(-36.0, 21.0), 0.0);
    let w = wolf(&sim);
    let wolf_at = sim.get::<Transform>(w).unwrap().pos;
    {
        let mut c = sim.get_mut::<Credit>(w).unwrap();
        c.0.push((a, Specialty::Warrior, 60.0));
        c.0.push((b, Specialty::Warrior, 35.0));
        c.0.push((tap, Specialty::Warrior, 5.0));
    }
    let e = sim.entity(w).unwrap();
    sim::combat::life::die(&mut sim.world, e, None);
    let piles = sim.world.resource::<Loot>().piles.clone();
    let mine = |id| piles.iter().filter(|p| p.owner == Some(id) && (p.at - wolf_at).length() < 0.1).count();
    assert_eq!((mine(a), mine(b), mine(tap)), (1, 1, 0));
}

#[test]
fn materials_stack_and_gear_takes_a_slot_each() {
    let eco = economy();
    let mut bag = Inventory::default();
    assert_eq!(bag.add(eco.item("copper_ore").unwrap(), 120, false), 0);
    assert_eq!(bag.slots.iter().flatten().count(), 2, "100 to a stack");
    assert_eq!(bag.add(eco.item("copper_ore").unwrap(), 80, false), 0);
    assert_eq!(bag.slots.iter().flatten().count(), 2, "topped up the second stack");
    let sword = eco.item("sword.arming").unwrap();
    assert_eq!(bag.add(sword, 3, false), 0);
    assert_eq!(bag.slots.iter().flatten().count(), 5);
    // Fine and plain don't share a stack, and only gear can be fine.
    assert_eq!(bag.add(eco.item("wolf_pelt").unwrap(), 1, true), 0);
    assert!(bag.slots.iter().flatten().any(|s| s.item == "wolf_pelt" && !s.fine));
    // Fill it up: what doesn't fit comes back.
    let left = bag.add(sword, SLOTS as u16, false);
    assert_eq!(left, 6);
    assert!(!bag.fits(eco.item("bread").unwrap(), 1, false));
}

/// A hero at a shop's counter with `coins`.
fn at_shop(shop: &str, coins: u32) -> (Sim, u32) {
    let mut sim = Sim::brambleford(3);
    let s = economy().shop(shop).unwrap();
    let hero = sim.spawn_player("Hero", Vec2::new(s.at.0 + 1.0, s.at.1), 0.0);
    sim.get_mut::<Inventory>(hero).unwrap().coins = coins;
    (sim, hero)
}

#[test]
fn shops_sell_at_their_prices_to_those_who_can_pay_and_carry() {
    let (mut sim, hero) = at_shop("brambleford_store", 20);
    let bread = economy().item("bread").unwrap();
    let me = sim.snapshot(Some(hero)).player.unwrap();
    let shop = me.shop.expect("standing at the counter");
    assert!(shop.sells.iter().any(|i| i.item == "bread" && i.price == bread.value));
    sim.command(hero, Command::Buy { shop: "brambleford_store".into(), item: "bread".into(), count: 3 });
    sim.step();
    assert_eq!(inv(&sim, hero).count("bread"), 3);
    assert_eq!(inv(&sim, hero).coins, 20 - 3 * bread.value);
    // Too dear, not on the shelf, or from the wrong shop: nothing changes.
    let before = inv(&sim, hero);
    sim.command(hero, Command::Buy { shop: "brambleford_store".into(), item: "leather_jerkin".into(), count: 1 });
    sim.command(hero, Command::Buy { shop: "brambleford_store".into(), item: "golem_core".into(), count: 1 });
    sim.command(hero, Command::Buy { shop: "brambleford_smith".into(), item: "copper_ore".into(), count: 1 });
    sim.step();
    assert_eq!(inv(&sim, hero), before);
}

#[test]
fn you_have_to_stand_at_the_counter() {
    let (mut sim, hero) = at_shop("brambleford_store", 100);
    sim.get_mut::<Transform>(hero).unwrap().pos.x += 20.0;
    sim.command(hero, Command::Buy { shop: "brambleford_store".into(), item: "bread".into(), count: 1 });
    sim.step();
    assert_eq!(inv(&sim, hero).count("bread"), 0);
    assert!(sim.snapshot(Some(hero)).player.unwrap().shop.is_none());
}

#[test]
fn shops_pay_what_their_town_pays_and_more_for_fine_gear() {
    let eco = economy();
    let (mut sim, hero) = at_shop("brambleford_store", 0);
    {
        let mut bag = sim.get_mut::<Inventory>(hero).unwrap();
        bag.add(eco.item("wolf_pelt").unwrap(), 10, false);
        bag.add(eco.item("dagger.hunting").unwrap(), 1, false);
        bag.add(eco.item("dagger.hunting").unwrap(), 1, true);
    }
    let pelt = eco.item("wolf_pelt").unwrap();
    let each = (pelt.value as f32 * SELL_SHARE * 1.5).floor() as u32;
    let me = sim.snapshot(Some(hero)).player.unwrap();
    assert_eq!(me.inventory[0].price, each, "Brambleford pays half again for hides");
    sim.command(hero, Command::Sell { shop: "brambleford_store".into(), slot: 0, count: 4 });
    sim.step();
    assert_eq!(inv(&sim, hero).coins, 4 * each);
    assert_eq!(inv(&sim, hero).count("wolf_pelt"), 6);
    let (plain, fine) = (me.inventory[1].price, me.inventory[2].price);
    assert!(me.inventory[2].fine && fine > plain * 2, "fine {fine} vs plain {plain}");
}

#[test]
fn the_smith_only_buys_metal() {
    let eco = economy();
    let (mut sim, hero) = at_shop("brambleford_smith", 0);
    sim.get_mut::<Inventory>(hero).unwrap().add(eco.item("wolf_pelt").unwrap(), 1, false);
    sim.command(hero, Command::Sell { shop: "brambleford_smith".into(), slot: 0, count: 1 });
    sim.step();
    assert_eq!(inv(&sim, hero).count("wolf_pelt"), 1);
    assert_eq!(inv(&sim, hero).coins, 0);
}
