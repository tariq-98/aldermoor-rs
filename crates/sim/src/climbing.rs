//! Climbing on handholds (docs/MOVEMENT.md, "Climbing"): hanging from a hold, shimmying along it
//! and round corners, climbing onto its top or to the next hold up or down, leaping and ejecting
//! off it, letting go, lowering yourself from an edge to hang, catching a hold in the air, and
//! running up walls. The holds are `world::Valley::holds`.

use std::ops::RangeInclusive;

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec3Swizzles};
use world::{Hold, Valley};

use crate::movement::{approach_angle, heading, ledge_top, CLIMB_HEIGHT, GRAVITY, MANTLE_FALLING, MANTLE_HEIGHT, MANTLE_TIME};
use crate::{Body, Forced, ForcedKind, Hang, Land, Motion, Transform, Unit, UnitKind, Vitals, DT};

/// Hanging puts the feet this far below the hands.
pub const HANG_DROP: f32 = 1.9;
/// The hands reach this far above the feet.
pub const GRAB_REACH: f32 = 2.1;
/// Shimmying speed along a hold.
pub const SHIMMY_SPEED: f32 = 1.2;
/// A shimmy carries on to the next hold when it starts within this gap of the end of this one.
pub const HOLD_GAP: f32 = 0.6;
/// Seconds to climb from a hang onto the top above.
pub const CLIMB_UP_TIME: f64 = 0.8;
/// The next hold up or down is reached hand over hand within this height.
pub const HOLD_CLIMB: f32 = 1.0;
/// Seconds to climb hand over hand to the next hold up or down.
pub const HOLD_CLIMB_TIME: f64 = 0.4;
/// Seconds to cross from the end of one hold to the next.
pub const HOLD_HOP_TIME: f64 = 0.3;
/// Seconds to lower yourself over an edge to hang from it.
pub const DROP_TO_HANG_TIME: f64 = 0.6;
/// Seconds to settle onto a hold caught in the air.
pub const CATCH_TIME: f64 = 0.2;
/// A hold is caught when the hands pass within this much of it.
pub const CATCH_REACH: f32 = 0.3;
/// The hands stop this far short of a hold's ends.
const END: f32 = 0.2;
/// A hold you let go of can't be caught again for this long.
const RELEASE: f64 = 0.4;
/// Holds that carry on from one another are within this much height of each other.
const STEP: f32 = 0.3;
/// A climb leap (Space while hanging, pressing into the wall or nothing) reaches a hold this much
/// higher and up to [`LEAP_SIDE`] to either side, in [`LEAP_TIME`] seconds.
pub const LEAP_REACH: RangeInclusive<f32> = 1.0..=1.8;
pub const LEAP_SIDE: f32 = 0.8;
pub const LEAP_TIME: f64 = 0.5;
/// A side eject (Space, pressing along the wall) reaches a hold between 0.8 m and this far along
/// it, up to [`EJECT_RISE`] up or down, in [`EJECT_TIME`] seconds, hopping [`EJECT_ARC`] high
/// on the way. With none there it jumps off that way, along the wall at `SIDE_EJECT` m/s.
pub const EJECT_REACH: f32 = 2.5;
pub const EJECT_RISE: f32 = 1.0;
pub const EJECT_TIME: f64 = 0.55;
const EJECT_ARC: f32 = 0.35;
const SIDE_EJECT: Vec2 = Vec2::new(4.0, 4.5);
/// A back eject (Space, pressing away from the wall) kicks off it at this speed out and up:
/// about 2.5 m out by the time the body is back at the height it left.
pub const BACK_EJECT: Vec2 = Vec2::new(5.0, 5.0);
/// After a leap or a jump off a wall, holds are caught for this long without Shift held.
pub const CATCH_WINDOW: f64 = 1.0;
/// A wall run carries the feet this far up the face, slowing, in [`WALL_RUN_TIME`] seconds: the
/// hands catch holds up to 4.2 m above where the feet set off.
pub const WALL_RUN_RISE: f32 = 1.8;
pub const WALL_RUN_TIME: f64 = 0.5;
/// A wall run starts with the face this close ahead of the body.
const WALL_AHEAD: f32 = 0.25;
/// Seconds to wrap round a corner onto the next hold.
pub const CORNER_TIME: f64 = 0.5;
/// A shimmy wraps round a corner onto a hold that starts within this of the end of this one.
const CORNER_GAP: f32 = 0.6;

