//! Skills: the bar, using a skill (and every reason it can't be used), casts and channels, the
//! global cooldown, basic attacks, swapping weapons, and charges landing.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Event, ResourceKind};

use super::effects::{self, Delivery};
use super::{auras, can_attack, chest, control, emit, entity, gap, is_ally, line_of_sight, missiles, now, source, tell, unit_id};
use crate::content::{catalog, downed_skills, AbilityDef, AbilityId, Effect, Grip, Targeting, UNARMED_SWING};
use crate::movement::{heading, wrap_angle};
use crate::spatial::Spatial;
use crate::stats::GCD;
use crate::{Appearance, Body, Cast, Fighter, ForcedKind, Land, Motion, PlayerControl, Queued, Transform, Vitals};

/// Seconds between weapon swaps.
pub const SWAP_COOLDOWN: f64 = 5.0;

/// How far (radians, about 12°) a player's blow turns toward the enemy nearest their aim, so a
/// near miss still lands.
pub const AIM_PULL: f32 = 0.21;
/// A skill pressed this long (or less) before you're free to use it waits and then goes off,
/// instead of being refused.
pub const SKILL_QUEUE: f64 = 0.5;

/// The bar for a weapon set: skills 1–5 from the weapons in hand, then the kit (6–0).
pub fn bar_for(set: &(Option<String>, Option<String>), kit: &[Option<AbilityId>; 5]) -> [Option<AbilityId>; 10] {
    let cat = catalog();
    let mut bar = [None; 10];
    let id = |k: &str| cat.ability_id(k);
    let main = set.0.as_deref().and_then(|i| cat.weapon(i));
    match main {
        Some(w) if w.grip == Grip::Two => {
            for (i, k) in w.skills.iter().take(5).enumerate() {
                bar[i] = id(k);
            }
        }
        Some(w) => {
            for (i, k) in w.skills.iter().take(3).enumerate() {
                bar[i] = id(k);
            }
        }
        None => bar[0] = id("punch"),
    }
    if main.is_none_or(|w| w.grip != Grip::Two) && let Some(off) = set.1.as_deref().and_then(|i| cat.weapon(i)) {
        let skills = if off.grip == Grip::Either { &off.off_skills } else { &off.skills };
        for (i, k) in skills.iter().take(2).enumerate() {
            bar[3 + i] = id(k);
        }
    }
    bar[5..].copy_from_slice(kit);
    bar
}

/// Puts the active weapon set in hand: the bar, the basic attack, and what the unit is drawn
/// holding.
pub fn equip(world: &mut World, e: Entity) {
    let cat = catalog();
    let Some(f) = world.get::<Fighter>(e) else { return };
    let set = f.sets.get(f.active_set).cloned().unwrap_or((None, None));
    let bar = bar_for(&set, &f.kit);
    let auto = bar[0].filter(|&a| cat.ability(a).auto);
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.bar = bar;
        f.auto = auto;
    }
    if let Some(mut look) = world.get_mut::<Appearance>(e) {
        look.edit(|look| {
            look.gear.retain(|(slot, _)| slot != "main_hand" && slot != "off_hand");
            if let Some(m) = &set.0 {
                look.gear.push(("main_hand".into(), m.clone()));
            }
            if let Some(o) = &set.1 {
                look.gear.push(("off_hand".into(), o.clone()));
            }
        });
    }
}

/// The skills a unit can use right now: its bar, or the downed skills.
pub fn current_bar(world: &World, e: Entity) -> [Option<AbilityId>; 10] {
    let cat = catalog();
    if world.get::<Vitals>(e).is_some_and(|v| v.downed) {
        let mut bar = [None; 10];
        for (i, k) in downed_skills().iter().enumerate() {
            bar[i] = cat.ability_id(k);
        }
        return bar;
    }
    world.get::<Fighter>(e).map_or([None; 10], |f| f.bar)
}

