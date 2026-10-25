//! What abilities do: damage and healing, auras, knockbacks and pulls, interrupts, taunts,
//! movement skills, cleanses, revives, fields and summons; who areas reach; and combos.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Avoid, Event, Pose, School, Shape};

use super::{auras, can_attack, chest, control, emit, entity, guard, line_of_sight, now, unit_id, HitSource};
use crate::content::{catalog, AbilityDef, Effect, Field, Finisher};
use crate::movement::{heading, ATTACK_POSE, HIT_POSE};
use crate::spatial::Spatial;
use crate::stats::{armor_reduction, scale, CRIT_MULTIPLIER};
use crate::{Anim, Auras, Body, Brain, Faction, Fighter, Forced, ForcedKind, Land, Mode, Motion, Rng, Stats, Threat, Transform, Unit, Vitals};

/// Blows from behind land 10% harder and can't be blocked.
pub const FLANK: f32 = 1.1;

/// How an effect arrives, which decides what can stop it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// A blow, or a blast on the ground: guards stop it.
    Strike,
    /// Only shields stop missiles.
    Missile,
    /// A field's pulse under your feet: nothing guards against it.
    Pulse,
}

/// Lands damage on `te`: immunity, then the attacker's and target's modifiers, armour for
/// physical blows, the crit, and shields. Returns what got through.
pub fn deal_damage(world: &mut World, src: &HitSource, te: Entity, raw: f32, crit: bool, periodic: bool) -> f32 {
    let t = now(world);
    let Some(target) = unit_id(world, te) else { return 0.0 };
    if world.get::<Vitals>(te).is_none_or(|v| v.dead) || raw <= 0.0 {
        return 0.0;
    }
    if auras::sum(world, te, |a| if a.invulnerable { 1.0 } else { 0.0 }) > 0.0 {
        emit(world, Event::Avoided { target, source: src.unit, how: Avoid::Immune });
        return 0.0;
    }
    let attacker = entity(world, src.unit);
    let mut amount = raw;
    if let Some(a) = attacker {
        amount *= 1.0 + auras::sum(world, a, |d| d.damage_done);
    }
    amount *= (1.0 + auras::sum(world, te, |d| d.damage_taken)).max(0.1);
    if src.school == School::Physical {
        let armor = world.get::<Stats>(te).map_or(0.0, |s| s.armor) * (1.0 + auras::sum(world, te, |d| d.armor));
        amount *= 1.0 - armor_reduction(armor, src.level);
    }
    if crit {
        amount *= CRIT_MULTIPLIER;
    }
    let amount = amount.round().max(1.0);

    // Shields take what they can.
    let mut absorbed = 0.0;
    if let Some(mut a) = world.get_mut::<Auras>(te) {
        for inst in a.list.iter_mut() {
            if absorbed >= amount {
                break;
            }
            if inst.absorb_left > 0.0 {
                let take = inst.absorb_left.min(amount - absorbed);
                inst.absorb_left -= take;
                absorbed += take;
            }
        }
    }
    let dealt = amount - absorbed;
    if absorbed > 0.0 {
        emit(world, Event::Avoided { target, source: src.unit, how: Avoid::Absorbed });
    }
    if dealt > 0.0 {
        emit(world, Event::Damage { target, source: src.unit, amount: dealt, crit, school: src.school, periodic });
        auras::remove_where(world, te, |d| d.breaks_on_damage);
    }
    let (lethal, finished) = {
        let mut v = world.get_mut::<Vitals>(te).unwrap();
        v.last_combat = t;
        if v.downed {
            v.downed_health -= dealt;
            (false, v.downed_health <= 0.0)
        } else {
            v.health -= dealt;
            (v.health <= 0.0, false)
        }
    };
    if let Some(a) = attacker {
        if let Some(mut v) = world.get_mut::<Vitals>(a) {
            v.last_combat = t;
        }
        if let Some(mut f) = world.get_mut::<Fighter>(a) {
            match f.recent_victims.iter_mut().find(|(id, _)| *id == target) {
                Some(r) => r.1 = t,
                None => f.recent_victims.push((target, t)),
            }
        }
    }
    // Monsters turn on whoever hurts them, and remember it.
    if world.get::<Threat>(te).is_some() {
        let mult = attacker.map_or(1.0, |a| auras::threat_multiplier(world, a));
        if let Some(mut th) = world.get_mut::<Threat>(te) {
            th.add(src.unit, (dealt + absorbed) * mult);
        }
        engage(world, te, src.unit);
    }
    // Rage builds from being hit.
    let max_health = world.get::<Vitals>(te).map_or(100.0, |v| v.max_health).max(1.0);
    if let Some(mut f) = world.get_mut::<Fighter>(te) && f.resource_kind == protocol::ResourceKind::Rage {
        f.resource = (f.resource + dealt / max_health * 60.0).min(f.max_resource);
    }
    if !periodic && dealt > 0.0 {
        let attacking = world.get::<Anim>(te).is_some_and(|a| a.pose == Pose::Attack && t < a.hold_until);
        let casting = world.get::<Fighter>(te).is_some_and(|f| f.cast.is_some());
        if !attacking && !casting && !lethal {
            hold_pose(world, te, Pose::Hit, t, HIT_POSE);
        }
    }
    if lethal {
        super::life::lethal(world, te, Some(src.unit));
    } else if finished {
        super::life::die(world, te, Some(src.unit));
    }
    dealt
}

