//! Specialty XP and levels: fight XP split by use, quest XP to assign, the cap, and switching (#66).

use glam::Vec2;
use protocol::{Command, Event};
use sim::content::progression::{kill_xp, to_next, MAX_LEVEL};
use sim::content::Rank;
use sim::{Brain, Credit, Progress, Sim, Specialty, Stats, Transform, Unit, Vitals};

fn wolf(sim: &Sim) -> u32 {
    let mut q = sim.world.try_query::<(&Unit, &Brain)>().unwrap();
    q.iter(&sim.world).filter(|(u, _)| u.body == "wolf").map(|(u, _)| u.id).min().unwrap()
}

/// Fights `target` until it dies (or `seconds` pass), collecting the events. Returns whether it died.
fn fight(sim: &mut Sim, hero: u32, target: u32, seconds: f32, events: &mut Vec<Event>) -> bool {
    sim.command(hero, Command::SetTarget(Some(target)));
    sim.command(hero, Command::SetAutoAttack(true));
    for _ in 0..(seconds * 60.0) as u32 {
        let (h, w) = (sim.get::<Transform>(hero).unwrap().pos, sim.get::<Transform>(target).unwrap().pos);
        let to = Vec2::new(w.x - h.x, w.z - h.z);
        let dir = if to.length() > 1.8 { to.normalize() } else { Vec2::ZERO };
        sim.command(hero, Command::Move { direction: dir, walk: false, sprint: false, face_yaw: None });
        sim.command(hero, Command::UseSlot { slot: 0, ground: None });
        sim.step();
        events.extend(sim.snapshot(Some(hero)).events);
        if sim.get::<Vitals>(target).unwrap().dead {
            return true;
        }
    }
    false
}

fn progress(sim: &Sim, hero: u32) -> Progress {
    sim.get::<Progress>(hero).unwrap().clone()
}

#[test]
fn killing_a_monster_pays_the_specialty_you_fought_with() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(1.0);
    let w = wolf(&sim);
    let level = sim.get::<Unit>(w).unwrap().level;
    let mut events = Vec::new();
    assert!(fight(&mut sim, hero, w, 20.0, &mut events), "the wolf survived");
    let p = progress(&sim, hero);
    assert_eq!(p.xp[Specialty::Warrior.index()], kill_xp(level, Rank::Normal, 1));
    for s in Specialty::ALL.into_iter().filter(|&s| s != Specialty::Warrior) {
        assert_eq!(p.xp[s.index()], 0, "{s:?} earned from a fight it wasn't in");
    }
}

#[test]
fn fight_xp_splits_between_the_specialties_used_by_how_often() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(1.0);
    let w = wolf(&sim);
    let level = sim.get::<Unit>(w).unwrap().level;
    // Three skills used on it as a mage earlier, and the warrior's blow that finishes it.
    sim.get_mut::<Credit>(w).unwrap().0.push((hero, Specialty::Mage, 3));
    sim.get_mut::<Vitals>(w).unwrap().health = 1.0;
    let mut events = Vec::new();
    assert!(fight(&mut sim, hero, w, 5.0, &mut events));
    let p = progress(&sim, hero);
    let full = kill_xp(level, Rank::Normal, 1);
    assert_eq!(p.xp[Specialty::Mage.index()], full * 3 / 4);
    assert_eq!(p.xp[Specialty::Warrior.index()], full / 4);
}

#[test]
fn a_monster_that_gets_away_pays_nothing() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(1.0);
    let w = wolf(&sim);
    let mut events = Vec::new();
    assert!(!fight(&mut sim, hero, w, 1.0, &mut events));
    assert!(!sim.get::<Credit>(w).unwrap().0.is_empty(), "the blows weren't counted");
    sim.command(hero, Command::Move { direction: Vec2::new(1.0, 0.0), walk: false, sprint: false, face_yaw: None });
    sim.run_for(15.0);
    assert_eq!(progress(&sim, hero).xp, [0; 6]);
}

#[test]
fn much_weaker_monsters_pay_less_then_nothing() {
    assert_eq!(kill_xp(10, Rank::Normal, 15), kill_xp(10, Rank::Normal, 10));
    assert!(kill_xp(10, Rank::Normal, 17) < kill_xp(10, Rank::Normal, 15));
    assert_eq!(kill_xp(10, Rank::Normal, 20), 0);
    assert!(kill_xp(10, Rank::Boss, 10) > kill_xp(10, Rank::Elite, 10));
    assert!(kill_xp(10, Rank::Elite, 10) > kill_xp(10, Rank::Normal, 10));
}

