//! The fight: resources, casts, cooldowns, interrupts, crowd control and defiance, missiles,
//! warned blows, guards and parries, blows from behind, being downed, help, summons, combos,
//! and what all of it looks like from outside.

use std::f32::consts::PI;

use glam::{Vec2, Vec3};
use protocol::{Avoid, Command, Event, ResourceKind, Shape};
use sim::combat::{abilities, auras, life, source};
use sim::content::catalog;
use sim::{Auras, Brain, Faction, Fighter, Motion, Outbox, Sim, Specialty, Stats, Transform, Unit, Vitals};

/// The north road: open ground, clear of walls and trees, and far from the valley's monsters.
const ROAD: Vec2 = Vec2::new(5.0, 10.0);

struct Fight {
    sim: Sim,
    log: Vec<Event>,
}

impl Fight {
    fn new() -> Fight {
        Fight { sim: Sim::new(7), log: Vec::new() }
    }

    fn step(&mut self) {
        self.sim.step();
        let events = std::mem::take(&mut self.sim.world.resource_mut::<Outbox>().0);
        self.log.extend(events);
    }

    fn run(&mut self, seconds: f32) {
        for _ in 0..(seconds * 60.0).round() as u32 {
            self.step();
        }
    }

    /// Steps until `done` holds (at most `seconds`). Returns whether it did.
    fn until(&mut self, seconds: f32, done: impl Fn(&Fight) -> bool) -> bool {
        for _ in 0..(seconds * 60.0).round() as u32 {
            if done(self) {
                return true;
            }
            self.step();
        }
        done(self)
    }

    /// A player who never lands a lucky crit, so numbers can be compared.
    fn hero(&mut self, s: Specialty, level: u8, at: Vec2, yaw: f32) -> u32 {
        let id = self.sim.spawn_hero("Hero", s, level, at, yaw);
        self.sim.get_mut::<Stats>(id).unwrap().crit = 0.0;
        id
    }

    /// A player turned against the others, for duels.
    fn foe(&mut self, s: Specialty, level: u8, at: Vec2, yaw: f32) -> u32 {
        let id = self.hero(s, level, at, yaw);
        self.sim.get_mut::<Unit>(id).unwrap().faction = Faction::Hostile;
        id
    }

    /// A monster with no mind of its own: it stands where it's put and takes what comes.
    fn dummy(&mut self, key: &str, at: Vec2, yaw: f32) -> u32 {
        let id = self.sim.spawn_mob(key, at).unwrap();
        let e = self.sim.entity(id).unwrap();
        self.sim.world.entity_mut(e).remove::<Brain>();
        self.sim.get_mut::<Transform>(id).unwrap().yaw = yaw;
        self.sim.get_mut::<Stats>(id).unwrap().crit = 0.0;
        id
    }

    fn cmd(&mut self, id: u32, c: Command) {
        self.sim.command(id, c);
    }

    fn target(&mut self, id: u32, t: u32) {
        self.cmd(id, Command::SetTarget(Some(t)));
    }

    fn press(&mut self, id: u32, slot: u8) {
        self.cmd(id, Command::UseSlot { slot, ground: None });
    }

    fn vitals(&self, id: u32) -> Vitals {
        *self.sim.get::<Vitals>(id).unwrap()
    }

    fn fighter(&self, id: u32) -> &Fighter {
        self.sim.get::<Fighter>(id).unwrap()
    }

    fn has(&self, id: u32, key: &str) -> bool {
        let aura = catalog().aura_id(key).unwrap();
        self.sim.get::<Auras>(id).unwrap().list.iter().any(|a| a.aura == aura)
    }

    fn remaining(&self, id: u32, key: &str) -> f64 {
        let aura = catalog().aura_id(key).unwrap();
        let t = self.sim.time();
        self.sim.get::<Auras>(id).unwrap().list.iter().find(|a| a.aura == aura).map_or(0.0, |a| a.expires - t)
    }

    /// Blows (not conditions ticking) that landed on `id`, in order.
    fn blows_on(&self, id: u32) -> Vec<f32> {
        self.log
            .iter()
            .filter_map(|e| match e {
                Event::Damage { target, amount, periodic: false, .. } if *target == id => Some(*amount),
                _ => None,
            })
            .collect()
    }

    fn damage_to(&self, id: u32) -> f32 {
        self.log
            .iter()
            .map(|e| match e {
                Event::Damage { target, amount, .. } if *target == id => *amount,
                _ => 0.0,
            })
            .sum()
    }

    fn avoided(&self, id: u32, how: Avoid) -> usize {
        self.log.iter().filter(|e| matches!(e, Event::Avoided { target, how: h, .. } if *target == id && *h == how)).count()
    }

    fn used(&self, id: u32, key: &str) -> usize {
        self.log.iter().filter(|e| matches!(e, Event::AbilityUsed { unit, ability, .. } if *unit == id && ability == key)).count()
    }

