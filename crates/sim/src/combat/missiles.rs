//! Things in flight and on the ground: arrows and bolts, warnings of blows about to land, and
//! lasting fields.
//!
//! Homing missiles follow their target and stop only for it (or a wall). Aimed ones fly
//! straight at where they were aimed and strike the first enemy in their path, so a target that
//! moves can make them miss. Monsters lead a moving target: running in a straight line won't
//! save you, but changing direction will.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Avoid, Event, Shape, UnitId};

use super::effects::{self, Delivery};
use super::{auras, chest, emit, entity, guard, now, unit_id, HitSource};
use crate::content::{catalog, AbilityDef, AbilityId, Effect, Field, Finisher};
use crate::spatial::Spatial;
use crate::{Body, Land, Motion, PlayerControl, Transform, Unit, Vitals, DT};

/// How much further than its range an aimed missile flies before it falls.
const OVERSHOOT: f32 = 6.0;
/// A missile's own thickness.
const THICKNESS: f32 = 0.15;

pub struct Projectile {
    pub id: u32,
    pub src: HitSource,
    pub ability: AbilityId,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Who a homing missile follows, or who an aimed one was meant for (to tell them it missed).
    pub target: Option<UnitId>,
    pub homing: bool,
    pub pierce: bool,
    pub reflectable: bool,
    pub visual: &'static str,
    pub effects: Vec<Effect>,
    /// Already struck (a piercing missile hits each unit once).
    pub struck: Vec<UnitId>,
    /// Metres left before it falls.
    pub left: f32,
    /// The combo field it flew through, if it's a projectile finisher.
    pub combo: Option<Field>,
}

#[derive(Resource, Default)]
pub struct Missiles {
    next: u32,
    pub list: Vec<Projectile>,
}

/// Something on the ground: a warned blow that lands once its time is up, or a field that
/// pulses until it ends.
pub struct Area {
    pub id: u32,
    pub warning: bool,
    pub src: HitSource,
    pub ability: Option<AbilityId>,
    pub center: Vec3,
    pub radius: f32,
    pub start: f64,
    pub duration: f64,
    pub interval: f64,
    pub next_pulse: f64,
    pub field: Option<Field>,
    /// Sprung by the first enemy to step in, then gone.
    pub trap: bool,
    /// What lands on enemies (a warned blow's effects, or a field's pulse) and on friends.
    pub enemies: Vec<Effect>,
    pub allies: Vec<Effect>,
}

#[derive(Resource, Default)]
pub struct Areas {
    next: u32,
    pub list: Vec<Area>,
}

/// Looses `def`'s missile from `e`: at `target` (homing), or straight at `ground` (where a
/// player aimed) or at where the target is (where a monster expects it to be).
pub fn launch(world: &mut World, e: Entity, def: &AbilityDef, target: Option<Entity>, ground: Option<Vec3>, effects: Vec<Effect>) {
    let Some(m) = def.missile else { return };
    let Some(src) = super::source(world, e, Some(def)) else { return };
    let Some(ability) = catalog().ability_id(def.key) else { return };
    let Some(me) = world.get::<Transform>(e).copied() else { return };
    let start = chest(world, e);
    let target_id = target.and_then(|t| unit_id(world, t));
    let aim = match target {
        Some(t) if m.homing => chest(world, t),
        Some(t) => {
            let at = chest(world, t);
            match ground {
                // The player's aim: the spot they chose, at the height of the target's chest.
                Some(g) if world.get::<PlayerControl>(e).is_some() => Vec3::new(g.x, at.y, g.z),
                _ if world.get::<PlayerControl>(e).is_none() => lead(world, t, start, at, m.speed),
                _ => at,
            }
        }
        None => ground.map_or(start + Vec3::new(me.forward().x, 0.0, me.forward().y) * def.range, |g| Vec3::new(g.x, start.y, g.z)),
    };
    let mut dir = aim - start;
    if dir.length_squared() < 1e-4 {
        dir = Vec3::new(me.forward().x, 0.0, me.forward().y);
    }
    let dir = dir.normalize();
    let radius = world.get::<Body>(e).map_or(0.4, |b| b.radius);
    let id = {
        let mut ms = world.resource_mut::<Missiles>();
        ms.next = ms.next.wrapping_add(1);
        let id = ms.next;
        ms.list.push(Projectile {
            id,
            src,
            ability,
            pos: start + Vec3::new(dir.x, 0.0, dir.z) * radius,
            vel: dir * m.speed,
            target: target_id,
            homing: m.homing && target_id.is_some(),
            pierce: m.pierce,
            reflectable: m.reflectable,
            visual: m.visual,
            effects,
            struck: Vec::new(),
            left: if m.homing { def.range * 2.0 + 10.0 } else { def.range + OVERSHOOT },
            combo: None,
        });
        id
    };
    emit(world, Event::ProjectileLaunched { id, from: src.unit, visual: m.visual.to_string() });
}

