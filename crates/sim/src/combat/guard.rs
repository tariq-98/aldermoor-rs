//! Guarding, parrying and attacking from behind: the skill in a fight.
//!
//! A raised guard blocks blows from in front at a cost in endurance (the same endurance rolls
//! spend): shields cheaply and in a wide arc, and they stop missiles too; other melee weapons in
//! a narrower arc, and not missiles. A stronger foe's blows cost far more to stop, so a guard
//! can't hold off someone well above your level for long. Raise it just before a blow you saw
//! winding up lands and it's a parry: no cost, the attacker staggers, and your next blow is a
//! riposte. Quick blows give no warning: they can be blocked, not parried. Big blows that can't
//! be blocked must be dodged, and nothing guards against a blow from behind.

use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::{Action, Avoid, Event};

use super::effects::Delivery;
use super::{auras, emit, entity, now, tell, unit_id, HitSource};
use crate::content::{catalog, AbilityDef, Guard};
use crate::stats::scale;
use crate::{Fighter, PlayerControl, Transform, Unit, Vitals};

/// A guard raised this soon before a blow lands parries it.
pub const PARRY_WINDOW: f64 = 0.25;
/// Endurance per blocked blow from a foe of your own level.
pub const SHIELD_COST: f32 = 15.0;
pub const WEAPON_COST: f32 = 25.0;
/// Half-arcs a guard covers, in radians.
pub const SHIELD_ARC: f32 = 1.3;
const WEAPON_ARC: f32 = 0.9;
/// Beyond this angle from facing, a blow comes from behind.
const BEHIND: f32 = 1.92;

/// What the weapons in hand can guard with.
pub fn grip(world: &World, e: Entity) -> Guard {
    let cat = catalog();
    let Some(f) = world.get::<Fighter>(e) else { return Guard::None };
    let Some((main, off)) = f.sets.get(f.active_set) else { return Guard::None };
    if off.as_deref().and_then(|i| cat.weapon(i)).is_some_and(|w| w.guard == Guard::Shield) {
        return Guard::Shield;
    }
    main.as_deref().and_then(|i| cat.weapon(i)).map_or(Guard::None, |w| w.guard)
}

pub fn raise(world: &mut World, e: Entity) {
    let t = now(world);
    let ok = world.get::<Vitals>(e).is_some_and(|v| v.alive()) && grip(world, e) != Guard::None;
    if !ok {
        tell(world, e, "You've nothing to guard with.");
        return;
    }
    if world.get::<Fighter>(e).is_some_and(|f| f.guard_since.is_some()) {
        return;
    }
    // Raising a guard breaks off a cast.
    super::abilities::cancel_cast(world, e, false);
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.guard_since = Some(t);
        f.action = Action::Block;
        f.action_time = t;
    }
}

pub fn lower(world: &mut World, e: Entity) {
    if let Some(mut f) = world.get_mut::<Fighter>(e) && f.guard_since.take().is_some() && f.action == Action::Block {
        f.action = Action::None;
    }
}

/// `from` lies behind the unit (outside 110° either side of where it faces).
pub fn behind(world: &World, e: Entity, from: glam::Vec3) -> bool {
    let Some(t) = world.get::<Transform>(e) else { return false };
    let d = Vec2::new(from.x - t.pos.x, from.z - t.pos.z);
    if d.length_squared() < 1e-4 {
        return false;
    }
    t.forward().angle_to(d).abs() > BEHIND
}

/// `from` lies within `arc` either side of where the unit faces.
pub fn facing(world: &World, e: Entity, from: glam::Vec3, arc: f32) -> bool {
    let Some(t) = world.get::<Transform>(e) else { return false };
    let d = Vec2::new(from.x - t.pos.x, from.z - t.pos.z);
    d.length_squared() < 1e-4 || t.forward().angle_to(d).abs() <= arc
}

/// Whether a hostile blow or effect fails to land on `te`, and why: immunity, a roll, a
/// shield aura, a block or a parry.
pub fn avoid(world: &mut World, src: &HitSource, te: Entity, def: Option<&AbilityDef>, delivery: Delivery) -> Option<Avoid> {
    let t = now(world);
    if auras::sum(world, te, |a| if a.invulnerable { 1.0 } else { 0.0 }) > 0.0 {
        return Some(Avoid::Immune);
    }
    if world.get::<Vitals>(te).is_some_and(|v| t < v.invulnerable_until) {
        return Some(Avoid::Evaded);
    }
    let unblockable = def.is_some_and(|d| d.unblockable) || delivery == Delivery::Pulse;
    if unblockable || behind(world, te, src.from) {
        return None;
    }
    if auras::sum(world, te, |a| if a.blocks { 1.0 } else { 0.0 }) > 0.0 && facing(world, te, src.from, SHIELD_ARC) {
        return Some(Avoid::Blocked);
    }
    let since = world.get::<Fighter>(te).and_then(|f| f.guard_since)?;
    let kind = grip(world, te);
    let (arc, cost) = match kind {
        Guard::Shield => (SHIELD_ARC, SHIELD_COST),
        Guard::Weapon => (WEAPON_ARC, WEAPON_COST),
        Guard::None => return None,
    };
    if (delivery == Delivery::Missile && kind != Guard::Shield) || !facing(world, te, src.from, arc) {
        return None;
    }
    // Only a blow seen winding up can be parried.
    let seen_coming = delivery == Delivery::Strike && def.is_some_and(|d| d.cast > 0.0 && !d.is_channel());
    if seen_coming && t - since <= PARRY_WINDOW {
        parry(world, te, src);
        return Some(Avoid::Parried);
    }
    let cost = cost * block_factor(src.level, world.get::<Unit>(te).map_or(src.level, |u| u.level));
    let enough = world.get::<Vitals>(te).is_some_and(|v| v.endurance >= cost);
    if enough {
        if let Some(mut v) = world.get_mut::<Vitals>(te) {
            v.endurance -= cost;
        }
        return Some(Avoid::Blocked);
    }
    // Out of endurance: the guard breaks, and the blow lands on a reeling defender.
    lower(world, te);
    super::effects::act(world, te, Action::Stagger);
    if world.get::<PlayerControl>(te).is_some() {
        emit(world, Event::Message { text: "Your guard breaks!".into() });
    }
    let own = super::source(world, te, None);
    if let Some(own) = own {
        auras::apply_key(world, "dazed", te, &own);
    }
    None
}

/// How much more a blow costs to block than one from a foe of your own level: as much more as
/// the attacker's level outweighs yours in health and power together.
pub fn block_factor(attacker: u8, defender: u8) -> f32 {
    let k = scale(attacker) / scale(defender);
    (k * k).clamp(0.5, 4.0)
}

/// A parry: the attacker staggers (if it's close enough to be thrown off balance) and the
/// defender's next blow is a riposte.
fn parry(world: &mut World, te: Entity, src: &HitSource) {
    let Some(defender) = super::source(world, te, None) else { return };
    if let Some(attacker) = entity(world, src.unit) {
        let close = super::gap(world, te, attacker) < 4.0;
        if close {
            super::abilities::cancel_cast(world, attacker, true);
            auras::apply_key(world, "staggered", attacker, &defender);
            super::effects::act(world, attacker, Action::Stagger);
            let by = unit_id(world, te).unwrap_or(0);
            emit(world, Event::Staggered { unit: src.unit, by });
        }
    }
    auras::apply_key(world, "riposte", te, &defender);
    super::effects::act(world, te, Action::Parry);
}
