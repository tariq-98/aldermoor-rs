use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Event, Pose};

use crate::combat::abilities::Arrivals;
use crate::{Anim, Body, Clock, Control, Fighter, Forced, ForcedKind, Land, Motion, Outbox, Transform, Unit, Vitals, DT};

pub const GRAVITY: f32 = 20.0;
/// About a 1.1 m jump, landing after 0.66 s.
pub const JUMP_SPEED: f32 = 6.6;
pub const WALK_SPEED: f32 = 2.5;
pub const DODGE_DISTANCE: f32 = 5.0;
pub const DODGE_DURATION: f64 = 0.45;
pub const DODGE_INVULNERABLE: f64 = 0.4;
pub const DODGE_COST: f32 = 50.0;
/// Downed: a crawl.
pub const CRAWL_SPEED: f32 = 1.0;
/// Backpedalling while strafing runs at this fraction of full speed.
const BACKPEDAL: f32 = 0.6;
/// The most a walker steps down without leaving the ground.
const STEP_DOWN: f32 = 0.6;
/// Sprinting (out of combat, on land) runs this much faster.
pub const SPRINT: f32 = 1.5;
/// Steeper than this (rise over run, about 63°) the ground is a wall you can't walk up.
pub const MAX_SLOPE: f32 = 2.0;
/// Walls up to about chest height are climbed over; taller ones stop you.
pub const CLIMB_HEIGHT: f32 = 1.5;
/// Seconds to pull up over a ledge.
pub const CLIMB_TIME: f64 = 0.6;
/// Deeper water than this is swum, and swimming stops once it's shallower than `WADE_OUT`.
pub const SWIM_DEPTH: f32 = 0.5;
const WADE_OUT: f32 = 0.4;
/// Swimmers float with their feet this far under the surface.
const SWIM_FLOAT: f32 = 0.5;
/// Swimming speed, and wading through shallower water, as fractions of running.
pub const SWIM: f32 = 0.45;
const WADE: f32 = 0.7;
/// Falls up to this many metres don't hurt; past it each metre costs a share of max health.
pub const SAFE_FALL: f32 = 4.0;
pub const FALL_DAMAGE: f32 = 0.06;

/// Landings and splashes this step, for [`after_moving`] to deal with.
#[derive(Resource, Default)]
pub struct Landings {
    /// Falls longer than [`SAFE_FALL`], and how far.
    pub falls: Vec<(Entity, f32)>,
    /// Units that just started swimming.
    pub swims: Vec<Entity>,
}

/// Whether moving from `from` (feet at `y`) to `to` would mean walking up a wall.
fn steep(land: &world::Valley, from: Vec2, y: f32, to: Vec2) -> bool {
    let run = (to - from).length();
    run > 1e-5 && land.ground(to.x, to.y) - y > MAX_SLOPE * run + 0.01
}

/// Where a walker at `from` (feet at `y`) going along `dir` ends up after climbing the face
/// ahead, or `None` if it is taller than [`CLIMB_HEIGHT`] or there's no room on top.
fn ledge_top(land: &world::Valley, from: Vec2, y: f32, dir: Vec2, radius: f32) -> Option<Vec3> {
    let dir = dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    const STEP: f32 = 0.25;
    let mut prev = y;
    for i in 1..=8 {
        let p = from + dir * (i as f32 * STEP);
        let h = land.ground(p.x, p.y);
        if h - y > CLIMB_HEIGHT {
            return None;
        }
        // The top: the ground levels out again above the feet.
        if i > 1 && h - prev <= MAX_SLOPE * STEP && h > y + 0.2 {
            let stand = p + dir * 0.3;
            let top = land.ground(stand.x, stand.y);
            if land.blocked(stand, radius) || top - y > CLIMB_HEIGHT {
                return None;
            }
            return Some(Vec3::new(stand.x, top, stand.y));
        }
        prev = h;
    }
    None
}

/// Heading of a direction on the ground (0 faces +z).
#[inline]
pub fn heading(v: Vec2) -> f32 {
    v.x.atan2(v.y)
}

