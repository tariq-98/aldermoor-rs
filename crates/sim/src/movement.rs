use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Event, Pose};

use crate::combat::abilities::Arrivals;
use crate::{climbing, Anim, Body, Clock, Control, Faction, Fighter, Forced, ForcedKind, Land, Motion, Outbox, Transform, Unit, UnitKind, Vitals, DT};

pub const GRAVITY: f32 = 20.0;
/// About a 1.1 m jump, landing after 0.66 s.
pub const JUMP_SPEED: f32 = 6.6;
pub const WALK_SPEED: f32 = 2.5;
pub const DODGE_DISTANCE: f32 = 5.0;
pub const DODGE_DURATION: f64 = 0.45;
pub const DODGE_INVULNERABLE: f64 = 0.4;
pub const DODGE_COST: f32 = 50.0;
/// A roll pressed this long before a roll, climb or other forced move ends goes off when it
/// ends, instead of being lost.
pub const DODGE_BUFFER: f64 = 0.2;
/// A jump pressed this long before the feet touch down goes off as they land.
pub const JUMP_BUFFER: f64 = 0.15;
/// Running off an edge, a jump still works for this long (coyote time).
pub const COYOTE_TIME: f64 = 0.1;
/// How quickly steering takes hold in the air (per second).
pub const AIR_CONTROL: f32 = 10.0;
/// Downed: a crawl.
pub const CRAWL_SPEED: f32 = 1.0;
/// Backpedalling while strafing runs at this fraction of full speed.
const BACKPEDAL: f32 = 0.6;
/// The most a walker steps down without leaving the ground.
const STEP_DOWN: f32 = 0.6;
/// Sprinting (out of combat, on land) runs this much faster.
pub const SPRINT: f32 = 1.75;
/// On foot, from standing to full speed takes this long (s), and from run to sprint `SPRINT_UP`.
/// Short enough to keep dodging sharp, long enough that a body has some weight.
pub const START_UP: f32 = 0.08;
pub const SPRINT_UP: f32 = 0.3;
/// Coming to a stop from a run takes this long (s).
pub const STOP_TIME: f32 = 0.06;
/// Steeper than this (rise over run, about 63°) the ground is a wall you can't walk up.
pub const MAX_SLOPE: f32 = 2.0;
/// Walls up to about chest height are climbed over; taller ones stop you.
pub const CLIMB_HEIGHT: f32 = 1.5;
/// Seconds to pull up over a ledge.
pub const CLIMB_TIME: f64 = 0.4;
/// In a jump, a ledge this high above the feet can be caught and pulled up onto (a mantle), as
/// long as you aren't yet falling faster than `MANTLE_FALLING` (m/s).
pub const MANTLE_HEIGHT: f32 = 2.2;
pub(crate) const MANTLE_FALLING: f32 = -6.0;
pub const MANTLE_TIME: f64 = 0.35;
/// Running (faster than a walk) at a wall up to this high and shallower than this, with clear
/// ground beyond, hops it in stride, at the speed you ran in, taking between `VAULT_MIN` and
/// `VAULT_TIME` seconds. Deeper ones (a 0.8 m wall, the 0.9 m field walls) are climbed onto.
pub const VAULT_HEIGHT: f32 = 1.2;
pub const VAULT_DEPTH: f32 = 0.7;
pub const VAULT_TIME: f64 = 0.45;
const VAULT_MIN: f64 = 0.2;
/// Deeper water than this is swum, and swimming stops once it's shallower than `WADE_OUT`.
pub const SWIM_DEPTH: f32 = 0.5;
const WADE_OUT: f32 = 0.4;
/// Swimmers float with their feet this far under the surface.
const SWIM_FLOAT: f32 = 0.5;
/// Swimming speed, and wading through shallower water, as fractions of running.
pub const SWIM: f32 = 0.45;
const WADE: f32 = 0.7;
/// Water deeper than this is waded through.
pub const WADE_DEPTH: f32 = 0.2;
/// Falls up to this many metres don't hurt; past it each metre costs a share of max health.
pub const SAFE_FALL: f32 = 8.0;
pub const FALL_DAMAGE: f32 = 0.06;
/// Roll pressed in the air this long before landing rolls the fall off: falls up to
/// `ROLL_SAFE_FALL` don't hurt, and the body rolls on `LANDING_ROLL_DISTANCE` the way it was going.
pub const LANDING_ROLL: f64 = 0.2;
pub const ROLL_SAFE_FALL: f32 = 16.0;
const LANDING_ROLL_DISTANCE: f32 = 3.0;
/// The air dash (#347): this far, this fast, following the aim up or down by at most
/// `AIR_DASH_PITCH` radians (35°), and keeping this share of its speed when it ends.
pub const AIR_DASH_DISTANCE: f32 = 4.0;
pub const AIR_DASH_TIME: f64 = 0.18;
pub const AIR_DASH_PITCH: f32 = 0.61;
const AIR_DASH_KEEP: f32 = 0.4;
/// The double jump rises 70% as high as a jump from the ground (height goes as speed squared).
pub const AIR_JUMP_SPEED: f32 = JUMP_SPEED * 0.8367;
/// Overlapping bodies are pushed apart by this share of the overlap each step, so a crowd eases
/// apart over a few steps rather than jumping.
const SEPARATE: f32 = 0.5;
/// Players stand their ground this many times more firmly than monsters, so a pack spreads round
/// them instead of shoving them about.
const PLAYER_WEIGHT: f32 = 4.0;
/// The push apart turns this far (radians) to one side, so crowds slide round each other.
const SIDESTEP: f32 = 0.5;

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

