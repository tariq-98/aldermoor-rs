//! Auras: boons, conditions, shields and crowd control. Crowd control diminishes when repeated,
//! and elites and bosses soak it into their defiance bar until it breaks.

use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::{Avoid, Event};

use super::{emit, entity, now, HitSource};
use crate::content::{catalog, AuraDef, AuraId, Cc, Effect, Rank};
use crate::stats::{diminish, scale, DEFIANCE_BACK, DEFIANCE_PER_SECOND, DR_WINDOW};
use crate::{AuraInstance, Auras, Brain, Control, ForcedKind, Motion, Transform, Unit, Vitals};

/// The sum of `f` over a unit's auras, each counted once per stack.
pub fn sum(world: &World, e: Entity, f: impl Fn(&AuraDef) -> f32) -> f32 {
    let cat = catalog();
    world.get::<Auras>(e).map_or(0.0, |a| a.list.iter().map(|i| f(cat.aura(i.aura)) * i.stacks as f32).sum())
}

pub fn has(world: &World, e: Entity, key: &str) -> bool {
    let Some(id) = catalog().aura_id(key) else { return false };
    world.get::<Auras>(e).is_some_and(|a| a.list.iter().any(|i| i.aura == id))
}

/// Cast and swing speed: 1 is normal, never below 0.3.
pub fn haste(world: &World, e: Entity) -> f64 {
    let base = world.get::<crate::Stats>(e).map_or(0.0, |s| s.haste);
    (1.0 + base + sum(world, e, |a| a.haste)).max(0.3) as f64
}

pub fn threat_multiplier(world: &World, e: Entity) -> f32 {
    let cat = catalog();
    world.get::<Auras>(e).map_or(1.0, |a| a.list.iter().map(|i| cat.aura(i.aura).threat).product())
}

/// What the unit's auras and situation allow right now.
pub fn compute(world: &World, e: Entity) -> Control {
    let (mut c, fear) = control_from(now(world), world.get::<Vitals>(e), world.get::<Motion>(e), world.get::<Auras>(e));
    if let Some(src) = fear {
        c.fleeing = Some(fear_from(world, src));
    }
    c
}

/// [`compute`] from the unit's own parts, plus the unit its last fear came from (whose
/// position the caller looks up).
fn control_from(t: f64, v: Option<&Vitals>, m: Option<&Motion>, a: Option<&Auras>) -> (Control, Option<protocol::UnitId>) {
    let mut c = Control { speed: 1.0, ..Default::default() };
    if v.is_some_and(|v| t < v.invulnerable_until) {
        c.invulnerable = true;
    }
    if m.and_then(|m| m.forced).is_some_and(|f| f.kind == ForcedKind::Knockback) {
        c.stunned = true;
    }
    let Some(a) = a.filter(|a| !a.list.is_empty()) else { return (c, None) };
    let cat = catalog();
    let mut speed = 0.0;
    let mut fear = None;
    for i in &a.list {
        let d = cat.aura(i.aura);
        speed += d.move_speed * i.stacks as f32;
        c.invulnerable |= d.invulnerable;
        c.cc_immune |= d.cc_immune;
        c.blocking |= d.blocks;
        c.reflecting |= d.reflects;
        match d.cc {
            Some(Cc::Stun | Cc::Knockdown | Cc::Sleep) => c.stunned = true,
            Some(Cc::Root) => c.rooted = true,
            Some(Cc::Fear) => fear = Some(i.source),
            Some(Cc::Daze) => c.dazed = true,
            Some(Cc::Silence) => c.silenced = true,
            Some(Cc::Slow) | None => {}
        }
    }
    c.speed = (1.0 + speed).max(0.1);
    (c, fear)
}

fn fear_from(world: &World, src: protocol::UnitId) -> Vec2 {
    entity(world, src).and_then(|s| world.get::<Transform>(s)).map_or(Vec2::ZERO, |t| t.xz())
}

/// Works out every unit's control state before anyone moves. Each unit's state depends only on
/// its own parts (and where whoever feared it stands), so the order doesn't matter.
pub fn refresh_control(world: &mut World) {
    let t = now(world);
    let fears = crate::queries::with(world, |q| &mut q.control, |world, q| {
        let mut fears = Vec::new();
        for (e, v, m, a, mut slot) in q.iter_mut(world) {
            let (c, fear) = control_from(t, v, m, a);
            *slot = c;
            if let Some(src) = fear {
                fears.push((e, src));
            }
        }
        fears
    });
    for (e, src) in fears {
        let from = fear_from(world, src);
        if let Some(mut c) = world.get_mut::<Control>(e) {
            c.fleeing = Some(from);
        }
    }
}