#[inline]
pub fn wrap_angle(a: f32) -> f32 {
    let mut a = a % std::f32::consts::TAU;
    if a > std::f32::consts::PI {
        a -= std::f32::consts::TAU;
    } else if a < -std::f32::consts::PI {
        a += std::f32::consts::TAU;
    }
    a
}

/// Turns `a` toward `b` by at most `step`.
#[inline]
pub fn approach_angle(a: f32, b: f32, step: f32) -> f32 {
    let d = wrap_angle(b - a);
    if d.abs() <= step { b } else { wrap_angle(a + step * d.signum()) }
}

/// What moving a unit reads and writes.
type Mover<'a> = (Entity, &'a Unit, &'a mut Transform, &'a mut Motion, &'a Body, &'a Vitals, Option<&'a Control>, Option<&'a Fighter>);

pub fn move_units(clock: Res<Clock>, land: Res<Land>, mut out: ResMut<Outbox>, mut arrivals: ResMut<Arrivals>, mut landings: ResMut<Landings>, mut q: Query<Mover>) {
    let land = &land.0;
    for (e, u, mut t, mut m, body, v, ctl, fighter) in &mut q {
        let start = t.pos;
        if v.dead {
            m.vel = Vec3::ZERO;
            m.observed = Vec3::ZERO;
            m.jump_queued = false;
            m.forced = None;
            m.swimming = false;
            m.sprinting = false;
            continue;
        }
        if let Some(f) = m.forced {
            // Carried through a roll, leap or throw: ease out, with a hop in the middle.
            let k = ((clock.time - f.start) / f.duration).clamp(0.0, 1.0) as f32;
            if f.kind == ForcedKind::Climb {
                // Up the face first, then over the edge, so the body never sinks into it.
                let up = 1.0 - (1.0 - (k * 1.6).min(1.0)).powi(2);
                let p = f.from.lerp(f.to, k * k);
                t.pos = Vec3::new(p.x, f.from.y + (f.to.y - f.from.y) * up, p.z);
            } else {
                let eased = 1.0 - (1.0 - k) * (1.0 - k);
                let p = f.from.lerp(f.to, eased);
                t.pos = Vec3::new(p.x, land.ground(p.x, p.z) + (k * std::f32::consts::PI).sin() * f.arc, p.z);
            }
            m.fall_from = t.pos.y;
            if k >= 1.0 {
                m.forced = None;
                m.grounded = true;
                m.swimming = false;
                m.vel = Vec3::ZERO;
                if let ForcedKind::Charge { target, ability } = f.kind {
                    arrivals.0.push((e, target, ability));
                }
            }
            m.observed = (t.pos - start) / DT;
            m.jump_queued = false;
            continue;
        }

        let ctl = ctl.copied().unwrap_or(Control { speed: 1.0, ..Default::default() });
        let mut dir = m.intent.direction;
        let mut face = m.intent.face_yaw;
        let mut walk = m.intent.walk;
        if let Some(from) = ctl.fleeing {
            // Feared: running blindly away.
            let away = Vec2::new(t.pos.x, t.pos.z) - from;
            dir = if away.length_squared() > 1e-4 { away.normalize() } else { t.forward() };
            face = None;
            walk = false;
        } else if !ctl.can_move() {
            dir = Vec2::ZERO;
        }
        if dir.length_squared() > 1.0 {
            dir = dir.normalize();
        }
        // Facing: strafing locks to the requested yaw; otherwise turn toward travel. The
        // stunned can't turn at all.
        if !ctl.stunned {
            if let Some(fy) = face {
                t.yaw = fy;
            } else if dir.length_squared() > 0.0025 {
                t.yaw = approach_angle(t.yaw, heading(dir), DT * 14.0);
            }
        }
        let in_combat = clock.time - v.last_combat < crate::vitals::COMBAT_TIMEOUT;
        let busy = fighter.is_some_and(|f| f.guard_since.is_some() || f.cast.is_some());
        m.sprinting = m.intent.sprint
            && !walk
            && !in_combat
            && !busy
            && !m.swimming
            && !v.downed
            && ctl.fleeing.is_none()
            && dir.length_squared() > 0.0025;
        let mut speed = if walk { WALK_SPEED } else if m.sprinting { m.run_speed * SPRINT } else { m.run_speed };
        if fighter.is_some_and(|f| f.guard_since.is_some()) {
            // Behind a raised guard you can only edge along.
            speed = speed.min(WALK_SPEED);
        }
        if v.downed {
            speed = CRAWL_SPEED;
        }
        speed *= ctl.speed;
        let depth = land.water_depth(t.pos.x, t.pos.z);
        if m.swimming {
            speed *= SWIM;
        } else if m.grounded && depth > 0.2 {
            speed *= WADE;
        }
        if dir.length_squared() > 0.0025 && face.is_some() && t.forward().dot(dir.normalize()) < -0.35 {
            speed *= BACKPEDAL;
        }
        let desired = dir * speed;
        if m.grounded {
            m.vel.x = desired.x;
            m.vel.z = desired.y;
        } else {
            // Limited air control.
            let k = (DT * 3.0).min(1.0);
            m.vel.x += (desired.x - m.vel.x) * k;
            m.vel.z += (desired.y - m.vel.z) * k;
        }
        if m.jump_queued && m.grounded && !m.swimming && ctl.can_move() && !v.downed {
            m.vel.y = JUMP_SPEED;
            m.grounded = false;
            out.0.push(Event::Jumped { unit: u.id });
        }
        m.jump_queued = false;

        let from = Vec2::new(t.pos.x, t.pos.z);
        let mut xz = land.slide(from, Vec2::new(m.vel.x, m.vel.z) * DT, body.radius);
        // Too steep to walk up: climb over it if it's low enough, or slide along it.
        if steep(land, from, t.pos.y, xz) {
            let top = (ctl.can_move() && !v.downed).then(|| ledge_top(land, from, t.pos.y, xz - from, body.radius)).flatten();
            if let Some(top) = top {
                m.forced = Some(Forced { kind: ForcedKind::Climb, from: t.pos, to: top, start: clock.time, duration: CLIMB_TIME, arc: 0.0 });
                m.vel = Vec3::ZERO;
                m.observed = Vec3::ZERO;
                m.sprinting = false;
                continue;
            }
            xz = [Vec2::new(xz.x, from.y), Vec2::new(from.x, xz.y)].into_iter().find(|p| !steep(land, from, t.pos.y, *p)).unwrap_or(from);
        }
        t.pos.x = xz.x;
        t.pos.z = xz.y;

        let ground = land.ground(t.pos.x, t.pos.z);
        let depth = land.water_depth(t.pos.x, t.pos.z);
        let float = (world::WATER_LEVEL - SWIM_FLOAT).max(ground);
        if m.swimming {
            if depth > WADE_OUT {
                t.pos.y = float;
                m.vel.y = 0.0;
            } else {
                // Out of the deep: wading again.
                m.swimming = false;
                t.pos.y = ground;
                m.vel.y = 0.0;
            }
        } else {
            // On the ground, follow it up and down; off it, or over the edge of a cliff, fall.
            if m.grounded {
                let run = Vec2::new(t.pos.x - start.x, t.pos.z - start.z).length();
                let drop = t.pos.y - ground;
                if drop < STEP_DOWN && drop <= MAX_SLOPE * run + 0.01 {
                    t.pos.y = ground;
                    m.vel.y = 0.0;
                } else {
                    m.grounded = false;
                }
            }
            if !m.grounded {
                m.vel.y -= GRAVITY * DT;
                t.pos.y += m.vel.y * DT;
                m.fall_from = m.fall_from.max(t.pos.y);
                if depth > SWIM_DEPTH && t.pos.y <= float {
                    // A splash: water breaks the fall.
                    t.pos.y = float;
                    m.vel.y = 0.0;
                    m.grounded = true;
                } else if t.pos.y <= ground {
                    t.pos.y = ground;
                    m.vel.y = 0.0;
                    // Touching a cliff face on the way down isn't landing: keep falling.
                    let along = Vec2::new(m.vel.x, m.vel.z).normalize_or_zero() * 0.25;
                    let ahead = Vec2::new(t.pos.x, t.pos.z) + along;
                    let cliff = along != Vec2::ZERO && ground - land.ground(ahead.x, ahead.y) > MAX_SLOPE * 0.25 + 0.01;
                    if !cliff {
                        m.grounded = true;
                        let fell = m.fall_from - ground;
                        if fell > SAFE_FALL {
                            landings.falls.push((e, fell));
                        }
                    }
                }
            }
            if m.grounded && depth > SWIM_DEPTH {
                m.swimming = true;
                m.sprinting = false;
                t.pos.y = float;
                landings.swims.push(e);
            }
        }
        if m.grounded {
            m.fall_from = t.pos.y;
        }
        m.observed = (t.pos - start) / DT;
    }
}