    /// Makes `id` use `key` right now, as a monster's mind would.
    fn force(&mut self, id: u32, key: &str, target: Option<u32>) -> bool {
        let e = self.sim.entity(id).unwrap();
        let ability = catalog().ability_id(key).unwrap();
        abilities::use_ability(&mut self.sim.world, e, ability, target, None, true)
    }

    /// Fells a monster for good (it won't come back mid-test).
    fn kill(&mut self, id: u32) {
        let e = self.sim.entity(id).unwrap();
        life::die(&mut self.sim.world, e, None);
        self.sim.get_mut::<Vitals>(id).unwrap().respawn_at = None;
    }

    /// Uses a skill once, without the basic attack carrying on after it.
    fn once(&mut self, id: u32, slot: u8) {
        self.press(id, slot);
        self.cmd(id, Command::SetAutoAttack(false));
    }

    fn apply(&mut self, key: &str, on: u32, from: u32) -> bool {
        let (te, se) = (self.sim.entity(on).unwrap(), self.sim.entity(from).unwrap());
        let src = source(&self.sim.world, se, None).unwrap();
        auras::apply_key(&mut self.sim.world, key, te, &src)
    }
}

#[test]
fn the_content_adds_up() {
    let problems = catalog().problems();
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_specialty_starts_with_a_full_bar() {
    let mut f = Fight::new();
    for s in Specialty::ALL {
        let id = f.hero(s, 1, ROAD, 0.0);
        let bar = f.fighter(id).bar;
        assert!(bar.iter().all(|slot| slot.is_some()), "{s:?} has gaps: {bar:?}");
        let p = f.sim.snapshot(Some(id)).player.unwrap();
        assert_eq!(p.bar.len(), 10);
        assert!(p.bar[0].auto, "{s:?}'s first skill isn't its basic attack");
        assert_ne!(p.resource_kind, ResourceKind::None);
    }
}

#[test]
fn every_skill_on_every_bar_goes_off() {
    let mut missing = Vec::new();
    for s in Specialty::ALL {
        for set in 0..2 {
            let mut f = Fight::new();
            let hero = f.hero(s, 10, ROAD, 0.0);
            let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 3.0), PI);
            if set == 1 {
                f.run(0.1);
                f.cmd(hero, Command::SwapWeapons);
                f.step();
            }
            f.target(hero, golem);
            let bar = f.fighter(hero).bar;
            for slot in 0..10u8 {
                // Plenty to spend, and time for casts and channels to play out.
                f.sim.get_mut::<Fighter>(hero).unwrap().resource = 100.0;
                f.once(hero, slot);
                f.run(3.2);
                f.cmd(hero, Command::Move { direction: Vec2::ZERO, walk: false, face_yaw: Some(0.0) });
                let back = f.sim.get::<Transform>(hero).unwrap().pos;
                if (Vec2::new(back.x, back.z) - ROAD).length() > 0.5 {
                    // Leaps and blinks: back to the start.
                    let y = f.sim.land().ground(ROAD.x, ROAD.y);
                    f.sim.get_mut::<Transform>(hero).unwrap().pos = glam::Vec3::new(ROAD.x, y, ROAD.y);
                }
                f.sim.get_mut::<Vitals>(golem).unwrap().health = 1.0e6;
            }
            for id in bar.into_iter().flatten() {
                let key = catalog().ability(id).key;
                if f.used(hero, key) == 0 {
                    missing.push(format!("{s:?} set {set}: {key}"));
                }
            }
        }
    }
    assert!(missing.is_empty(), "never went off: {missing:#?}");
}

#[test]
fn rage_builds_from_blows_given_and_taken_and_ebbs_away_after() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 2.5), PI);
    assert_eq!(f.fighter(hero).resource, 0.0, "warriors start calm");
    f.target(hero, golem);
    f.press(hero, 0);
    f.run(5.0);
    let rage = f.fighter(hero).resource;
    assert!(rage >= 15.0, "rage after five seconds of swinging: {rage}");
    assert!(f.damage_to(golem) > 0.0);

    // Taking blows builds it too.
    f.cmd(hero, Command::SetAutoAttack(false));
    f.cmd(hero, Command::SetTarget(None));
    f.sim.get_mut::<Fighter>(hero).unwrap().resource = 0.0;
    let wolf = f.sim.spawn_mob("wolf", ROAD + Vec2::new(-2.0, 0.0)).unwrap();
    f.until(5.0, |f| f.damage_to(hero) > 0.0);
    assert!(f.fighter(hero).resource > 0.0, "being bitten built no rage");

    // Out of the fight, it ebbs away.
    f.kill(wolf);
    f.run(30.0);
    assert_eq!(f.fighter(hero).resource, 0.0);
}

