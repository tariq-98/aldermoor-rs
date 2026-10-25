//! Upkeep every step: endurance, health and resource come back, the downed bleed, monsters walk
//! home healing and come back after dying, revives progress, and summons run out.

use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::Event;

use crate::combat::{emit, life, now};
use crate::content::catalog;
use crate::stats::DOWNED_TIME;
use crate::{Anim, Auras, Brain, Control, Fighter, Home, Mode, Motion, Threat, Transform, Unit, Vitals, DT};

pub const ENDURANCE_MAX: f32 = 100.0;
pub const ENDURANCE_REGEN: f32 = 8.0;
/// Seconds after the last blow before a unit counts as out of combat.
pub const COMBAT_TIMEOUT: f64 = 6.0;

pub fn update(world: &mut World) {
    let t = now(world);
    let cat = catalog();
    // Upkeep only touches each unit's own numbers, so one pass does it in any order. Units that
    // bleed out or are due back are handled after, in ID order.
    let mut due: Vec<(protocol::UnitId, Entity, bool)> = crate::queries::with(world, |q| &mut q.upkeep, |world, q| {
    let mut due = Vec::new();
    for (e, u, mut v, f, m, b) in q.iter_mut(world) {
        if v.dead {
            if v.respawn_at.is_some_and(|at| t >= at) {
                due.push((u.id, e, true));
            }
            continue;
        }
        if v.downed {
            v.downed_health -= v.downed_max / DOWNED_TIME as f32 * DT;
            if v.downed_health <= 0.0 {
                due.push((u.id, e, false));
            }
            continue;
        }
        let guarding = f.as_ref().is_some_and(|f| f.guard_since.is_some());
        let rolling = m.is_some_and(|m| m.forced.is_some());
        let returning = b.is_some_and(|b| b.mode == Mode::Return);
        let in_combat = t - v.last_combat < COMBAT_TIMEOUT;
        // Endurance waits while you roll or hold up your guard.
        if !rolling && !guarding {
            v.endurance = (v.endurance + ENDURANCE_REGEN * DT).min(ENDURANCE_MAX);
        }
        if returning {
            // Giving up a fight heals a monster as it walks home, so pulling and dropping it
            // can't wear it down.
            v.health = (v.health + v.max_health * 0.25 * DT).min(v.max_health);
        } else if !in_combat {
            v.health = (v.health + v.max_health * 0.04 * DT).min(v.max_health);
        }
        if let Some(mut f) = f {
            if let Some(s) = f.specialty {
                let d = cat.specialty(s);
                let rate = if in_combat { d.regen_in } else { d.regen_out };
                f.resource = (f.resource + rate * DT).clamp(0.0, f.max_resource);
            }
            if !f.recent_victims.is_empty() {
                f.recent_victims.retain(|&(_, at)| t - at <= life::RALLY_WINDOW);
            }
        }
    }
    due
    });
    due.sort_unstable_by_key(|d| d.0);
    for (_, e, back) in due {
        if back {
            respawn_mob(world, e);
        } else {
            life::die(world, e, None);
        }
    }
    life::update_revives(world);
    life::expire_summons(world);
    life::clear_remains(world);
}

/// A monster comes back where it started, as it was.
fn respawn_mob(world: &mut World, e: Entity) {
    let t = now(world);
    let (Some(b), Some(home), Some(id)) = (world.get::<Brain>(e).copied(), world.get::<Home>(e).copied(), world.get::<Unit>(e).map(|u| u.id)) else {
        return;
    };
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        *v = Vitals::new(v.max_health);
    }
    if let Some(mut tr) = world.get_mut::<Transform>(e) {
        tr.pos = home.pos;
        tr.yaw = home.yaw;
    }
    if let Some(mut m) = world.get_mut::<Motion>(e) {
        *m = Motion::new(m.run_speed);
    }
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.target = None;
        f.auto_attack = false;
        f.cast = None;
        f.queued = None;
        f.cooldowns.clear();
        f.gcd_until = 0.0;
        f.lockout_until = 0.0;
        f.guard_since = None;
        f.action = protocol::Action::None;
        f.recent_victims.clear();
    }
    if let Some(mut a) = world.get_mut::<Auras>(e) {
        a.list.clear();
        a.dr = Default::default();
        a.defiance = a.defiance_max;
    }
    if let Some(mut th) = world.get_mut::<Threat>(e) {
        th.clear();
    }
    if let Some(mut c) = world.get_mut::<Control>(e) {
        *c = Control { speed: 1.0, ..Default::default() };
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        *a = Anim::default();
    }
    if let Some(mut brain) = world.get_mut::<Brain>(e) {
        *brain = Brain::new(b.mob, Vec2::new(home.pos.x, home.pos.z), b.aggro_radius, b.wander);
        brain.next_think = t + crate::brain::think_offset(id);
    }
    emit(world, Event::Respawned { unit: id });
}