/// Where a body of `radius` stands while hanging from `hold` with its hands `at` metres along: out
/// from the wall by its radius, the feet [`HANG_DROP`] below the hands or on the ground, if that's
/// higher.
pub fn hang_point(land: &Valley, hold: &Hold, at: f32, radius: f32) -> Vec3 {
    let hands = hold.at(at);
    let p = hands.xz() + hold.out * radius;
    Vec3::new(p.x, (hands.y - HANG_DROP).max(land.ground(p.x, p.y)), p.y)
}

/// `s` kept where the hands can go on a hold `len` long.
fn on_hold(s: f32, len: f32) -> f32 {
    s.clamp(END.min(len / 2.0), (len - END).max(len / 2.0))
}

/// Whether a body could hang from `hold` at `at`: nothing in the way, and the ground well below
/// the hands (otherwise it would just be standing).
fn hangable(land: &Valley, hold: &Hold, at: f32, radius: f32) -> bool {
    let spot = hang_point(land, hold, at, radius);
    clear(land, spot, radius) && land.ground(spot.x, spot.z) < hold.at(at).y - CLIMB_HEIGHT
}

/// Whether a body of `radius` hanging at `spot` is clear of obstacles, and of the faces it would
/// press into (another wall, across an inner corner).
fn clear(land: &Valley, spot: Vec3, radius: f32) -> bool {
    let r = radius * 0.9;
    !land.blocked(spot.xz(), r) && land.off_faces(spot.xz(), r, spot.y + CLIMB_HEIGHT) == spot.xz()
}

/// Starts a carry onto a hold, to hang from it as `onto` says.
fn to_hold(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, onto: Hang, seconds: f64) {
    let Hang { hold, at } = onto;
    let h = land.holds.get(hold);
    m.hang = None;
    m.forced = Some(Forced { kind: ForcedKind::ToHold { hold, at }, from: t.pos, to: hang_point(land, h, at, radius), start: now, duration: seconds, arc: 0.0 });
    m.vel = Vec3::ZERO;
    m.grounded = false;
    m.sprinting = false;
}

/// Where a carry onto a hold has got to, `k` of the way through: over the edge before dropping
/// when lowering yourself, up before across when climbing, so the body never cuts into the wall,
/// with a hop of `arc` in the middle (an eject's).
pub fn carry(f: &Forced, k: f32) -> Vec3 {
    let (xz, y) = if f.to.y < f.from.y - 0.5 {
        (ease((k * 1.6).min(1.0)), k * k)
    } else if f.to.y > f.from.y + 0.3 {
        (k * k, ease((k * 1.6).min(1.0)))
    } else {
        (ease(k), ease(k))
    };
    let p = f.from.lerp(f.to, xz);
    Vec3::new(p.x, f.from.y + (f.to.y - f.from.y) * y + (k * std::f32::consts::PI).sin() * f.arc, p.z)
}

/// Easing out: quick to start, slowing to a stop.
fn ease(k: f32) -> f32 {
    1.0 - (1.0 - k) * (1.0 - k)
}

/// Where a wrap round a corner has got to, `k` of the way through: swinging round the corner at
/// `pivot`, so the body keeps clear of it.
pub fn corner_carry(f: &Forced, pivot: Vec2, k: f32) -> Vec3 {
    let (r0, r1) = (f.from.xz() - pivot, f.to.xz() - pivot);
    let a0 = r0.y.atan2(r0.x);
    let turn = crate::movement::wrap_angle(r1.y.atan2(r1.x) - a0);
    let e = ease(k);
    let (a, r) = (a0 + turn * e, r0.length() + (r1.length() - r0.length()) * e);
    let p = pivot + Vec2::new(a.cos(), a.sin()) * r;
    Vec3::new(p.x, f.from.y + (f.to.y - f.from.y) * e, p.y)
}