#[test]
fn casts_take_time_and_moving_breaks_off_those_that_need_stillness() {
    let mut f = Fight::new();
    let mage = f.hero(Specialty::Mage, 1, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 12.0), PI);
    f.target(mage, golem);
    f.once(mage, 1); // Frost Lance: 1.4 seconds, standing still.
    f.run(0.5);
    let cast = f.fighter(mage).cast.expect("casting");
    assert_eq!(catalog().ability(cast.ability).key, "staff_frostlance");
    assert_eq!(f.damage_to(golem), 0.0);
    assert!(f.sim.snapshot(Some(mage)).player.unwrap().cast.is_some(), "the cast bar shows");

    f.cmd(mage, Command::Move { direction: Vec2::X, walk: false, face_yaw: None });
    f.step();
    f.step();
    assert!(f.fighter(mage).cast.is_none(), "still casting on the move");
    assert!(f.log.iter().any(|e| matches!(e, Event::Message { text } if text.contains("moved"))));
    f.cmd(mage, Command::Move { direction: Vec2::ZERO, walk: false, face_yaw: None });
    f.run(0.2);

    // Standing still, it goes off, flies, and chills.
    f.once(mage, 1);
    f.run(1.3);
    assert_eq!(f.damage_to(golem), 0.0, "it landed before the cast was done");
    f.run(1.0);
    assert!(f.damage_to(golem) > 0.0);
    assert!(f.has(golem, "chilled"));
    assert!(f.fighter(mage).resource < 100.0 || catalog().ability(cast.ability).cost == 0.0);
}

#[test]
fn skills_wait_for_their_cooldowns() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 2.5), PI);
    f.target(hero, golem);
    for _ in 0..(7 * 60) {
        f.press(hero, 1); // Rending Blade: six seconds.
        f.step();
    }
    assert_eq!(f.used(hero, "sword_rend"), 2);
    assert!(f.has(golem, "bleeding"));
}

#[test]
fn a_skill_pressed_just_before_the_global_cooldown_ends_goes_off_when_it_does() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 2.5), PI);
    f.target(hero, golem);
    f.press(hero, 1);
    f.run(0.7);
    f.press(hero, 3); // Shield Bash, 0.3 seconds early.
    f.step();
    assert_eq!(f.used(hero, "shield_bash"), 0);
    f.run(0.4);
    assert_eq!(f.used(hero, "shield_bash"), 1, "the queued skill never went off");
}

#[test]
fn a_shield_bash_interrupts_a_cast_and_locks_the_caster_out() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let mage = f.foe(Specialty::Mage, 1, ROAD + Vec2::new(0.0, 2.5), PI);
    f.target(mage, hero);
    f.press(mage, 1);
    f.run(0.4);
    assert!(f.fighter(mage).cast.is_some());
    f.target(hero, mage);
    f.press(hero, 3);
    f.step();
    assert!(f.log.iter().any(|e| matches!(e, Event::CastInterrupted { unit } if *unit == mage)));
    assert!(f.fighter(mage).cast.is_none());
    assert!(f.has(mage, "dazed"));
    // Locked out for three seconds, dazed or not.
    f.run(1.7);
    f.press(mage, 1);
    f.step();
    assert!(f.fighter(mage).cast.is_none(), "cast through the lockout");
    f.run(1.5);
    f.press(mage, 1);
    f.step();
    assert!(f.fighter(mage).cast.is_some(), "still locked out");
}

#[test]
fn crowd_control_wears_off_faster_each_time_then_stops_landing() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let goblin = f.dummy("goblin", ROAD + Vec2::new(0.0, 2.0), PI);
    for expected in [2.0, 1.0, 0.5] {
        assert!(f.apply("stunned", goblin, hero));
        let left = f.remaining(goblin, "stunned");
        assert!((left - expected).abs() < 0.02, "stunned for {left}, expected {expected}");
        f.run(expected as f32 + 0.1);
    }
    assert!(!f.apply("stunned", goblin, hero), "a fourth stun landed");
    f.step();
    assert_eq!(f.avoided(goblin, Avoid::Immune), 1);
    // Roots are another matter.
    assert!(f.apply("rooted", goblin, hero));
    // Once the window has passed, a stun lands in full again.
    f.run(18.0);
    assert!(f.apply("stunned", goblin, hero));
    assert!((f.remaining(goblin, "stunned") - 2.0).abs() < 0.02);
}