/// Hurts units that fell too far, and stops swimmers fighting: their guard drops and their
/// cast breaks off.
pub fn after_moving(world: &mut World) {
    let Landings { falls, swims } = std::mem::take(&mut *world.resource_mut::<Landings>());
    for (e, fell) in falls {
        let max_health = world.get::<Vitals>(e).map_or(0.0, |v| v.max_health);
        crate::combat::effects::fall_damage(world, e, (fell - SAFE_FALL) * FALL_DAMAGE * max_health);
    }
    for e in swims {
        crate::combat::guard::lower(world, e);
        crate::combat::abilities::cancel_cast(world, e, false);
        if let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.auto_attack = false;
        }
    }
}

/// How long action poses hold before the body goes back to moving or standing.
pub const ATTACK_POSE: f64 = 0.5;
pub const HIT_POSE: f64 = 0.25;

/// Picks each unit's pose from what its body is doing, unless an action pose is holding. The
/// downed lie as the dead do (the combat view says they're only down).
pub fn update_poses(clock: Res<Clock>, mut q: Query<(&Motion, &Vitals, &mut Anim, Option<&Fighter>)>) {
    for (m, v, mut a, f) in &mut q {
        let pose = if v.dead || v.downed {
            Pose::Dead
        } else if clock.time < a.hold_until {
            a.pose
        } else if let Some(forced) = m.forced {
            match forced.kind {
                ForcedKind::Dodge => Pose::Dodge,
                ForcedKind::Knockback => Pose::Hit,
                ForcedKind::Leap => Pose::Jump,
                ForcedKind::Charge { .. } => Pose::Run,
                ForcedKind::Climb => Pose::Jump,
            }
        } else if let Some(c) = f.and_then(|f| f.cast) {
            if crate::content::catalog().ability(c.ability).is_swing() { Pose::Attack } else { Pose::Cast }
        } else if !m.grounded {
            if m.vel.y > 0.0 { Pose::Jump } else { Pose::Fall }
        } else {
            let s = Vec2::new(m.observed.x, m.observed.z).length();
            if s > 3.2 {
                Pose::Run
            } else if s > 0.3 {
                Pose::Walk
            } else {
                Pose::Idle
            }
        };
        if pose != a.pose {
            a.pose = pose;
            a.since = clock.time;
        }
    }
}

/// Starts a dodge roll if there's enough endurance. Returns whether it rolled.
pub fn start_dodge(land: &world::Valley, now: f64, t: &mut Transform, m: &mut Motion, v: &mut Vitals, body: &Body, direction: Vec2) -> bool {
    if !v.alive() || m.forced.is_some() || v.endurance < DODGE_COST {
        return false;
    }
    let dir = if direction.length_squared() > 0.01 { direction.normalize() } else { -t.forward() };
    v.endurance -= DODGE_COST;
    let end = land.slide(Vec2::new(t.pos.x, t.pos.z), dir * DODGE_DISTANCE, body.radius);
    m.forced = Some(crate::Forced {
        kind: ForcedKind::Dodge,
        from: t.pos,
        to: Vec3::new(end.x, t.pos.y, end.y),
        start: now,
        duration: DODGE_DURATION,
        arc: 0.15,
    });
    v.invulnerable_until = now + DODGE_INVULNERABLE;
    t.yaw = m.intent.face_yaw.unwrap_or(heading(dir));
    true
}