/// The hang a carry onto a hold ends in, if `kind` is one.
pub fn onto(kind: ForcedKind) -> Option<Hang> {
    match kind {
        ForcedKind::ToHold { hold, at } | ForcedKind::Corner { hold, at, .. } => Some(Hang { hold, at }),
        _ => None,
    }
}

/// The top to stand on above the hands, if there's room for the body: the point to climb to.
/// Not against a face too tall to step onto: a ledge with the next one overhanging it, too narrow
/// to stand on, is only hung from.
fn room_on_top(land: &Valley, hold: &Hold, at: f32, radius: f32) -> Option<Vec3> {
    let hands = hold.at(at);
    // Deep tops take the whole body; a thin wall's top is stood on in its middle.
    [radius + 0.1, 0.3, 0.15].into_iter().find_map(|d| {
        let p = hands.xz() - hold.out * d;
        let top = land.ground(p.x, p.y);
        let r = radius * 0.9;
        ((top - hands.y).abs() < 0.3 && !land.blocked(p, r) && land.off_faces(p, r, top + CLIMB_HEIGHT) == p).then(|| Vec3::new(p.x, top, p.y))
    })
}

/// The nearest other hold the hands could reach `rise` metres up or down (within the range) and
/// up to `side` to either side, on the same face: its index and where along it.
fn hold_beside(land: &Valley, current: u32, at: f32, rise: RangeInclusive<f32>, side: f32, radius: f32) -> Option<(u32, f32)> {
    let hold = land.holds.get(current);
    let hands = hold.at(at);
    let mut best: Option<(f32, u32, f32)> = None;
    for i in land.holds.near(hands.xz(), side + 0.3) {
        let h = land.holds.get(i);
        if i == current || h.out.dot(hold.out) < 0.7 {
            continue;
        }
        let (s, off) = h.locate(hands.xz());
        let to = on_hold(s, h.len());
        let dy = h.at(to).y - hands.y;
        if (s - to).abs() > side || off.abs() > 0.5 || !rise.contains(&dy) || !hangable(land, h, to, radius) {
            continue;
        }
        if best.is_none_or(|b| dy.abs() < b.0) {
            best = Some((dy.abs(), i, to));
        }
    }
    best.map(|(_, i, s)| (i, s))
}

/// The hold a shimmy carries on to past the `end` of `current` (1.0 for its `b` end, -1.0 for
/// its `a` end): one starting within [`HOLD_GAP`], on the same face and at about the same height.
fn next_along(land: &Valley, current: u32, end: f32, radius: f32) -> Option<(u32, f32)> {
    let hold = land.holds.get(current);
    let tip = if end > 0.0 { hold.b } else { hold.a };
    let mut best: Option<(f32, u32, f32)> = None;
    for i in land.holds.near(tip.xz(), HOLD_GAP) {
        let h = land.holds.get(i);
        if i == current || h.out.dot(hold.out) < 0.9 || h.along().dot(hold.along()) < 0.9 {
            continue;
        }
        let (s, _) = h.locate(tip.xz());
        // It has to carry on the way the shimmy goes, not double back over this hold.
        let to = on_hold(if end > 0.0 { s.max(0.0) } else { s.min(h.len()) }, h.len());
        let gap = (h.at(to).xz() - tip.xz()).length();
        if (h.at(to).y - tip.y).abs() > STEP || !hangable(land, h, to, radius) {
            continue;
        }
        if best.is_none_or(|b| gap < b.0) {
            best = Some((gap, i, to));
        }
    }
    best.map(|(_, i, s)| (i, s))
}