#[test]
fn defiance_soaks_crowd_control_until_it_breaks() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let dire = f.dummy("dire_wolf", ROAD + Vec2::new(0.0, 3.0), PI);
    let full = f.sim.get::<Auras>(dire).unwrap().defiance_max;
    assert!(full > 0.0);
    assert!(!f.apply("stunned", dire, hero), "an elite was stunned outright");
    assert!(!f.has(dire, "stunned"));
    let after = f.sim.get::<Auras>(dire).unwrap().defiance;
    assert!(after < full, "defiance untouched: {after}");
    // A shield bash breaks it the rest of the way: stunned and exposed.
    f.target(hero, dire);
    f.press(hero, 3);
    f.press(hero, 3);
    f.until(3.0, |f| f.sim.get::<Auras>(dire).unwrap().defiance <= 0.0);
    f.step();
    assert!(f.log.iter().any(|e| matches!(e, Event::DefianceBroken { unit } if *unit == dire)));
    assert!(f.has(dire, "broken") && f.has(dire, "exposed"));
    assert!(f.sim.get::<sim::Control>(dire).unwrap().stunned);
    f.run(10.0);
    assert_eq!(f.sim.get::<Auras>(dire).unwrap().defiance, full, "defiance never came back");
}

#[test]
fn a_warned_slam_can_be_rolled_through() {
    for roll in [false, true] {
        let mut f = Fight::new();
        let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
        let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 3.0), PI);
        assert!(f.force(golem, "golem_quake", Some(hero)));
        f.step();
        let view = f.sim.combat_view(Some(hero));
        let warning = view.areas.iter().find(|a| a.warning).expect("a warning on the ground");
        assert!(warning.hostile);
        assert!(matches!(warning.shape, Shape::Circle { radius } if radius == 5.0));
        f.run(0.75);
        if roll {
            f.cmd(hero, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
        }
        f.run(1.0);
        let hurt = f.damage_to(hero);
        if roll {
            assert_eq!(hurt, 0.0, "rolled and still hit");
            assert!(!f.has(hero, "knockdown"));
        } else {
            assert!(hurt > 0.0, "stood in it and wasn't hit");
            assert!(f.log.iter().any(|e| matches!(e, Event::AuraApplied { unit, aura } if *unit == hero && aura == "knockdown")));
        }
    }
}

#[test]
fn a_raised_shield_blocks_blows_from_the_front_for_endurance() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let foe = f.foe(Specialty::Warrior, 1, ROAD + Vec2::new(0.0, 2.0), PI);
    f.cmd(hero, Command::Guard(true));
    f.run(0.5);
    assert!(f.sim.snapshot(Some(hero)).player.unwrap().guarding);
    f.target(foe, hero);
    f.once(foe, 1); // Rending Blade: a heavy blow that winds up first.
    f.run(0.45);
    assert_eq!(f.avoided(hero, Avoid::Blocked), 1);
    assert_eq!(f.damage_to(hero), 0.0);
    let e = f.vitals(hero).endurance;
    assert!((e - (100.0 - sim::combat::guard::SHIELD_COST)).abs() < 0.01, "endurance {e}");
    // While the guard is up, endurance doesn't come back.
    f.run(0.5);
    assert!(f.vitals(hero).endurance <= e + 0.01);
}

#[test]
fn blocking_a_much_stronger_foe_costs_far_more() {
    let cost = |level: u8| {
        let mut f = Fight::new();
        let hero = f.hero(Specialty::Warrior, 5, ROAD, 0.0);
        let foe = f.foe(Specialty::Warrior, level, ROAD + Vec2::new(0.0, 2.0), PI);
        f.cmd(hero, Command::Guard(true));
        f.run(0.5);
        f.target(foe, hero);
        f.once(foe, 1);
        f.run(0.45);
        assert_eq!(f.avoided(hero, Avoid::Blocked), 1);
        100.0 - f.vitals(hero).endurance
    };
    let (even, five, ten) = (cost(5), cost(10), cost(15));
    assert!((five / even - 1.79).abs() < 0.05, "five levels up: {five} against {even}");
    assert!((ten / even - 3.21).abs() < 0.08, "ten levels up: {ten} against {even}");
}

#[test]
fn a_guard_raised_just_in_time_parries_staggers_and_sets_up_a_riposte() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let foe = f.foe(Specialty::Warrior, 1, ROAD + Vec2::new(0.0, 2.0), PI);
    // The foe winds up a heavy blow; the hero sees it coming and raises the guard just before
    // it lands.
    f.target(foe, hero);
    f.once(foe, 1);
    f.run(0.2);
    assert!(f.sim.combat_view(Some(hero)).units.iter().any(|u| u.id == foe && u.cast.is_some()), "no wind-up to see");
    f.cmd(hero, Command::Guard(true));
    f.run(0.25);
    assert_eq!(f.avoided(hero, Avoid::Parried), 1);
    assert_eq!(f.vitals(hero).endurance, 100.0, "a parry costs nothing");
    assert!(f.log.iter().any(|e| matches!(e, Event::Staggered { unit, by } if *unit == foe && *by == hero)));
    assert!(f.has(foe, "staggered"));
    assert!(f.has(hero, "riposte"));
    // A staggered foe can't strike back for a moment.
    f.press(foe, 3);
    f.step();
    assert_eq!(f.used(foe, "shield_bash"), 0);

    // The riposte: half as hard again, on a foe that takes more while staggered.
    f.cmd(hero, Command::Guard(false));
    f.target(hero, foe);
    let before = f.blows_on(foe).len();
    f.once(hero, 1);
    f.run(0.45);
    let riposte = *f.blows_on(foe).get(before).expect("the riposte landed");
    assert!(!f.has(hero, "riposte"), "the riposte wasn't spent");

    let mut g = Fight::new();
    let a = g.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let b = g.foe(Specialty::Warrior, 1, ROAD + Vec2::new(0.0, 2.0), PI);
    g.target(a, b);
    g.once(a, 1);
    g.run(0.45);
    let plain = g.blows_on(b)[0];
    assert!(riposte >= plain * 1.6, "riposte {riposte} against {plain}");
}

