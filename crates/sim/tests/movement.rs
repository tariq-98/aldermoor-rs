//! Movement, checked against the numbers the Swift game's tests pinned down.

use glam::Vec2;
use protocol::Command;
use sim::{Motion, Sim, Transform, Vitals};

fn xz(sim: &Sim, id: u32) -> Vec2 {
    let t = sim.get::<Transform>(id).unwrap();
    Vec2::new(t.pos.x, t.pos.z)
}

/// A player standing on the north road, clear of walls and trees.
fn on_the_road() -> (Sim, u32) {
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), std::f32::consts::PI);
    sim.step();
    (sim, id)
}

fn walk(sim: &mut Sim, id: u32, direction: Vec2, walk: bool, face_yaw: Option<f32>, seconds: f32) -> f32 {
    let start = xz(sim, id);
    sim.command(id, Command::Move { direction, walk, sprint: false, face_yaw });
    sim.run_for(seconds);
    sim.command(id, Command::Move { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: None });
    sim.step();
    (xz(sim, id) - start).length()
}

#[test]
fn runs_about_six_metres_a_second() {
    let (mut sim, id) = on_the_road();
    let d = walk(&mut sim, id, Vec2::new(0.0, -1.0), false, None, 1.0);
    assert!((d - 6.0).abs() < 0.25, "ran {d} m");
}

#[test]
fn walks_about_two_and_a_half_metres_a_second() {
    let (mut sim, id) = on_the_road();
    let d = walk(&mut sim, id, Vec2::new(0.0, -1.0), true, None, 1.0);
    assert!((d - 2.5).abs() < 0.15, "walked {d} m");
}

#[test]
fn backpedalling_is_slower_and_strafing_keeps_facing() {
    let (mut sim, id) = on_the_road();
    // Facing south (+z), moving north: backpedalling.
    let d = walk(&mut sim, id, Vec2::new(0.0, -1.0), false, Some(0.0), 1.0);
    assert!((d - 3.6).abs() < 0.25, "backpedalled {d} m");
    // Facing south, moving east: strafing keeps the facing.
    sim.command(id, Command::Move { direction: Vec2::new(1.0, 0.0), walk: false, sprint: false, face_yaw: Some(0.0) });
    sim.run_for(0.5);
    assert!(sim.get::<Transform>(id).unwrap().yaw.abs() < 1e-4);
}

#[test]
fn walls_stop_you() {
    let mut sim = Sim::new(7);
    // South of the first cottage, walking north into it.
    let id = sim.spawn_player("Hero", Vec2::new(-12.0, -2.0), std::f32::consts::PI);
    sim.command(id, Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, sprint: false, face_yaw: None });
    sim.run_for(3.0);
    let z = xz(&sim, id).y;
    assert!(z > -9.6 && z < -7.0, "stopped at z = {z}");
}

#[test]
fn jumps_about_a_metre_and_lands_in_two_thirds_of_a_second() {
    let (mut sim, id) = on_the_road();
    let ground = |sim: &Sim| {
        let p = xz(sim, id);
        sim.land().ground(p.x, p.y)
    };
    sim.command(id, Command::Jump);
    let mut peak: f32 = 0.0;
    let mut landed_at = None;
    for step in 1..=90 {
        sim.step();
        let t = sim.get::<Transform>(id).unwrap();
        peak = peak.max(t.pos.y - ground(&sim));
        if step > 2 && sim.get::<Motion>(id).unwrap().grounded && landed_at.is_none() {
            landed_at = Some(step as f32 / 60.0);
        }
    }
    assert!((peak - 1.09).abs() < 0.08, "peak {peak} m");
    let landed = landed_at.expect("landed");
    assert!((landed - 0.66).abs() < 0.08, "landed after {landed} s");
}

#[test]
fn dodging_rolls_five_metres_for_half_your_endurance() {
    let (mut sim, id) = on_the_road();
    let start = xz(&sim, id);
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.run_for(0.6);
    let rolled = (xz(&sim, id) - start).length();
    assert!((rolled - 5.0).abs() < 0.3, "rolled {rolled} m");
    // 50 spent; it regenerates at 8 a second once the 0.45-second roll ends.
    let e = sim.get::<Vitals>(id).unwrap().endurance;
    assert!((e - 51.2).abs() < 0.5, "endurance {e}");
    // A second roll spends the rest; a third has nothing left to spend.
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.step();
    let after_two = sim.get::<Vitals>(id).unwrap().endurance;
    sim.run_for(0.5);
    let before_three = xz(&sim, id);
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.run_for(0.5);
    assert!(after_two < 10.0, "endurance after two rolls: {after_two}");
    assert!((xz(&sim, id) - before_three).length() < 0.1, "rolled without endurance");
}

fn go(sim: &mut Sim, id: u32, direction: Vec2, sprint: bool) {
    sim.command(id, Command::Move { direction, walk: false, sprint, face_yaw: None });
}