/// Applies an aura. Returns false if it didn't take (immunity, defiance, diminishing returns).
pub fn apply(world: &mut World, id: AuraId, te: Entity, src: &HitSource) -> bool {
    if world.get::<Vitals>(te).is_none_or(|v| v.dead) || world.get::<Auras>(te).is_none() {
        return false;
    }
    let cat = catalog();
    let def = cat.aura(id);
    let mut duration = def.duration;
    let t = now(world);
    let target = super::unit_id(world, te).unwrap_or(0);
    if let Some(cc) = def.cc {
        let broken = def.key == "broken";
        let ctl = compute(world, te);
        if (ctl.cc_immune || ctl.invulnerable) && !broken {
            emit(world, Event::Avoided { target, source: src.unit, how: Avoid::Immune });
            return false;
        }
        let (defiance_max, defiance) = world.get::<Auras>(te).map_or((0.0, 0.0), |a| (a.defiance_max, a.defiance));
        if defiance_max > 0.0 && !broken {
            if cc == Cc::Slow {
                // Elites can be slowed; bosses can't.
                if is_boss(world, te) {
                    return false;
                }
            } else {
                if defiance > 0.0 {
                    chip_defiance(world, te, def.duration as f32 * DEFIANCE_PER_SECOND, src);
                }
                return false;
            }
        }
        if let Some(category) = cc.category().filter(|_| !broken) {
            let mut a = world.get_mut::<Auras>(te).unwrap();
            let dr = &mut a.dr[category];
            if t >= dr.reset_at {
                dr.count = 0;
            }
            let mult = diminish(dr.count);
            if mult == 0.0 {
                emit(world, Event::Avoided { target, source: src.unit, how: Avoid::Immune });
                return false;
            }
            duration *= mult;
            dr.count += 1;
            dr.reset_at = t + DR_WINDOW;
        }
        if cc.hard() {
            super::abilities::cancel_cast(world, te, true);
            super::guard::lower(world, te);
        }
    }
    let absorb = def.absorb.map_or(0.0, |(base, coeff)| base * scale(src.level) + coeff * src.power);
    let mut a = world.get_mut::<Auras>(te).unwrap();
    match a.list.iter_mut().find(|i| i.aura == id) {
        Some(i) => {
            i.stacks = (i.stacks + 1).min(def.max_stacks.max(1));
            i.expires = i.expires.max(t + duration);
            i.duration = duration;
            i.source = src.unit;
            i.power = src.power;
            i.level = src.level;
            i.absorb_left = i.absorb_left.max(absorb);
        }
        None => {
            a.list.push(AuraInstance {
                aura: id,
                source: src.unit,
                power: src.power,
                level: src.level,
                expires: t + duration,
                duration,
                stacks: 1,
                next_tick: t + def.tick.as_ref().map_or(0.0, |(every, _)| *every),
                absorb_left: absorb,
            });
            emit(world, Event::AuraApplied { unit: target, aura: def.key.to_string() });
        }
    }
    true
}

pub fn apply_key(world: &mut World, key: &str, te: Entity, src: &HitSource) -> bool {
    match catalog().aura_id(key) {
        Some(id) => apply(world, id, te, src),
        None => false,
    }
}

fn is_boss(world: &World, e: Entity) -> bool {
    world.get::<Brain>(e).and_then(|b| catalog().mob(b.mob)).is_some_and(|m| m.rank == Rank::Boss)
}

pub fn remove_at(world: &mut World, e: Entity, index: usize) {
    let unit = super::unit_id(world, e).unwrap_or(0);
    let Some(mut a) = world.get_mut::<Auras>(e) else { return };
    if index >= a.list.len() {
        return;
    }
    let gone = a.list.remove(index);
    emit(world, Event::AuraRemoved { unit, aura: catalog().aura(gone.aura).key.to_string() });
}

pub fn remove(world: &mut World, e: Entity, id: AuraId) {
    if let Some(i) = world.get::<Auras>(e).and_then(|a| a.list.iter().position(|i| i.aura == id)) {
        remove_at(world, e, i);
    }
}

pub fn remove_key(world: &mut World, e: Entity, key: &str) {
    if let Some(id) = catalog().aura_id(key) {
        remove(world, e, id);
    }
}

/// Removes every aura matching `f` (newest first).
pub fn remove_where(world: &mut World, e: Entity, f: impl Fn(&AuraDef) -> bool) {
    let cat = catalog();
    let n = world.get::<Auras>(e).map_or(0, |a| a.list.len());
    for i in (0..n).rev() {
        let matches = world.get::<Auras>(e).and_then(|a| a.list.get(i)).is_some_and(|inst| f(cat.aura(inst.aura)));
        if matches {
            remove_at(world, e, i);
        }
    }
}

pub fn clear(world: &mut World, e: Entity) {
    remove_where(world, e, |_| true);
}

