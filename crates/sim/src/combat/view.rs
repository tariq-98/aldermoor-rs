//! What the fight looks like from outside: the player's own bar and numbers, and the combat
//! detail (actions, casts, auras, projectiles, warnings and fields) for everyone in view.

use bevy_ecs::prelude::*;
use glam::Vec3;
use protocol::{AreaView, AuraView, CastBar, CombatView, PlayerState, ProjectileView, Shape, Slot, UnitCombat, UnitId};

use super::abilities::current_bar;
use super::{auras, effects, now};
use crate::content::{catalog, Targeting};
use crate::vitals::{COMBAT_TIMEOUT, ENDURANCE_MAX};
use crate::{Auras, Faction, Fighter, Transform, Unit, Units, Vitals};

/// How far a viewer sees.
pub const VIEW_RANGE: f32 = 160.0;

fn cast_bar(f: &Fighter, t: f64) -> Option<CastBar> {
    let c = f.cast?;
    let def = catalog().ability(c.ability);
    Some(CastBar {
        ability: def.key.to_string(),
        name: def.name.to_string(),
        elapsed: (t - c.start).max(0.0) as f32,
        duration: c.duration as f32,
        channel: c.channel,
        interruptible: def.interruptible,
    })
}

fn aura_list(a: Option<&Auras>, t: f64) -> Vec<AuraView> {
    let cat = catalog();
    a.map(|a| {
        a.list
            .iter()
            .map(|i| {
                let d = cat.aura(i.aura);
                AuraView {
                    key: d.key.to_string(),
                    name: d.name.to_string(),
                    stacks: i.stacks,
                    remaining: auras::remaining(i, t),
                    duration: i.duration as f32,
                    debuff: d.debuff,
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The viewer's own numbers: endurance, resource, the bar, the cast, auras.
pub fn player_state(world: &World, id: UnitId) -> Option<PlayerState> {
    let e = world.resource::<Units>().entity(id)?;
    let v = world.get::<Vitals>(e)?;
    let f = world.get::<Fighter>(e)?;
    let progress = world.get::<crate::Progress>(e);
    let (coins, inventory, loot, shop) = crate::loot::player_view(world, e);
    let (skills, work, nodes, station) = crate::crafting::player_view(world, e);
    let t = now(world);
    let cat = catalog();
    let bar_ids = current_bar(world, e);
    let bar: Vec<Slot> = bar_ids
        .iter()
        .map(|slot| match slot {
            Some(id) => {
                let d = cat.ability(*id);
                Slot {
                    ability: d.key.to_string(),
                    name: d.name.to_string(),
                    cooldown: (f.ready_at(*id) - t).max(0.0) as f32,
                    recharge: d.cooldown as f32,
                    cost: d.cost,
                    range: d.range,
                    ground: matches!(d.targeting, Targeting::Ground { .. }),
                    radius: match d.targeting {
                        Targeting::Ground { radius } => radius,
                        _ => 0.0,
                    },
                    auto: d.auto,
                }
            }
            None => Slot::default(),
        })
        .collect();
    Some(PlayerState {
        unit: id,
        endurance: v.endurance,
        max_endurance: ENDURANCE_MAX,
        cooldowns: bar.iter().map(|s| s.cooldown).collect(),
        auto_attack: f.auto_attack,
        in_combat: t - v.last_combat < COMBAT_TIMEOUT,
        can_respawn: v.dead,
        resource_kind: f.resource_kind,
        resource: f.resource,
        max_resource: f.max_resource,
        bar,
        gcd: (f.gcd_until - t).max(0.0) as f32,
        cast: cast_bar(f, t),
        auras: aura_list(world.get::<Auras>(e), t),
        guarding: f.guard_since.is_some(),
        downed: v.downed,
        weapon_set: f.active_set as u8,
        swap_ready: (f.swap_ready_at - t).max(0.0) as f32,
        swimming: world.get::<crate::Motion>(e).is_some_and(|m| m.swimming),
        sprinting: world.get::<crate::Motion>(e).is_some_and(|m| m.sprinting),
        specialty: f.specialty.map_or("", |s| s.key()).to_string(),
        level: world.get::<crate::Unit>(e).map_or(1, |u| u.level),
        xp: progress.zip(f.specialty).map_or(0, |(p, s)| p.xp[s.index()]),
        xp_next: progress.zip(f.specialty).and_then(|(p, s)| crate::content::progression::to_next(p.level(s))).unwrap_or(0),
        unassigned_xp: progress.map_or(0, |p| p.unassigned),
        coins,
        inventory,
        loot,
        shop,
        skills,
        work,
        nodes,
        station,
        potion_ready: crate::consumables::potion_ready(world, e, t),
    })
}

/// Combat detail for everything within sight of `viewer` (everything, without one).
pub fn combat_view(world: &World, viewer: Option<UnitId>) -> CombatView {
    let t = now(world);
    let cat = catalog();
    let units = world.resource::<Units>();
    let me = viewer.and_then(|id| units.entity(id));
    let center = me.and_then(|e| world.get::<Transform>(e)).map(|t| t.pos);
    let side = me.and_then(|e| world.get::<Unit>(e)).map_or(Faction::Player, |u| u.faction);
    let near = |p: Vec3| center.is_none_or(|c| (p - c).length_squared() <= VIEW_RANGE * VIEW_RANGE);

    // Room for everyone up front: a view's units are big, and growing the list a step at a time
    // copies them over and over.
    let mut view = CombatView { tick: world.resource::<crate::Clock>().tick, units: Vec::with_capacity(units.entities.len() + units.foreign.len()), ..Default::default() };
    for e in units.all() {
        // Where it is first: most of a big world is out of sight, and needn't be looked at further.
        let Some(tr) = world.get::<Transform>(e).filter(|tr| near(tr.pos)) else { continue };
        let (Some(u), Some(v), Some(f)) = (world.get::<Unit>(e), world.get::<Vitals>(e), world.get::<Fighter>(e)) else {
            continue;
        };
        if v.dead && t - v.died_at > 8.0 && world.get::<crate::PlayerControl>(e).is_none() {
            continue;
        }
        let a = world.get::<Auras>(e);
        view.units.push(UnitCombat {
            id: u.id,
            action: f.action,
            action_time: (t - f.action_time).max(0.0) as f32,
            cast: cast_bar(f, t),
            auras: aura_list(a, t),
            resource_kind: f.resource_kind,
            resource: f.resource,
            max_resource: f.max_resource,
            guarding: f.guard_since.is_some(),
            downed: v.downed,
            defiance: a.filter(|a| a.defiance_max > 0.0).map(|a| (a.defiance, a.defiance_max)),
        });
        // A big blow winding up: warn everyone where it will land.
        if let Some(c) = f.cast.filter(|c| !c.channel) {
            let def = cat.ability(c.ability);
            if def.telegraph && let Some(shape) = effects::shape_of(def) {
                let at = if matches!(def.targeting, Targeting::Ground { .. }) { c.ground.unwrap_or(tr.pos) } else { tr.pos };
                view.areas.push(AreaView {
                    id: 0x8000_0000 | u.id,
                    warning: true,
                    shape,
                    center: at,
                    yaw: c.yaw,
                    elapsed: (t - c.start).max(0.0) as f32,
                    duration: c.duration as f32,
                    hostile: u.faction.is_enemy(side),
                    school: def.school,
                    field: None,
                });
            }
        }
    }
    for p in &world.resource::<super::missiles::Missiles>().list {
        if near(p.pos) {
            view.projectiles.push(ProjectileView { id: p.id, position: p.pos, velocity: p.vel, visual: p.visual.to_string(), school: p.src.school });
        }
    }
    for a in &world.resource::<super::missiles::Areas>().list {
        if !near(a.center) {
            continue;
        }
        view.areas.push(AreaView {
            id: a.id,
            warning: a.warning,
            shape: Shape::Circle { radius: a.radius },
            center: a.center,
            yaw: 0.0,
            elapsed: (t - a.start).max(0.0) as f32,
            duration: a.duration as f32,
            hostile: a.src.faction.is_enemy(side) && !a.enemies.is_empty(),
            school: a.src.school,
            field: a.field.map(|f| f.name().to_string()),
        });
    }
    view
}