#[test]
fn a_guard_raised_too_early_only_blocks() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let foe = f.foe(Specialty::Warrior, 1, ROAD + Vec2::new(0.0, 2.0), PI);
    f.cmd(hero, Command::Guard(true));
    f.target(foe, hero);
    f.once(foe, 1);
    f.run(0.45);
    assert_eq!(f.avoided(hero, Avoid::Parried), 0);
    assert_eq!(f.avoided(hero, Avoid::Blocked), 1);
}

#[test]
fn a_guard_with_no_endurance_left_breaks() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let foe = f.foe(Specialty::Warrior, 1, ROAD + Vec2::new(0.0, 2.0), PI);
    f.cmd(hero, Command::Guard(true));
    f.run(0.5);
    f.sim.get_mut::<Vitals>(hero).unwrap().endurance = 5.0;
    f.target(foe, hero);
    f.once(foe, 1);
    f.run(0.45);
    assert!(f.damage_to(hero) > 0.0, "the blow didn't land");
    assert!(f.fighter(hero).guard_since.is_none(), "the guard held");
    assert!(f.has(hero, "dazed"));
}

#[test]
fn nothing_guards_against_a_blow_from_behind_and_it_lands_harder() {
    let blow = |behind: bool, guard: bool| {
        let mut f = Fight::new();
        // Facing the foe, or with its back to it.
        let hero = f.hero(Specialty::Warrior, 1, ROAD, if behind { PI } else { 0.0 });
        let foe = f.foe(Specialty::Warrior, 20, ROAD + Vec2::new(0.0, 2.0), PI);
        if guard {
            f.cmd(hero, Command::Guard(true));
            f.run(0.5);
        }
        f.target(foe, hero);
        f.once(foe, 1);
        f.run(0.45);
        f.blows_on(hero).first().copied().unwrap_or(0.0)
    };
    let front = blow(false, false);
    let back = blow(true, true);
    assert!(front > 0.0);
    assert!(back > 0.0, "a guard stopped a blow from behind");
    let ratio = back / front;
    assert!((ratio - 1.1).abs() < 0.04, "from behind: {ratio:.3} times as hard");
    assert_eq!(blow(false, true), 0.0, "a guard facing the blow let it through");
}

#[test]
fn a_backstab_lands_twice_as_hard_from_behind() {
    let stab = |behind: bool| {
        let mut f = Fight::new();
        let rogue = f.hero(Specialty::Rogue, 20, ROAD, 0.0);
        let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 2.5), if behind { 0.0 } else { PI });
        f.target(rogue, golem);
        f.once(rogue, 1);
        f.run(0.45);
        f.blows_on(golem)[0]
    };
    let ratio = stab(true) / stab(false);
    assert!((ratio - 2.2).abs() < 0.08, "a backstab from behind: {ratio:.3} times as hard");
}

#[test]
fn a_blow_too_heavy_to_block_has_to_be_dodged() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 3.0), PI);
    f.cmd(hero, Command::Guard(true));
    f.run(0.5);
    assert!(f.force(golem, "golem_quake", Some(hero)));
    f.run(1.3);
    assert!(f.damage_to(hero) > 0.0, "a guard stopped the quake");
    assert_eq!(f.avoided(hero, Avoid::Blocked) + f.avoided(hero, Avoid::Parried), 0);
}

#[test]
fn shields_stop_arrows_and_other_guards_do_not() {
    for swap in [false, true] {
        let mut f = Fight::new();
        let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
        if swap {
            // The greatsword: it guards against blows, not arrows.
            f.run(0.1);
            f.cmd(hero, Command::SwapWeapons);
            f.step();
            assert_eq!(f.sim.snapshot(Some(hero)).player.unwrap().weapon_set, 1);
        }
        let archer = f.dummy("goblin_archer", ROAD + Vec2::new(0.0, 8.0), PI);
        f.cmd(hero, Command::Guard(true));
        f.run(0.5);
        f.sim.get_mut::<Fighter>(archer).unwrap().target = Some(hero);
        f.sim.get_mut::<Fighter>(archer).unwrap().auto_attack = true;
        f.run(0.6);
        if swap {
            assert!(f.damage_to(hero) > 0.0, "a greatsword stopped an arrow");
        } else {
            assert_eq!(f.damage_to(hero), 0.0, "an arrow got past the shield");
            assert_eq!(f.avoided(hero, Avoid::Blocked), 1);
        }
    }
}

