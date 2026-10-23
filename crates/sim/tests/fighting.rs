//! Monsters notice you, fight, give up, and come back; and the world is deterministic.

use glam::Vec2;
use protocol::{Command, Event};
use sim::{Brain, Fighter, Mode, Sim, Transform, Vitals};

fn wolves(sim: &Sim) -> Vec<u32> {
    let mut q = sim.world.try_query::<(&sim::Unit, &Brain)>().unwrap();
    let mut ids: Vec<u32> = q.iter(&sim.world).filter(|(u, _)| u.body == "wolf").map(|(u, _)| u.id).collect();
    ids.sort();
    ids
}

#[test]
fn a_wolf_pack_notices_you_and_bites() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(3.0);
    let pack = wolves(&sim);
    assert_eq!(pack.len(), 3);
    assert!(pack.iter().any(|&w| sim.get::<Fighter>(w).unwrap().target == Some(hero)), "nobody noticed");
    assert!(sim.get::<Vitals>(hero).unwrap().health < 250.0, "nobody bit");
}

#[test]
fn you_can_kill_a_wolf_and_it_comes_back() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(1.0);
    let wolf = wolves(&sim)[0];
    sim.command(hero, Command::SetTarget(Some(wolf)));
    sim.command(hero, Command::SetAutoAttack(true));
    let mut died = false;
    let mut events = Vec::new();
    for _ in 0..(20 * 60) {
        // Keep the hero next to the wolf and striking.
        let (h, w) = (sim.get::<Transform>(hero).unwrap().pos, sim.get::<Transform>(wolf).unwrap().pos);
        let to = Vec2::new(w.x - h.x, w.z - h.z);
        let dir = if to.length() > 1.8 { to.normalize() } else { Vec2::ZERO };
        sim.command(hero, Command::Move { direction: dir, walk: false, face_yaw: None });
        sim.command(hero, Command::UseSlot { slot: 0, ground: None });
        sim.step();
        events.extend(sim.snapshot(Some(hero)).events);
        if sim.get::<Vitals>(wolf).unwrap().dead {
            died = true;
            break;
        }
    }
    assert!(died, "the wolf survived");
    assert!(events.iter().any(|e| matches!(e, Event::Died { unit, killer: Some(k) } if *unit == wolf && *k == hero)));
    assert!(sim.get::<Vitals>(hero).unwrap().health > 0.0, "the hero fell");
    sim.command(hero, Command::Move { direction: Vec2::new(0.0, 1.0), walk: false, face_yaw: None });
    sim.run_for(21.0);
    assert!(!sim.get::<Vitals>(wolf).unwrap().dead, "the wolf didn't come back");
}

#[test]
fn monsters_give_up_far_from_home_and_heal() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 21.0), 0.0);
    sim.run_for(0.5);
    let wolf = wolves(&sim)[0];
    sim.command(hero, Command::SetTarget(Some(wolf)));
    sim.command(hero, Command::UseSlot { slot: 0, ground: None });
    sim.run_for(0.2);
    // Run east, far past the leash.
    sim.command(hero, Command::Move { direction: Vec2::new(1.0, 0.0), walk: false, face_yaw: None });
    let mut gave_up = false;
    for _ in 0..(15 * 60) {
        sim.step();
        if sim.get::<Brain>(wolf).unwrap().mode == Mode::Return {
            gave_up = true;
        }
    }
    assert!(gave_up, "the wolf never gave up");
    sim.run_for(10.0);
    let v = sim.get::<Vitals>(wolf).unwrap();
    assert!((v.health - v.max_health).abs() < 0.5, "not healed: {}", v.health);
}

#[test]
fn the_same_seed_and_commands_give_the_same_world() {
    fn run() -> Vec<Vec<u8>> {
        let mut sim = Sim::brambleford(11);
        let hero = sim.spawn_player("Hero", Vec2::new(-38.0, 22.0), 0.0);
        let mut frames = Vec::new();
        for step in 0..900u32 {
            match step {
                10 => sim.command(hero, Command::Move { direction: Vec2::new(0.3, 1.0), walk: false, face_yaw: None }),
                120 => sim.command(hero, Command::Jump),
                200 => sim.command(hero, Command::Dodge { direction: Vec2::new(1.0, 0.0) }),
                300 => sim.command(hero, Command::SetAutoAttack(true)),
                _ => {}
            }
            sim.step();
            if step % 60 == 0 {
                frames.push(sim.snapshot(Some(hero)).to_bytes());
            }
        }
        frames
    }
    assert_eq!(run(), run());
}