/// Where a target running at its current speed will be when a missile from `from` reaches it.
fn lead(world: &World, t: Entity, from: Vec3, at: Vec3, speed: f32) -> Vec3 {
    let v = world.get::<Motion>(t).map_or(Vec3::ZERO, |m| Vec3::new(m.observed.x, 0.0, m.observed.z));
    let mut aim = at;
    for _ in 0..2 {
        let flight = (aim - from).length() / speed.max(1.0);
        aim = at + v * flight;
    }
    aim
}

/// Missiles fly, strike, and fall.
pub fn update(world: &mut World) {
    let flying = std::mem::take(&mut world.resource_mut::<Missiles>().list);
    let mut keep = Vec::with_capacity(flying.len());
    for mut p in flying {
        if fly(world, &mut p) {
            keep.push(p);
        }
    }
    // Anything loosed meanwhile flies from the next step.
    let mut ms = world.resource_mut::<Missiles>();
    keep.append(&mut ms.list);
    ms.list = keep;
}

/// One step of flight. Returns false once it's done.
fn fly(world: &mut World, p: &mut Projectile) -> bool {
    let speed = p.vel.length();
    if p.homing {
        let target = p.target.and_then(|id| entity(world, id)).filter(|&t| world.get::<Vitals>(t).is_some_and(|v| !v.dead));
        let Some(t) = target else {
            end(world, p, None);
            return false;
        };
        let d = chest(world, t) - p.pos;
        if d.length_squared() > 1e-6 {
            p.vel = d.normalize() * speed;
        }
    }
    let from = p.pos;
    let step = p.vel * DT;
    let to = from + step;
    if p.combo.is_none() && catalog().ability(p.ability).finisher == Some(Finisher::Projectile) {
        p.combo = effects::field_at(world, to, p.src.faction);
    }
    for (te, s) in struck(world, p, from, to) {
        let at = from + step * s;
        match land(world, p, te, at) {
            Landed::Through => continue,
            Landed::Turned => {
                p.pos = at;
                return true;
            }
            Landed::Stopped => {
                if !p.pierce {
                    p.pos = at;
                    let hit = unit_id(world, te);
                    end(world, p, hit);
                    return false;
                }
            }
        }
    }
    let land = &world.resource::<Land>().0;
    if land.blocked(Vec2::new(to.x, to.z), 0.05) || to.y < land.ground(to.x, to.z) - 0.05 {
        p.pos = to;
        end(world, p, None);
        return false;
    }
    p.pos = to;
    p.left -= step.length();
    if p.left <= 0.0 {
        end(world, p, None);
        return false;
    }
    true
}

