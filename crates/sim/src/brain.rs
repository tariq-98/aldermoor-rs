//! Monster minds: they wander about home, notice enemies, fight (reaching for their skills when
//! the moment suits), give up past the leash or when nobody's fighting back, and walk home.
//! Summoned helpers follow their owner and fight what it fights.
//!
//! Decisions (who to fight, which skill) are made four times a second, spread across the steps;
//! steering toward the current goal happens every step.

use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::UnitId;

use crate::combat::{abilities, auras, can_attack, chest, control, effects, entity, gap, line_of_sight, now, source, unit_id};
use crate::content::{catalog, MobDef, Targeting};
use crate::movement::heading;
use crate::spatial::Spatial;
use crate::{Body, Brain, Fighter, Land, Mode, Motion, MoveIntent, PlayerControl, Rng, Summoned, Threat, Transform, Vitals};

/// How far a monster follows before giving up and walking home.
pub const LEASH: f32 = 45.0;
/// A summon strays no further than this from its owner.
const SUMMON_LEASH: f32 = 25.0;
/// Monsters give up on a fight nobody has touched for this long.
const BORED: f64 = 12.0;

pub fn think(world: &mut World) {
    let t = now(world);
    let cat = catalog();
    let entities = crate::take_units(world);
    for &e in &entities {
        let Some(b) = world.get::<Brain>(e).copied() else { continue };
        if world.get::<PlayerControl>(e).is_some() || world.get::<Vitals>(e).is_none_or(|v| !v.alive()) {
            continue;
        }
        let deciding = t >= b.next_think;
        if deciding {
            world.get_mut::<Brain>(e).unwrap().next_think = t + 0.25;
        }
        let Some(mob) = cat.mob(b.mob) else { continue };
        // A summon's home is wherever its owner stands.
        if let Some(s) = world.get::<Summoned>(e).copied()
            && let Some(o) = entity(world, s.owner).and_then(|o| world.get::<Transform>(o)).map(|t| t.xz())
        {
                world.get_mut::<Brain>(e).unwrap().home = o;
            }
        match b.mode {
            Mode::Return => go_home(world, e),
            Mode::Chase => chase(world, e, mob, deciding),
            Mode::Idle => idle(world, e, deciding),
        }
    }
    crate::return_units(world, entities);
}

/// When a monster first thinks: decisions are spread over the 15 steps of a quarter-second.
pub fn think_offset(id: protocol::UnitId) -> f64 {
    (id % 15) as f64 / crate::TICK_RATE as f64
}

fn steer(world: &mut World, e: Entity, intent: MoveIntent) {
    if let Some(mut m) = world.get_mut::<Motion>(e) {
        m.intent = intent;
    }
}

fn here(world: &World, e: Entity) -> Vec2 {
    world.get::<Transform>(e).map_or(Vec2::ZERO, |t| t.xz())
}

fn go_home(world: &mut World, e: Entity) {
    let home = world.get::<Brain>(e).unwrap().home;
    let to = home - here(world, e);
    if to.length() < 1.5 {
        let t = now(world);
        let wait = world.resource_mut::<Rng>().range(2.0, 6.0) as f64;
        if let Some(mut b) = world.get_mut::<Brain>(e) {
            b.mode = Mode::Idle;
            b.next_wander = t + wait;
        }
        if let Some(mut th) = world.get_mut::<Threat>(e) {
            th.clear();
        }
        auras::remove_key(world, e, "evading");
        steer(world, e, MoveIntent::default());
        return;
    }
    // Untouchable all the way home, so pulling and dropping a monster can't wear it down.
    if !auras::has(world, e, "evading") && let Some(src) = source(world, e, None) {
        auras::apply_key(world, "evading", e, &src);
    }
    steer(world, e, MoveIntent { direction: to.normalize(), walk: false, sprint: false, face_yaw: None });
}