/// A player pressed a key on the bar.
pub fn use_slot(world: &mut World, e: Entity, slot: u8, ground: Option<Vec3>) {
    let cat = catalog();
    let bar = current_bar(world, e);
    let Some(id) = bar.get(slot as usize).copied().flatten() else { return };
    let def = cat.ability(id);
    if def.auto && let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.auto_attack = true;
    }
    // Pressed just before the global cooldown, a cast or wind-up, or a climb or leap ends: it
    // goes off the moment that does. The newest press takes the place of an older one.
    if !def.auto && let Some(free) = free_soon(world, e, id, def) {
        if let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.queued = Some(Queued { ability: id, ground, until: free + 0.1 });
        }
        return;
    }
    // Pressing the basic attack turns it on; it says nothing while it's mid-swing.
    if use_ability(world, e, id, None, ground, def.auto) {
        // Attacking a target keeps you swinging at it.
        let target = world.get::<Fighter>(e).and_then(|f| f.target).and_then(|t| entity(world, t));
        let offensive = !def.allies && !matches!(def.targeting, Targeting::Ally | Targeting::SelfOnly);
        if offensive && target.is_some_and(|t| can_attack(world, e, t)) && let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.auto_attack = true;
        }
    }
}

/// When `e`, busy now, will be free to use `id`, if that is within [`SKILL_QUEUE`]: the end of
/// the global cooldown (for a skill on it), of a cast or wind-up, and of a forced move other than
/// a roll. `None` if nothing is in the way, or it won't end soon enough to wait for.
fn free_soon(world: &World, e: Entity, id: AbilityId, def: &AbilityDef) -> Option<f64> {
    let t = now(world);
    let f = world.get::<Fighter>(e)?;
    let mut free = f64::NEG_INFINITY;
    if def.gcd {
        free = free.max(f.gcd_until);
    }
    if let Some(c) = f.cast {
        let current = catalog().ability(c.ability);
        // A basic attack or a channel gives way at once, and pressing the skill being cast does
        // nothing.
        if c.ability == id || current.auto || c.channel {
            return None;
        }
        free = free.max(c.start + c.duration);
    }
    if let Some(fm) = world.get::<Motion>(e).and_then(|m| m.forced).filter(|fm| fm.kind != ForcedKind::Dodge) {
        free = free.max(fm.start + fm.duration);
    }
    (free > t && free - t <= SKILL_QUEUE).then_some(free)
}

