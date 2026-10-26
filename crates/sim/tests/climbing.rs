//! Climbing on handholds (docs/MOVEMENT.md, "Climbing"): hanging, shimmying and wrapping round
//! corners, climbing up and down between holds and onto tops, leaping and ejecting, letting go,
//! lowering yourself from an edge, catching a hold, running up walls and rolling a landing.

use std::f32::consts::{FRAC_PI_2, PI};

use glam::{Vec2, Vec3};
use protocol::{Command, Event, Pose, Traversal};
use sim::climbing::{CORNER_TIME, EJECT_TIME, HANG_DROP, LEAP_TIME, SHIMMY_SPEED};
use sim::{Anim, ForcedKind, Hang, Motion, Sim, Transform, Vitals};

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

/// Puts `id` hanging from the hold `y` high on the face looking along `out`, its hands as near
/// `near` as they go.
fn hang_on(sim: &mut Sim, id: u32, near: Vec2, y: f32, out: Vec2) -> Hang {
    let land = sim.land();
    let (i, h) = land.holds.iter().enumerate().find(|(_, h)| (h.a.y - y).abs() < 0.01 && h.out.dot(out) > 0.99).expect("no such hold");
    let at = h.locate(near).0.clamp(0.2, h.len() - 0.2);
    let spot = sim::climbing::hang_point(land, h, at, 0.4);
    let hang = Hang { hold: i as u32, at };
    {
        let mut m = sim.get_mut::<Motion>(id).unwrap();
        m.hang = Some(hang);
        m.grounded = false;
        m.vel = Vec3::ZERO;
    }
    {
        let mut t = sim.get_mut::<Transform>(id).unwrap();
        t.pos = spot;
        t.yaw = (-out.x).atan2(-out.y);
    }
    sim.step();
    hang
}

/// What others see `id` doing.
fn traversal(sim: &mut Sim, id: u32) -> Traversal {
    sim.snapshot(Some(id)).units.into_iter().find(|u| u.id == id).unwrap().traversal
}