fn chase(world: &mut World, e: Entity, mob: &MobDef, deciding: bool) {
    let t = now(world);
    let cat = catalog();
    let b = *world.get::<Brain>(e).unwrap();
    let pos = here(world, e);
    let summoned = world.get::<Summoned>(e).is_some();
    let leash = if summoned { SUMMON_LEASH } else { LEASH };
    let last_combat = world.get::<Vitals>(e).map_or(t, |v| v.last_combat);
    let current = world.get::<Fighter>(e).and_then(|f| f.target);

    let pick = {
        let valid = |id| {
            entity(world, id).is_some_and(|te| can_attack(world, e, te) && world.get::<Transform>(te).is_some_and(|tt| (tt.xz() - b.home).length() < leash))
        };
        let melee = |id| entity(world, id).is_some_and(|te| gap(world, e, te) < 4.0);
        world.get::<Threat>(e).and_then(|th| th.pick(current, t, valid, melee))
    };
    let bored = !summoned && t - last_combat.max(b.engaged_at) > BORED;
    let strayed = (pos - b.home).length() > leash;
    let Some(target) = pick.filter(|_| !bored && !strayed) else {
        if summoned {
            stand_down(world, e);
        } else {
            give_up(world, e);
        }
        return;
    };
    let Some(te) = entity(world, target) else { return };
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.target = Some(target);
        f.auto_attack = true;
    }
    let to = world.get::<Transform>(te).map_or(Vec2::ZERO, |t| t.xz()) - pos;

    // Winding up or channelling: stand still. A warned blow keeps the aim it started with.
    if let Some(c) = world.get::<Fighter>(e).and_then(|f| f.cast) {
        let def = cat.ability(c.ability);
        let face = if def.telegraph || def.targeting.is_area() { c.yaw } else { heading(to) };
        steer(world, e, MoveIntent { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: Some(face) });
        return;
    }
    if deciding && control(world, e).can_act() && use_skills(world, e, mob, te) {
        steer(world, e, MoveIntent { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: None });
        return;
    }
    // Close in (or keep to its range), then face the target.
    let reach = world.get::<Fighter>(e).and_then(|f| f.auto).map_or(2.0, |a| cat.ability(a).range);
    let want = if mob.range > 0.0 { mob.range.min(reach) } else { reach };
    let in_sight = mob.range <= 0.0 || line_of_sight(world, chest(world, e), chest(world, te));
    let g = gap(world, e, te);
    // Up close, a fighter makes for its own place round the target.
    if mob.range <= 0.0 && g < want + SPREAD_FROM {
        let (slot, ring) = match b.slot {
            Some(s) if !deciding => s,
            _ => {
                let s = find_slot(world, e, te, want);
                world.get_mut::<Brain>(e).unwrap().slot = Some(s);
                s
            }
        };
        let goal = to + Vec2::from_angle(slot) * ring;
        let intent = if goal.length() > 0.25 || g > want - 0.3 {
            MoveIntent { direction: goal.normalize_or_zero(), walk: false, sprint: false, face_yaw: None }
        } else {
            MoveIntent { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: (to.length_squared() > 1e-4).then(|| heading(to)) }
        };
        steer(world, e, intent);
        return;
    }
    if b.slot.is_some() {
        world.get_mut::<Brain>(e).unwrap().slot = None;
    }
    let intent = if g > want - 0.3 || !in_sight {
        MoveIntent { direction: to.normalize_or_zero(), walk: false, sprint: false, face_yaw: None }
    } else {
        MoveIntent { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: (to.length_squared() > 1e-4).then(|| heading(to)) }
    };
    steer(world, e, intent);
}

/// How far past its reach a melee monster starts making for its own place round the target.
const SPREAD_FROM: f32 = 2.5;

/// How far from the target's body a melee fighter with this reach stands.
fn stand(reach: f32) -> f32 {
    ((reach - 0.3) * 0.5).max(0.1)
}

/// Where round `te` that `e` should stand, as a bearing and a distance from its centre: where it
/// is now, nudged away from anyone else already standing round the target, so each keeps at least
/// a body's width of the circle to itself.
fn find_slot(world: &World, e: Entity, te: Entity, reach: f32) -> (f32, f32) {
    let (Some(at), Some(t)) = (world.get::<Transform>(e).map(|t| t.xz()), world.get::<Transform>(te).map(|t| t.xz())) else { return (0.0, 0.0) };
    let radius = |x| world.get::<Body>(x).map_or(0.4, |b| b.radius);
    let (rm, rt) = (radius(e), radius(te));
    let ring = rt + rm + stand(reach);
    let mine = (at - t).to_angle();
    let me = unit_id(world, e);
    let target = unit_id(world, te);
    let others: Vec<(UnitId, f32, f32, bool)> = world
        .resource::<Spatial>()
        .near(t, ring + rm + 1.0)
        .into_iter()
        .filter(|&(id, _)| Some(id) != me && Some(id) != target)
        .filter_map(|(id, p)| {
            let n = entity(world, id)?;
            Some((id, (p - t).to_angle(), radius(n), can_attack(world, n, te)))
        })
        .collect();
    // A pack shares the circle out evenly between its fighters, so it surrounds its prey.
    let share = std::f32::consts::TAU / (1 + others.iter().filter(|o| o.3).count()) as f32 * 0.9;
    let mut nudge = 0.0;
    for &(id, theirs, rn, fighting) in &others {
        // The angle two bodies side by side on the ring take up, with a little room to spare.
        let body = 2.0 * ((rm + rn + 0.2) / (2.0 * ring)).min(1.0).asin();
        let need = if fighting { body.max(share) } else { body };
        let apart = wrap(mine - theirs);
        if apart.abs() < need {
            // Dead level: the higher id gives way to the left, the lower to the right.
            let side = if apart.abs() > 1e-3 { apart.signum() } else if me > Some(id) { 1.0 } else { -1.0 };
            nudge += side * (need - apart.abs()) * 0.5;
        }
    }
    (mine + nudge.clamp(-0.6, 0.6), ring)
}

/// An angle brought into -pi..pi.
fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