/// Checks and starts an ability. Returns false (telling a player why) when it can't be used.
pub fn use_ability(world: &mut World, e: Entity, id: AbilityId, explicit: Option<protocol::UnitId>, ground: Option<Vec3>, quiet: bool) -> bool {
    let cat = catalog();
    let def = cat.ability(id);
    let t = now(world);
    let fail = |world: &mut World, text: &str| {
        if !quiet {
            tell(world, e, text);
        }
        false
    };
    let Some(v) = world.get::<Vitals>(e).copied() else { return false };
    if v.dead {
        return false;
    }
    if v.downed && !def.downed {
        return fail(world, "You can't do that while downed.");
    }
    if !v.downed && def.downed {
        return false;
    }
    let ctl = control(world, e);
    let Some(f) = world.get::<Fighter>(e).cloned() else { return false };
    if !def.breaks_stun {
        if ctl.stunned || ctl.fleeing.is_some() {
            return fail(world, "You can't act right now.");
        }
        if ctl.dazed || ctl.silenced || t < f.lockout_until {
            return fail(world, "You can't use skills right now.");
        }
    }
    let motion = world.get::<Motion>(e).copied();
    if motion.and_then(|m| m.forced).is_some_and(|fm| fm.kind != ForcedKind::Dodge) {
        return false;
    }
    if motion.is_some_and(|m| m.swimming) {
        return fail(world, "You can't fight while swimming.");
    }
    // A basic attack winding up, or a channel, gives way to another skill (once that skill is
    // known to be usable); anything else in progress has to finish first.
    let mut give_way = false;
    if let Some(c) = f.cast {
        if c.ability == id {
            return false;
        }
        let current = cat.ability(c.ability);
        if current.auto || c.channel {
            give_way = true;
        } else {
            return fail(world, "You're busy.");
        }
    }
    if def.gcd && t < f.gcd_until {
        return if quiet { false } else { fail(world, "That isn't ready yet.") };
    }
    if t < f.ready_at(id) {
        return if quiet { false } else { fail(world, "That isn't ready yet.") };
    }
    if def.cost > 0.0 && f.resource_kind != ResourceKind::None && f.resource < def.cost {
        let name = format!("{:?}", f.resource_kind).to_lowercase();
        return fail(world, &format!("Not enough {name}."));
    }
    let player = world.get::<PlayerControl>(e).is_some();
    if !def.movable && player && motion.is_some_and(|m| m.moving()) {
        return fail(world, "You can't do that while moving.");
    }

    let me = world.get::<Transform>(e).copied().unwrap();
    let mut target: Option<Entity> = None;
    let mut point: Option<Vec3> = None;
    // Which way a player's blow sweeps when it isn't at a target in reach.
    let mut swing: Option<f32> = None;
    match def.targeting {
        Targeting::Enemy if sweeps(world, e, def) => {
            // A target in reach takes the blow (the swing faces it); otherwise it goes where the
            // player aims, pulled toward the nearest enemy.
            let chosen = explicit.or(f.target).and_then(|id| entity(world, id)).filter(|&te| can_attack(world, e, te));
            let in_reach = chosen.filter(|&te| gap(world, e, te) <= def.range + 0.5 && line_of_sight(world, chest(world, e), chest(world, te)));
            if let Some(te) = in_reach {
                if auras::sum(world, te, |a| if a.invulnerable { 1.0 } else { 0.0 }) > 0.0 {
                    return fail(world, "Your target can't be harmed right now.");
                }
                target = Some(te);
            } else if let Some(y) = aim_swing(world, e, def, ground) {
                swing = Some(y);
            } else {
                return fail(world, if chosen.is_some() { "Out of range." } else { "Nothing in reach." });
            }
        }
        Targeting::Enemy => {
            let t_e = explicit.or(f.target).and_then(|id| entity(world, id)).filter(|&te| can_attack(world, e, te));
            let Some(te) = t_e else { return fail(world, "You need an enemy target.") };
            if auras::sum(world, te, |a| if a.invulnerable { 1.0 } else { 0.0 }) > 0.0 {
                return fail(world, "Your target can't be harmed right now.");
            }
            if gap(world, e, te) > def.range + 0.5 {
                return fail(world, "Out of range.");
            }
            if !line_of_sight(world, chest(world, e), chest(world, te)) {
                return fail(world, "Your target isn't in sight.");
            }
            target = Some(te);
        }
        Targeting::Ally => {
            let mut te = explicit.or(f.target).and_then(|id| entity(world, id)).filter(|&o| is_ally(world, e, o)).unwrap_or(e);
            if world.get::<Vitals>(te).is_some_and(|v| v.dead) && !def.reviving() {
                te = e;
            }
            if gap(world, e, te) > def.range + 0.5 {
                return fail(world, "Out of range.");
            }
            target = Some(te);
        }
        Targeting::SelfOnly => target = Some(e),
        Targeting::Ground { .. } => {
            let aim = ground
                .or_else(|| f.target.and_then(|id| entity(world, id)).and_then(|te| world.get::<Transform>(te)).map(|t| t.pos))
                .unwrap_or(me.pos + Vec3::new(me.forward().x, 0.0, me.forward().y) * def.range.min(8.0));
            let mut d = Vec2::new(aim.x - me.pos.x, aim.z - me.pos.z);
            if d.length() > def.range {
                d = d.normalize() * def.range;
            }
            let land = &world.resource::<Land>().0;
            let x = me.pos.x + d.x;
            let z = me.pos.z + d.y;
            point = Some(Vec3::new(x, land.ground(x, z), z));
        }
        Targeting::Cone { .. } | Targeting::Line { .. } | Targeting::AroundSelf { .. } => {
            target = explicit.or(f.target).and_then(|id| entity(world, id)).filter(|&te| !def.allies && can_attack(world, e, te));
        }
    }

    if give_way {
        cancel_cast(world, e, false);
    }
    // A player's aim for an instant shot that flies straight (a cast's bolt flies at where the
    // target is when it's loosed).
    if def.missile.is_some_and(|m| !m.homing) && def.cast <= 0.0 && !def.is_channel() && point.is_none() {
        point = ground.filter(|_| player);
    }

    // Turn to face what it's used on (unless a player is strafing with a locked facing).
    let locked = player && motion.is_some_and(|m| m.intent.face_yaw.is_some());
    let face = target.filter(|&te| te != e).and_then(|te| world.get::<Transform>(te)).map(|t| t.pos).or(point);
    if let Some(p) = face {
        let d = Vec2::new(p.x - me.pos.x, p.z - me.pos.z);
        if d.length_squared() > 1e-4 && !locked && let Some(mut tr) = world.get_mut::<Transform>(e) {
            tr.yaw = heading(d);
        }
    }
    if let Some(y) = swing && !locked && let Some(mut tr) = world.get_mut::<Transform>(e) {
        tr.yaw = y;
    }
    if def.breaks_stun {
        auras::remove_where(world, e, |a| a.cc.is_some_and(|c| c.hard() || c == crate::content::Cc::Root || c == crate::content::Cc::Daze));
    }
    // Using a skill drops the guard.
    super::guard::lower(world, e);

    let haste = auras::haste(world, e);
    let yaw = swing.unwrap_or_else(|| world.get::<Transform>(e).map_or(0.0, |t| t.yaw));
    let target_id = target.and_then(|te| unit_id(world, te));
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        if def.gcd {
            f.gcd_until = t + GCD / haste;
        }
        f.queued = None;
    }
    let unit = unit_id(world, e).unwrap_or(0);
    if def.cast > 0.0 {
        let duration = def.cast / haste;
        if let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.cast = Some(Cast { ability: id, target: target_id, ground: point, yaw, start: t, duration, channel: false, next_tick: 0.0, ticks_done: 0 });
        }
        if def.is_swing() {
            // The swing starts now and lands when the wind-up ends.
            effects::act(world, e, def.action);
        } else {
            effects::begin(world, e, def.action);
        }
        emit(world, Event::CastStarted { unit, ability: def.key.to_string(), duration: duration as f32, channel: false });
    } else if def.is_channel() {
        pay(world, e, def);
        start_cooldown(world, e, id, haste);
        let every = def.channel / def.ticks.max(1) as f64;
        if let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.cast = Some(Cast {
                ability: id,
                target: target_id,
                ground: point,
                yaw,
                start: t,
                duration: def.channel,
                channel: true,
                next_tick: t + every * 0.5,
                ticks_done: 0,
            });
        }
        effects::act(world, e, def.action);
        emit(world, Event::CastStarted { unit, ability: def.key.to_string(), duration: def.channel as f32, channel: true });
        if def.effects.is_empty() {
            emit(world, Event::AbilityUsed { unit, ability: def.key.to_string(), target: target_id, ground: point });
        } else {
            resolve(world, e, def, target_id, point, yaw);
        }
    } else {
        pay(world, e, def);
        start_cooldown(world, e, id, haste);
        resolve(world, e, def, target_id, point, yaw);
    }
    if !def.allies && !matches!(def.targeting, Targeting::Ally | Targeting::SelfOnly) && let Some(mut v) = world.get_mut::<Vitals>(e) {
        v.last_combat = v.last_combat.max(t - 3.0);
    }
    true
}