/// Steps until `id` starts a forced move of the kind `is` picks (up to `seconds`); whether it did.
fn until(sim: &mut Sim, id: u32, seconds: f32, is: impl Fn(&ForcedKind) -> bool) -> bool {
    for _ in 0..(seconds * 60.0) as u32 {
        sim.step();
        if motion(sim, id).forced.is_some_and(|f| is(&f.kind)) {
            return true;
        }
    }
    false
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
    let (mut sim, id, top) = on_a_tall_wall();
    // A taller wall against its north end, so there's no going round the corner there.
    wall_to(&mut sim, Vec2::new(8.0, 8.0), Vec2::new(0.45, 1.0), top + 1.0);
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
            // Round the first wall's south end instead.
            let h = sim.land().holds.get(hang.hold);
            assert!(h.a.z.max(h.b.z) < 15.01, "crossed a {gap} m gap onto {h:?}");
            assert!(z < 15.5, "got to z = {z}");
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

#[test]
fn shimmies_round_an_outer_corner_onto_the_walls_end() {
    let (mut sim, id, _) = on_a_tall_wall();
    hang_from_the_west_face(&mut sim, id);
    // North along the west face to its end, and on round the corner.
    go(&mut sim, id, -Vec2::Y, false);
    assert!(until(&mut sim, id, 4.0, |k| matches!(k, ForcedKind::Corner { .. })), "never went round, stopped at {}", pos(&sim, id));
    assert!(matches!(traversal(&mut sim, id), Traversal::Forced(f) if f.kind == protocol::ForcedKind::Corner));
    go(&mut sim, id, Vec2::ZERO, false);
    // Swinging round it, clear of the corner.
    let corner = Vec2::new(7.55, 9.0);
    for _ in 0..(CORNER_TIME * 60.0) as u32 + 6 {
        sim.step();
        let p = pos(&sim, id);
        assert!((Vec2::new(p.x, p.z) - corner).length() > 0.39, "cut the corner at {p}");
    }
    // Hanging from the wall's north end, facing south into it.
    let hang = motion(&sim, id).hang.expect("fell off the corner");
    assert!(sim.land().holds.get(hang.hold).out.y < -0.99);
    let t = *sim.get::<Transform>(id).unwrap();
    assert!((t.pos.x - 7.75).abs() < 0.02 && (t.pos.z - 8.6).abs() < 0.02, "hung at {}", t.pos);
    assert!(t.yaw.abs() < 0.05, "facing {}", t.yaw);
}

#[test]
fn shimmies_into_an_inner_corner_onto_the_wall_across_it() {
    let (mut sim, id, top) = on_a_tall_wall();
    // A wall of the same height running west from the north end: its south face meets the west
    // face at z = 9.8.
    wall_to(&mut sim, Vec2::new(6.55, 9.4), Vec2::new(1.0, 0.4), top);
    hang_from_the_west_face(&mut sim, id);
    go(&mut sim, id, -Vec2::Y, false);
    assert!(until(&mut sim, id, 4.0, |k| matches!(k, ForcedKind::Corner { .. })), "never went round, stopped at {}", pos(&sim, id));
    go(&mut sim, id, Vec2::ZERO, false);
    sim.run_for(CORNER_TIME as f32 + 0.1);
    // On the other wall's south face, facing north into it, clear of the first.
    let hang = motion(&sim, id).hang.expect("fell out of the corner");
    assert!(sim.land().holds.get(hang.hold).out.y > 0.99);
    let t = *sim.get::<Transform>(id).unwrap();
    assert!(t.pos.x < 7.55 - 0.36 && (t.pos.z - 10.2).abs() < 0.02, "hung at {}", t.pos);
    assert!((t.yaw.abs() - PI).abs() < 0.05, "facing {}", t.yaw);
}

#[test]
fn a_climb_leap_reaches_a_hold_up_to_1_8_m_higher() {
    for (below, leaps) in [(1.5, true), (2.2, false)] {
        // A wall 5 m tall with a sill `below` its top on the west face, to hang from.
        let mut sim = Sim::new(7);
        let top = wall(&mut sim, Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 5.0);
        let sill = top - below;
        wall_to(&mut sim, Vec2::new(7.5, 12.0), Vec2::new(0.05, 3.0), sill);
        let id = sim.spawn_player("Hero", Vec2::new(6.0, 12.0), FRAC_PI_2);
        sim.step();
        let start = hang_on(&mut sim, id, Vec2::new(7.45, 12.0), sill, -Vec2::X);
        // Into the wall alone climbs no more than a metre.
        go(&mut sim, id, Vec2::X, false);
        sim.run_for(0.5);
        assert_eq!(motion(&sim, id).hang, Some(start), "climbed {below} m hand over hand");
        sim.command(id, Command::Jump);
        sim.step();
        if leaps {
            assert!(motion(&sim, id).forced.is_some_and(|f| f.duration == LEAP_TIME), "{:?}", motion(&sim, id));
            go(&mut sim, id, Vec2::ZERO, false);
            sim.run_for(0.6);
            let hang = motion(&sim, id).hang.expect("fell");
            assert!((sim.land().holds.get(hang.hold).a.y - top).abs() < 0.01, "on the wrong hold");
            assert!((pos(&sim, id).y - (top - HANG_DROP)).abs() < 0.02);
        } else {
            assert_eq!(motion(&sim, id).hang, Some(start), "leapt {below} m");
        }
    }
}

#[test]
fn a_side_eject_reaches_a_hold_along_the_wall_or_jumps_off_that_way() {
    for gap in [1.2, 3.0] {
        let (mut sim, id, top) = on_a_tall_wall();
        // A second wall carrying on south past a gap too wide to shimmy across.
        wall_to(&mut sim, Vec2::new(8.0, 17.0 + gap), Vec2::new(0.45, 2.0), top);
        hang_on(&mut sim, id, Vec2::new(7.55, 14.3), top, -Vec2::X);
        go(&mut sim, id, Vec2::Y, false);
        sim.command(id, Command::Jump);
        sim.step();
        let m = motion(&sim, id);
        if gap < 2.0 {
            assert!(m.forced.is_some_and(|f| f.duration == EJECT_TIME), "{m:?}");
            go(&mut sim, id, Vec2::ZERO, false);
            sim.run_for(0.6);
            let hang = motion(&sim, id).hang.expect("fell");
            let h = *sim.land().holds.get(hang.hold);
            assert!(h.a.z > 15.0 + gap - 0.01 && (h.a.y - top).abs() < 0.01, "landed on {h:?}");
        } else {
            // Nothing in reach: off the wall, along it.
            assert!(m.hang.is_none() && m.forced.is_none() && m.vel.z > 3.0 && m.vel.y > 3.0, "{m:?}");
            for _ in 0..120 {
                sim.step();
                if motion(&sim, id).grounded {
                    break;
                }
            }
            let m = motion(&sim, id);
            assert!(m.grounded && m.hang.is_none(), "{m:?}");
            assert!(pos(&sim, id).z > 16.0, "landed at {}", pos(&sim, id));
        }
    }
}

#[test]
fn a_back_eject_kicks_off_the_wall_to_catch_one_behind_without_shift() {
    let (mut sim, id, top) = on_a_tall_wall();
    // Across a 2.35 m lane to the west, a sheer wall with a sill as high as this wall's top.
    wall(&mut sim, Vec2::new(4.5, 12.0), Vec2::new(0.6, 3.0), 6.0);
    wall_to(&mut sim, Vec2::new(5.15, 12.0), Vec2::new(0.05, 3.0), top);
    hang_from_the_west_face(&mut sim, id);
    go(&mut sim, id, -Vec2::X, false);
    sim.command(id, Command::Jump);
    sim.step();
    let m = motion(&sim, id);
    assert!(m.hang.is_none() && m.vel.x < -4.0 && m.vel.y > 4.0, "{m:?}");
    assert!(m.catch_until > sim.time() + 0.9, "no window to catch in");
    sim.run_for(1.0);
    let hang = motion(&sim, id).hang.expect("didn't catch the sill behind");
    let h = *sim.land().holds.get(hang.hold);
    assert!(h.out.x > 0.99 && (h.a.y - top).abs() < 0.01, "caught {h:?}");
    let t = *sim.get::<Transform>(id).unwrap();
    assert!((t.pos.x - 5.6).abs() < 0.02, "hung at {}", t.pos);
    assert!((t.yaw + FRAC_PI_2).abs() < 0.05, "facing {}", t.yaw);
}

/// A sheer wall 6 m tall, 0.9 m deep and 6 m long by the field gate, its west face at x = 7.55,
/// with a sill standing 0.1 m proud of it `sill` metres up, if asked, and a player on the road
/// 3.5 m west of it, facing it. The sill's top.
fn before_a_sheer_wall(sill: Option<f32>) -> (Sim, u32, Option<f32>) {
    let mut sim = Sim::new(7);
    wall(&mut sim, Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 6.0);
    let top = sill.map(|s| wall(&mut sim, Vec2::new(7.5, 12.0), Vec2::new(0.05, 3.0), s));
    let id = sim.spawn_player("Hero", Vec2::new(4.0, 12.0), FRAC_PI_2);
    sim.step();
    (sim, id, top)
}

/// Sprints `id` along `dir` until it runs up a wall (up to two seconds), then lets go of the keys;
/// whether it did.
fn wall_run(sim: &mut Sim, id: u32, dir: Vec2) -> bool {
    go(sim, id, dir, true);
    let ran = until(sim, id, 2.0, |k| matches!(k, ForcedKind::WallRun { .. }));
    go(sim, id, Vec2::ZERO, false);
    ran
}

#[test]
fn a_wall_run_grabs_a_hold_up_to_4_2_m_above_the_feet() {
    for (height, grabs) in [(4.0, true), (4.6, false)] {
        let (mut sim, id, sill) = before_a_sheer_wall(Some(height));
        let health = sim.get::<Vitals>(id).unwrap().health;
        assert!(wall_run(&mut sim, id, Vec2::X), "never ran up the wall");
        sim.run_for(1.5);
        let m = motion(&sim, id);
        if grabs {
            let hang = m.hang.expect("didn't grab the sill");
            assert!((sim.land().holds.get(hang.hold).a.y - sill.unwrap()).abs() < 0.01);
        } else {
            // Back down at its foot, unhurt.
            assert!(m.hang.is_none() && m.grounded, "{m:?}");
            let p = pos(&sim, id);
            assert!(p.x > 6.0 && p.x < 7.06, "landed at {p}");
            assert_eq!(sim.get::<Vitals>(id).unwrap().health, health);
        }
    }
}

#[test]
fn a_wall_run_climbs_straight_onto_a_top_in_reach() {
    let mut sim = Sim::new(7);
    // 3.6 m: out of reach of a jump.
    let top = wall(&mut sim, Vec2::new(8.15, 12.0), Vec2::new(0.6, 3.0), 3.6);
    let id = sim.spawn_player("Hero", Vec2::new(4.0, 12.0), FRAC_PI_2);
    sim.step();
    assert!(wall_run(&mut sim, id, Vec2::X), "never ran up the wall");
    assert!(until(&mut sim, id, 0.6, |k| *k == ForcedKind::Mantle), "didn't pull up onto the top");
    sim.run_for(1.0);
    let p = pos(&sim, id);
    assert!(motion(&sim, id).grounded && (p.y - top).abs() < 0.05 && p.x > 7.55, "ended at {p}");
}

#[test]
fn space_on_a_wall_run_kicks_off_it_backwards() {
    let (mut sim, id, _) = before_a_sheer_wall(None);
    assert!(wall_run(&mut sim, id, Vec2::X), "never ran up the wall");
    sim.run_for(0.15);
    sim.command(id, Command::Jump);
    sim.step();
    let m = motion(&sim, id);
    assert!(m.forced.is_none() && m.vel.x < -4.0 && m.vel.y > 4.0, "{m:?}");
    assert!(m.catch_until > sim.time() + 0.9);
}

#[test]
fn a_wall_run_up_a_cottage_drops_back_off_and_none_starts_up_a_tree() {
    let mut sim = Sim::new(7);
    let land = sim.land();
    let cottage = land.solids[0];
    let tree = *land.solids.iter().find(|s| s.top.is_none() && s.half.x < 0.7).unwrap();
    // 3 m out from the middle of a side, facing it.
    let before = |s: &world::Solid| {
        let (sin, cos) = s.yaw.sin_cos();
        let turn = |v: Vec2| Vec2::new(cos * v.x + sin * v.y, -sin * v.x + cos * v.y);
        (s.center + turn(Vec2::new(0.0, -(s.half.y + 3.0))), turn(Vec2::Y))
    };
    let (at, into) = before(&cottage);
    let id = sim.spawn_player("Hero", at, into.x.atan2(into.y));
    sim.step();
    let health = sim.get::<Vitals>(id).unwrap().health;
    assert!(wall_run(&mut sim, id, into), "never ran up the cottage");
    sim.run_for(1.5);
    let m = motion(&sim, id);
    assert!(m.grounded && m.hang.is_none(), "{m:?}");
    assert_eq!(sim.get::<Vitals>(id).unwrap().health, health);
    let (at, into) = before(&tree);
    let id = sim.spawn_player("Hero", at, into.x.atan2(into.y));
    sim.step();
    assert!(!wall_run(&mut sim, id, into), "ran up a tree");
}

#[test]
fn without_shift_or_in_combat_a_tall_wall_stops_you_a_body_short() {
    for fighting in [false, true] {
        let (mut sim, id, _) = before_a_sheer_wall(None);
        if fighting {
            let now = sim.time();
            sim.get_mut::<Vitals>(id).unwrap().last_combat = now;
        }
        go(&mut sim, id, Vec2::X, fighting);
        for _ in 0..90 {
            sim.step();
            assert!(motion(&sim, id).forced.is_none(), "{:?}", motion(&sim, id));
        }
        // #216: the body stops against the face, not in it.
        let p = pos(&sim, id);
        assert!(p.x > 7.1 && p.x <= 7.15 + 1e-3, "stopped at {p}");
    }
}

#[test]
fn a_jump_at_a_top_in_reach_pulls_up_onto_it_even_with_shift_held() {
    for shift in [true, false] {
        let mut sim = Sim::new(7);
        // 3.1 m: a hold just in reach of the hands at the top of a jump, and the top in reach to
        // pull straight up onto.
        let top = wall(&mut sim, Vec2::new(8.15, 12.0), Vec2::new(0.6, 3.0), 3.1);
        let id = sim.spawn_player("Hero", Vec2::new(4.0, 12.0), FRAC_PI_2);
        sim.step();
        go(&mut sim, id, Vec2::X, shift);
        while pos(&sim, id).x < 5.5 {
            sim.step();
        }
        sim.command(id, Command::Jump);
        let mut kinds = Vec::new();
        for _ in 0..90 {
            sim.step();
            if let Some(f) = motion(&sim, id).forced
                && kinds.last() != Some(&f.kind)
            {
                kinds.push(f.kind);
            }
        }
        assert_eq!(kinds.first(), Some(&ForcedKind::Mantle), "shift {shift}: {kinds:?}");
        let p = pos(&sim, id);
        assert!((p.y - top).abs() < 0.05 || p.x > 8.75, "shift {shift}: not on top or past it: {p}");
    }
}

/// A player dropped from `height` metres onto the road, pressing Roll once it's `roll` metres
/// above the ground (if asked): the share of their health the fall cost, and whether they rolled.
fn roll_off(height: f32, roll: Option<f32>) -> (f32, bool) {
    let mut sim = Sim::new(7);
    let id = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), PI);
    sim.step();
    sim.get_mut::<Transform>(id).unwrap().pos.y += height;
    sim.get_mut::<Motion>(id).unwrap().grounded = false;
    let mut pressed = false;
    while !motion(&sim, id).grounded {
        let p = pos(&sim, id);
        if !pressed && roll.is_some_and(|r| p.y - sim.land().ground(p.x, p.z) < r) {
            sim.command(id, Command::Dodge { direction: Vec2::ZERO });
            pressed = true;
        }
        sim.step();
    }
    let rolled = motion(&sim, id).forced.is_some_and(|f| f.kind == ForcedKind::Dodge);
    sim.step();
    let v = sim.get::<Vitals>(id).unwrap();
    (1.0 - v.health / v.max_health, rolled)
}