/// How `m` is getting about, for the snapshot.
pub fn traversal(m: &Motion) -> protocol::Traversal {
    use protocol::{ForcedMove, Traversal};
    if let Some(f) = m.forced {
        let kind = match f.kind {
            ForcedKind::Dodge => protocol::ForcedKind::Roll,
            ForcedKind::Knockback => protocol::ForcedKind::Knockback,
            ForcedKind::Leap => protocol::ForcedKind::Leap,
            ForcedKind::Charge { .. } => protocol::ForcedKind::Charge,
            ForcedKind::Climb => protocol::ForcedKind::Climb,
            ForcedKind::Mantle => protocol::ForcedKind::Mantle,
            ForcedKind::Vault { .. } => protocol::ForcedKind::Vault,
            ForcedKind::ToHold { .. } => protocol::ForcedKind::Hold,
            ForcedKind::Corner { .. } => protocol::ForcedKind::Corner,
            ForcedKind::WallRun { .. } => protocol::ForcedKind::WallRun,
            // Drawn as a leap until it has poses of its own (#353).
            ForcedKind::AirDash => protocol::ForcedKind::Leap,
        };
        return Traversal::Forced(ForcedMove { kind, from: f.from, to: f.to, start: f.start, duration: f.duration as f32 });
    }
    if m.hang.is_some() {
        Traversal::Hanging
    } else if m.swimming {
        Traversal::Swimming
    } else if m.wading {
        Traversal::Wading
    } else {
        Traversal::Ground
    }
}

/// Where a walker at `from` (feet at `y`) going along `dir` ends up after climbing the face
/// ahead, or `None` if it is taller than `reach` or there's no room on top.
pub(crate) fn ledge_top(land: &world::Valley, from: Vec2, y: f32, dir: Vec2, radius: f32, reach: f32) -> Option<Vec3> {
    let dir = dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    const STEP: f32 = 0.25;
    let mut prev = y;
    for i in 1..=8 {
        let p = from + dir * (i as f32 * STEP);
        let h = land.ground(p.x, p.y);
        if h - y > reach {
            return None;
        }
        // The top: the ground levels out again above the feet. Stand a little past the edge, or
        // right on it if that's where the body fits (a top as narrow as a gable's coping, with a
        // buttress beyond), or nearer the front still if the last sample was on it too (a cliff's
        // ledge, with the next one up overhanging it).
        if i > 1 && h - prev <= MAX_SLOPE * STEP && h > y + 0.2 {
            let stand = p + dir * 0.3;
            let top = land.ground(stand.x, stand.y);
            if top - y > reach {
                return None;
            }
            // The front of the top: back from the last sample while still on it.
            let front = ((h - prev).abs() < 0.05).then(|| {
                let on = |q: Vec2| (land.ground(q.x, q.y) - h).abs() < 0.05;
                let mut q = p - dir * STEP;
                while on(q - dir * 0.05) && (q - from).dot(dir) > 0.05 {
                    q -= dir * 0.05;
                }
                (q, h)
            });
            return [(stand, top), (p, h)].into_iter().chain(front).find(|&(at, top)| room(land, at, top, radius)).map(|(at, top)| Vec3::new(at.x, top, at.y));
        }
        prev = h;
    }
    // A wall too thin for those samples to land on its top (#238): stand on it.
    if let Some((p, h)) = land.top_ahead(from, dir, 0.6)
        && h > y + 0.2
        && h - y <= reach
        && room(land, p, h, radius)
    {
        return Some(Vec3::new(p.x, h, p.y));
    }
    None
}