/// Spends the cost and gains what the ability builds (rage and essence only build).
pub fn pay(world: &mut World, e: Entity, def: &AbilityDef) {
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        if f.resource_kind == ResourceKind::None {
            return;
        }
        let builds = matches!(f.resource_kind, ResourceKind::Rage | ResourceKind::Essence);
        let gain = if builds { def.generates } else { 0.0 };
        f.resource = (f.resource - def.cost + gain).clamp(0.0, f.max_resource);
    }
}

pub fn start_cooldown(world: &mut World, e: Entity, id: AbilityId, haste: f64) {
    let def = catalog().ability(id);
    let mut cd = def.cooldown;
    if def.auto {
        cd /= haste;
    }
    if cd > 0.0 {
        let t = now(world);
        if let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.set_cooldown(id, t + cd);
        }
    }
}

/// Stops a cast or channel (`interrupted` tells everyone it was stopped by force).
pub fn cancel_cast(world: &mut World, e: Entity, interrupted: bool) {
    let had = world.get_mut::<Fighter>(e).and_then(|mut f| f.cast.take()).is_some();
    if had && interrupted && let Some(unit) = unit_id(world, e) {
        emit(world, Event::CastInterrupted { unit });
    }
}

/// Moving breaks off casts that need you to stand still (rolling breaks off any cast that
/// isn't a channel you can move in).
pub fn cancel_standing_cast(world: &mut World, e: Entity) {
    let Some(c) = world.get::<Fighter>(e).and_then(|f| f.cast) else { return };
    let def = catalog().ability(c.ability);
    if !(c.channel && def.movable) {
        cancel_cast(world, e, false);
    }
}

