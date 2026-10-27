//! The air dash and the double jump everyone has (#347): one of each per jump, refreshed by the
//! ground and by walls.

use glam::Vec2;
use protocol::Command;
use sim::{ForcedKind, Motion, Sim, Transform, Vitals};

/// The dash goes 4 m, pitched by the aim up to 35°; the double jump leaves the ground at 5.5 m/s;
/// Roll less than 0.2 s before landing is a roll landing (docs/MOVEMENT.md).
const AIR_DASH_DISTANCE: f32 = 4.0;
const AIR_DASH_PITCH: f32 = 0.61;
const AIR_JUMP_SPEED: f32 = 5.5;
const LANDING_ROLL: f64 = 0.2;

/// A player standing on the north road, clear of walls and trees.
fn on_the_road() -> (Sim, u32) {
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), std::f32::consts::PI);
    sim.step();
    (sim, id)
}

fn motion(sim: &Sim, id: u32) -> Motion {
    *sim.get::<Motion>(id).unwrap()
}

fn dashing(sim: &Sim, id: u32) -> bool {
    motion(sim, id).forced.is_some_and(|f| f.kind == ForcedKind::AirDash)
}

/// Jumps, and steps until `seconds` into the jump.
fn jump(sim: &mut Sim, id: u32, seconds: f32) {
    sim.command(id, Command::Jump);
    sim.run_for(seconds);
}

#[test]
fn one_air_dash_and_one_double_jump_a_jump_and_both_come_back_on_landing() {
    let (mut sim, id) = on_the_road();
    let endurance = sim.get::<Vitals>(id).unwrap().endurance;
    jump(&mut sim, id, 0.15);
    let start = sim.get::<Transform>(id).unwrap().pos;
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.step();
    assert!(dashing(&sim, id), "Roll in a jump didn't dash");
    sim.run_for(0.2);
    let end = sim.get::<Transform>(id).unwrap().pos;
    let along = (end - start).length();
    assert!((along - AIR_DASH_DISTANCE).abs() < 0.6, "dashed {along} m");
    assert!(sim.get::<Vitals>(id).unwrap().endurance < endurance, "the dash was free");

    // A second dash in the same jump does nothing; the double jump goes, once.
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.step();
    assert!(!dashing(&sim, id), "dashed twice in one jump");
    sim.command(id, Command::Jump);
    sim.step();
    let vy = motion(&sim, id).vel.y;
    assert!(vy > AIR_JUMP_SPEED - 1.0, "no double jump: rising at {vy}");
    sim.run_for(0.2);
    sim.command(id, Command::Jump);
    sim.step();
    assert!(motion(&sim, id).vel.y < vy - 3.0, "jumped a third time in the air");

    // Down again, both are back.
    sim.run_for(2.0);
    assert!(motion(&sim, id).grounded, "never landed");
    let m = motion(&sim, id);
    assert!(m.air_dash && m.air_jump, "{m:?}");
}

#[test]
fn a_dash_follows_the_aim_up() {
    let (mut sim, id) = on_the_road();
    sim.command(id, Command::Aim { pitch: protocol::aim_pitch_byte(1.2) });
    jump(&mut sim, id, 0.1);
    let y0 = sim.get::<Transform>(id).unwrap().pos.y;
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.run_for(0.2);
    let rise = sim.get::<Transform>(id).unwrap().pos.y - y0;
    // Pitched as far as it goes (35°, though the aim is steeper): enough to reach a ledge
    // 2 m above where it began.
    let most = AIR_DASH_DISTANCE * AIR_DASH_PITCH.sin();
    assert!(rise > most - 0.3 && rise < most + 0.3, "rose {rise} m");
}

#[test]
fn roll_just_before_landing_is_still_a_roll_landing() {
    let (mut sim, id) = on_the_road();
    // A jump lands after about 0.66 s: press with less than `LANDING_ROLL` to go.
    jump(&mut sim, id, 0.66 - LANDING_ROLL as f32 * 0.5);
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.step();
    assert!(!dashing(&sim, id), "dashed into the ground");
    let mut rolled = false;
    for _ in 0..30 {
        sim.step();
        rolled |= motion(&sim, id).forced.is_some_and(|f| f.kind == ForcedKind::Dodge);
    }
    assert!(rolled, "didn't roll on landing");
}

#[test]
fn no_air_moves_when_downed() {
    let (mut sim, id) = on_the_road();
    jump(&mut sim, id, 0.1);
    sim.get_mut::<Vitals>(id).unwrap().downed = true;
    sim.command(id, Command::Dodge { direction: Vec2::new(0.0, -1.0) });
    sim.command(id, Command::Jump);
    sim.step();
    let m = motion(&sim, id);
    assert!(!dashing(&sim, id) && m.vel.y < AIR_JUMP_SPEED - 1.0, "{m:?}");
}