/// The hold round the corner past the `end` of `current` (1.0 for its `b` end, -1.0 for its `a`
/// end): one at a right angle to it starting within [`CORNER_GAP`] of that end, round an outer
/// corner or into an inner one, at about the same height. Its index, where along it to hang, and
/// the corner, where the two holds' lines meet.
fn round_corner(land: &Valley, current: u32, end: f32, radius: f32) -> Option<(u32, f32, Vec2)> {
    let hold = land.holds.get(current);
    let (tip, along) = (if end > 0.0 { hold.b } else { hold.a }, hold.along() * end);
    let mut best: Option<(f32, u32, f32, Vec2)> = None;
    for i in land.holds.near(tip.xz(), CORNER_GAP) {
        let h = land.holds.get(i);
        // Its face turned a quarter either way from this one's; going right, a climber comes onto
        // the next hold's `a` end round either kind of corner, and going left its `b` end.
        if i == current || h.out.dot(along).abs() < 0.9 {
            continue;
        }
        let (start, s) = if end > 0.0 { (h.a, 0.0) } else { (h.b, h.len()) };
        let gap = (start.xz() - tip.xz()).length();
        if gap > CORNER_GAP || (start.y - tip.y).abs() > STEP {
            continue;
        }
        // As near its end as the body fits: further along, into an inner corner, to keep clear
        // of the wall just left.
        let Some(at) = [0.0, 0.2, 0.4, 0.6].map(|d| on_hold(s + d * end, h.len())).into_iter().find(|&at| hangable(land, h, at, radius)) else { continue };
        if best.is_none_or(|b| gap < b.0) {
            best = Some((gap, i, at, tip.xz() + along * (start.xz() - tip.xz()).dot(along)));
        }
    }
    best.map(|(_, i, at, corner)| (i, at, corner))
}

/// A step hanging from a hold. The direction pressed, against the wall, picks the move: along it
/// shimmies (on to the next hold past the end, or round the corner), into it climbs onto the top
/// or to the next hold up, and away from it climbs down to the next hold below; with Space, it
/// leaps or ejects instead ([`jump_off`]). Stunned or rooted, the hands just hold on. If the
/// ground comes up to the feet (shimmying along a wall that climbs a slope), the body stands on
/// it.
pub fn hang_step(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, can_move: bool) {
    let Some(mut hang) = m.hang else { return };
    let start = t.pos;
    m.vel = Vec3::ZERO;
    m.grounded = false;
    m.swimming = false;
    m.sprinting = false;
    let hold = *land.holds.get(hang.hold);
    let mut dir = if can_move { m.intent.direction } else { Vec2::ZERO };
    if dir.length_squared() > 1.0 {
        dir = dir.normalize();
    }
    if now <= m.jump_until {
        m.jump_until = f64::NEG_INFINITY;
        if can_move && jump_off(land, now, t, m, radius, hang, dir) {
            m.observed = Vec3::ZERO;
            return;
        }
    }
    let along = dir.dot(hold.along());
    let up = dir.dot(-hold.out);
    if up.abs() > along.abs() && up.abs() > 0.4 {
        if up > 0.0 && let Some(stand) = room_on_top(land, &hold, hang.at, radius) {
            m.hang = None;
            m.forced = Some(Forced { kind: ForcedKind::Climb, from: t.pos, to: stand, start: now, duration: CLIMB_UP_TIME, arc: 0.0 });
            m.observed = Vec3::ZERO;
            return;
        }
        let reach = if up > 0.0 { 0.3..=HOLD_CLIMB } else { -HOLD_CLIMB..=-0.3 };
        if let Some((i, at)) = hold_beside(land, hang.hold, hang.at, reach, 0.5, radius) {
            to_hold(land, now, t, m, radius, Hang { hold: i, at }, HOLD_CLIMB_TIME);
            m.observed = Vec3::ZERO;
            return;
        }
    } else if along.abs() > 0.2 {
        let len = hold.len();
        let at = hang.at + along.signum() * SHIMMY_SPEED * along.abs().min(1.0) * DT;
        let moved = Hang { hold: hang.hold, at: on_hold(at, len) };
        let blocked = !clear(land, hang_point(land, &hold, moved.at, radius), radius);
        let end = at < END || at > len - END;
        // At the end of the hold: across to the next one, if it starts close enough. At its end
        // or up against a wall across it: round the corner.
        if end && !blocked && let Some((i, s)) = next_along(land, hang.hold, along.signum(), radius) {
            to_hold(land, now, t, m, radius, Hang { hold: i, at: s }, HOLD_HOP_TIME);
            m.observed = Vec3::ZERO;
            return;
        }
        if (end || blocked) && let Some((i, s, corner)) = round_corner(land, hang.hold, along.signum(), radius) {
            let to = hang_point(land, land.holds.get(i), s, radius);
            m.hang = None;
            m.forced = Some(Forced { kind: ForcedKind::Corner { hold: i, at: s, corner }, from: t.pos, to, start: now, duration: CORNER_TIME, arc: 0.0 });
            m.observed = Vec3::ZERO;
            return;
        }
        if !blocked {
            hang = moved;
        }
    }
    m.hang = Some(hang);
    let hold = land.holds.get(hang.hold);
    t.pos = hang_point(land, hold, hang.at, radius);
    t.yaw = heading(-hold.out);
    let ground = land.ground(t.pos.x, t.pos.z);
    if ground >= hold.at(hang.at).y - CLIMB_HEIGHT {
        m.hang = None;
        m.grounded = true;
        t.pos.y = ground;
    }
    m.fall_from = t.pos.y;
    m.observed = (t.pos - start) / DT;
}