/// Casts and channels progress, queued skills fire, and basic attacks swing.
pub fn update(world: &mut World) {
    let cat = catalog();
    let t = now(world);
    let entities = crate::take_units(world);
    for &e in &entities {
        let Some(v) = world.get::<Vitals>(e).copied() else { continue };
        if v.dead {
            continue;
        }
        let Some((cast, queued, gcd_until)) = world.get::<Fighter>(e).map(|f| (f.cast, f.queued, f.gcd_until)) else { continue };
        if let Some(c) = cast {
            let def = cat.ability(c.ability);
            let player = world.get::<PlayerControl>(e).is_some();
            if !def.movable && player && world.get::<Motion>(e).is_some_and(|m| m.moving()) {
                cancel_cast(world, e, false);
                tell(world, e, "Interrupted: you moved.");
                continue;
            }
            if c.channel {
                let every = def.channel / def.ticks.max(1) as f64;
                let mut cur = c;
                while cur.ticks_done < def.ticks && t >= cur.next_tick {
                    cur.ticks_done += 1;
                    cur.next_tick += every;
                    if let Some(mut f) = world.get_mut::<Fighter>(e) {
                        f.cast = Some(cur);
                    }
                    channel_tick(world, e, def, cur);
                    if world.get::<Fighter>(e).is_none_or(|f| f.cast.is_none()) {
                        break;
                    }
                }
                let gone = def.targeting == Targeting::Enemy
                    && !cur.target.and_then(|id| entity(world, id)).is_some_and(|te| can_attack(world, e, te));
                if (t >= c.start + c.duration || gone) && let Some(mut f) = world.get_mut::<Fighter>(e) {
                    f.cast = None;
                }
            } else if t >= c.start + c.duration {
                if let Some(mut f) = world.get_mut::<Fighter>(e) {
                    f.cast = None;
                }
                finish_cast(world, e, def, c);
            }
            continue;
        }
        if let Some(q) = queued {
            if t > q.until {
                if let Some(mut f) = world.get_mut::<Fighter>(e) {
                    f.queued = None;
                }
            } else if t >= gcd_until && !world.get::<Motion>(e).and_then(|m| m.forced).is_some_and(|fm| fm.kind != ForcedKind::Dodge) {
                if let Some(mut f) = world.get_mut::<Fighter>(e) {
                    f.queued = None;
                }
                use_ability(world, e, q.ability, None, q.ground, false);
                continue;
            }
        }
        auto_attack(world, e);
    }
    crate::return_units(world, entities);
}

fn finish_cast(world: &mut World, e: Entity, def: &AbilityDef, c: Cast) {
    // A player's swing lands on whatever is in front when it comes down.
    if def.targeting == Targeting::Enemy && !sweeps(world, e, def) {
        let ok = c.target.and_then(|id| entity(world, id)).is_some_and(|te| {
            can_attack(world, e, te) && gap(world, e, te) <= def.range + 2.5 && line_of_sight(world, chest(world, e), chest(world, te))
        });
        if !ok {
            tell(world, e, "Your target is out of reach.");
            return;
        }
    }
    let f = world.get::<Fighter>(e).cloned().unwrap_or_default();
    if def.cost > 0.0 && f.resource_kind != ResourceKind::None && f.resource < def.cost {
        tell(world, e, "Not enough to finish that.");
        return;
    }
    let haste = auras::haste(world, e);
    pay(world, e, def);
    if let Some(id) = catalog().ability_id(def.key) {
        start_cooldown(world, e, id, haste);
    }
    resolve(world, e, def, c.target, c.ground, c.yaw);
}