#[test]
fn a_shield_wall_turns_arrows_back_on_the_archer() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let archer = f.dummy("goblin_archer", ROAD + Vec2::new(0.0, 8.0), PI);
    f.press(hero, 4);
    f.step();
    assert!(f.has(hero, "shield_wall"));
    f.sim.get_mut::<Fighter>(archer).unwrap().target = Some(hero);
    f.sim.get_mut::<Fighter>(archer).unwrap().auto_attack = true;
    f.run(1.0);
    assert_eq!(f.avoided(hero, Avoid::Reflected), 1);
    assert_eq!(f.damage_to(hero), 0.0);
    assert!(f.log.iter().any(|e| matches!(e, Event::Damage { target, source, .. } if *target == archer && *source == hero)), "the arrow never came back");
}

#[test]
fn aimed_shots_miss_a_target_that_steps_aside_unless_you_lead_it() {
    let shoot = |lead: bool| {
        let mut f = Fight::new();
        let archer = f.hero(Specialty::Archer, 1, ROAD, 0.0);
        let goblin = f.dummy("goblin", ROAD + Vec2::new(0.0, 20.0), PI);
        f.sim.get_mut::<Motion>(goblin).unwrap().intent.direction = Vec2::X;
        f.run(0.5);
        f.target(archer, goblin);
        let ground = lead.then(|| {
            let p = f.sim.get::<Transform>(goblin).unwrap().pos;
            let v = f.sim.get::<Motion>(goblin).unwrap().observed;
            p + v * (20.0 / 55.0)
        });
        f.cmd(archer, Command::UseSlot { slot: 0, ground });
        f.cmd(archer, Command::SetAutoAttack(false));
        f.run(0.8);
        (f.damage_to(goblin) > 0.0, f.avoided(goblin, Avoid::Missed))
    };
    assert_eq!(shoot(false), (false, 1), "an arrow at where it stood should miss");
    assert!(shoot(true).0, "an arrow at where it was going should hit");
}

#[test]
fn homing_missiles_follow_their_target() {
    let mut f = Fight::new();
    let mage = f.hero(Specialty::Mage, 1, ROAD, 0.0);
    f.run(0.1);
    f.cmd(mage, Command::SwapWeapons); // The wand: homing missiles.
    let goblin = f.dummy("goblin", ROAD + Vec2::new(0.0, 15.0), PI);
    f.sim.get_mut::<Motion>(goblin).unwrap().intent.direction = Vec2::X;
    f.run(0.5);
    f.target(mage, goblin);
    f.press(mage, 0);
    f.cmd(mage, Command::SetAutoAttack(false));
    f.run(1.0);
    assert!(f.damage_to(goblin) > 0.0);
    assert_eq!(f.avoided(goblin, Avoid::Missed), 0);
}

#[test]
fn monsters_lead_their_shots_so_only_a_change_of_direction_dodges() {
    let dodge = |turn: bool| {
        let mut f = Fight::new();
        let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
        let archer = f.dummy("goblin_archer", ROAD + Vec2::new(0.0, 10.0), PI);
        f.cmd(hero, Command::Move { direction: Vec2::X, walk: false, face_yaw: Some(0.0) });
        f.run(0.5);
        f.sim.get_mut::<Fighter>(archer).unwrap().target = Some(hero);
        f.sim.get_mut::<Fighter>(archer).unwrap().auto_attack = true;
        f.until(1.0, |f| f.log.iter().any(|e| matches!(e, Event::ProjectileLaunched { .. })));
        f.sim.get_mut::<Fighter>(archer).unwrap().auto_attack = false;
        if turn {
            f.cmd(hero, Command::Move { direction: -Vec2::X, walk: false, face_yaw: Some(0.0) });
        }
        f.run(0.6);
        f.damage_to(hero) > 0.0
    };
    assert!(dodge(false), "running straight on, the arrow should find you");
    assert!(!dodge(true), "doubling back, it should miss");
}

#[test]
fn a_downed_player_fights_on_and_gets_up_by_felling_the_attacker() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let wolf = f.sim.spawn_mob("wolf", ROAD + Vec2::new(0.0, 2.0)).unwrap();
    f.sim.get_mut::<Vitals>(hero).unwrap().health = 1.0;
    assert!(f.until(6.0, |f| f.vitals(hero).downed), "never went down");
    assert!(f.log.iter().any(|e| matches!(e, Event::Downed { unit } if *unit == hero)));
    let p = f.sim.snapshot(Some(hero)).player.unwrap();
    assert!(p.downed);
    assert_eq!(p.bar[0].ability, "downed_strike");
    assert!(f.sim.combat_view(Some(hero)).units.iter().any(|u| u.id == hero && u.downed));

    f.sim.get_mut::<Vitals>(wolf).unwrap().health = 5.0;
    f.target(hero, wolf);
    f.press(hero, 0);
    assert!(f.until(5.0, |f| f.vitals(wolf).dead), "the wolf survived");
    f.step();
    assert!(f.log.iter().any(|e| matches!(e, Event::Rallied { unit } if *unit == hero)));
    let v = f.vitals(hero);
    assert!(!v.downed && !v.dead);
    assert!((v.health / v.max_health - life::RALLY_HEALTH).abs() < 0.05, "up with {} health", v.health);
}