/// Whether a body of `radius` fits standing at `p` with its feet at `y`: no obstacle there, nor a
/// face too tall to step onto (a sill's top against the wall it stands out from).
fn room(land: &world::Valley, p: Vec2, y: f32, radius: f32) -> bool {
    !land.blocked(p, radius) && land.off_faces(p, radius, y + CLIMB_HEIGHT) == p
}

/// Where a runner at `from` (feet at `y`) going along `dir` lands after hopping the wall just
/// ahead, and how high above the feet its top is: `None` unless it is a wall no higher than
/// [`VAULT_HEIGHT`] and no deeper than [`VAULT_DEPTH`], with clear ground about level with the
/// feet beyond.
fn vault_over(land: &world::Valley, from: Vec2, y: f32, dir: Vec2, radius: f32) -> Option<(Vec3, f32)> {
    let dir = dir.normalize_or_zero();
    if dir == Vec2::ZERO {
        return None;
    }
    const STEP: f32 = 0.05;
    // The face: where the ground first rises, just ahead.
    let face = (1..=12).map(|i| i as f32 * STEP).find(|d| {
        let p = from + dir * *d;
        land.ground(p.x, p.y) - y > 0.3
    })?;
    let mut top = 0.0f32;
    for i in 0..=((VAULT_DEPTH / STEP) as i32 + 1) {
        let d = face + i as f32 * STEP;
        let p = from + dir * d;
        let h = land.ground(p.x, p.y) - y;
        // Too tall to hop: no need to look further (a runner pressed against a cottage would
        // otherwise sample its whole depth every step).
        if h > VAULT_HEIGHT {
            return None;
        }
        if h <= 0.3 {
            // Over it: the ground beyond must be about level and clear.
            let at = p + dir * (radius + 0.1);
            let g = land.ground(at.x, at.y);
            if d - face >= VAULT_DEPTH || (g - y).abs() > STEP_DOWN || land.blocked(at, radius) {
                return None;
            }
            return Some((Vec3::new(at.x, g, at.y), top));
        }
        top = top.max(h);
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
            // The dead drop where they are: off a wall, out of a jump or a climb.
            let mut fall = 0.0;
            if !m.grounded || m.hang.is_some() || m.forced.is_some() {
                let ground = land.ground(t.pos.x, t.pos.z);
                let floor = if land.water_depth(t.pos.x, t.pos.z) > SWIM_DEPTH { (world::WATER_LEVEL - SWIM_FLOAT).max(ground) } else { ground };
                fall = m.vel.y.min(0.0) - GRAVITY * DT;
                t.pos.y = (t.pos.y + fall * DT).max(floor);
                m.grounded = t.pos.y <= floor;
            }
            m.vel = Vec3::new(0.0, if m.grounded { 0.0 } else { fall }, 0.0);
            m.observed = Vec3::ZERO;
            m.jump_until = f64::NEG_INFINITY;
            m.dodge_queued = None;
            m.forced = None;
            m.swimming = false;
            m.wading = false;
            m.sprinting = false;
            m.hang = None;
            continue;
        }
        // On the ground or a wall, the air dash and the double jump are ready again.
        let on_a_wall = m.forced.is_some_and(|f| matches!(f.kind, ForcedKind::Climb | ForcedKind::Mantle | ForcedKind::ToHold { .. } | ForcedKind::Corner { .. } | ForcedKind::WallRun { .. }));
        if m.grounded || m.hang.is_some() || on_a_wall {
            m.air_dash = true;
            m.air_jump = true;
            m.jumped = false;
        }
        // Hanging from a hold: the climb moves the body, unless it's been downed off the wall.
        if m.hang.is_some() && m.forced.is_none() {
            if v.downed {
                climbing::let_go(land, clock.time, &mut m);
            } else {
                climbing::hang_step(land, clock.time, &mut t, &mut m, body.radius, ctl.is_none_or(|c| c.can_move()));
                continue;
            }
        }
        if let Some(f) = m.forced {
            // Carried through a roll, leap or throw: ease out, with a hop in the middle.
            let k = ((clock.time - f.start) / f.duration).clamp(0.0, 1.0) as f32;
            if let ForcedKind::WallRun { .. } = f.kind {
                // Up a wall, until a hold is caught, a top climbed onto, or the run gives out.
                climbing::wall_run_step(land, clock.time, &mut t, &mut m, body.radius, f);
                m.observed = (t.pos - start) / DT;
                continue;
            }
            if let ForcedKind::AirDash = f.kind {
                air_dash_step(land, clock.time, &mut t, &mut m, body.radius, f);
                m.observed = (t.pos - start) / DT;
                continue;
            }
            if matches!(f.kind, ForcedKind::Climb | ForcedKind::Mantle) {
                // Up the face first, then over the edge, so the body never sinks into it.
                let up = 1.0 - (1.0 - (k * 1.6).min(1.0)).powi(2);
                let p = f.from.lerp(f.to, k * k);
                t.pos = Vec3::new(p.x, f.from.y + (f.to.y - f.from.y) * up, p.z);
            } else if matches!(f.kind, ForcedKind::Vault { .. }) {
                // Straight over at an even pace, in a hop that clears the top.
                let p = f.from.lerp(f.to, k);
                let rise = (f.arc - (f.to.y - f.from.y) * 0.5).max(0.0);
                t.pos = Vec3::new(p.x, p.y + (k * std::f32::consts::PI).sin() * rise, p.z);
            } else if let ForcedKind::ToHold { hold, .. } = f.kind {
                t.pos = climbing::carry(&f, k);
                // Turning to face the wall on the way there, rather than in one snap.
                t.yaw = approach_angle(t.yaw, heading(-land.holds.get(hold).out), DT * 14.0);
            } else if let ForcedKind::Corner { hold, corner, .. } = f.kind {
                t.pos = climbing::corner_carry(&f, corner, k);
                // A quarter turn by 0.785 of the move, as the corner clips are made (2 rad over it).
                t.yaw = approach_angle(t.yaw, heading(-land.holds.get(hold).out), DT * 2.0 / climbing::CORNER_TIME as f32);
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
                m.wading = land.water_depth(t.pos.x, t.pos.z) > WADE_DEPTH;
                m.vel = Vec3::ZERO;
                if let ForcedKind::Vault { speed } = f.kind {
                    // Landing in stride.
                    let along = Vec2::new(f.to.x - f.from.x, f.to.z - f.from.z).normalize_or_zero() * speed;
                    m.vel = Vec3::new(along.x, 0.0, along.y);
                }
                if let ForcedKind::Charge { target, ability } = f.kind {
                    arrivals.0.push((e, target, ability));
                }
                if let Some(hang) = climbing::onto(f.kind) {
                    m.hang = Some(hang);
                    m.grounded = false;
                }
            }
            m.observed = (t.pos - start) / DT;
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
        // A skill pressed this step (or waiting on the global cooldown) wins over parkour that
        // would start on the same step (#363).
        let acting = busy || fighter.is_some_and(|f| f.queued.is_some() || f.action_time >= clock.time);
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
        } else if m.grounded && depth > WADE_DEPTH {
            speed *= WADE;
        }
        if dir.length_squared() > 0.0025 && face.is_some() && t.forward().dot(dir.normalize()) < -0.35 {
            speed *= BACKPEDAL;
        }
        let desired = dir * speed;
        if m.grounded && !ctl.can_move() {
            // Rooted or stunned: stopped dead.
            m.vel.x = 0.0;
            m.vel.z = 0.0;
        } else if m.grounded && m.swimming {
            m.vel.x = desired.x;
            m.vel.z = desired.y;
        } else if m.grounded {
            let cruise = if m.sprinting { speed / SPRINT } else { speed };
            let v = ramp(Vec2::new(m.vel.x, m.vel.z), desired, cruise, m.run_speed * ctl.speed / STOP_TIME);
            m.vel.x = v.x;
            m.vel.z = v.y;
        } else {
            // Air control: steering answers in about a tenth of a second (#363).
            let k = (DT * AIR_CONTROL).min(1.0);
            m.vel.x += (desired.x - m.vel.x) * k;
            m.vel.z += (desired.y - m.vel.z) * k;
        }
        // A jump pressed a moment before landing, or a moment after running off an edge, still
        // goes off.
        let footing = m.grounded || clock.time <= m.coyote_until;
        // In a fight, parkour starts only on intent (#363): a jump into a wall runs up it. A roll
        // or a skill pressed the same step wins (`busy`, or the roll's forced move, comes first).
        let player = u.kind == UnitKind::Player;
        if in_combat
            && player
            && clock.time <= m.jump_until
            && m.grounded
            && !acting
            && !m.swimming
            && ctl.can_move()
            && !v.downed
            && dir.length_squared() > 0.01
            && climbing::wall_run(land, clock.time, &mut t, &mut m, body.radius, dir)
        {
            m.jump_until = f64::NEG_INFINITY;
            m.observed = Vec3::ZERO;
            continue;
        }
        if clock.time <= m.jump_until && footing && !m.swimming && ctl.can_move() && !v.downed {
            m.vel.y = JUMP_SPEED;
            m.jumped = true;
            m.grounded = false;
            m.jump_until = f64::NEG_INFINITY;
            m.coyote_until = f64::NEG_INFINITY;
            out.0.push(Event::Jumped { unit: u.id });
        } else if clock.time <= m.jump_until
            && !footing
            && m.air_jump
            && !m.swimming
            && ctl.can_move()
            && !v.downed
            && time_to_land(land, &t, &m) > JUMP_BUFFER
        {
            // A second jump in the air, unless the feet are about to land: then the press waits
            // to jump from the ground, as the buffer has it.
            m.vel.y = AIR_JUMP_SPEED;
            m.air_jump = false;
            m.jumped = true;
            m.jump_until = f64::NEG_INFINITY;
            out.0.push(Event::Jumped { unit: u.id });
        }

        // Sprinting (Shift, out of combat: parkour up) into a wall too tall to climb runs up it.
        if m.sprinting && m.grounded && player && !acting && ctl.can_move() && climbing::wall_run(land, clock.time, &mut t, &mut m, body.radius, dir) {
            m.observed = Vec3::ZERO;
            continue;
        }
        let from = Vec2::new(t.pos.x, t.pos.z);
        let mut xz = land.slide(from, Vec2::new(m.vel.x, m.vel.z) * DT, body.radius);
        // A face too tall to step onto stops the body a radius short of it, as obstacles do (#216).
        let tall = t.pos.y + CLIMB_HEIGHT;
        let mut off = land.off_faces(xz, body.radius, tall);
        // Too steep to walk up, or up against a face too tall to step onto: hop it if it's a low
        // wall and you're running, climb over it if it's low enough (or, in a jump, catch its
        // edge), or slide along it.
        if steep(land, from, t.pos.y, xz) || off != xz {
            let able = ctl.can_move() && !v.downed;
            let ahead = xz - from;
            let speed = Vec2::new(m.vel.x, m.vel.z).length();
            let hop = (able && !busy && !m.swimming && m.grounded && speed > WALK_SPEED * 1.2).then(|| vault_over(land, from, t.pos.y, ahead, body.radius)).flatten();
            if let Some((to, top)) = hop {
                let travel = Vec2::new(to.x - t.pos.x, to.z - t.pos.z).length();
                let duration = (travel as f64 / speed as f64).clamp(VAULT_MIN, VAULT_TIME);
                m.forced = Some(Forced { kind: ForcedKind::Vault { speed }, from: t.pos, to, start: clock.time, duration, arc: top + 0.25 });
                m.vel = Vec3::ZERO;
                m.observed = Vec3::ZERO;
                continue;
            }
            // Not straight back up onto the top just let go of: letting go drops you.
            let let_go = m.released.is_some_and(|(_, until)| clock.time < until);
            let mantle = able && !busy && !m.grounded && !let_go && m.vel.y > MANTLE_FALLING;
            let reach = if mantle { MANTLE_HEIGHT } else { CLIMB_HEIGHT };
            let top = able.then(|| ledge_top(land, from, t.pos.y, ahead, body.radius, reach)).flatten();
            if let Some(top) = top {
                let (kind, duration) = if mantle { (ForcedKind::Mantle, MANTLE_TIME) } else { (ForcedKind::Climb, CLIMB_TIME) };
                m.forced = Some(Forced { kind, from: t.pos, to: top, start: clock.time, duration, arc: 0.0 });
                m.vel = Vec3::ZERO;
                m.observed = Vec3::ZERO;
                m.sprinting = false;
                continue;
            }
            if steep(land, from, t.pos.y, xz) {
                xz = [Vec2::new(xz.x, from.y), Vec2::new(from.x, xz.y)].into_iter().find(|p| !steep(land, from, t.pos.y, *p)).unwrap_or(from);
                off = land.off_faces(xz, body.radius, tall);
            }
        }
        let xz = off;
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
                    // Off the edge: a jump still works for a moment.
                    m.grounded = false;
                    m.coyote_until = clock.time + COYOTE_TIME;
                }
            }
            if !m.grounded {
                let before = t.pos.y;
                m.vel.y -= GRAVITY * DT;
                t.pos.y += m.vel.y * DT;
                m.fall_from = m.fall_from.max(t.pos.y);
                // Pressing into a wall catches the holds the hands pass (#363: no Shift needed),
                // and so does anything a moment after jumping off a wall. In a fight only a jump
                // you made catches, never a fall or a throw, and a roll or a skill comes first.
                let window = clock.time < m.catch_until;
                let pressing = m.intent.direction.length_squared() > 0.01 && (!in_combat || m.jumped);
                let catching = player && (window || pressing) && !acting && ctl.can_move() && !v.downed;
                let into = (!window).then_some(m.intent.direction);
                if catching && climbing::catch(land, clock.time, &mut t, &mut m, body.radius, before, into) {
                    m.observed = (t.pos - start) / DT;
                    continue;
                }
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
                        m.jumped = false;
                        let fell = m.fall_from - ground;
                        let rolled = clock.time <= m.landing_roll_until;
                        let safe = if rolled { ROLL_SAFE_FALL } else { SAFE_FALL };
                        if fell > safe {
                            landings.falls.push((e, fell - (safe - SAFE_FALL)));
                        }
                        if rolled {
                            landing_roll(land, clock.time, &mut t, &mut m, body.radius);
                        }
                        m.catch_until = f64::NEG_INFINITY;
                        m.landing_roll_until = f64::NEG_INFINITY;
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
        m.wading = m.grounded && !m.swimming && depth > WADE_DEPTH;
        m.observed = (t.pos - start) / DT;
    }
}