#[test]
fn sprints_half_again_as_fast_out_of_combat_only() {
    let (mut sim, id) = on_the_road();
    let start = xz(&sim, id);
    go(&mut sim, id, Vec2::new(0.0, -1.0), true);
    sim.run_for(1.0);
    let d = (xz(&sim, id) - start).length();
    assert!((d - 9.0).abs() < 0.4, "sprinted {d} m");
    assert!(sim.get::<Motion>(id).unwrap().sprinting);
    // Just hit: back to running.
    let now = sim.time();
    sim.get_mut::<Vitals>(id).unwrap().last_combat = now;
    let start = xz(&sim, id);
    sim.run_for(1.0);
    let d = (xz(&sim, id) - start).length();
    assert!((d - 6.0).abs() < 0.3, "ran {d} m in combat");
    assert!(!sim.get::<Motion>(id).unwrap().sprinting);
}

#[test]
fn climbs_onto_the_bridge_from_the_bank() {
    // Beside the north end of the bridge, about 1.2 m below its deck.
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(-2.5, -50.0), std::f32::consts::FRAC_PI_2);
    sim.step();
    let before = sim.get::<Transform>(id).unwrap().pos.y;
    go(&mut sim, id, Vec2::X, false);
    let mut climbing = false;
    for _ in 0..120 {
        sim.step();
        let forced = sim.get::<Motion>(id).unwrap().forced.is_some();
        if climbing && !forced {
            break;
        }
        climbing |= forced;
    }
    let t = sim.get::<Transform>(id).unwrap().pos;
    assert!(climbing, "never started climbing");
    assert!(t.x > -1.0 && t.y > before + 0.9, "ended at {t} from y {before}");
    assert!((t.y - sim.land().ground(t.x, t.z)).abs() < 0.05, "standing on the deck");
}

#[test]
fn cliffs_taller_than_chest_height_stop_you() {
    // East of the limestone terraces, running west at their cliff.
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(-30.0, -10.0), -std::f32::consts::FRAC_PI_2);
    sim.step();
    go(&mut sim, id, -Vec2::X, false);
    sim.run_for(4.0);
    let t = sim.get::<Transform>(id).unwrap().pos;
    assert!(t.x > -39.0 && t.x < -36.0, "stopped at x = {}", t.x);
    assert!(sim.get::<Motion>(id).unwrap().forced.is_none());
    assert!(t.y < sim.land().ground(-42.0, -10.0) - 2.0, "got up the cliff to {t}");
}

#[test]
fn swims_the_river_slowly_and_cannot_fight_in_it() {
    let r = world::river_z(40.0);
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(40.0, r + 9.0), std::f32::consts::PI);
    sim.step();
    go(&mut sim, id, Vec2::new(0.0, -1.0), true);
    let mut swam = false;
    let mut swim_speed: f32 = 0.0;
    for _ in 0..480 {
        sim.step();
        let m = *sim.get::<Motion>(id).unwrap();
        if m.swimming {
            // The step it dived in was still at wading speed.
            if swam {
                swim_speed = swim_speed.max(Vec2::new(m.observed.x, m.observed.z).length());
            }
            swam = true;
            assert!(!m.sprinting);
            let t = sim.get::<Transform>(id).unwrap().pos;
            let float = (world::WATER_LEVEL - 0.5).max(sim.land().ground(t.x, t.z));
            assert!((t.y - float).abs() < 0.01, "floating at {}", t.y);
        }
    }
    assert!(swam, "never swam");
    assert!(swim_speed < 3.0, "swam at {swim_speed} m/s");
    // Out on the far bank, dry and walking again.
    let p = xz(&sim, id);
    assert!(p.y < r - 6.0, "only got to z = {}", p.y);
    assert!(!sim.get::<Motion>(id).unwrap().swimming);

    // Back in, skills are refused.
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(40.0, r), 0.0);
    sim.run_for(0.2);
    assert!(sim.get::<Motion>(id).unwrap().swimming);
    sim.command(id, Command::UseSlot { slot: 1, ground: None });
    sim.command(id, Command::Dodge { direction: Vec2::X });
    sim.step();
    let s = sim.snapshot(Some(id));
    assert!(s.player.as_ref().unwrap().swimming);
    assert!(s.events.iter().any(|e| matches!(e, protocol::Event::Message { text } if text.contains("swimming"))));
    assert!(!s.events.iter().any(|e| matches!(e, protocol::Event::Dodged { .. })));
}

#[test]
fn long_falls_hurt_and_short_ones_do_not() {
    let fall = |height: f32| {
        let (mut sim, id) = on_the_road();
        {
            let mut t = sim.get_mut::<Transform>(id).unwrap();
            t.pos.y += height;
        }
        sim.get_mut::<Motion>(id).unwrap().grounded = false;
        // Until just after landing, before health starts coming back.
        sim.step();
        while !sim.get::<Motion>(id).unwrap().grounded {
            sim.step();
        }
        sim.step();
        let v = sim.get::<Vitals>(id).unwrap();
        1.0 - v.health / v.max_health
    };
    assert_eq!(fall(1.0), 0.0);
    assert_eq!(fall(3.9), 0.0);
    let lost = fall(10.0);
    assert!((lost - 0.36).abs() < 0.02, "lost {lost} of max health from 10 m");
}
