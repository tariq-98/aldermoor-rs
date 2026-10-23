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
    sim.command(id, Command::Move { direction, walk, face_yaw });
    sim.run_for(seconds);
    sim.command(id, Command::Move { direction: Vec2::ZERO, walk: false, face_yaw: None });
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
    sim.command(id, Command::Move { direction: Vec2::new(1.0, 0.0), walk: false, face_yaw: Some(0.0) });
    sim.run_for(0.5);
    assert!(sim.get::<Transform>(id).unwrap().yaw.abs() < 1e-4);
}

#[test]
fn walls_stop_you() {
    let mut sim = Sim::new(7);
    // South of the first cottage, walking north into it.
    let id = sim.spawn_player("Hero", Vec2::new(-12.0, -2.0), std::f32::consts::PI);
    sim.command(id, Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, face_yaw: None });
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
