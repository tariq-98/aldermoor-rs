//! Going down, getting up, dying, being revived and respawning, and summoned helpers.
//!
//! A player whose health runs out is downed, not dead: they fight on with the downed skills
//! while their downed pool bleeds away. Felling an enemy they hurt, a friend's help, or healing
//! the pool back to full gets them up; if it runs dry they're defeated, and come back by choice
//! at home (or a friend raises them once the fight's over).

use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::{Action, Event, Pose, UnitId};

use super::{abilities, auras, effects, emit, entity, gap, guard, is_ally, now, tell, unit_id};
use crate::content::catalog;
use crate::{Anim, Brain, Fighter, Home, Motion, PlayerControl, Summoned, Threat, Transform, Unit, Units, Vitals};

/// A downed unit's pool, as a share of its health.
pub const DOWNED_POOL: f32 = 0.5;
/// Health on getting up from downed.
pub const RALLY_HEALTH: f32 = 0.25;
/// Felling an enemy you hurt this recently gets you up.
pub const RALLY_WINDOW: f64 = 10.0;
/// Seconds to help a downed friend up, and to raise a defeated one.
pub const REVIVE_DOWNED: f64 = 3.0;
pub const REVIVE_DEAD: f64 = 6.0;
/// How close a reviver must stay.
pub const REVIVE_REACH: f32 = 2.0;
/// Health a raised friend comes back with.
pub const REVIVE_HEALTH: f32 = 0.3;

/// Health ran out. Players go down; everything else dies.
pub fn lethal(world: &mut World, e: Entity, killer: Option<UnitId>) {
    if world.get::<PlayerControl>(e).is_none() {
        die(world, e, killer);
        return;
    }
    let t = now(world);
    abilities::cancel_cast(world, e, false);
    guard::lower(world, e);
    // Whatever was holding them lets go: the downed skills must be usable.
    auras::remove_where(world, e, |a| a.cc.is_some_and(|c| c.hard() || c.category().is_some()));
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        v.health = 0.0;
        v.downed = true;
        v.downed_max = (v.max_health * DOWNED_POOL).max(1.0);
        v.downed_health = v.downed_max;
        v.last_combat = t;
    }
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.queued = None;
        f.reviving = None;
    }
    effects::act(world, e, Action::Downed);
    if let Some(mut m) = world.get_mut::<Motion>(e) {
        m.jump_queued = false;
    }
    let unit = unit_id(world, e).unwrap_or(0);
    emit(world, Event::Downed { unit });
    tell(world, e, "You're down! Fight on, or get help.");
}

/// Defeated: out of the fight until respawned or raised.
pub fn die(world: &mut World, e: Entity, killer: Option<UnitId>) {
    let t = now(world);
    let Some(id) = unit_id(world, e) else { return };
    if world.get::<Vitals>(e).is_none_or(|v| v.dead) {
        return;
    }
    abilities::cancel_cast(world, e, false);
    guard::lower(world, e);
    auras::clear(world, e);
    let respawn = world.get::<Brain>(e).and_then(|b| catalog().mob(b.mob)).map(|m| m.respawn).filter(|&r| r > 0.0);
    let summoned = world.get::<Summoned>(e).is_some();
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        v.health = 0.0;
        v.dead = true;
        v.downed = false;
        v.downed_health = 0.0;
        v.died_at = t;
        v.respawn_at = if summoned { None } else { respawn.map(|r| t + r) };
    }
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.target = None;
        f.auto_attack = false;
        f.queued = None;
        f.reviving = None;
        f.action = Action::None;
    }
    if let Some(mut th) = world.get_mut::<Threat>(e) {
        th.clear();
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        a.pose = Pose::Dead;
        a.since = t;
        a.hold_until = 0.0;
    }
    emit(world, Event::Died { unit: id, killer });
    crate::loot::drop_loot(world, e);
    crate::progress::pay_out(world, e);

    // Everyone fighting it lets it go; monsters stop swinging at a corpse. Each unit only
    // changes itself here, so one pass over the components does it in any order.
    crate::queries::with(world, |q| &mut q.let_go, |world, q| {
    for (mut f, th, player) in q.iter_mut(world) {
        if f.target == Some(id) {
            f.target = None;
            if !player {
                f.auto_attack = false;
            }
        }
        if f.reviving.is_some_and(|(r, _)| r == id) {
            f.reviving = None;
        }
        if let Some(mut th) = th
            && (th.contains(id) || th.taunted_by == Some(id))
        {
            th.remove(id);
        }
    }
    });
    // Downed players who hurt it lately get up (only players go down).
    let mut rallying: Vec<(UnitId, Entity)> = crate::queries::with(world, |q| &mut q.rally, |world, q| {
        q.iter(world)
            .filter(|(_, _, v, f)| v.downed && f.recent_victims.iter().any(|&(v, at)| v == id && t - at <= RALLY_WINDOW))
            .map(|(e, u, _, _)| (u.id, e))
            .collect()
    });
    rallying.sort_unstable_by_key(|r| r.0);
    for (_, other) in rallying {
        rally(world, other, RALLY_HEALTH);
    }
    // Its summons go with it.
    for other in summons(world, |s, v| s.owner == id && !v.dead) {
        die(world, other, None);
    }
}

