//! Combat: skills and casting, effects, auras and crowd control, projectiles and areas, guards
//! and parries, threat, being downed, and the commands players send.
//!
//! Combat code works on the whole world (`&mut World`): one blow touches its target, its
//! attacker's threat, both units' timers and the events. Units are visited in ID order.

pub mod abilities;
pub mod auras;
pub mod effects;
pub mod guard;
pub mod life;
pub mod missiles;
pub mod view;

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Command, Event, School, UnitId};

use crate::content::{AbilityDef, AbilityId};
use crate::{movement, Body, Clock, Control, Faction, Fighter, Inbox, Land, Motion, MoveIntent, Outbox, PlayerControl, Stats, Transform, Unit, Units, Vitals};

/// Who dealt an effect, captured when it set out, so missiles, fields and conditions keep
/// working after their source moves on or dies.
#[derive(Clone, Copy, Debug)]
pub struct HitSource {
    pub unit: UnitId,
    pub power: f32,
    pub level: u8,
    pub school: School,
    pub crit: f32,
    pub faction: Faction,
    pub ability: Option<AbilityId>,
    /// Where the blow comes from (the attacker, or the missile): what a guard must face.
    pub from: Vec3,
}

pub(crate) fn now(world: &World) -> f64 {
    world.resource::<Clock>().time
}

pub(crate) fn entity(world: &World, id: UnitId) -> Option<Entity> {
    world.resource::<Units>().entity(id)
}

pub(crate) fn emit(world: &mut World, e: Event) {
    world.resource_mut::<Outbox>().0.push(e);
}

/// A message for a player (monsters' failures stay quiet).
pub(crate) fn tell(world: &mut World, e: Entity, text: &str) {
    if world.get::<PlayerControl>(e).is_some() {
        emit(world, Event::Message { text: text.to_string() });
    }
}

pub(crate) fn unit_id(world: &World, e: Entity) -> Option<UnitId> {
    world.get::<Unit>(e).map(|u| u.id)
}

/// The point a blow aims for: the middle of the body.
pub(crate) fn chest(world: &World, e: Entity) -> Vec3 {
    let t = world.get::<Transform>(e).map_or(Vec3::ZERO, |t| t.pos);
    let h = world.get::<Body>(e).map_or(1.6, |b| b.height);
    t + Vec3::Y * h * 0.6
}

pub(crate) fn control(world: &World, e: Entity) -> Control {
    world.get::<Control>(e).copied().unwrap_or(Control { speed: 1.0, ..Default::default() })
}

/// The space between two bodies (centre distance less both radii).
pub(crate) fn gap(world: &World, a: Entity, b: Entity) -> f32 {
    let (Some(ta), Some(tb)) = (world.get::<Transform>(a), world.get::<Transform>(b)) else { return f32::MAX };
    let ra = world.get::<Body>(a).map_or(0.4, |b| b.radius);
    let rb = world.get::<Body>(b).map_or(0.4, |b| b.radius);
    ((tb.xz() - ta.xz()).length() - ra - rb).max(0.0)
}

/// `b` is a living enemy of `a` (downed counts: it can still be finished off).
pub(crate) fn can_attack(world: &World, a: Entity, b: Entity) -> bool {
    let (Some(ua), Some(ub), Some(vb)) = (world.get::<Unit>(a), world.get::<Unit>(b), world.get::<Vitals>(b)) else { return false };
    a != b && ua.faction.is_enemy(ub.faction) && !vb.dead
}

pub(crate) fn is_ally(world: &World, a: Entity, b: Entity) -> bool {
    match (world.get::<Unit>(a), world.get::<Unit>(b)) {
        (Some(ua), Some(ub)) => ua.faction == ub.faction,
        _ => false,
    }
}