/// Seconds until a body in the air comes down to the ground below it, falling as it is.
pub fn time_to_land(land: &world::Valley, t: &Transform, m: &Motion) -> f64 {
    let h = (t.pos.y - land.ground(t.pos.x, t.pos.z)).max(0.0);
    let vy = m.vel.y;
    ((vy + (vy * vy + 2.0 * GRAVITY * h).sqrt()) / GRAVITY) as f64
}

/// Starts an air dash (#347) along `direction` (or facing), pitched by the aim up to
/// `AIR_DASH_PITCH`, if the body is in the air with its dash unused and enough endurance. A
/// roll pressed in the last `LANDING_ROLL` before landing stays a roll landing instead.
pub fn start_air_dash(land: &world::Valley, now: f64, t: &mut Transform, m: &mut Motion, v: &mut Vitals, direction: Vec2) -> bool {
    if m.grounded || m.swimming || m.forced.is_some() || m.hang.is_some() || !m.air_dash || !v.alive() || v.downed || v.endurance < DODGE_COST {
        return false;
    }
    if time_to_land(land, t, m) <= LANDING_ROLL {
        return false;
    }
    let dir = if direction.length_squared() > 0.01 { direction.normalize() } else { t.forward() };
    let pitch = protocol::aim_pitch_radians(m.aim_pitch).clamp(-AIR_DASH_PITCH, AIR_DASH_PITCH);
    let (sin, cos) = pitch.sin_cos();
    let to = t.pos + Vec3::new(dir.x * cos, sin, dir.y * cos) * AIR_DASH_DISTANCE;
    v.endurance -= DODGE_COST;
    m.air_dash = false;
    m.landing_roll_until = f64::NEG_INFINITY;
    m.forced = Some(Forced { kind: ForcedKind::AirDash, from: t.pos, to, start: now, duration: AIR_DASH_TIME, arc: 0.0 });
    m.vel = Vec3::ZERO;
    t.yaw = m.intent.face_yaw.unwrap_or(heading(dir));
    true
}