/// Hurts `te` for a hard landing: no attacker, armour or shields, and no threat. Can down or
/// kill, as any blow can.
pub fn fall_damage(world: &mut World, te: Entity, amount: f32) {
    let t = now(world);
    let Some(target) = unit_id(world, te) else { return };
    if world.get::<Vitals>(te).is_none_or(|v| v.dead) {
        return;
    }
    let amount = amount.round().max(1.0);
    emit(world, Event::Damage { target, source: target, amount, crit: false, school: School::Physical, periodic: false });
    let (lethal, finished) = {
        let mut v = world.get_mut::<Vitals>(te).unwrap();
        if v.downed {
            v.downed_health -= amount;
            (false, v.downed_health <= 0.0)
        } else {
            v.health -= amount;
            (v.health <= 0.0, false)
        }
    };
    if lethal {
        super::life::lethal(world, te, None);
    } else if finished {
        super::life::die(world, te, None);
    } else {
        hold_pose(world, te, Pose::Hit, t, HIT_POSE);
    }
}

/// Heals `te`; a downed unit's heals go to its downed pool. Healing someone in a fight draws a
/// little of their attackers' attention.
pub fn heal(world: &mut World, src: &HitSource, te: Entity, raw: f32, crit: bool) -> f32 {
    let Some(target) = unit_id(world, te) else { return 0.0 };
    if world.get::<Vitals>(te).is_none_or(|v| v.dead) || raw <= 0.0 {
        return 0.0;
    }
    let mut amount = raw * (1.0 + auras::sum(world, te, |d| d.healing_taken)).max(0.0);
    if crit {
        amount *= CRIT_MULTIPLIER;
    }
    let amount = amount.round();
    if amount <= 0.0 {
        return 0.0;
    }
    let (effective, filled) = {
        let mut v = world.get_mut::<Vitals>(te).unwrap();
        if v.downed {
            let e = amount.min(v.downed_max - v.downed_health);
            v.downed_health += e;
            (e, e > 0.0 && v.downed_health >= v.downed_max - 0.01)
        } else {
            let e = amount.min(v.max_health - v.health);
            v.health += e;
            (e, false)
        }
    };
    emit(world, Event::Heal { target, source: src.unit, amount, crit });
    // Healing a downed unit's pool back to full gets them up: help from someone else, or the
    // downed skills (a self-heal that merely ticks on doesn't).
    let own_downed_skill = src.ability.is_some_and(|a| catalog().ability(a).downed);
    if filled && (src.unit != target || own_downed_skill) {
        super::life::rally(world, te, super::life::RALLY_HEALTH);
    }
    if effective > 0.0 && src.faction == Faction::Player {
        let healer_mult = entity(world, src.unit).map_or(1.0, |h| auras::threat_multiplier(world, h));
        let mut q = world.query::<(&mut Threat, &Brain)>();
        for (mut th, b) in q.iter_mut(world) {
            if b.mode == Mode::Chase && th.contains(target) {
                th.add(src.unit, effective * 0.5 * healer_mult);
            }
        }
    }
    effective
}

