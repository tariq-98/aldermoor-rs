//! Climbing on handholds (docs/MOVEMENT.md, "Climbing"): hanging from a hold, shimmying along it,
//! climbing onto its top or to the next hold up or down, letting go, lowering yourself from an
//! edge to hang, and catching a hold in the air. The holds are `world::Valley::holds`.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, Vec3Swizzles};
use world::{Hold, Valley};

use crate::movement::{heading, CLIMB_HEIGHT};
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
    !land.blocked(spot.xz(), radius * 0.9) && land.ground(spot.x, spot.z) < hold.at(at).y - CLIMB_HEIGHT
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
    t.yaw = heading(-h.out);
}

/// Where a carry onto a hold has got to, `k` of the way through: over the edge before dropping
/// when lowering yourself, up before across when climbing, so the body never cuts into the wall.
pub fn carry(f: &Forced, k: f32) -> Vec3 {
    let ease = |k: f32| 1.0 - (1.0 - k) * (1.0 - k);
    let (xz, y) = if f.to.y < f.from.y - 0.5 {
        (ease((k * 1.6).min(1.0)), k * k)
    } else if f.to.y > f.from.y + 0.3 {
        (k * k, ease((k * 1.6).min(1.0)))
    } else {
        (ease(k), ease(k))
    };
    let p = f.from.lerp(f.to, xz);
    Vec3::new(p.x, f.from.y + (f.to.y - f.from.y) * y, p.z)
}

/// The top to stand on above the hands, if there's room for the body: the point to climb to.
fn room_on_top(land: &Valley, hold: &Hold, at: f32, radius: f32) -> Option<Vec3> {
    let hands = hold.at(at);
    // Deep tops take the whole body; a thin wall's top is stood on in its middle.
    [radius + 0.1, 0.3, 0.15].into_iter().find_map(|d| {
        let p = hands.xz() - hold.out * d;
        let top = land.ground(p.x, p.y);
        ((top - hands.y).abs() < 0.3 && !land.blocked(p, radius * 0.9)).then(|| Vec3::new(p.x, top, p.y))
    })
}

/// The nearest other hold the hands could reach `rise` metres up or down (within the range), on
/// the same face: its index and where along it.
fn hold_beside(land: &Valley, current: u32, at: f32, rise: std::ops::RangeInclusive<f32>, radius: f32) -> Option<(u32, f32)> {
    let hold = land.holds.get(current);
    let hands = hold.at(at);
    let mut best: Option<(f32, u32, f32)> = None;
    for i in land.holds.near(hands.xz(), 0.8) {
        let h = land.holds.get(i);
        if i == current || h.out.dot(hold.out) < 0.7 {
            continue;
        }
        let (s, off) = h.locate(hands.xz());
        let to = on_hold(s, h.len());
        let dy = h.at(to).y - hands.y;
        if (s - to).abs() > 0.5 || off.abs() > 0.5 || !rise.contains(&dy) || !hangable(land, h, to, radius) {
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

/// A step hanging from a hold. The direction pressed, against the wall, picks the move: along it
/// shimmies (on to the next hold past the end), into it climbs onto the top or to the next hold
/// up, and away from it climbs down to the next hold below. Stunned or rooted, the hands just hold
/// on. If the ground comes up to the feet (shimmying along a wall that climbs a slope), the body
/// stands on it.
pub fn hang_step(land: &Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, can_move: bool) {
    let Some(mut hang) = m.hang else { return };
    let start = t.pos;
    m.vel = Vec3::ZERO;
    m.grounded = false;
    m.swimming = false;
    m.sprinting = false;
    // Jumping off a wall comes with leaps and ejects (#246).
    m.jump_until = f64::NEG_INFINITY;
    let hold = *land.holds.get(hang.hold);
    let mut dir = if can_move { m.intent.direction } else { Vec2::ZERO };
    if dir.length_squared() > 1.0 {
        dir = dir.normalize();
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
        if let Some((i, at)) = hold_beside(land, hang.hold, hang.at, reach, radius) {
            to_hold(land, now, t, m, radius, Hang { hold: i, at }, HOLD_CLIMB_TIME);
            m.observed = Vec3::ZERO;
            return;
        }
    } else if along.abs() > 0.2 {
        let len = hold.len();
        let at = hang.at + along.signum() * SHIMMY_SPEED * along.abs().min(1.0) * DT;
        if at < END || at > len - END {
            // At the end of the hold: across to the next one, if it starts close enough.
            if let Some((i, s)) = next_along(land, hang.hold, along.signum(), radius) {
                to_hold(land, now, t, m, radius, Hang { hold: i, at: s }, HOLD_HOP_TIME);
                m.observed = Vec3::ZERO;
                return;
            }
        }
        let moved = Hang { hold: hang.hold, at: on_hold(at, len) };
        let spot = hang_point(land, &hold, moved.at, radius);
        if !land.blocked(spot.xz(), radius * 0.9) {
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
/// lowers you to hang from it. Says whether the roll was used up so (and shouldn't roll).
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
    player && can_move && !downed && drop_to_hang(&land, now, &mut t, &mut m, radius, direction)
}