/// One step of an air dash: straight on with gravity paused, stopped short by a wall or the
/// ground. When it ends the body falls on with `AIR_DASH_KEEP` of the dash's speed.
fn air_dash_step(land: &world::Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32, f: Forced) {
    let k = ((now - f.start) / f.duration).clamp(0.0, 1.0) as f32;
    let aim = f.from.lerp(f.to, k);
    let from = Vec2::new(t.pos.x, t.pos.z);
    let want = Vec2::new(aim.x, aim.z);
    let xz = land.off_faces(land.slide(from, want - from, radius), radius, aim.y + CLIMB_HEIGHT);
    let ground = land.ground(xz.x, xz.y);
    t.pos = Vec3::new(xz.x, aim.y.max(ground), xz.y);
    m.fall_from = t.pos.y;
    let blocked = (xz - want).length() > 0.05;
    let down = aim.y <= ground;
    if k >= 1.0 || blocked || down {
        m.forced = None;
        let speed = (f.to - f.from) / f.duration as f32;
        m.vel = if blocked { Vec3::new(0.0, speed.y.min(0.0), 0.0) } else { speed } * AIR_DASH_KEEP;
        m.grounded = down;
        if down {
            m.vel.y = 0.0;
        }
    }
}

/// A roll landing: on along the ground the way the body was going (or facing), taking the fall.
fn landing_roll(land: &world::Valley, now: f64, t: &mut Transform, m: &mut Motion, radius: f32) {
    let going = Vec2::new(m.vel.x, m.vel.z);
    let dir = if going.length_squared() > 0.25 { going.normalize() } else { t.forward() };
    let end = land.slide_below(Vec2::new(t.pos.x, t.pos.z), dir * LANDING_ROLL_DISTANCE, radius, t.pos.y + CLIMB_HEIGHT);
    m.forced = Some(Forced { kind: ForcedKind::Dodge, from: t.pos, to: Vec3::new(end.x, t.pos.y, end.y), start: now, duration: DODGE_DURATION, arc: 0.15 });
    m.vel = Vec3::ZERO;
    t.yaw = heading(dir);
}