pub(crate) fn hold_pose(world: &mut World, e: Entity, pose: Pose, now: f64, seconds: f64) {
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        a.pose = pose;
        a.since = now;
        a.hold_until = now + seconds;
    }
}

/// A monster starts fighting `attacker`, and calls the members of its pack close by (they
/// don't call theirs in turn, so pulling one wolf doesn't pull the whole meadow).
pub fn engage(world: &mut World, npc: Entity, attacker: protocol::UnitId) {
    engage_with(world, npc, attacker, true);
}

fn engage_with(world: &mut World, npc: Entity, attacker: protocol::UnitId, call_pack: bool) {
    let t = now(world);
    let Some(b) = world.get::<Brain>(npc).copied() else { return };
    if b.mode == Mode::Return || world.get::<Vitals>(npc).is_none_or(|v| !v.alive()) {
        return;
    }
    let was_idle = b.mode == Mode::Idle;
    if let Some(mut b) = world.get_mut::<Brain>(npc) {
        b.mode = Mode::Chase;
        if was_idle {
            b.engaged_at = t;
            b.wander_target = None;
        }
    }
    if let Some(mut th) = world.get_mut::<Threat>(npc) {
        th.add(attacker, 1.0);
    }
    if let Some(mut f) = world.get_mut::<Fighter>(npc) {
        if f.target.is_none() {
            f.target = Some(attacker);
        }
        f.auto_attack = true;
    }
    if !call_pack || !was_idle || !catalog().mob(b.mob).is_some_and(|m| m.social) {
        return;
    }
    let here = world.get::<Transform>(npc).map_or(Vec2::ZERO, |t| t.xz());
    let pack: Vec<Entity> = {
        let spatial = world.resource::<Spatial>();
        spatial.near(here, 10.0).into_iter().filter_map(|(id, _)| entity(world, id)).collect()
    };
    for other in pack {
        if other != npc && world.get::<Brain>(other).is_some_and(|o| o.mob == b.mob && o.mode == Mode::Idle) {
            engage_with(world, other, attacker, false);
        }
    }
}