/// Units the missile passes through between `from` and `to`, nearest first, with how far along
/// the step each is met.
fn struck(world: &World, p: &Projectile, from: Vec3, to: Vec3) -> Vec<(Entity, f32)> {
    let a = Vec2::new(from.x, from.z);
    let b = Vec2::new(to.x, to.z);
    let seg = b - a;
    let len2 = seg.length_squared();
    let mid = (a + b) * 0.5;
    let spatial = world.resource::<Spatial>();
    let mut out: Vec<(Entity, f32, UnitId)> = Vec::new();
    for (id, c) in spatial.near(mid, seg.length() * 0.5 + 3.0) {
        if p.struck.contains(&id) || (p.homing && Some(id) != p.target) {
            continue;
        }
        let Some(e) = entity(world, id) else { continue };
        let (Some(u), Some(v), Some(t)) = (world.get::<Unit>(e), world.get::<Vitals>(e), world.get::<Transform>(e)) else { continue };
        if !u.faction.is_enemy(p.src.faction) || v.dead {
            continue;
        }
        let body = world.get::<Body>(e).copied().unwrap_or(Body { radius: 0.4, height: 1.8 });
        let s = if len2 > 1e-8 { ((c - a).dot(seg) / len2).clamp(0.0, 1.0) } else { 0.0 };
        if (a + seg * s - c).length() > body.radius + THICKNESS {
            continue;
        }
        let y = from.y + (to.y - from.y) * s;
        if y < t.pos.y - 0.2 || y > t.pos.y + body.height + 0.2 {
            continue;
        }
        out.push((e, s, id));
    }
    out.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.2.cmp(&y.2)));
    out.into_iter().map(|(e, s, _)| (e, s)).collect()
}

enum Landed {
    /// Rolled through: it flies on.
    Through,
    /// Turned back by a reflecting shield.
    Turned,
    /// It struck (or was blocked).
    Stopped,
}

fn land(world: &mut World, p: &mut Projectile, te: Entity, at: Vec3) -> Landed {
    let t = now(world);
    let Some(target) = unit_id(world, te) else { return Landed::Through };
    p.struck.push(target);
    // Rolling: it passes through.
    if world.get::<Vitals>(te).is_some_and(|v| t < v.invulnerable_until) {
        emit(world, Event::Avoided { target, source: p.src.unit, how: Avoid::Evaded });
        return Landed::Through;
    }
    // A reflecting shield facing it sends it back at whoever loosed it.
    let reflecting = auras::sum(world, te, |a| if a.reflects { 1.0 } else { 0.0 }) > 0.0;
    if p.reflectable && reflecting && guard::facing(world, te, at, guard::SHIELD_ARC) && let Some(shooter) = entity(world, p.src.unit) {
        emit(world, Event::Avoided { target, source: p.src.unit, how: Avoid::Reflected });
        let faction = world.get::<Unit>(te).map_or(p.src.faction, |u| u.faction);
        p.target = Some(p.src.unit);
        p.src.unit = target;
        p.src.faction = faction;
        p.homing = world.get::<Vitals>(shooter).is_some_and(|v| !v.dead);
        p.reflectable = false;
        p.struck.clear();
        p.left = 40.0;
        p.vel = -p.vel;
        return Landed::Turned;
    }
    let mut src = p.src;
    src.from = at;
    let def = catalog().ability(p.ability);
    effects::apply_effects(world, &p.effects, &src, Some(def), &[te], at, Delivery::Missile);
    if let Some(field) = p.combo {
        effects::combo_with(world, field, Finisher::Projectile, at, &src, Some(te));
    }
    Landed::Stopped
}

/// The missile's flight is over. An aimed one that never reached its mark missed it.
fn end(world: &mut World, p: &Projectile, hit: Option<UnitId>) {
    if hit.is_none() && !p.homing && let Some(meant) = p.target.filter(|id| !p.struck.contains(id)) {
        emit(world, Event::Avoided { target: meant, source: p.src.unit, how: Avoid::Missed });
    }
    emit(world, Event::ProjectileEnded { id: p.id, at: p.pos, hit });
}

/// A blow that lands on `at` once `def.impact_delay` has passed, warned of meanwhile.
pub fn spawn_impact(world: &mut World, src: &HitSource, def: &AbilityDef, at: Vec3, radius: f32) {
    let t = now(world);
    let mut src = *src;
    src.from = at;
    let mut areas = world.resource_mut::<Areas>();
    areas.next = areas.next.wrapping_add(1);
    let id = areas.next;
    areas.list.push(Area {
        id,
        warning: true,
        src,
        ability: catalog().ability_id(def.key),
        center: at,
        radius,
        start: t,
        duration: def.impact_delay,
        interval: 0.0,
        next_pulse: t + def.impact_delay,
        field: None,
        trap: false,
        enemies: def.effects.clone(),
        allies: Vec::new(),
    });
}