fn channel_tick(world: &mut World, e: Entity, def: &AbilityDef, c: Cast) {
    let Some(mut src) = source(world, e, Some(def)) else { return };
    let target = c.target.and_then(|id| entity(world, id));
    if def.channel_missile {
        if let Some(te) = target.filter(|&te| can_attack(world, e, te)) {
            missiles::launch(world, e, def, Some(te), None, def.channel_effects.clone());
            effects::act(world, e, def.action);
        }
        return;
    }
    match def.targeting {
        Targeting::Enemy => {
            let Some(te) = target.filter(|&te| can_attack(world, e, te) && gap(world, e, te) <= def.range + 2.5) else {
                cancel_cast(world, e, false);
                return;
            };
            let center = world.get::<Transform>(te).map_or(Vec3::ZERO, |t| t.pos);
            effects::apply_effects(world, &def.channel_effects, &src, Some(def), &[te], center, Delivery::Strike);
        }
        Targeting::Ally | Targeting::SelfOnly => {
            let te = target.unwrap_or(e);
            let center = world.get::<Transform>(te).map_or(Vec3::ZERO, |t| t.pos);
            effects::apply_effects(world, &def.channel_effects, &src, Some(def), &[te], center, Delivery::Strike);
        }
        _ => {
            let Some(shape) = effects::shape_of(def) else { return };
            let here = world.get::<Transform>(e).copied().unwrap();
            let origin = c.ground.unwrap_or(here.pos);
            src.from = origin;
            // Spinning and channelled areas around you follow you; aimed ones stay put.
            let yaw = if matches!(def.targeting, Targeting::AroundSelf { .. }) { here.yaw } else { c.yaw };
            let reached = effects::victims(world, shape, origin, yaw, &src, def.allies, def.max_targets);
            effects::apply_effects(world, &def.channel_effects, &src, Some(def), &reached, origin, Delivery::Strike);
            if def.finisher == Some(crate::content::Finisher::Whirl) {
                effects::combo(world, crate::content::Finisher::Whirl, here.pos, &src, None);
            }
        }
    }
}

/// The ability goes off: it looses its missile, or strikes its target or area.
pub fn resolve(world: &mut World, e: Entity, def: &AbilityDef, target: Option<protocol::UnitId>, ground: Option<Vec3>, yaw: f32) {
    use crate::content::Finisher;
    let Some(mut src) = source(world, e, Some(def)) else { return };
    let unit = src.unit;
    emit(world, Event::AbilityUsed { unit, ability: def.key.to_string(), target, ground });
    if def.is_swing() {
        effects::release(world, e, def.action);
    } else {
        effects::act(world, e, def.action);
    }
    let te = target.and_then(|id| entity(world, id));
    if def.missile.is_some() && !def.channel_missile && let Some(t) = te.filter(|&t| t != e) {
        let user_side: Vec<_> = def.effects.iter().filter(|x| x.on_user()).cloned().collect();
        if !user_side.is_empty() {
            let at = world.get::<Transform>(e).map_or(Vec3::ZERO, |t| t.pos);
            effects::apply_effects(world, &user_side, &src, Some(def), &[], at, Delivery::Strike);
        }
        let rest = def.effects.iter().filter(|x| !x.on_user()).cloned().collect();
        missiles::launch(world, e, def, Some(t), ground, rest);
        return;
    }
    match def.targeting {
        Targeting::Enemy if sweeps(world, e, def) => strike(world, e, def, &src, te, yaw),
        Targeting::Enemy | Targeting::Ally | Targeting::SelfOnly => {
            let Some(t) = te else { return };
            let center = world.get::<Transform>(t).map_or(Vec3::ZERO, |t| t.pos);
            effects::apply_effects(world, &def.effects, &src, Some(def), &[t], center, Delivery::Strike);
            if def.finisher == Some(Finisher::Blast) {
                effects::combo(world, Finisher::Blast, center, &src, Some(t));
            }
        }
        Targeting::Ground { radius } => {
            let p = ground.unwrap_or(src.from);
            if def.impact_delay > 0.0 {
                missiles::spawn_impact(world, &src, def, p, radius);
                return;
            }
            src.from = p;
            let reached = effects::victims(world, protocol::Shape::Circle { radius }, p, yaw, &src, def.allies, def.max_targets);
            effects::apply_effects(world, &def.effects, &src, Some(def), &reached, p, Delivery::Strike);
            if def.finisher == Some(Finisher::Blast) {
                effects::combo(world, Finisher::Blast, p, &src, None);
            }
        }
        Targeting::Cone { .. } | Targeting::Line { .. } | Targeting::AroundSelf { .. } => {
            let Some(shape) = effects::shape_of(def) else { return };
            let here = src.from;
            let reached = effects::victims(world, shape, here, yaw, &src, def.allies, def.max_targets);
            effects::apply_effects(world, &def.effects, &src, Some(def), &reached, here, Delivery::Strike);
            if let Some(fin @ (Finisher::Blast | Finisher::Whirl)) = def.finisher {
                effects::combo(world, fin, here, &src, None);
            }
        }
    }
}