/// Back up from downed with `fraction` of health.
pub fn rally(world: &mut World, e: Entity, fraction: f32) {
    let Some(v) = world.get::<Vitals>(e).copied() else { return };
    if !v.downed || v.dead {
        return;
    }
    let t = now(world);
    abilities::cancel_cast(world, e, false);
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        v.downed = false;
        v.downed_health = 0.0;
        v.health = (v.max_health * fraction).max(1.0);
    }
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.action = Action::None;
        f.action_time = t;
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        a.pose = Pose::Idle;
        a.since = t;
        a.hold_until = 0.0;
    }
    let unit = unit_id(world, e).unwrap_or(0);
    emit(world, Event::Rallied { unit });
}

/// A defeated player raised where they lie by someone else's hand.
pub fn revive_dead(world: &mut World, e: Entity, fraction: f32) {
    if world.get::<PlayerControl>(e).is_none() || world.get::<Vitals>(e).is_none_or(|v| !v.dead) {
        return;
    }
    let t = now(world);
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        let max = v.max_health;
        *v = Vitals::new(max);
        v.health = (max * fraction).max(1.0);
        v.endurance = 0.0;
        v.last_combat = t;
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        *a = Anim { pose: Pose::Idle, since: t, hold_until: 0.0 };
    }
    let unit = unit_id(world, e).unwrap_or(0);
    if let Some(src) = super::source(world, e, None) {
        auras::apply_key(world, "revive_sickness", e, &src);
    }
    emit(world, Event::Revived { unit });
}

/// A defeated player chooses to come back at home.
pub fn respawn_player(world: &mut World, e: Entity) {
    let Some(v) = world.get::<Vitals>(e).copied() else { return };
    if !v.dead || world.get::<PlayerControl>(e).is_none() {
        return;
    }
    let Some(home) = world.get::<Home>(e).copied() else { return };
    let t = now(world);
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
    let start = world.get::<Fighter>(e).and_then(|f| f.specialty).map(|s| catalog().specialty(s).start);
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.cast = None;
        f.queued = None;
        f.target = None;
        f.auto_attack = false;
        f.guard_since = None;
        f.reviving = None;
        f.action = Action::None;
        f.resource = start.unwrap_or(0.0).min(f.max_resource);
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        *a = Anim { pose: Pose::Idle, since: t, hold_until: 0.0 };
    }
    let unit = unit_id(world, e).unwrap_or(0);
    emit(world, Event::Respawned { unit });
}

/// Starts helping `target` up: a downed friend, or a defeated one once nobody's fighting.
pub fn start_revive(world: &mut World, e: Entity, target: UnitId) {
    let t = now(world);
    let Some(te) = entity(world, target) else { return };
    if te == e || !is_ally(world, e, te) || world.get::<Vitals>(e).is_none_or(|v| !v.alive()) {
        return;
    }
    let Some(tv) = world.get::<Vitals>(te).copied() else { return };
    let in_combat = world.get::<Vitals>(e).is_some_and(|v| t - v.last_combat < crate::vitals::COMBAT_TIMEOUT);
    if !tv.downed && !tv.dead {
        return;
    }
    if tv.dead && in_combat {
        tell(world, e, "You can't raise the fallen in the middle of a fight.");
        return;
    }
    if gap(world, e, te) > REVIVE_REACH {
        tell(world, e, "Too far away.");
        return;
    }
    abilities::cancel_cast(world, e, false);
    guard::lower(world, e);
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.reviving = Some((target, t));
        f.auto_attack = false;
    }
    effects::begin(world, e, Action::Revive);
}