#[test]
fn roll_just_before_landing_and_falls_up_to_8_m_dont_hurt() {
    let (lost, rolled) = roll_off(7.0, None);
    assert!((lost - 0.18).abs() < 0.02 && !rolled, "lost {lost} from 7 m");
    // About a tenth of a second before landing.
    let (lost, rolled) = roll_off(7.0, Some(1.5));
    assert!(lost == 0.0 && rolled, "lost {lost} from 7 m with a roll");
    let (lost, rolled) = roll_off(10.0, Some(1.5));
    assert!((lost - 0.12).abs() < 0.02 && rolled, "lost {lost} from 10 m with a roll");
    // Half a second early is too early.
    let (lost, rolled) = roll_off(7.0, Some(6.0));
    assert!((lost - 0.18).abs() < 0.02 && !rolled, "lost {lost} from 7 m with an early roll");
}

#[test]
fn killed_on_a_wall_the_body_drops_to_the_ground() {
    let (mut sim, id, _) = on_a_tall_wall();
    hang_from_the_west_face(&mut sim, id);
    {
        let mut v = sim.get_mut::<Vitals>(id).unwrap();
        v.health = 0.0;
        v.dead = true;
    }
    sim.run_for(1.0);
    let p = pos(&sim, id);
    assert!(motion(&sim, id).hang.is_none());
    assert!((p.y - sim.land().ground(p.x, p.z)).abs() < 0.02, "the body hangs in the air at {p}");
}