/// Lifts up to `count` conditions that can be lifted.
pub fn cleanse(world: &mut World, e: Entity, count: u8) {
    let cat = catalog();
    let mut left = count;
    let n = world.get::<Auras>(e).map_or(0, |a| a.list.len());
    for i in (0..n).rev() {
        if left == 0 {
            break;
        }
        let lift = world.get::<Auras>(e).and_then(|a| a.list.get(i)).is_some_and(|inst| {
            let d = cat.aura(inst.aura);
            d.debuff && d.dispellable
        });
        if lift {
            remove_at(world, e, i);
            left -= 1;
        }
    }
}

/// Chips an elite's or boss's defiance; when it breaks, they're stunned and exposed.
pub fn chip_defiance(world: &mut World, e: Entity, amount: f32, src: &HitSource) {
    let t = now(world);
    let broke = {
        let Some(mut a) = world.get_mut::<Auras>(e) else { return };
        if a.defiance_max <= 0.0 || a.defiance <= 0.0 || amount <= 0.0 {
            return;
        }
        a.defiance = (a.defiance - amount).max(0.0);
        if a.defiance <= 0.0 {
            a.defiance_back_at = t + DEFIANCE_BACK;
            true
        } else {
            false
        }
    };
    if broke {
        apply_key(world, "broken", e, src);
        apply_key(world, "exposed", e, src);
        super::abilities::cancel_cast(world, e, true);
        let unit = super::unit_id(world, e).unwrap_or(0);
        let name = world.get::<Unit>(e).map(|u| u.name.clone()).unwrap_or_default();
        emit(world, Event::DefianceBroken { unit });
        emit(world, Event::Message { text: format!("{name}'s defiance is broken!") });
    }
}

/// Ticks conditions and boons over time, expires finished auras and spent shields, and refills
/// broken defiance.
pub fn update(world: &mut World) {
    let t = now(world);
    let cat = catalog();
    let entities = crate::take_units(world);
    for &e in &entities {
        if world.get::<Vitals>(e).is_none_or(|v| v.dead) {
            continue;
        }
        let mut i = 0;
        while let Some(inst) = world.get::<Auras>(e).and_then(|a| a.list.get(i).copied()) {
            let def = cat.aura(inst.aura);
            if let Some((every, effect)) = &def.tick && t >= inst.next_tick {
                if let Some(mut a) = world.get_mut::<Auras>(e) {
                    a.list[i].next_tick += every.max(0.1);
                }
                let src = HitSource {
                    unit: inst.source,
                    power: inst.power,
                    level: inst.level,
                    school: def.school,
                    crit: 0.0,
                    faction: entity(world, inst.source).and_then(|s| world.get::<Unit>(s)).map_or(crate::Faction::Hostile, |u| u.faction),
                    ability: None,
                    from: world.get::<Transform>(e).map_or(glam::Vec3::ZERO, |t| t.pos),
                };
                periodic(world, effect, inst.stacks, e, &src);
                if world.get::<Vitals>(e).is_none_or(|v| v.dead) {
                    break;
                }
                // Something may have removed auras meanwhile: look at this slot again.
                if world.get::<Auras>(e).and_then(|a| a.list.get(i)).is_none_or(|x| x.aura != inst.aura) {
                    continue;
                }
            }
            let (expires, absorb_left) = world.get::<Auras>(e).map(|a| (a.list[i].expires, a.list[i].absorb_left)).unwrap();
            let spent = def.absorb.is_some() && absorb_left <= 0.01;
            if t >= expires || spent {
                remove_at(world, e, i);
                continue;
            }
            i += 1;
        }
        if let Some(mut a) = world.get_mut::<Auras>(e) && a.defiance_max > 0.0 && a.defiance <= 0.0 && t >= a.defiance_back_at {
            a.defiance = a.defiance_max;
        }
    }
    crate::return_units(world, entities);
}

fn periodic(world: &mut World, effect: &Effect, stacks: u8, e: Entity, src: &HitSource) {
    let k = stacks as f32;
    match effect {
        Effect::Damage { base, coeff } => {
            super::effects::deal_damage(world, src, e, (base * scale(src.level) + coeff * src.power) * k, false, true);
        }
        Effect::Heal { base, coeff } => {
            super::effects::heal(world, src, e, (base * scale(src.level) + coeff * src.power) * k, false);
        }
        _ => {}
    }
}

/// Seconds left on the unit's casts and auras, for views.
pub fn remaining(inst: &AuraInstance, now: f64) -> f32 {
    (inst.expires - now).max(0.0) as f32
}

/// The unit ID of an aura's source, if it's still around.
pub fn source_entity(world: &World, inst: &AuraInstance) -> Option<Entity> {
    entity(world, inst.source)
}