/// Space pressed while hanging, with `dir` pressed: into the wall or nothing, a climb leap to a
/// hold in reach above; along it, a side eject, to a hold that way or off into the air; away
/// from it, a back eject, kicking off the wall to catch whatever is behind. Says whether it left
/// the hold (a climb leap with nothing in reach doesn't).
fn jump_off(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, hang: Hang, dir: Vec2) -> bool {
    let hold = *land.holds.get(hang.hold);
    let (along, up) = (dir.dot(hold.along()), dir.dot(-hold.out));
    if dir.length_squared() < 0.04 || up >= along.abs() {
        let Some((i, at)) = hold_beside(land, hang.hold, hang.at, LEAP_REACH, LEAP_SIDE, radius) else { return false };
        to_hold(land, now, t, m, radius, Hang { hold: i, at }, LEAP_TIME);
    } else if -up > along.abs() {
        let_go(land, now, m);
        m.vel = Vec3::new(hold.out.x * BACK_EJECT.x, BACK_EJECT.y, hold.out.y * BACK_EJECT.x);
    } else if let Some((i, at)) = eject_target(land, hang, along.signum(), radius) {
        to_hold(land, now, t, m, radius, Hang { hold: i, at }, EJECT_TIME);
        if let Some(f) = &mut m.forced {
            f.arc = EJECT_ARC;
        }
    } else {
        // Nothing to land on that way: off the wall into the air.
        let_go(land, now, m);
        let a = hold.along() * along.signum() * SIDE_EJECT.x + hold.out;
        m.vel = Vec3::new(a.x, SIDE_EJECT.y, a.y);
    }
    m.catch_until = now + CATCH_WINDOW;
    true
}

/// The hold a side eject lands on, `side` (1.0 toward the hold's `b` end, -1.0 toward its `a`
/// end) of the hands: on the same face, between 0.8 m and [`EJECT_REACH`] that way and up to
/// [`EJECT_RISE`] up or down, the one nearest the point 2 m along (which may be further along
/// the same hold). Its index and where along it.
fn eject_target(land: &Valley, hang: Hang, side: f32, radius: f32) -> Option<(u32, f32)> {
    let hold = land.holds.get(hang.hold);
    let hands = hold.at(hang.at);
    let aim = hands.xz() + hold.along() * side * 2.0;
    let mut best: Option<(f32, u32, f32)> = None;
    for i in land.holds.near(aim, 1.2) {
        let h = land.holds.get(i);
        if h.out.dot(hold.out) < 0.7 {
            continue;
        }
        let (s, off) = h.locate(aim);
        let at = on_hold(s, h.len());
        let p = h.at(at);
        let reach = (p.xz() - hands.xz()).dot(hold.along()) * side;
        if off.abs() > 0.5 || !(0.8..=EJECT_REACH).contains(&reach) || (p.y - hands.y).abs() > EJECT_RISE || !hangable(land, h, at, radius) {
            continue;
        }
        let miss = (p.xz() - aim).length();
        if best.is_none_or(|b| miss < b.0) {
            best = Some((miss, i, at));
        }
    }
    best.map(|(_, i, at)| (i, at))
}

