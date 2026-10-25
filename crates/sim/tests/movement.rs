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

/// Runs until a climb starts and finishes (or `steps` pass); whether it climbed.
fn climb(sim: &mut Sim, id: u32, steps: u32) -> bool {
    let mut climbing = false;
    for _ in 0..steps {
        sim.step();
        let forced = sim.get::<Motion>(id).unwrap().forced.is_some();
        if climbing && !forced {
            break;
        }
        climbing |= forced;
    }
    climbing
}

/// A player at `at` heading along `dir` (and jumping, if asked) for two thirds of a second:
/// how far up the ledge they climbed was, if they climbed one.
fn try_climb(at: Vec2, dir: Vec2, jump: bool) -> (Option<f32>, Sim, u32) {
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", at, dir.x.atan2(dir.y));
    sim.step();
    go(&mut sim, id, dir, false);
    if jump {
        sim.command(id, Command::Jump);
    }
    let mut climbed = None;
    for _ in 0..40 {
        sim.step();
        if let Some(f) = sim.get::<Motion>(id).unwrap().forced {
            climbed = Some(f.to.y - f.from.y);
        }
    }
    (climbed, sim, id)
}

#[test]
fn climbs_a_low_ledge_by_walking_into_it() {
    // A step in the terraces north-west of the village, going south-west.
    let (climbed, mut sim, id) = try_climb(Vec2::new(-50.0, 26.0), Vec2::new(-1.0, -1.0).normalize(), false);
    let rise = climbed.expect("never climbed");
    assert!(rise > 0.3 && rise <= 1.5, "climbed {rise} m");
    sim.run_for(0.5);
    let t = sim.get::<Transform>(id).unwrap().pos;
    assert!((t.y - sim.land().ground(t.x, t.z)).abs() < 0.05, "standing on the top");
}

#[test]
fn a_jump_reaches_a_ledge_too_tall_to_walk_up() {
    // Below a ledge on the eastern hills, going south-west.
    let at = Vec2::new(106.0, -106.0);
    let dir = Vec2::new(-1.0, -1.0).normalize();
    assert!(try_climb(at, dir, false).0.is_none(), "walked up it");
    assert!(try_climb(at, dir, true).0.is_some(), "the jump didn't reach it");
}

#[test]
fn the_bridge_parapets_keep_you_off_its_sides() {
    // Beside the north end of the bridge, about 1.2 m below its deck: the parapet is in the way.
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(-2.5, -50.0), std::f32::consts::FRAC_PI_2);
    sim.step();
    go(&mut sim, id, Vec2::X, false);
    assert!(!climb(&mut sim, id, 90), "climbed through the parapet");
    assert!(xz(&sim, id).x < -0.5);
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

#[test]
fn running_off_a_cliff_is_a_fall() {
    // On top of the west terraces, running east off the 16 m limestone face.
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(-50.0, -30.0), std::f32::consts::FRAC_PI_2);
    sim.step();
    let top = sim.get::<Transform>(id).unwrap().pos.y;
    go(&mut sim, id, Vec2::X, true);
    let mut airborne = false;
    for _ in 0..240 {
        sim.step();
        airborne |= !sim.get::<Motion>(id).unwrap().grounded;
    }
    let t = sim.get::<Transform>(id).unwrap().pos;
    let v = sim.get::<Vitals>(id).unwrap();
    assert!(top - t.y > 8.0, "only came down {} m", top - t.y);
    assert!(airborne, "ran down the face");
    assert!(v.health < v.max_health * 0.8, "fell {} m and kept {} of {}", top - t.y, v.health, v.max_health);
}
