//! Eating food and drinking potions from the pack (#205).

use content::items::{economy, POTION_COOLDOWN};
use glam::Vec2;
use protocol::{Command, Event, PlayerState};
use sim::{Inventory, Sim, Vitals};

/// A player on the north road carrying `items`, with half their health gone.
fn hungry(items: &[(&str, u16)]) -> (Sim, u32) {
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), 0.0);
    let mut inv = sim.get_mut::<Inventory>(id).unwrap();
    for (key, n) in items {
        assert_eq!(inv.add(economy().item(key).unwrap(), *n, false), 0);
    }
    let mut v = sim.get_mut::<Vitals>(id).unwrap();
    v.health = v.max_health * 0.5;
    sim.step();
    (sim, id)
}

fn count(sim: &Sim, id: u32, item: &str) -> u32 {
    sim.get::<Inventory>(id).unwrap().count(item)
}

fn me(sim: &mut Sim, id: u32) -> PlayerState {
    sim.snapshot(Some(id)).player.unwrap()
}

fn has_aura(sim: &mut Sim, id: u32, key: &str) -> bool {
    me(sim, id).auras.iter().any(|a| a.key == key)
}

/// Uses slot 0, and returns the messages it got.
fn use_first(sim: &mut Sim, id: u32) -> Vec<String> {
    sim.take_events();
    sim.command(id, Command::UseItem { slot: 0 });
    sim.step();
    sim.take_events().into_iter().filter_map(|e| if let Event::Message { text } = e { Some(text) } else { None }).collect()
}

/// Keeps the player in combat (so health doesn't come back on its own) for `seconds`, and
/// returns the health they gained.
fn health_gained_in_combat(sim: &mut Sim, id: u32, seconds: f32) -> f32 {
    let before = sim.get::<Vitals>(id).unwrap().health;
    for _ in 0..(seconds * 60.0) as u32 {
        let t = sim.time();
        sim.get_mut::<Vitals>(id).unwrap().last_combat = t;
        sim.step();
    }
    sim.get::<Vitals>(id).unwrap().health - before
}

#[test]
fn eating_takes_one_and_the_buff_lasts_into_a_fight() {
    let (mut sim, id) = hungry(&[("bread", 3)]);
    assert!(use_first(&mut sim, id).is_empty());
    assert_eq!(count(&sim, id, "bread"), 2);
    assert!(has_aura(&mut sim, id, "fed"));
    let fed = health_gained_in_combat(&mut sim, id, 9.0);

    let (mut sim, id) = hungry(&[]);
    let unfed = health_gained_in_combat(&mut sim, id, 9.0);
    assert!(fed > unfed + 1.0, "fed {fed}, unfed {unfed}");
}

#[test]
fn the_last_one_empties_the_slot() {
    let (mut sim, id) = hungry(&[("grilled_trout", 1)]);
    use_first(&mut sim, id);
    assert!(sim.get::<Inventory>(id).unwrap().slots[0].is_none());
    assert!(has_aura(&mut sim, id, "well_fed"));
}

#[test]
fn food_cannot_be_eaten_while_fighting() {
    let (mut sim, id) = hungry(&[("bread", 3)]);
    let t = sim.time();
    sim.get_mut::<Vitals>(id).unwrap().last_combat = t;
    let said = use_first(&mut sim, id);
    assert_eq!(count(&sim, id, "bread"), 3, "nothing eaten");
    assert!(!has_aura(&mut sim, id, "fed"));
    assert!(said.iter().any(|m| m.contains("while fighting")), "{said:?}");
}

#[test]
fn a_potion_heals_over_time_even_in_a_fight() {
    let (mut sim, id) = hungry(&[("healing_draught", 1)]);
    let t = sim.time();
    sim.get_mut::<Vitals>(id).unwrap().last_combat = t;
    assert!(use_first(&mut sim, id).is_empty());
    assert_eq!(count(&sim, id, "healing_draught"), 0);
    let healed = health_gained_in_combat(&mut sim, id, 9.0);
    assert!(healed > 20.0, "healed {healed}");
}

#[test]
fn potions_share_a_cooldown() {
    let (mut sim, id) = hungry(&[("healing_draught", 3)]);
    use_first(&mut sim, id);
    assert_eq!(count(&sim, id, "healing_draught"), 2);
    let wait = me(&mut sim, id).potion_ready;
    assert!(wait > POTION_COOLDOWN as f32 - 1.0 && wait <= POTION_COOLDOWN as f32, "{wait}");

    let said = use_first(&mut sim, id);
    assert_eq!(count(&sim, id, "healing_draught"), 2, "the second waits");
    assert!(said.iter().any(|m| m.contains("another potion")), "{said:?}");

    sim.run_for(POTION_COOLDOWN as f32);
    assert_eq!(me(&mut sim, id).potion_ready, 0.0);
    use_first(&mut sim, id);
    assert_eq!(count(&sim, id, "healing_draught"), 1);
}

#[test]
fn only_consumables_can_be_used() {
    let (mut sim, id) = hungry(&[("copper_ore", 5)]);
    let said = use_first(&mut sim, id);
    assert_eq!(count(&sim, id, "copper_ore"), 5);
    assert!(said.iter().any(|m| m.contains("can't use")), "{said:?}");
    // An empty slot does nothing.
    sim.command(id, Command::UseItem { slot: 20 });
    sim.step();
}

#[test]
fn the_dead_eat_nothing() {
    let (mut sim, id) = hungry(&[("bread", 2)]);
    sim.get_mut::<Vitals>(id).unwrap().dead = true;
    use_first(&mut sim, id);
    assert_eq!(count(&sim, id, "bread"), 2);
}

#[test]
fn the_potion_cooldown_crosses_a_region_border() {
    let (mut west, id) = hungry(&[("healing_draught", 2)]);
    use_first(&mut west, id);
    let ready = west.get::<sim::PotionCooldown>(id).unwrap().0;
    let mut east = Sim::new(7);
    east.import(vec![west.export(id).unwrap()]);
    assert_eq!(east.get::<sim::PotionCooldown>(id).map(|c| c.0), Some(ready));
}