/// Revives in progress: they finish, or stop when the helper moves, is hurt down, or the
/// friend is gone.
pub fn update_revives(world: &mut World) {
    let t = now(world);
    let mut helpers: Vec<(UnitId, Entity)> = crate::queries::with(world, |q| &mut q.revivers, |world, q| {
        q.iter(world).filter(|(_, _, f)| f.reviving.is_some()).map(|(e, u, _)| (u.id, e)).collect()
    });
    helpers.sort_unstable_by_key(|h| h.0);
    for (_, e) in helpers {
        let Some((target, since)) = world.get::<Fighter>(e).and_then(|f| f.reviving) else { continue };
        let te = entity(world, target);
        let helper_ok = world.get::<Vitals>(e).is_some_and(|v| v.alive())
            && world.get::<Motion>(e).is_some_and(|m| !m.moving())
            && world.get::<Fighter>(e).is_some_and(|f| f.cast.is_none() && f.guard_since.is_none());
        let tv = te.and_then(|te| world.get::<Vitals>(te).copied());
        let target_ok = te.is_some_and(|te| gap(world, e, te) <= REVIVE_REACH + 0.5) && tv.is_some_and(|v| v.downed || v.dead);
        if !helper_ok || !target_ok {
            stop_revive(world, e);
            continue;
        }
        let (te, tv) = (te.unwrap(), tv.unwrap());
        let needed = if tv.downed { REVIVE_DOWNED } else { REVIVE_DEAD };
        if t - since >= needed {
            stop_revive(world, e);
            if tv.downed {
                rally(world, te, 0.5);
            } else {
                revive_dead(world, te, REVIVE_HEALTH);
            }
        }
    }
}

fn stop_revive(world: &mut World, e: Entity) {
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.reviving = None;
        if f.action == Action::Revive {
            f.action = Action::None;
        }
    }
}

/// Calls `count` of `mob` to fight beside `owner` for `lasts` seconds, at the owner's level.
pub fn summon(world: &mut World, owner: Entity, mob: &'static str, count: u8, lasts: f64) {
    let t = now(world);
    let Some(ou) = world.get::<Unit>(owner).cloned() else { return };
    let Some(ot) = world.get::<Transform>(owner).copied() else { return };
    // Old summons of the same kind make way.
    for e in summons(world, |s, v| s.owner == ou.id && !v.dead) {
        if world.get::<Brain>(e).is_some_and(|b| b.mob == mob) {
            die(world, e, None);
        }
    }
    for i in 0..count {
        // Beside the owner, alternating right and left.
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        let a = ot.yaw + side * std::f32::consts::FRAC_PI_2;
        let mut at = ot.xz() + Vec2::new(a.sin(), a.cos()) * (1.8 + (i / 2) as f32 * 0.9);
        if world.resource::<crate::Land>().0.blocked(at, 0.5) {
            at = ot.xz();
        }
        let Some(id) = crate::spawn_mob(world, mob, at, Some(ou.level), ou.faction) else { continue };
        let Some(e) = entity(world, id) else { continue };
        world.entity_mut(e).insert(Summoned { owner: ou.id, until: t + lasts });
        // A summon fights what its owner fights.
        let target = world.get::<Fighter>(owner).and_then(|f| f.target).filter(|&tid| entity(world, tid).is_some_and(|te| super::can_attack(world, e, te)));
        if let Some(tid) = target {
            effects::engage(world, e, tid);
        }
        if let Some(mut tr) = world.get_mut::<Transform>(e) {
            tr.yaw = ot.yaw;
        }
    }
}

/// Summons whose time is up, or whose owner is gone, crumble.
pub fn expire_summons(world: &mut World) {
    let t = now(world);
    for e in summons(world, |_, v| !v.dead) {
        let Some(s) = world.get::<Summoned>(e).copied() else { continue };
        if world.get::<Vitals>(e).is_none_or(|v| v.dead) {
            continue;
        }
        let owner_gone = entity(world, s.owner).and_then(|o| world.get::<Vitals>(o)).is_none_or(|v| v.dead);
        if t >= s.until || owner_gone {
            die(world, e, None);
        }
    }
}

/// Summoned units that pass `keep`, in ID order: only summons are visited, however many other
/// units there are.
fn summons(world: &mut World, keep: impl Fn(&Summoned, &Vitals) -> bool) -> Vec<Entity> {
    let mut list: Vec<(UnitId, Entity)> = crate::queries::with(world, |q| &mut q.summons, |world, q| {
        q.iter(world).filter(|(_, _, s, v)| keep(s, v)).map(|(e, u, _, _)| (u.id, e)).collect()
    });
    list.sort_unstable_by_key(|s| s.0);
    list.into_iter().map(|s| s.1).collect()
}

/// Removes summons' remains a while after they fall.
pub fn clear_remains(world: &mut World) {
    let t = now(world);
    for e in summons(world, |_, v| v.dead && t - v.died_at > 8.0) {
        if let Some(id) = unit_id(world, e) {
            world.resource_mut::<Units>().forget(id);
        }
        world.despawn(e);
    }
}