/// Lets go of the hold: the body drops, pushing off the wall a little.
pub fn let_go(land: &Valley, now: f64, m: &mut Motion) {
    let Some(hang) = m.hang.take() else { return };
    let out = land.holds.get(hang.hold).out;
    m.released = Some((hang.hold, now + RELEASE));
    m.grounded = false;
    m.coyote_until = f64::NEG_INFINITY;
    m.vel = Vec3::new(out.x, 0.0, out.y);
}

/// Thrown off the wall by a blow (`delta` over `seconds`, as a knockback would carry a standing
/// body): the hands lose the hold and the body falls away from the wall.
pub fn knock_off(land: &Valley, now: f64, m: &mut Motion, delta: Vec2, seconds: f64) {
    let_go(land, now, m);
    let v = delta / seconds.max(0.1) as f32;
    m.vel = Vec3::new(v.x, 2.0, v.y);
}

/// Grabs a hold the hands passed this step, falling or jumping (`before` is where the feet were
/// at the start of the step): the nearest one in front of the body with room to hang below it.
/// Starts the carry onto it and says whether it caught one.
pub fn catch(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, before: f32) -> bool {
    let p = t.pos.xz();
    let (hands, was) = (t.pos.y + GRAB_REACH, before + GRAB_REACH);
    let (lo, hi) = (hands.min(was) - CATCH_REACH, hands.max(was) + CATCH_REACH);
    let mut best: Option<(f32, u32, f32)> = None;
    for i in land.holds.near(p, radius + 0.6) {
        if m.released.is_some_and(|(h, until)| h == i && now < until) {
            continue;
        }
        let h = land.holds.get(i);
        let (s, off) = h.locate(p);
        if s < -0.2 || s > h.len() + 0.2 || !(-0.05..=radius + 0.6).contains(&off) {
            continue;
        }
        let at = on_hold(s, h.len());
        let y = h.at(at).y;
        if y < lo || y > hi || !hangable(land, h, at, radius) {
            continue;
        }
        // Heading into the face of a top in reach of this jump (by its height) with room on it:
        // pulled straight up onto it (a mantle) rather than hanging.
        let into = Vec2::new(m.vel.x, m.vel.z).dot(-h.out);
        let apex = t.pos.y + m.vel.y.max(0.0).powi(2) / (2.0 * GRAVITY);
        if into > 1.0 && m.vel.y > MANTLE_FALLING && y - apex <= MANTLE_HEIGHT && room_on_top(land, h, at, radius).is_some() {
            continue;
        }
        let score = (off - radius).abs() + (y - hands).abs();
        if best.is_none_or(|b| score < b.0) {
            best = Some((score, i, at));
        }
    }
    let Some((_, i, at)) = best else { return false };
    to_hold(land, now, t, m, radius, Hang { hold: i, at }, CATCH_TIME);
    true
}

/// Lowers a body standing at an edge over it, to hang from the hold there: Roll pressed toward the
/// drop, or with no direction while backing onto it. Says whether it did.
pub fn drop_to_hang(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, direction: Vec2) -> bool {
    if !m.grounded || m.swimming || m.forced.is_some() || m.hang.is_some() {
        return false;
    }
    let toward = if direction.length_squared() > 0.01 { direction.normalize() } else { -t.forward() };
    let p = t.pos.xz();
    let mut best: Option<(f32, u32, f32)> = None;
    for i in land.holds.near(p, 1.2) {
        let h = land.holds.get(i);
        let (s, off) = h.locate(p);
        if s < 0.0 || s > h.len() || !(-1.0..=0.1).contains(&off) || toward.dot(h.out) < 0.3 {
            continue;
        }
        let at = on_hold(s, h.len());
        if (h.at(at).y - t.pos.y).abs() > 0.35 || !hangable(land, h, at, radius) {
            continue;
        }
        if best.is_none_or(|b| -off < b.0) {
            best = Some((-off, i, at));
        }
    }
    let Some((_, i, at)) = best else { return false };
    to_hold(land, now, t, m, radius, Hang { hold: i, at }, DROP_TO_HANG_TIME);
    true
}