/// Whether `e`'s use of `def` sweeps the cone in front: a player's melee blow.
pub fn sweeps(world: &World, e: Entity, def: &AbilityDef) -> bool {
    def.is_strike() && world.get::<PlayerControl>(e).is_some()
}

/// The full angle a unit's blows sweep: its main-hand weapon's swing.
pub fn swing_arc(world: &World, e: Entity) -> f32 {
    let cat = catalog();
    world
        .get::<Fighter>(e)
        .and_then(|f| f.sets.get(f.active_set))
        .and_then(|set| set.0.as_deref())
        .and_then(|item| cat.weapon(item))
        .map_or(UNARMED_SWING, |w| w.swing)
}

/// How far a blow reaches from the user's centre to a body's edge: its range past both edges,
/// as the range check for a target measures it.
fn reach(world: &World, e: Entity, def: &AbilityDef) -> f32 {
    def.range + 0.5 + world.get::<Body>(e).map_or(0.4, |b| b.radius)
}

/// Which way a player's blow sweeps when no target is in reach: along their aim (toward the
/// point they sent, else the way they face), turned by up to `AIM_PULL` toward the enemy nearest
/// that line. None when no enemy is in reach within the swing and the pull.
fn aim_swing(world: &World, e: Entity, def: &AbilityDef, ground: Option<Vec3>) -> Option<f32> {
    let me = world.get::<Transform>(e).copied()?;
    let aim = ground.map(|g| Vec2::new(g.x - me.pos.x, g.z - me.pos.z)).filter(|d| d.length_squared() > 0.25).map_or(me.yaw, heading);
    let half = swing_arc(world, e) * 0.5;
    let reach = reach(world, e, def);
    let eye = chest(world, e);
    let mut best: Option<f32> = None;
    for (id, _) in world.resource::<Spatial>().near(me.xz(), reach + 2.0) {
        let Some(o) = entity(world, id).filter(|&o| can_attack(world, e, o)) else { continue };
        let Some(t) = world.get::<Transform>(o) else { continue };
        let r = world.get::<Body>(o).map_or(0.4, |b| b.radius);
        let d = t.xz() - me.xz();
        let dist = d.length();
        if dist > reach + r {
            continue;
        }
        // How far the swing must turn to touch its body (none for one pressed against you).
        let off = if dist < r + 0.5 {
            0.0
        } else {
            let a = wrap_angle(heading(d) - aim);
            a.signum() * (a.abs() - (r / dist).atan()).max(0.0)
        };
        if off.abs() > half + AIM_PULL || best.is_some_and(|b| b.abs() <= off.abs()) || !line_of_sight(world, eye, chest(world, o)) {
            continue;
        }
        best = Some(off);
    }
    best.map(|off| wrap_angle(aim + off.clamp(-AIM_PULL, AIM_PULL)))
}

/// Effects that only the blow's main victim takes: control, interrupts, taunts and defiance.
/// Everyone else in the swing takes the harm.
fn controls(effect: &Effect) -> bool {
    match effect {
        Effect::Interrupt { .. } | Effect::Taunt | Effect::Defiance(_) | Effect::Knockback { .. } | Effect::Pull { .. } => true,
        Effect::Aura { key, .. } => catalog().aura_id(key).is_some_and(|a| catalog().aura(a).cc.is_some()),
        _ => false,
    }
}

/// A player's blow lands on every enemy in the swing in front: the locked target first (the
/// swing faces it while it's still in reach), then the nearest, up to the ability's limit.
fn strike(world: &mut World, e: Entity, def: &AbilityDef, src: &super::HitSource, target: Option<Entity>, yaw: f32) {
    use crate::content::Finisher;
    let here = src.from;
    let locked = target.filter(|&t| can_attack(world, e, t) && gap(world, e, t) <= def.range + 2.5 && line_of_sight(world, chest(world, e), chest(world, t)));
    let yaw = locked.and_then(|t| world.get::<Transform>(t)).map(|t| t.xz() - Vec2::new(here.x, here.z)).filter(|d| d.length_squared() > 1e-4).map_or(yaw, heading);
    let shape = protocol::Shape::Cone { range: reach(world, e, def), angle: swing_arc(world, e) };
    let mut hit = effects::victims(world, shape, here, yaw, src, false, def.max_targets);
    if let Some(t) = locked {
        hit.retain(|&o| o != t);
        hit.insert(0, t);
        hit.truncate(def.max_targets.max(1) as usize);
    }
    let Some(&first) = hit.first() else {
        tell(world, e, "Your blow misses.");
        return;
    };
    let center = world.get::<Transform>(first).map_or(here, |t| t.pos);
    effects::apply_effects(world, &def.effects, src, Some(def), &[first], center, Delivery::Strike);
    if hit.len() > 1 {
        let harm: Vec<_> = def.effects.iter().filter(|x| !x.on_user() && !controls(x)).cloned().collect();
        effects::apply_effects(world, &harm, src, Some(def), &hit[1..], center, Delivery::Strike);
    }
    if def.finisher == Some(Finisher::Blast) {
        effects::combo(world, Finisher::Blast, center, src, Some(first));
    }
}