/// Who an area reaches: enemies (or friends) of the source inside the shape, in sight of its
/// centre, nearest first.
pub fn victims(world: &World, shape: Shape, origin: Vec3, yaw: f32, src: &HitSource, allies: bool, max: u8) -> Vec<Entity> {
    let reach = match shape {
        Shape::Circle { radius } => radius,
        Shape::Cone { range, .. } => range,
        Shape::Line { length, .. } => length,
    };
    let o2 = Vec2::new(origin.x, origin.z);
    let spatial = world.resource::<Spatial>();
    let mut found: Vec<(f32, Entity)> = Vec::new();
    for (id, _) in spatial.near(o2, reach + 2.0) {
        let Some(e) = entity(world, id) else { continue };
        let (Some(u), Some(v), Some(t)) = (world.get::<Unit>(e), world.get::<Vitals>(e), world.get::<Transform>(e)) else { continue };
        let r = world.get::<Body>(e).map_or(0.4, |b| b.radius);
        if !in_shape(shape, origin, yaw, t.xz(), r) {
            continue;
        }
        if allies {
            if u.faction != src.faction {
                continue;
            }
        } else if !u.faction.is_enemy(src.faction) || v.dead {
            continue;
        }
        if !line_of_sight(world, origin + Vec3::Y, chest(world, e)) {
            continue;
        }
        found.push(((t.xz() - o2).length_squared(), e));
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().take(max as usize).map(|(_, e)| e).collect()
}

pub fn in_shape(shape: Shape, origin: Vec3, yaw: f32, p: Vec2, radius: f32) -> bool {
    let d = p - Vec2::new(origin.x, origin.z);
    let dist = d.length();
    match shape {
        Shape::Circle { radius: r } => dist <= r + radius,
        Shape::Cone { range, angle } => {
            if dist > range + radius {
                return false;
            }
            if dist < radius + 0.5 {
                return true;
            }
            let fwd = Vec2::new(yaw.sin(), yaw.cos());
            let off = fwd.dot(d / dist).clamp(-1.0, 1.0).acos();
            off <= angle * 0.5 + (radius / dist.max(0.1)).atan()
        }
        Shape::Line { length, width } => {
            let fwd = Vec2::new(yaw.sin(), yaw.cos());
            let along = d.dot(fwd);
            let across = (d.x * fwd.y - d.y * fwd.x).abs();
            along >= -radius && along <= length + radius && across <= width * 0.5 + radius
        }
    }
}

pub fn shape_of(def: &AbilityDef) -> Option<Shape> {
    use crate::content::Targeting::*;
    match def.targeting {
        Ground { radius } | AroundSelf { radius } => Some(Shape::Circle { radius }),
        Cone { angle } => Some(Shape::Cone { range: def.range, angle }),
        Line { width } => Some(Shape::Line { length: def.range, width }),
        _ => None,
    }
}

/// Runs an ability's effects: the user's own once, the rest on each target (unless it
/// dodges, blocks, parries or is immune).
pub fn apply_effects(world: &mut World, effects: &[Effect], src: &HitSource, def: Option<&AbilityDef>, targets: &[Entity], center: Vec3, delivery: Delivery) {
    let user = entity(world, src.unit);
    for e in effects.iter().filter(|e| e.on_user()) {
        let Some(u) = user else { continue };
        match e {
            Effect::Aura { key, .. } => {
                // Stances are toggles: using one again drops it.
                let toggle = catalog().aura_id(key).is_some_and(|id| catalog().aura(id).duration >= 3600.0);
                if toggle && auras::has(world, u, key) {
                    auras::remove_key(world, u, key);
                } else {
                    auras::apply_key(world, key, u, src);
                }
            }
            Effect::Resource(n) => {
                if let Some(mut f) = world.get_mut::<Fighter>(u) {
                    f.resource = (f.resource + n).clamp(0.0, f.max_resource);
                }
            }
            Effect::Dash { distance } => {
                let fwd = world.get::<Transform>(u).map_or(Vec2::Y, |t| t.forward());
                leap(world, u, fwd * *distance, 0.3, 0.4, ForcedKind::Leap);
            }
            Effect::Blink { distance } => blink(world, u, *distance),
            Effect::Shadowstep => {
                if let Some(&t) = targets.first() {
                    shadowstep(world, u, t);
                }
            }
            Effect::SelfHeal { base, coeff } => {
                heal(world, src, u, base * scale(src.level) + coeff * src.power, false);
            }
            Effect::Field { field, radius, duration, interval, enemies, allies } => {
                super::missiles::spawn_field(world, src, *field, *radius, *duration, *interval, enemies.clone(), allies.clone(), center);
            }
            Effect::Trap { radius, lasts, enemies } => super::missiles::spawn_trap(world, src, *radius, *lasts, enemies.clone(), center),
            Effect::Summon { mob, count, lasts } => super::life::summon(world, u, mob, *count, *lasts),
            Effect::Charge => {
                // The rest of the ability lands when the user arrives.
                if let (Some(&t), Some(d)) = (targets.first(), def) && Some(t) != user && let Some(id) = catalog().ability_id(d.key) {
                    charge(world, u, t, id);
                    return;
                }
            }
            _ => {}
        }
    }
    let rest: Vec<&Effect> = effects.iter().filter(|e| !e.on_user()).collect();
    if rest.is_empty() {
        return;
    }
    for &t in targets {
        let hostile = user.is_none_or(|u| can_attack(world, u, t)) || world.get::<Unit>(t).is_some_and(|tu| tu.faction.is_enemy(src.faction));
        if hostile && let Some(how) = guard::avoid(world, src, t, def, delivery) {
            let target = unit_id(world, t).unwrap_or(0);
            emit(world, Event::Avoided { target, source: src.unit, how });
            continue;
        }
        let crit = world.resource_mut::<Rng>().chance(src.crit);
        // Striking from behind: harder, and backstabs harder still.
        let mut mult = 1.0;
        if hostile && guard::behind(world, t, src.from) {
            mult *= FLANK * def.map_or(1.0, |d| d.backstab);
        }
        let mut hurt = false;
        for e in &rest {
            hurt |= apply(world, e, t, src, def, crit, center, mult);
        }
        if hurt && let Some(u) = user {
            auras::remove_key(world, u, "riposte");
        }
    }
}

/// One effect on one target. Returns whether it did damage.
#[allow(clippy::too_many_arguments)]
fn apply(world: &mut World, e: &Effect, te: Entity, src: &HitSource, def: Option<&AbilityDef>, crit: bool, center: Vec3, mult: f32) -> bool {
    match e {
        Effect::Damage { base, coeff } => {
            let mut amount = (base * scale(src.level) + coeff * src.power) * mult;
            if let Some((below, times)) = def.and_then(|d| d.execute) && world.get::<Vitals>(te).is_some_and(|v| v.fraction() < below) {
                amount *= times;
            }
            deal_damage(world, src, te, amount, crit, false) > 0.0
        }
        Effect::Heal { base, coeff } => {
            heal(world, src, te, base * scale(src.level) + coeff * src.power, crit);
            false
        }
        Effect::Aura { key, .. } => {
            auras::apply_key(world, key, te, src);
            false
        }
        Effect::Knockback { distance } => {
            let p = world.get::<Transform>(te).map_or(Vec2::ZERO, |t| t.xz());
            let from = Vec2::new(src.from.x, src.from.z);
            let mut dir = p - from;
            if dir.length_squared() < 1e-4 {
                dir = -world.get::<Transform>(te).map_or(Vec2::Y, |t| t.forward());
            }
            knock(world, te, dir.normalize(), *distance, src);
            false
        }
        Effect::Pull { distance } => {
            let (Some(u), Some(tt)) = (entity(world, src.unit), world.get::<Transform>(te).copied()) else { return false };
            let Some(ut) = world.get::<Transform>(u).copied() else { return false };
            let d = ut.xz() - tt.xz();
            let reach = world.get::<Body>(u).map_or(0.4, |b| b.radius) + world.get::<Body>(te).map_or(0.4, |b| b.radius) + 0.3;
            let dist = d.length();
            if dist > reach {
                knock(world, te, d / dist, distance.min(dist - reach), src);
            }
            false
        }
        Effect::Interrupt { lockout } => {
            interrupt(world, te, *lockout, src);
            false
        }
        Effect::Taunt => {
            if world.get::<Threat>(te).is_some() && world.get::<Vitals>(te).is_some_and(|v| v.alive()) {
                engage(world, te, src.unit);
                let until = now(world) + 3.0;
                if let Some(mut th) = world.get_mut::<Threat>(te) {
                    th.taunt(src.unit, until);
                }
                if let Some(mut f) = world.get_mut::<Fighter>(te) {
                    f.target = Some(src.unit);
                }
            }
            false
        }
        Effect::Defiance(n) => {
            auras::chip_defiance(world, te, *n, src);
            false
        }
        Effect::Cleanse { count } => {
            auras::cleanse(world, te, *count);
            false
        }
        Effect::Revive { fraction } => {
            let (downed, dead) = world.get::<Vitals>(te).map_or((false, false), |v| (v.downed, v.dead));
            if downed {
                super::life::rally(world, te, *fraction);
            } else if dead {
                super::life::revive_dead(world, te, *fraction);
            }
            let _ = center;
            false
        }
        _ => false,
    }
}

/// Throws a unit: it can't act until it lands. Elites soak it into their defiance.
pub fn knock(world: &mut World, te: Entity, dir: Vec2, distance: f32, src: &HitSource) {
    if world.get::<Vitals>(te).is_none_or(|v| !v.alive()) || distance < 0.1 {
        return;
    }
    let ctl = control(world, te);
    if ctl.cc_immune || ctl.invulnerable {
        let target = unit_id(world, te).unwrap_or(0);
        emit(world, Event::Avoided { target, source: src.unit, how: Avoid::Immune });
        return;
    }
    if world.get::<Auras>(te).is_some_and(|a| a.defiance_max > 0.0) {
        auras::chip_defiance(world, te, crate::stats::DEFIANCE_PER_SECOND * 1.5, src);
        return;
    }
    super::abilities::cancel_cast(world, te, true);
    guard::lower(world, te);
    leap(world, te, dir * distance, 0.35, 0.7, ForcedKind::Knockback);
}

/// Carries a unit `delta` over the ground (sliding along walls) in `seconds`, with a hop.
pub fn leap(world: &mut World, e: Entity, delta: Vec2, seconds: f64, arc: f32, kind: ForcedKind) {
    let t = now(world);
    let Some(tr) = world.get::<Transform>(e).copied() else { return };
    let radius = world.get::<Body>(e).map_or(0.4, |b| b.radius);
    let end = world.resource::<Land>().0.slide(tr.xz(), delta, radius);
    if let Some(mut m) = world.get_mut::<Motion>(e) {
        m.forced = Some(Forced { kind, from: tr.pos, to: Vec3::new(end.x, tr.pos.y, end.y), start: t, duration: seconds, arc });
    }
}

/// Rushes the user at a target; `abilities::land_charges` finishes the ability on arrival.
fn charge(world: &mut World, u: Entity, target: Entity, ability: crate::content::AbilityId) {
    let (Some(ut), Some(tt)) = (world.get::<Transform>(u).copied(), world.get::<Transform>(target).copied()) else { return };
    let Some(tid) = unit_id(world, target) else { return };
    let reach = world.get::<Body>(u).map_or(0.4, |b| b.radius) + world.get::<Body>(target).map_or(0.4, |b| b.radius) + 0.3;
    let d = tt.xz() - ut.xz();
    let dist = d.length();
    let travel = (dist - reach).max(0.0);
    let dir = if dist > 1e-3 { d / dist } else { ut.forward() };
    let seconds = (travel as f64 / 20.0).max(0.15);
    if let Some(mut tr) = world.get_mut::<Transform>(u) {
        tr.yaw = heading(dir);
    }
    leap(world, u, dir * travel, seconds, 0.3, ForcedKind::Charge { target: tid, ability });
}

fn blink(world: &mut World, u: Entity, distance: f32) {
    let Some(tr) = world.get::<Transform>(u).copied() else { return };
    let radius = world.get::<Body>(u).map_or(0.4, |b| b.radius);
    let land = &world.resource::<Land>().0;
    let end = land.slide(tr.xz(), tr.forward() * distance, radius);
    let y = land.ground(end.x, end.y);
    if let Some(mut tr) = world.get_mut::<Transform>(u) {
        tr.pos = Vec3::new(end.x, y, end.y);
    }
    if let Some(mut m) = world.get_mut::<Motion>(u) {
        m.forced = None;
        m.vel = Vec3::ZERO;
    }
}

/// Appears behind the target, facing it.
fn shadowstep(world: &mut World, u: Entity, target: Entity) {
    let Some(tt) = world.get::<Transform>(target).copied() else { return };
    let reach = world.get::<Body>(u).map_or(0.4, |b| b.radius) + world.get::<Body>(target).map_or(0.4, |b| b.radius) + 0.4;
    let spot = tt.xz() - tt.forward() * reach;
    let land = &world.resource::<Land>().0;
    if land.blocked(spot, 0.3) {
        return;
    }
    let y = land.ground(spot.x, spot.y);
    if let Some(mut tr) = world.get_mut::<Transform>(u) {
        tr.pos = Vec3::new(spot.x, y, spot.y);
        tr.yaw = tt.yaw;
    }
    if let Some(mut m) = world.get_mut::<Motion>(u) {
        m.forced = None;
        m.vel = Vec3::ZERO;
    }
}

/// Stops a cast and locks out skills for a while (casts that can be interrupted).
pub fn interrupt(world: &mut World, te: Entity, lockout: f64, src: &HitSource) {
    let cat = catalog();
    let Some(cast) = world.get::<Fighter>(te).and_then(|f| f.cast) else { return };
    if !cat.ability(cast.ability).interruptible {
        return;
    }
    super::abilities::cancel_cast(world, te, true);
    let until = now(world) + lockout;
    if let Some(mut f) = world.get_mut::<Fighter>(te) {
        f.lockout_until = until;
    }
    let _ = src;
}

/// The combo field (of the source's side) under `p`, if any.
pub fn field_at(world: &World, p: Vec3, faction: Faction) -> Option<Field> {
    let areas = world.resource::<super::missiles::Areas>();
    areas.list.iter().filter(|a| a.field.is_some() && a.src.faction == faction).find_map(|a| {
        let d = Vec2::new(a.center.x - p.x, a.center.z - p.z).length();
        if d <= a.radius { a.field } else { None }
    })
}

/// A finisher going off in a field: the combo's effects on whoever it reaches.
pub fn combo(world: &mut World, finisher: Finisher, at: Vec3, src: &HitSource, target: Option<Entity>) {
    if let Some(field) = field_at(world, at, src.faction) {
        combo_with(world, field, finisher, at, src, target);
    }
}

/// A finisher meeting `field` (a projectile carries the field it flew through).
pub fn combo_with(world: &mut World, field: Field, finisher: Finisher, at: Vec3, src: &HitSource, target: Option<Entity>) {
    let cat = catalog();
    let Some(rule) = cat.combos.iter().find(|c| c.field == field && c.finisher == finisher) else { return };
    let reached = if finisher == Finisher::Projectile {
        target.into_iter().collect::<Vec<_>>()
    } else {
        victims(world, Shape::Circle { radius: 4.0 }, at, 0.0, src, rule.allies, 5)
    };
    let effects = rule.effects.clone();
    for t in reached {
        for e in &effects {
            apply(world, e, t, src, None, false, at, 1.0);
        }
    }
    emit(world, Event::Combo { unit: src.unit, name: rule.name.to_string(), at });
}

/// The unit's body does `action` now: the client animates it, and swings and releases hold
/// the attack pose for a moment.
pub(crate) fn act(world: &mut World, e: Entity, action: protocol::Action) {
    use protocol::Action::*;
    let t = now(world);
    begin(world, e, action);
    match action {
        None | Downed | Block | Revive | Channel => {}
        Stagger => hold_pose(world, e, Pose::Hit, t, 0.5),
        _ => hold_pose(world, e, Pose::Attack, t, ATTACK_POSE),
    }
}

/// A heavy blow lands at the end of its wind-up: the swing that began then carries on (the
/// same pose and action, so its animation doesn't restart), held a little past the blow.
pub(crate) fn release(world: &mut World, e: Entity, action: protocol::Action) {
    let t = now(world);
    let swinging = world.get::<Anim>(e).is_some_and(|a| a.pose == Pose::Attack && t < a.hold_until)
        && world.get::<Fighter>(e).is_some_and(|f| f.action == action);
    if !swinging {
        act(world, e, action);
        return;
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        a.hold_until = a.hold_until.max(t + ATTACK_POSE);
    }
}

/// Records the action without holding a pose (a cast winding up shows the cast pose).
pub(crate) fn begin(world: &mut World, e: Entity, action: protocol::Action) {
    let t = now(world);
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.action = action;
        f.action_time = t;
    }
}