/// Roll pressed by a climber: hanging, it lets go; standing at an edge, rolling toward the drop
/// lowers you to hang from it; in the air, it readies a roll landing
/// ([`crate::movement::LANDING_ROLL`]). Says whether the roll was used up so (and shouldn't roll).
pub fn roll(world: &mut World, e: Entity, direction: Vec2, now: f64, can_move: bool) -> bool {
    let land = world.resource::<Land>().0.clone();
    let player = world.get::<Unit>(e).is_some_and(|u| u.kind == UnitKind::Player);
    let downed = world.get::<Vitals>(e).is_none_or(|v| v.downed || v.dead);
    let radius = world.get::<Body>(e).map_or(0.4, |b| b.radius);
    let mut q = world.query::<(&mut Transform, &mut Motion)>();
    let Ok((mut t, mut m)) = q.get_mut(world, e) else { return false };
    if m.hang.is_some() {
        let_go(&land, now, &mut m);
        return true;
    }
    if !m.grounded && !m.swimming && m.forced.is_none() {
        if can_move && !downed {
            m.landing_roll_until = now + crate::movement::LANDING_ROLL;
        }
        return true;
    }
    player && can_move && !downed && drop_to_hang(&land, now, &mut t, &mut m, radius, direction)
}

/// Sprinting along `dir` into a wall too tall to climb (`world::Valley::wall_ahead`), straight
/// enough at it: runs up it, a forced wall run ([`wall_run_step`]). Says whether it started.
pub fn wall_run(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, dir: Vec2) -> bool {
    let dir = dir.normalize_or_zero();
    let p = t.pos.xz();
    let Some((normal, gap)) = land.wall_ahead(p, dir, radius, WALL_AHEAD, t.pos.y + CLIMB_HEIGHT) else { return false };
    if dir.dot(-normal) < 0.6 {
        return false;
    }
    // Up against it, then up it.
    let foot = p - normal * (gap - 0.01).max(0.0);
    let from = Vec3::new(foot.x, t.pos.y, foot.y);
    m.forced = Some(Forced { kind: ForcedKind::WallRun { away: normal }, from, to: from + Vec3::Y * WALL_RUN_RISE, start: now, duration: WALL_RUN_TIME, arc: 0.0 });
    m.vel = Vec3::ZERO;
    m.grounded = false;
    m.sprinting = false;
    true
}

/// A step of the wall run `f`: the feet run up the face, slowing. A top in reach of the hands with
/// room on it is pulled straight up onto (a mantle); otherwise the hands catch a hold they pass;
/// Space kicks off the wall backwards, as from a hang. At the end, with nothing caught, the body
/// drops back off.
pub fn wall_run_step(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, f: Forced) {
    let ForcedKind::WallRun { away } = f.kind else { return };
    let k = ((now - f.start) / f.duration).clamp(0.0, 1.0) as f32;
    let before = t.pos.y;
    t.pos = Vec3::new(f.from.x, f.from.y + (f.to.y - f.from.y) * ease(k), f.from.z);
    t.yaw = approach_angle(t.yaw, heading(-away), DT * 14.0);
    m.fall_from = t.pos.y;
    if now <= m.jump_until {
        m.jump_until = f64::NEG_INFINITY;
        m.forced = None;
        m.vel = Vec3::new(away.x * BACK_EJECT.x, BACK_EJECT.y, away.y * BACK_EJECT.x);
        m.catch_until = now + CATCH_WINDOW;
        return;
    }
    // A building's side has no top to pull up onto (nor should one be found through it).
    let obstacle = land.blocked(t.pos.xz() - away * (radius + 0.1), 0.05);
    let top = (!obstacle).then(|| ledge_top(land, t.pos.xz(), t.pos.y, -away, radius, GRAB_REACH + CATCH_REACH)).flatten();
    if let Some(top) = top {
        m.forced = Some(Forced { kind: ForcedKind::Mantle, from: t.pos, to: top, start: now, duration: MANTLE_TIME, arc: 0.0 });
        return;
    }
    if catch(land, now, t, m, radius, before) || k < 1.0 {
        return;
    }
    m.forced = None;
    m.vel = Vec3::new(away.x * 1.5, 0.0, away.y * 1.5);
}