/// One step of on-foot speed toward `desired`: the part of the velocity along the wished
/// direction builds to `cruise` over `START_UP` and on to a sprint over `SPRINT_UP`, while the
/// part across it (and all of it when stopping) bleeds off at `brake` (m/s²). Turning round
/// drops the backward part at once, so reversing is as quick as starting.
fn ramp(vel: Vec2, desired: Vec2, cruise: f32, brake: f32) -> Vec2 {
    // Standing still or already at speed, the common case.
    if vel == desired {
        return desired;
    }
    let target = desired.length();
    let shed = |v: Vec2| v * (1.0 - brake * DT / v.length().max(1e-6)).max(0.0);
    if target < 1e-4 {
        return shed(vel);
    }
    let wish = desired / target;
    let mut along = vel.dot(wish).max(0.0);
    let across = shed(vel - wish * vel.dot(wish));
    if along < target {
        let rate = if along < cruise { cruise / START_UP } else { (target - cruise) / SPRINT_UP };
        along = (along + rate * DT).min(target);
    } else {
        along = (along - brake * DT).max(target);
    }
    wish * along + across
}

/// A unit in the crowd: who, where, its radius and weight (0 for one that neither pushes nor is
/// pushed), the push so far, and a player's side (players on the same side pass through each
/// other).
type Member = (Entity, Vec2, f32, f32, Vec2, Option<Faction>);

