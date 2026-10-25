//! Climbing on handholds (docs/MOVEMENT.md, "Climbing"): hanging, shimmying, climbing up and down
//! between holds and onto tops, letting go, lowering yourself from an edge, and catching a hold.

use glam::{Vec2, Vec3};
use protocol::{Command, Event, Pose, Traversal};
use sim::climbing::{HANG_DROP, SHIMMY_SPEED};
use sim::{Anim, Hang, Motion, Sim, Transform, Vitals};

/// Puts a wall `rise` metres tall (to its top, from the ground at its middle) with its middle at
/// `center`, `half` its half-size (x across, z along), and returns its top.
fn wall(sim: &mut Sim, center: Vec2, half: Vec2, rise: f32) -> f32 {
    let top = sim.land().ground(center.x, center.y) + rise;
    wall_to(sim, center, half, top);
    top
}

/// A wall as [`wall`], with its top at the height `top`.
fn wall_to(sim: &mut Sim, center: Vec2, half: Vec2, top: f32) {
    sim.land_mut().add_solid(world::Solid { center, half, yaw: 0.0, top: Some(top), rise: 0.0 });
}

/// By the field gate east of the road (nothing but the field beyond): a wall 3.4 m tall, 0.9 m
/// deep and 6 m long running north-south, its west face at x = 7.55, with a player standing on its
/// top by that edge, facing east (their back to the drop).
fn on_a_tall_wall() -> (Sim, u32, f32) {
    let mut sim = Sim::new(7);
    let top = wall(&mut sim, Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 3.4);
    let id = sim.spawn_player("Hero", Vec2::new(7.8, 12.0), std::f32::consts::FRAC_PI_2);
    sim.step();
    (sim, id, top)
}

fn pos(sim: &Sim, id: u32) -> Vec3 {
    sim.get::<Transform>(id).unwrap().pos
}

fn motion(sim: &Sim, id: u32) -> Motion {
    *sim.get::<Motion>(id).unwrap()
}

fn go(sim: &mut Sim, id: u32, direction: Vec2, sprint: bool) {
    sim.command(id, Command::Move { direction, walk: false, sprint, face_yaw: None });
}

/// Rolls toward the west edge and waits for the drop to finish: hanging from the wall's west face.
fn hang_from_the_west_face(sim: &mut Sim, id: u32) -> Hang {
    sim.command(id, Command::Dodge { direction: -Vec2::X });
    sim.step();
    // Other players see the move onto the hold.
    let s = sim.snapshot(Some(id));
    let u = s.units.iter().find(|u| u.id == id).unwrap();
    assert!(matches!(u.traversal, Traversal::Forced(f) if f.kind == protocol::ForcedKind::Hold), "{:?}", u.traversal);
    sim.run_for(0.7);
    motion(sim, id).hang.expect("never hung from the edge")
}

#[test]
fn the_ruins_tall_broken_walls_have_holds_and_the_field_walls_none() {
    let sim = Sim::new(7);
    let land = sim.land();
    let ruin = land.masonry.iter().find(|m| m.prop == "ruin.hall").unwrap().origin;
    assert!(!land.holds.is_empty());
    for h in land.holds.iter() {
        // Only the ruin stands tall enough to hang from; the field walls are climbed onto.
        let mid = h.at(h.len() / 2.0);
        assert!((Vec2::new(mid.x, mid.z) - Vec2::new(ruin.x, ruin.z)).length() < 8.0, "hold at {mid}");
        let below = Vec2::new(mid.x, mid.z) + h.out * 0.3;
        assert!(mid.y - land.ground(below.x, below.y) >= world::HANG_MIN_DROP - 1e-3, "shallow hold at {mid}");
        // `a` to `b` runs to the right of a climber facing the wall.
        let along = Vec2::new(h.b.x - h.a.x, h.b.z - h.a.z);
        assert!(along.dot(Vec2::new(h.out.y, -h.out.x)) > 0.0);
    }
    // The lookout 4.2 m up the broken east wall can be hung from on its outer (east) face.
    assert!(land.holds.iter().any(|h| (h.a.y - (ruin.y + 4.2)).abs() < 0.01 && h.out.x > 0.9), "no hold under the lookout");
}