#[test]
fn left_alone_a_downed_player_bleeds_out_and_respawns_at_home() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let wolf = f.sim.spawn_mob("wolf", ROAD + Vec2::new(0.0, 2.0)).unwrap();
    f.sim.get_mut::<Vitals>(hero).unwrap().health = 1.0;
    assert!(f.until(6.0, |f| f.vitals(hero).downed));
    f.kill(wolf);
    f.cmd(hero, Command::Move { direction: Vec2::X, walk: false, face_yaw: None });
    f.run(2.0);
    let crawled = (f.sim.get::<Transform>(hero).unwrap().pos - Vec3::new(ROAD.x, 0.0, ROAD.y)).with_y(0.0).length();
    assert!(crawled > 0.5 && crawled < 3.0, "crawled {crawled} m");
    f.cmd(hero, Command::Move { direction: Vec2::ZERO, walk: false, face_yaw: None });
    assert!(f.until(18.0, |f| f.vitals(hero).dead), "never bled out");
    assert!(f.log.iter().any(|e| matches!(e, Event::Died { unit, .. } if *unit == hero)));
    assert!(f.sim.snapshot(Some(hero)).player.unwrap().can_respawn);

    f.cmd(hero, Command::Respawn);
    f.step();
    let v = f.vitals(hero);
    assert!(!v.dead && v.health == v.max_health);
    let at = f.sim.get::<Transform>(hero).unwrap().pos;
    assert!((Vec2::new(at.x, at.z) - ROAD).length() < 0.01, "respawned at {at}");
}

#[test]
fn a_friend_can_help_you_up() {
    let mut f = Fight::new();
    let hero = f.hero(Specialty::Warrior, 1, ROAD, 0.0);
    let friend = f.hero(Specialty::Healer, 1, ROAD + Vec2::new(1.5, 0.0), 0.0);
    let wolf = f.sim.spawn_mob("wolf", ROAD + Vec2::new(0.0, 2.0)).unwrap();
    f.sim.get_mut::<Vitals>(hero).unwrap().health = 1.0;
    assert!(f.until(6.0, |f| f.vitals(hero).downed));
    f.kill(wolf);
    f.cmd(friend, Command::Revive { target: hero });
    f.run(1.5);
    assert!(f.vitals(hero).downed, "up too soon");
    f.run(2.0);
    assert!(!f.vitals(hero).downed, "never got up");
    assert!(f.log.iter().any(|e| matches!(e, Event::Rallied { unit } if *unit == hero)));
}

#[test]
fn a_necromancer_raises_a_skeleton_that_fights_and_crumbles() {
    let mut f = Fight::new();
    let necro = f.hero(Specialty::Necromancer, 3, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 8.0), PI);
    f.target(necro, golem);
    f.press(necro, 4); // Raise Skeleton.
    f.run(2.0);
    let skeleton = {
        let mut q = f.sim.world.query::<(&Unit, &sim::Summoned)>();
        q.iter(&f.sim.world).map(|(u, s)| (u.id, u.faction, u.level, s.owner)).next().expect("a skeleton rose")
    };
    assert_eq!(skeleton.1, Faction::Player);
    assert_eq!(skeleton.2, 3, "summons fight at their owner's level");
    assert_eq!(skeleton.3, necro);
    f.run(8.0);
    assert!(f.log.iter().any(|e| matches!(e, Event::Damage { target, source, .. } if *target == golem && *source == skeleton.0)), "it never fought");
    f.run(30.0);
    assert!(f.sim.entity(skeleton.0).is_none(), "it never crumbled away");
}

#[test]
fn a_bolt_through_a_fire_field_sets_its_target_burning() {
    let mut f = Fight::new();
    let mage = f.hero(Specialty::Mage, 1, ROAD, 0.0);
    let golem = f.dummy("golem", ROAD + Vec2::new(0.0, 12.0), PI);
    f.target(mage, golem);
    f.press(mage, 4); // Ring of Fire around the mage.
    f.run(1.1);
    f.press(mage, 0); // Fire Bolt, out through the ring.
    f.cmd(mage, Command::SetAutoAttack(false));
    f.run(2.0);
    assert!(f.log.iter().any(|e| matches!(e, Event::Combo { unit, name, .. } if *unit == mage && name == "Burning Bolt")));
    assert!(f.has(golem, "burning"));
}