/// Scratch space for `separate`, kept between steps (by `spatial::rebuild`) so it doesn't
/// allocate.
#[derive(Default)]
pub struct Crowd {
    bodies: Vec<Member>,
    widest: f32,
    /// Indices into `bodies` by band (north-south) then west to east, with the band and the
    /// east-west position. Units barely move between steps, so last step's order is nearly
    /// sorted and an insertion sort puts it right cheaply.
    order: Vec<(i32, f32, u32)>,
}

impl Crowd {
    /// Separation runs every other step, which is plenty to keep a crowd apart.
    pub fn due(tick: u64) -> bool {
        tick % 2 == 0
    }

    pub fn clear(&mut self) {
        self.bodies.clear();
        self.widest = 0.0;
    }

    /// Adds a unit as `spatial::rebuild` passes it (one pass over the units does both).
    pub fn add(&mut self, e: Entity, at: Vec2, radius: f32, free: bool, player: Option<Faction>) {
        let weight = if !free { 0.0 } else if player.is_some() { radius * radius * PLAYER_WEIGHT } else { radius * radius };
        self.widest = self.widest.max(radius);
        self.bodies.push((e, at, radius, weight, Vec2::ZERO, player));
    }

    /// The bodies `separate` pushed: who, from where, their radius, and the push.
    pub fn pushes(&self) -> impl Iterator<Item = (Entity, Vec2, f32, Vec2)> + '_ {
        self.bodies.iter().filter(|b| b.4 != Vec2::ZERO).map(|&(e, from, r, _, push, _)| (e, from, r, push))
    }
}