#[test]
fn rolling_at_an_edge_lowers_you_to_hang_and_rolling_again_lets_go() {
    let (mut sim, id, top) = on_a_tall_wall();
    let hang = hang_from_the_west_face(&mut sim, id);
    let hold = *sim.land().holds.get(hang.hold);
    assert!(hold.out.x < -0.99, "hung from the wrong face: {hold:?}");
    let t = *sim.get::<Transform>(id).unwrap();
    // Hands on the edge, feet below it, out from the wall by the body's width, facing the wall.
    assert!((t.pos.y - (top - HANG_DROP)).abs() < 0.02, "feet at {}, top {top}", t.pos.y);
    assert!((t.pos.x - (7.55 - 0.4)).abs() < 0.02, "hung at x = {}", t.pos.x);
    assert!((t.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "facing {}", t.yaw);
    assert_eq!(sim.get::<Anim>(id).unwrap().pose, Pose::Hang);
    let unit = |sim: &mut Sim| sim.snapshot(Some(id)).units.into_iter().find(|u| u.id == id).unwrap();
    assert_eq!(unit(&mut sim).traversal, Traversal::Hanging);
    // It holds: a second later, still there.
    sim.run_for(1.0);
    assert!((pos(&sim, id) - t.pos).length() < 1e-4);
    // Roll again: let go, drop to the road, unhurt.
    let health = sim.get::<Vitals>(id).unwrap().health;
    sim.command(id, Command::Dodge { direction: Vec2::ZERO });
    sim.run_for(1.0);
    let m = motion(&sim, id);
    assert!(m.hang.is_none() && m.grounded, "still on the wall: {m:?}");
    let p = pos(&sim, id);
    assert!((p.y - sim.land().ground(p.x, p.z)).abs() < 0.05 && p.x < 7.3, "landed at {p}");
    assert_eq!(sim.get::<Vitals>(id).unwrap().health, health);
}

#[test]
fn rolling_along_an_edge_still_rolls() {
    for direction in [Vec2::Y, -Vec2::Y] {
        let (mut sim, id, _) = on_a_tall_wall();
        sim.command(id, Command::Dodge { direction });
        sim.step();
        let m = motion(&sim, id);
        assert!(m.hang.is_none() && m.forced.is_some_and(|f| f.kind == sim::ForcedKind::Dodge), "{m:?}");
    }
}

#[test]
fn shimmies_along_a_hold_and_stops_at_its_end() {
    let (mut sim, id, _) = on_a_tall_wall();
    let hang = hang_from_the_west_face(&mut sim, id);
    let start = pos(&sim, id);
    // Pressing north (along the wall): about 1.2 m a second.
    go(&mut sim, id, -Vec2::Y, false);
    sim.run_for(1.0);
    let moved = pos(&sim, id) - start;
    assert!((moved.z.abs() - SHIMMY_SPEED).abs() < 0.05 && moved.x.abs() < 1e-3 && moved.y.abs() < 1e-3, "shimmied {moved}");
    assert_eq!(motion(&sim, id).hang.unwrap().hold, hang.hold);
    // On to the north end, where the hands stop short of the corner.
    sim.run_for(4.0);
    let p = pos(&sim, id);
    assert!((p.z - 9.2).abs() < 0.05, "stopped at z = {}", p.z);
    assert!(motion(&sim, id).hang.is_some());
}

#[test]
fn shimmies_across_a_narrow_gap_to_the_next_hold_but_not_a_wide_one() {
    for (gap, crosses) in [(0.4, true), (1.0, false)] {
        let (mut sim, id, top) = on_a_tall_wall();
        // A second wall of the same height carrying on south of the first, past a gap.
        wall_to(&mut sim, Vec2::new(8.0, 17.0 + gap), Vec2::new(0.45, 2.0), top);
        let first = hang_from_the_west_face(&mut sim, id);
        go(&mut sim, id, Vec2::Y, false);
        sim.run_for(5.0);
        let hang = motion(&sim, id).hang.expect("fell off");
        let z = pos(&sim, id).z;
        if crosses {
            assert_ne!(hang.hold, first.hold, "stayed on the first wall, at z = {z}");
            assert!(z > 15.0 + gap, "only got to z = {z}");
        } else {
            assert_eq!(hang.hold, first.hold, "jumped a {gap} m gap");
            assert!((z - 14.8).abs() < 0.05, "stopped at z = {z}");
        }
    }
}

#[test]
fn climbs_up_onto_the_top_from_a_hang() {
    let (mut sim, id, top) = on_a_tall_wall();
    hang_from_the_west_face(&mut sim, id);
    // East is into the wall: up.
    go(&mut sim, id, Vec2::X, false);
    sim.step();
    assert!(motion(&sim, id).forced.is_some_and(|f| f.kind == sim::ForcedKind::Climb), "didn't start climbing");
    go(&mut sim, id, Vec2::ZERO, false);
    sim.run_for(1.0);
    let m = motion(&sim, id);
    let p = pos(&sim, id);
    assert!(m.grounded && m.hang.is_none(), "{m:?}");
    assert!((p.y - top).abs() < 0.02 && p.x > 7.55 && p.x < 8.45, "stood at {p}");
}

#[test]
fn climbs_hand_over_hand_between_holds_and_back_up() {
    let (mut sim, id, top) = on_a_tall_wall();
    // A sill 0.8 m below the top, standing 0.1 m proud of the west face.
    let sill = top - 0.8;
    let ground = sim.land().ground(7.5, 12.0);
    wall(&mut sim, Vec2::new(7.5, 12.0), Vec2::new(0.05, 3.0), sill - ground);
    let upper = hang_from_the_west_face(&mut sim, id);
    // West is away from the wall: down, to the sill.
    go(&mut sim, id, -Vec2::X, false);
    sim.run_for(0.6);
    let lower = motion(&sim, id).hang.expect("let go instead of climbing down");
    assert_ne!(lower.hold, upper.hold);
    assert!((sim.land().holds.get(lower.hold).a.y - sill).abs() < 0.01);
    assert!((pos(&sim, id).y - (sill - HANG_DROP)).abs() < 0.02);
    // Nothing further down: pressing on holds still.
    sim.run_for(0.6);
    assert_eq!(motion(&sim, id).hang, Some(lower));
    // Back up to the top hold, then onto the top.
    go(&mut sim, id, Vec2::X, false);
    sim.step();
    go(&mut sim, id, Vec2::ZERO, false);
    sim.run_for(0.6);
    assert_eq!(motion(&sim, id).hang.map(|h| h.hold), Some(upper.hold));
    go(&mut sim, id, Vec2::X, false);
    sim.step();
    go(&mut sim, id, Vec2::ZERO, false);
    sim.run_for(1.0);
    let p = pos(&sim, id);
    assert!(motion(&sim, id).grounded && (p.y - top).abs() < 0.02, "ended at {p}");
}

#[test]
fn shift_held_catches_the_next_hold_down_after_letting_go() {
    for shift in [true, false] {
        let (mut sim, id, top) = on_a_tall_wall();
        let sill = top - 0.8;
        let ground = sim.land().ground(7.5, 12.0);
        wall(&mut sim, Vec2::new(7.5, 12.0), Vec2::new(0.05, 3.0), sill - ground);
        let upper = hang_from_the_west_face(&mut sim, id);
        go(&mut sim, id, Vec2::ZERO, shift);
        sim.command(id, Command::Dodge { direction: Vec2::ZERO });
        sim.run_for(0.8);
        let m = motion(&sim, id);
        if shift {
            let hang = m.hang.expect("didn't catch the sill");
            assert_ne!(hang.hold, upper.hold);
            assert!((pos(&sim, id).y - (sill - HANG_DROP)).abs() < 0.02);
        } else {
            assert!(m.hang.is_none() && m.grounded, "caught a hold without Shift: {m:?}");
        }
    }
}

#[test]
fn a_jump_with_shift_held_grabs_a_hold_in_reach_and_without_it_does_not() {
    for shift in [true, false] {
        let mut sim = Sim::new(7);
        // A wall 4.5 m tall with a sill 3 m up: out of reach to pull up, in reach of a jump.
        wall(&mut sim, Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 4.5);
        let sill = wall(&mut sim, Vec2::new(7.5, 12.0), Vec2::new(0.05, 3.0), 3.0);
        let id = sim.spawn_player("Hero", Vec2::new(6.8, 12.0), std::f32::consts::FRAC_PI_2);
        sim.step();
        go(&mut sim, id, Vec2::X, shift);
        sim.command(id, Command::Jump);
        sim.run_for(0.8);
        let m = motion(&sim, id);
        if shift {
            assert!(m.hang.is_some(), "didn't grab the sill: {m:?} at {}", pos(&sim, id));
            assert!((pos(&sim, id).y - (sill - HANG_DROP)).abs() < 0.02);
        } else {
            assert!(m.hang.is_none() && m.grounded, "grabbed the sill without Shift");
        }
    }
}

#[test]
fn no_fighting_or_guarding_on_a_wall() {
    let (mut sim, id, _) = on_a_tall_wall();
    hang_from_the_west_face(&mut sim, id);
    sim.command(id, Command::UseSlot { slot: 1, ground: None });
    sim.command(id, Command::Guard(true));
    sim.step();
    let s = sim.snapshot(Some(id));
    let messages: Vec<&str> =
        s.events.iter().filter_map(|e| if let Event::Message { text } = e { Some(text.as_str()) } else { None }).collect();
    assert!(messages.iter().any(|m| m.contains("fight while climbing")), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("guard while climbing")), "{messages:?}");
    assert!(motion(&sim, id).hang.is_some());
}

#[test]
fn a_thin_low_wall_is_climbed_at_a_walk() {
    // #238: a 1 m wall only 0.4 m deep, across the road.
    let mut sim = Sim::new(7);
    let top = wall(&mut sim, Vec2::new(8.0, 12.0), Vec2::new(0.2, 2.0), 1.0);
    let id = sim.spawn_player("Hero", Vec2::new(5.0, 12.0), std::f32::consts::FRAC_PI_2);
    sim.step();
    sim.command(id, Command::Move { direction: Vec2::X, walk: true, sprint: false, face_yaw: None });
    let mut climbed = false;
    for _ in 0..240 {
        sim.step();
        climbed |= motion(&sim, id).forced.is_some_and(|f| f.kind == sim::ForcedKind::Climb);
        if pos(&sim, id).x > 8.6 {
            break;
        }
    }
    let p = pos(&sim, id);
    assert!(climbed && p.x > 8.0, "stuck at {p} (top {top})");
}