/// Tries the monster's skills in order: the first that's ready, suits its health, reaches the
/// target and passes its roll goes off. Returns whether one did.
fn use_skills(world: &mut World, e: Entity, mob: &MobDef, te: Entity) -> bool {
    let t = now(world);
    let cat = catalog();
    let health = world.get::<Vitals>(e).map_or(1.0, |v| v.fraction());
    let target = unit_id(world, te);
    for s in &mob.skills {
        let Some(id) = cat.ability_id(s.ability) else { continue };
        let def = cat.ability(id);
        let Some(f) = world.get::<Fighter>(e) else { return false };
        if t < f.ready_at(id) || (def.gcd && t < f.gcd_until) || health >= s.below {
            continue;
        }
        let g = gap(world, e, te);
        let reaches = match def.targeting {
            Targeting::Enemy | Targeting::Ground { .. } => g <= def.range,
            Targeting::AroundSelf { radius } => def.allies || g <= radius - 0.3,
            Targeting::Cone { .. } | Targeting::Line { .. } => g <= def.range - 0.5,
            Targeting::SelfOnly | Targeting::Ally => true,
        };
        if !reaches || !world.resource_mut::<Rng>().chance(s.chance) {
            continue;
        }
        if abilities::use_ability(world, e, id, target, None, true) {
            return true;
        }
    }
    false
}

fn idle(world: &mut World, e: Entity, deciding: bool) {
    let t = now(world);
    let b = *world.get::<Brain>(e).unwrap();
    let pos = here(world, e);
    let summoned = world.get::<Summoned>(e).copied();
    if deciding {
        // A summon joins its owner's fight.
        if let Some(s) = summoned {
            let theirs = entity(world, s.owner).and_then(|o| world.get::<Fighter>(o)).and_then(|f| f.target);
            if let Some(tid) = theirs.filter(|&tid| entity(world, tid).is_some_and(|te| can_attack(world, e, te))) {
                effects::engage(world, e, tid);
                return;
            }
        }
        // Notice the nearest enemy inside the aggro circle, if it's in sight.
        let near = world.resource::<Spatial>().near(pos, b.aggro_radius);
        let eyes = chest(world, e);
        let seen = near
            .into_iter()
            .filter_map(|(id, p)| {
                let te = entity(world, id)?;
                (can_attack(world, e, te) && line_of_sight(world, eyes, chest(world, te))).then_some((id, (p - pos).length_squared()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        if let Some((id, _)) = seen {
            effects::engage(world, e, id);
            return;
        }
    }
    if summoned.is_some() {
        // Keep up with the owner.
        let to = b.home - pos;
        let intent = if to.length() > 3.0 { MoveIntent { direction: to.normalize(), walk: false, sprint: false, face_yaw: None } } else { MoveIntent::default() };
        steer(world, e, intent);
        return;
    }
    // Wander about home now and then.
    let radius = world.get::<Body>(e).map_or(0.4, |b| b.radius);
    match b.wander_target {
        Some(w) if (w - pos).length() > 0.5 && t < b.next_wander + 12.0 => {
            steer(world, e, MoveIntent { direction: (w - pos).normalize(), walk: true, sprint: false, face_yaw: None });
        }
        Some(_) => {
            let wait = world.resource_mut::<Rng>().range(4.0, 12.0) as f64;
            if let Some(mut b) = world.get_mut::<Brain>(e) {
                b.wander_target = None;
                b.next_wander = t + wait;
            }
            steer(world, e, MoveIntent::default());
        }
        None if t >= b.next_wander && b.wander > 0.0 => {
            let (a, r) = {
                let mut rng = world.resource_mut::<Rng>();
                (rng.range(0.0, std::f32::consts::TAU), rng.range(1.0, b.wander))
            };
            let w = b.home + Vec2::new(a.sin(), a.cos()) * r;
            let land = &world.resource::<Land>().0;
            // Monsters wander on dry land.
            let clear = !land.blocked(w, radius) && land.water_depth(w.x, w.y) == 0.0;
            if let Some(mut b) = world.get_mut::<Brain>(e) {
                if clear {
                    b.wander_target = Some(w);
                }
                b.next_wander = t;
            }
        }
        None => steer(world, e, MoveIntent::default()),
    }
}

/// Walks home, untouchable, and forgets the fight.
fn give_up(world: &mut World, e: Entity) {
    abilities::cancel_cast(world, e, false);
    if let Some(mut b) = world.get_mut::<Brain>(e) {
        b.mode = Mode::Return;
        b.wander_target = None;
    }
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.target = None;
        f.auto_attack = false;
        f.queued = None;
    }
    if let Some(mut th) = world.get_mut::<Threat>(e) {
        th.clear();
    }
    if let Some(src) = source(world, e, None) {
        auras::apply_key(world, "evading", e, &src);
    }
    steer(world, e, MoveIntent::default());
}

/// A summon with nothing left to fight goes back to its owner's side.
fn stand_down(world: &mut World, e: Entity) {
    if let Some(mut b) = world.get_mut::<Brain>(e) {
        b.mode = Mode::Idle;
    }
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.target = None;
        f.auto_attack = false;
    }
    if let Some(mut th) = world.get_mut::<Threat>(e) {
        th.clear();
    }
}