/// Keeps bodies from overlapping: each pair that does is eased apart, the lighter body moving
/// further (bigger bodies outweigh smaller ones, and players outweigh monsters). Players who
/// can't harm each other pass through each other, so nobody can block a doorway or a bridge;
/// monsters, and players on opposing sides, still collide. The push turns a
/// little to one side, so a body caught behind another slides round it rather than queueing.
/// Bodies in forced motion (rolls, leaps, charges, climbs) neither push nor are pushed. Works out
/// the pushes for the bodies `spatial::rebuild` gathered, every other step; `shift` applies them.
pub fn separate(crowd: &mut Crowd) {
    let Crowd { bodies, order, widest } = crowd;
    let widest = *widest;
    // Bodies sorted into east-west bands as tall as the widest pair, west to east within each:
    // a body only overlaps bodies in its own band or the next, within that width east or west.
    let span = 2.0 * widest;
    if order.len() != bodies.len() {
        order.clear();
        order.extend((0..bodies.len() as u32).map(|i| (0, 0.0, i)));
    }
    for o in order.iter_mut() {
        let p = bodies[o.2 as usize].1;
        *o = ((p.y / span).floor() as i32, p.x, o.2);
    }
    let before = |a: &(i32, f32, u32), b: &(i32, f32, u32)| a.0 < b.0 || (a.0 == b.0 && a.1 < b.1);
    for k in 1..order.len() {
        let mut j = k;
        while j > 0 && before(&order[j], &order[j - 1]) {
            order.swap(j - 1, j);
            j -= 1;
        }
    }
    let (turn_cos, turn_sin) = (SIDESTEP.cos(), SIDESTEP.sin());
    let shove = |bodies: &mut [Member], i: usize, j: usize| {
        let (_, pa, ra, wa, _, sa) = bodies[i];
        let (_, pb, rb, wb, _, sb) = bodies[j];
        if wb == 0.0 {
            return;
        }
        if let (Some(fa), Some(fb)) = (sa, sb) && !fa.is_enemy(fb) {
            return;
        }
        let d = pa - pb;
        let reach = ra + rb;
        let dist2 = d.length_squared();
        if dist2 >= reach * reach {
            return;
        }
        let dist = dist2.sqrt();
        // Stacked exactly: part along a direction fixed by their order.
        let away = if dist > 1e-4 { d / dist } else { Vec2::new((i as f32).sin(), (i as f32).cos()) };
        let away = Vec2::new(away.x * turn_cos - away.y * turn_sin, away.x * turn_sin + away.y * turn_cos);
        let push = away * (reach - dist) * SEPARATE / (wa + wb);
        bodies[i].4 += push * wb;
        bodies[j].4 -= push * wa;
    };
    let mut k = 0;
    // Where the next band starts in `order`, and how far its scan has got.
    let (mut next, mut cursor) = (0, 0);
    while k < order.len() {
        let band = order[k].0;
        let end = order[k..].iter().position(|o| o.0 != band).map_or(order.len(), |n| k + n);
        let next_end = if end < order.len() && order[end].0 == band + 1 { order[end..].iter().position(|o| o.0 != band + 1).map_or(order.len(), |n| end + n) } else { end };
        if next != end {
            next = end;
            cursor = end;
        }
        for a in k..end {
            let (_, x, i) = order[a];
            if bodies[i as usize].3 == 0.0 {
                continue;
            }
            for &(_, xb, j) in &order[a + 1..end] {
                if xb - x >= span {
                    break;
                }
                shove(bodies, i as usize, j as usize);
            }
            while cursor < next_end && order[cursor].1 <= x - span {
                cursor += 1;
            }
            for &(_, xb, j) in &order[cursor..next_end] {
                if xb - x >= span {
                    break;
                }
                shove(bodies, i as usize, j as usize);
            }
        }
        k = end;
    }

}

/// Moves a body by the push `separate` gave it, along any wall in the way, unless that would
/// carry it up a wall or off a cliff.
pub fn shift(land: &world::Valley, t: &mut Transform, m: &Motion, from: Vec2, push: Vec2, radius: f32) {
    let grounded = m.grounded && !m.swimming;
    let to = land.off_faces(land.slide(from, push, radius), radius, t.pos.y + CLIMB_HEIGHT);
    let drop = t.pos.y - land.ground(to.x, to.y);
    if steep(land, from, t.pos.y, to) || (grounded && drop > MAX_SLOPE * (to - from).length() + 0.01) {
        return;
    }
    t.pos.x = to.x;
    t.pos.z = to.y;
    if grounded {
        t.pos.y = land.ground(to.x, to.y);
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
                // The air dash's own poses are #353.
                ForcedKind::Leap | ForcedKind::AirDash => Pose::Jump,
                ForcedKind::Charge { .. } => Pose::Run,
                ForcedKind::Climb | ForcedKind::Mantle | ForcedKind::Vault { .. } | ForcedKind::WallRun { .. } => Pose::Jump,
                ForcedKind::ToHold { .. } | ForcedKind::Corner { .. } => Pose::Hang,
            }
        } else if m.hang.is_some() {
            Pose::Hang
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
    // Stopped by masonry too tall to step onto, as a walker is (#311).
    let end = land.slide_below(Vec2::new(t.pos.x, t.pos.z), dir * DODGE_DISTANCE, body.radius, t.pos.y + CLIMB_HEIGHT);
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
