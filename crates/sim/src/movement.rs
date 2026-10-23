use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Event, Pose};

use crate::{Anim, Body, Clock, Land, Motion, Outbox, Transform, Unit, Vitals, DT};

pub const GRAVITY: f32 = 20.0;
/// About a 1.1 m jump, landing after 0.66 s.
pub const JUMP_SPEED: f32 = 6.6;
pub const WALK_SPEED: f32 = 2.5;
pub const DODGE_DISTANCE: f32 = 5.0;
pub const DODGE_DURATION: f64 = 0.45;
pub const DODGE_INVULNERABLE: f64 = 0.4;
pub const DODGE_COST: f32 = 50.0;
/// Backpedalling while strafing runs at this fraction of full speed.
const BACKPEDAL: f32 = 0.6;
/// The most a walker steps down without leaving the ground.
const STEP_DOWN: f32 = 0.6;

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

pub fn move_units(
    clock: Res<Clock>,
    land: Res<Land>,
    mut out: ResMut<Outbox>,
    mut q: Query<(&Unit, &mut Transform, &mut Motion, &Body, &Vitals)>,
) {
    let land = &land.0;
    for (u, mut t, mut m, body, v) in &mut q {
        let start = t.pos;
        if v.dead {
            m.vel = Vec3::ZERO;
            m.observed = Vec3::ZERO;
            m.jump_queued = false;
            continue;
        }
        if let Some(f) = m.forced {
            // Carried through a dodge: ease out, with a small hop in the middle.
            let k = ((clock.time - f.start) / f.duration).clamp(0.0, 1.0) as f32;
            let eased = 1.0 - (1.0 - k) * (1.0 - k);
            let p = f.from.lerp(f.to, eased);
            t.pos = Vec3::new(p.x, land.ground(p.x, p.z) + (k * std::f32::consts::PI).sin() * f.arc, p.z);
            if k >= 1.0 {
                m.forced = None;
                m.grounded = true;
                m.vel = Vec3::ZERO;
            }
            m.observed = (t.pos - start) / DT;
            m.jump_queued = false;
            continue;
        }

        let mut dir = m.intent.direction;
        if dir.length_squared() > 1.0 {
            dir = dir.normalize();
        }
        // Facing: strafing locks to the requested yaw; otherwise turn toward travel.
        if let Some(fy) = m.intent.face_yaw {
            t.yaw = fy;
        } else if dir.length_squared() > 0.0025 {
            t.yaw = approach_angle(t.yaw, heading(dir), DT * 14.0);
        }
        let mut speed = if m.intent.walk { WALK_SPEED } else { m.run_speed };
        if dir.length_squared() > 0.0025 && m.intent.face_yaw.is_some() && t.forward().dot(dir.normalize()) < -0.35 {
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
        if m.jump_queued && m.grounded {
            m.vel.y = JUMP_SPEED;
            m.grounded = false;
            out.0.push(Event::Jumped { unit: u.id });
        }
        m.jump_queued = false;

        let xz = land.slide(Vec2::new(t.pos.x, t.pos.z), Vec2::new(m.vel.x, m.vel.z) * DT, body.radius);
        t.pos.x = xz.x;
        t.pos.z = xz.y;

        // On the ground, follow it up and down; off it, fall.
        let ground = land.ground(t.pos.x, t.pos.z);
        if m.grounded {
            if t.pos.y - ground < STEP_DOWN {
                t.pos.y = ground;
                m.vel.y = 0.0;
            } else {
                m.grounded = false;
            }
        }
        if !m.grounded {
            m.vel.y -= GRAVITY * DT;
            t.pos.y += m.vel.y * DT;
            if t.pos.y <= ground {
                t.pos.y = ground;
                m.vel.y = 0.0;
                m.grounded = true;
            }
        }
        m.observed = (t.pos - start) / DT;
    }
}

/// How long action poses hold before the body goes back to moving or standing.
pub const ATTACK_POSE: f64 = 0.5;
pub const HIT_POSE: f64 = 0.25;

/// Picks each unit's pose from what its body is doing, unless an action pose is holding.
pub fn update_poses(clock: Res<Clock>, mut q: Query<(&Motion, &Vitals, &mut Anim)>) {
    for (m, v, mut a) in &mut q {
        let pose = if v.dead {
            Pose::Dead
        } else if clock.time < a.hold_until {
            a.pose
        } else if m.forced.is_some() {
            Pose::Dodge
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
    if v.dead || m.forced.is_some() || v.endurance < DODGE_COST {
        return false;
    }
    let dir = if direction.length_squared() > 0.01 { direction.normalize() } else { -t.forward() };
    v.endurance -= DODGE_COST;
    let end = land.slide(Vec2::new(t.pos.x, t.pos.z), dir * DODGE_DISTANCE, body.radius);
    m.forced = Some(crate::Forced {
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