#[test]
fn quest_xp_goes_to_whichever_specialty_the_player_picks() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(0.0, 28.0), 0.0);
    sim.grant_quest_xp(hero, 1000);
    sim.command(hero, Command::AssignXp { specialty: "healer".into(), amount: 400 });
    // More than is left, and a specialty that doesn't exist, are refused.
    sim.command(hero, Command::AssignXp { specialty: "rogue".into(), amount: 700 });
    sim.command(hero, Command::AssignXp { specialty: "bard".into(), amount: 100 });
    sim.step();
    let p = progress(&sim, hero);
    assert_eq!(p.unassigned, 600);
    assert_eq!(p.level(Specialty::Healer), 2, "400 XP takes level 1 to 2");
    assert_eq!(p.xp[Specialty::Healer.index()], 400 - to_next(1).unwrap());
    assert_eq!(p.xp[Specialty::Rogue.index()], 0);
    // The warrior is the one fighting, and it didn't change.
    assert_eq!(sim.get::<Unit>(hero).unwrap().level, 1);
}

#[test]
fn specialties_stop_at_level_50() {
    let mut p = Progress::new(Specialty::Archer, 1);
    let ups = p.add(Specialty::Archer, u32::MAX);
    assert_eq!(ups.len(), MAX_LEVEL as usize - 1);
    assert_eq!(p.level(Specialty::Archer), MAX_LEVEL);
    assert_eq!(p.xp[Specialty::Archer.index()], 0);
    assert!(p.add(Specialty::Archer, 1000).is_empty());
    assert_eq!(p.level(Specialty::Archer), MAX_LEVEL);
    assert_eq!(to_next(MAX_LEVEL), None);
}

#[test]
fn levelling_the_equipped_specialty_makes_you_stronger_at_once() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(0.0, 28.0), 0.0);
    let (power, health) = (sim.get::<Stats>(hero).unwrap().power, sim.get::<Vitals>(hero).unwrap().max_health);
    sim.grant_quest_xp(hero, to_next(1).unwrap() + to_next(2).unwrap());
    sim.command(hero, Command::AssignXp { specialty: "warrior".into(), amount: to_next(1).unwrap() + to_next(2).unwrap() });
    sim.step();
    let events = sim.snapshot(Some(hero)).events;
    let ups: Vec<u8> = events.iter().filter_map(|e| match e {
        Event::LevelUp { unit, specialty, level } if *unit == hero && specialty == "warrior" => Some(*level),
        _ => None,
    }).collect();
    assert_eq!(ups, [2, 3]);
    assert_eq!(sim.get::<Unit>(hero).unwrap().level, 3);
    assert!(sim.get::<Stats>(hero).unwrap().power > power * 1.1);
    assert!(sim.get::<Vitals>(hero).unwrap().max_health > health * 1.1);
    let me = sim.snapshot(Some(hero)).player.unwrap();
    assert_eq!((me.specialty.as_str(), me.level, me.xp, me.xp_next), ("warrior", 3, 0, to_next(3).unwrap()));
}

#[test]
fn switching_specialty_changes_the_level_that_fights() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_hero("Hero", Specialty::Warrior, 10, Vec2::new(0.0, 28.0), 0.0);
    let warrior = (sim.get::<Stats>(hero).unwrap().power, sim.get::<Vitals>(hero).unwrap().max_health);
    assert!(sim.switch_specialty(hero, Specialty::Mage));
    assert_eq!(sim.get::<Unit>(hero).unwrap().level, 1);
    let mage = (sim.get::<Stats>(hero).unwrap().power, sim.get::<Vitals>(hero).unwrap().max_health);
    assert!(mage.0 < warrior.0 && mage.1 < warrior.1, "a level-1 mage is weaker than a level-10 warrior");
    let me = sim.snapshot(Some(hero)).player.unwrap();
    assert_eq!((me.specialty.as_str(), me.level), ("mage", 1));
    assert_eq!(me.resource_kind, protocol::ResourceKind::Mana);
    // Back again, and the warrior is still level 10.
    assert!(sim.switch_specialty(hero, Specialty::Warrior));
    assert_eq!(sim.get::<Unit>(hero).unwrap().level, 10);
    assert_eq!(sim.get::<Stats>(hero).unwrap().power, warrior.0);
}

#[test]
fn you_cant_switch_specialty_in_a_fight() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(1.0);
    let w = wolf(&sim);
    let mut events = Vec::new();
    fight(&mut sim, hero, w, 1.0, &mut events);
    assert!(!sim.switch_specialty(hero, Specialty::Rogue));
    assert_eq!(progress(&sim, hero).level(Specialty::Warrior), 1);
}