/// A lasting area that pulses on whoever stands in it (maybe a combo field).
#[allow(clippy::too_many_arguments)]
pub fn spawn_field(world: &mut World, src: &HitSource, field: Option<Field>, radius: f32, duration: f64, interval: f64, enemies: Vec<Effect>, allies: Vec<Effect>, at: Vec3) {
    let t = now(world);
    let mut src = *src;
    src.from = at;
    let mut areas = world.resource_mut::<Areas>();
    areas.next = areas.next.wrapping_add(1);
    let id = areas.next;
    areas.list.push(Area {
        id,
        warning: false,
        src,
        ability: src.ability,
        center: at,
        radius,
        start: t,
        duration,
        interval: interval.max(0.1),
        next_pulse: t,
        field,
        trap: false,
        enemies,
        allies,
    });
}

/// A trap that waits on the ground for `lasts` seconds for an enemy to step in.
pub fn spawn_trap(world: &mut World, src: &HitSource, radius: f32, lasts: f64, enemies: Vec<Effect>, at: Vec3) {
    spawn_field(world, src, None, radius, lasts, 0.1, enemies, Vec::new(), at);
    if let Some(a) = world.resource_mut::<Areas>().list.last_mut() {
        a.trap = true;
    }
}

/// Warned blows land, fields pulse, and spent ones go.
pub fn update_areas(world: &mut World) {
    let t = now(world);
    let mut landing = Vec::new();
    let mut pulses = Vec::new();
    {
        let mut areas = world.resource_mut::<Areas>();
        let list = std::mem::take(&mut areas.list);
        let mut keep = Vec::with_capacity(list.len());
        for mut a in list {
            if a.warning {
                if t >= a.start + a.duration {
                    landing.push(a);
                } else {
                    keep.push(a);
                }
                continue;
            }
            let end = a.start + a.duration;
            while t >= a.next_pulse && a.next_pulse < end {
                pulses.push((a.id, a.trap, a.src, a.ability, a.center, a.radius, a.enemies.clone(), a.allies.clone()));
                a.next_pulse += a.interval;
            }
            if t < end {
                keep.push(a);
            }
        }
        areas.list = keep;
    }
    let cat = catalog();
    for a in landing {
        let def = a.ability.map(|id| cat.ability(id));
        let allies = def.is_some_and(|d| d.allies);
        let reached = effects::victims(world, Shape::Circle { radius: a.radius }, a.center, 0.0, &a.src, allies, 10);
        emit(world, Event::AreaImpact { id: a.id, at: a.center, radius: a.radius, school: a.src.school });
        effects::apply_effects(world, &a.enemies, &a.src, def, &reached, a.center, Delivery::Strike);
        if def.and_then(|d| d.finisher) == Some(Finisher::Blast) {
            effects::combo(world, Finisher::Blast, a.center, &a.src, None);
        }
    }
    let mut sprung = Vec::new();
    for (id, trap, src, ability, center, radius, enemies, allies) in pulses {
        let def = ability.map(|id| cat.ability(id));
        if trap && sprung.contains(&id) {
            continue;
        }
        if !enemies.is_empty() {
            let reached = effects::victims(world, Shape::Circle { radius }, center, 0.0, &src, false, 10);
            if trap && !reached.is_empty() {
                sprung.push(id);
            }
            effects::apply_effects(world, &enemies, &src, def, &reached, center, Delivery::Pulse);
        }
        if !allies.is_empty() {
            let reached = effects::victims(world, Shape::Circle { radius }, center, 0.0, &src, true, 10);
            effects::apply_effects(world, &allies, &src, def, &reached, center, Delivery::Pulse);
        }
    }
    if !sprung.is_empty() {
        world.resource_mut::<Areas>().list.retain(|a| !sprung.contains(&a.id));
    }
}