/// Nothing solid between two points (houses, the mill, the shed). Sampled every half metre.
pub(crate) fn line_of_sight(world: &World, a: Vec3, b: Vec3) -> bool {
    let land = &world.resource::<Land>().0;
    let (a2, b2) = (Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
    let d = b2 - a2;
    let steps = (d.length() / 0.5).ceil() as i32;
    for i in 1..steps {
        let p = a2 + d * (i as f32 / steps as f32);
        if land.blocked(p, 0.05) {
            return false;
        }
    }
    true
}

/// `e` as the source of an effect (with `ability`'s school), as of now.
pub fn source(world: &World, e: Entity, ability: Option<&AbilityDef>) -> Option<HitSource> {
    let u = world.get::<Unit>(e)?;
    let s = world.get::<Stats>(e).copied().unwrap_or(Stats { power: 10.0, armor: 0.0, crit: 0.0, haste: 0.0 });
    let crit = s.crit + auras::sum(world, e, |a| a.crit);
    Some(HitSource {
        unit: u.id,
        power: s.power,
        level: u.level,
        school: ability.map_or(School::Physical, |a| a.school),
        crit,
        faction: u.faction,
        ability: ability.and_then(|a| crate::content::catalog().ability_id(a.key)),
        from: world.get::<Transform>(e).map_or(Vec3::ZERO, |t| t.pos),
    })
}

/// Applies queued commands in the order they arrived.
pub fn apply_commands(world: &mut World) {
    let commands = std::mem::take(&mut world.resource_mut::<Inbox>().0);
    let now = now(world);
    for (id, c) in commands {
        let Some(e) = entity(world, id) else { continue };
        let dead = world.get::<Vitals>(e).is_none_or(|v| v.dead);
        match c {
            Command::Move { direction, walk, sprint, face_yaw } => {
                if let Some(mut m) = world.get_mut::<Motion>(e) {
                    m.intent = MoveIntent { direction, walk, sprint, face_yaw };
                }
            }
            Command::Jump => {
                let ctl = control(world, e);
                let downed = world.get::<Vitals>(e).is_some_and(|v| v.downed);
                if let Some(mut m) = world.get_mut::<Motion>(e) && ctl.can_move() && !downed {
                    m.jump_queued = true;
                }
            }
            Command::Dodge { direction } => {
                let ctl = control(world, e);
                let swimming = world.get::<Motion>(e).is_some_and(|m| m.swimming);
                if ctl.stunned || ctl.fleeing.is_some() || swimming || world.get::<Vitals>(e).is_some_and(|v| v.downed) {
                    continue;
                }
                let rolled = world.resource_scope(|world, land: Mut<Land>| {
                    let mut q = world.query::<(&mut Transform, &mut Motion, &mut Vitals, &Body)>();
                    match q.get_mut(world, e) {
                        Ok((mut t, mut m, mut v, b)) => movement::start_dodge(&land.0, now, &mut t, &mut m, &mut v, b, direction),
                        Err(_) => false,
                    }
                });
                if rolled {
                    // Rolling drops the guard and breaks off a cast (not a channel you can move in).
                    guard::lower(world, e);
                    abilities::cancel_standing_cast(world, e);
                    emit(world, Event::Dodged { unit: id });
                }
            }
            Command::SetTarget(t) => {
                if let Some(mut f) = world.get_mut::<Fighter>(e) {
                    f.target = t;
                }
            }
            Command::SetAutoAttack(on) => {
                if let Some(mut f) = world.get_mut::<Fighter>(e) {
                    f.auto_attack = on;
                }
            }
            Command::UseSlot { slot, ground } if !dead => abilities::use_slot(world, e, slot, ground),
            Command::UseSlot { .. } => {}
            Command::Respawn => life::respawn_player(world, e),
            Command::Guard(up) => {
                if up && world.get::<Motion>(e).is_some_and(|m| m.swimming) {
                    tell(world, e, "You can't guard while swimming.");
                } else if up {
                    guard::raise(world, e);
                } else {
                    guard::lower(world, e);
                }
            }
            Command::SwapWeapons => abilities::swap(world, e),
            Command::Revive { target } => life::start_revive(world, e, target),
        }
    }
}