/// Keeps the basic attack going while there's something in reach to hit.
fn auto_attack(world: &mut World, e: Entity) {
    let cat = catalog();
    let Some(f) = world.get::<Fighter>(e) else { return };
    if !f.auto_attack || f.guard_since.is_some() || f.reviving.is_some() {
        return;
    }
    let (target, auto) = (f.target, f.auto);
    if world.get::<Motion>(e).is_some_and(|m| m.forced.is_some()) {
        return;
    }
    let downed = world.get::<Vitals>(e).is_some_and(|v| v.downed);
    let id = if downed { cat.ability_id(downed_skills()[0]) } else { auto };
    let Some(id) = id else { return };
    let Some(te) = target.and_then(|t| entity(world, t)).filter(|&te| can_attack(world, e, te)) else { return };
    if world.get::<Fighter>(e).is_some_and(|f| now(world) < f.ready_at(id)) {
        return;
    }
    let def = cat.ability(id);
    if gap(world, e, te) > def.range + 0.3 {
        return;
    }
    let player = world.get::<PlayerControl>(e).is_some();
    if !def.movable && player && world.get::<Motion>(e).is_some_and(|m| m.moving()) {
        return;
    }
    use_ability(world, e, id, None, None, true);
}

/// Changes to the other weapon set (not mid-cast, and not again for a few seconds).
pub fn swap(world: &mut World, e: Entity) {
    let t = now(world);
    let Some(f) = world.get::<Fighter>(e).cloned() else { return };
    if f.sets.len() < 2 || world.get::<Vitals>(e).is_none_or(|v| !v.alive()) {
        return;
    }
    if t < f.swap_ready_at {
        tell(world, e, "You can't swap weapons yet.");
        return;
    }
    if let Some(c) = f.cast {
        let def = catalog().ability(c.ability);
        if !c.channel && !def.auto {
            return;
        }
    }
    cancel_cast(world, e, false);
    super::guard::lower(world, e);
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.active_set = (f.active_set + 1) % f.sets.len();
        f.swap_ready_at = t + SWAP_COOLDOWN;
    }
    equip(world, e);
    if let Some(unit) = unit_id(world, e) {
        emit(world, Event::WeaponsSwapped { unit });
    }
}

/// Charges that arrived this step: the rest of their ability lands.
#[derive(Resource, Default)]
pub struct Arrivals(pub Vec<(Entity, protocol::UnitId, AbilityId)>);

pub fn land_charges(world: &mut World) {
    let arrived = std::mem::take(&mut world.resource_mut::<Arrivals>().0);
    let cat = catalog();
    for (e, target, ability) in arrived {
        let Some(te) = entity(world, target) else { continue };
        if !can_attack(world, e, te) || world.get::<Vitals>(e).is_none_or(|v| !v.alive()) {
            continue;
        }
        let def = cat.ability(ability);
        let Some(src) = source(world, e, Some(def)) else { continue };
        let rest: Vec<_> = def.effects.iter().filter(|x| !matches!(x, crate::content::Effect::Charge)).cloned().collect();
        let center = world.get::<Transform>(te).map_or(Vec3::ZERO, |t| t.pos);
        effects::apply_effects(world, &rest, &src, Some(def), &[te], center, Delivery::Strike);
        if def.finisher == Some(crate::content::Finisher::Leap) {
            let at = world.get::<Transform>(e).map_or(Vec3::ZERO, |t| t.pos);
            effects::combo(world, crate::content::Finisher::Leap, at, &src, None);
        }
    }
}