#[test]
fn the_combat_view_shows_actions_casts_auras_missiles_and_defiance() {
    let mut f = Fight::new();
    let mage = f.hero(Specialty::Mage, 1, ROAD, 0.0);
    let dire = f.dummy("dire_wolf", ROAD + Vec2::new(0.0, 14.0), PI);
    f.target(mage, dire);
    f.press(mage, 0);
    f.run(0.3);
    let view = f.sim.combat_view(Some(mage));
    let me = view.units.iter().find(|u| u.id == mage).unwrap();
    assert_eq!(me.cast.as_ref().unwrap().ability, "staff_firebolt");
    assert_eq!(me.action, protocol::Action::CastBolt);
    assert_eq!(me.resource_kind, ResourceKind::Mana);
    let wolf = view.units.iter().find(|u| u.id == dire).unwrap();
    assert!(matches!(wolf.defiance, Some((d, m)) if d == m && m > 0.0));
    f.run(0.75);
    let view = f.sim.combat_view(Some(mage));
    assert!(view.projectiles.iter().any(|p| p.visual == "fire_bolt"), "{:?}", view.projectiles);
    f.cmd(mage, Command::SetAutoAttack(false));
    f.run(1.0);
    assert!(f.damage_to(dire) > 0.0);
    let view = f.sim.combat_view(Some(mage));
    assert!(view.projectiles.is_empty());
}

#[test]
fn busy_fights_play_out_the_same_every_time() {
    fn run() -> (Vec<protocol::Snapshot>, Vec<protocol::CombatView>) {
        let mut sim = Sim::brambleford(5);
        let warrior = sim.spawn_hero("Warrior", Specialty::Warrior, 3, Vec2::new(-38.0, 22.0), 0.0);
        let mage = sim.spawn_hero("Mage", Specialty::Mage, 3, Vec2::new(-36.0, 18.0), 0.0);
        let archer = sim.spawn_hero("Archer", Specialty::Archer, 3, Vec2::new(-40.0, 17.0), 0.0);
        sim.spawn_mob("dire_wolf", Vec2::new(-30.0, 32.0));
        let mut snaps = Vec::new();
        let mut views = Vec::new();
        for step in 0..1200u32 {
            if step % 20 == 0 {
                let pack: Vec<u32> = {
                    let mut q = sim.world.query::<(&Unit, &Vitals)>();
                    let mut ids: Vec<u32> = q.iter(&sim.world).filter(|(u, v)| u.faction == Faction::Hostile && !v.dead).map(|(u, _)| u.id).collect();
                    ids.sort();
                    ids
                };
                if let Some(&t) = pack.first() {
                    for (who, slot) in [(warrior, (step / 20 % 5) as u8), (mage, (step / 40 % 3) as u8), (archer, (step / 60 % 4) as u8)] {
                        sim.command(who, Command::SetTarget(Some(t)));
                        sim.command(who, Command::UseSlot { slot, ground: None });
                    }
                }
            }
            if step == 300 {
                sim.command(warrior, Command::Guard(true));
            }
            if step == 330 {
                sim.command(warrior, Command::Guard(false));
                sim.command(mage, Command::Dodge { direction: Vec2::new(1.0, 0.0) });
            }
            sim.step();
            if step % 30 == 0 {
                snaps.push(sim.snapshot(Some(warrior)));
                views.push(sim.combat_view(Some(warrior)));
            }
        }
        (snaps, views)
    }
    let (a, b) = (run(), run());
    assert!(a.0.iter().flat_map(|s| &s.events).any(|e| matches!(e, Event::Damage { .. })), "nothing fought");
    assert!(a == b, "two runs of the same fight differed");
}

/// A crowd fighting stays well inside the step budget (run with --release).
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn a_crowd_fighting_fits_the_step_budget() {
    let mut sim = Sim::new(9);
    sim.spawn_crowd_in(300, Vec2::new(0.0, -40.0), Vec2::new(60.0, 60.0));
    for i in 0..12 {
        let s = Specialty::ALL[i % 6];
        let id = sim.spawn_hero("Hero", s, 4, Vec2::new(-30.0 + i as f32 * 5.0, -40.0), 0.0);
        sim.command(id, Command::SetAutoAttack(true));
    }
    sim.run_for(2.0);
    let start = std::time::Instant::now();
    let steps = 600;
    let mut blows = 0;
    for _ in 0..steps {
        sim.step();
        blows += sim.snapshot(None).events.iter().filter(|e| matches!(e, Event::Damage { .. })).count();
        let _ = sim.combat_view(None);
    }
    let per = start.elapsed().as_secs_f64() * 1000.0 / steps as f64;
    println!("{per:.3} ms a step, {blows} blows in ten seconds");
    assert!(blows > 200, "the crowd hardly fought: {blows} blows");
    assert!(per < 2.0, "{per:.3} ms a step");
}
