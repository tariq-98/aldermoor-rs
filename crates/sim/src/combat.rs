use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::{Command, Event, Pose, UnitId, UnitKind};

use crate::brain::engage;
use crate::movement::{self, heading, ATTACK_POSE, HIT_POSE};
use crate::spatial::Spatial;
use crate::vitals::MONSTER_RESPAWN;
use crate::{Anim, Body, Brain, Clock, Fighter, Home, Inbox, Land, Mode, Motion, MoveIntent, Outbox, Rng, Transform, Unit, Units, Vitals};

/// Slot 1: a heavy strike on your target.
const STRIKE_RANGE: f32 = 2.6;
const STRIKE_POWER: f32 = 1.7;
const STRIKE_COOLDOWN: f64 = 4.0;
/// Slot 2: a sweeping cut through everything in front of you.
const SWEEP_RANGE: f32 = 3.2;
const SWEEP_HALF_ANGLE: f32 = 1.05;
const SWEEP_POWER: f32 = 1.0;
const SWEEP_COOLDOWN: f64 = 6.0;
const CRIT_CHANCE: f32 = 0.1;
const CRIT_POWER: f32 = 1.5;
/// Monsters this close to one that's attacked join in.
const PACK_RADIUS: f32 = 10.0;

/// Applies queued commands in the order they arrived.
pub fn apply_commands(world: &mut World) {
    let commands = std::mem::take(&mut world.resource_mut::<Inbox>().0);
    let now = world.resource::<Clock>().time;
    for (id, c) in commands {
        let Some(e) = world.resource::<Units>().entity(id) else { continue };
        match c {
            Command::Move { direction, walk, face_yaw } => {
                if let Some(mut m) = world.get_mut::<Motion>(e) {
                    m.intent = MoveIntent { direction, walk, face_yaw };
                }
            }
            Command::Jump => {
                if let Some(mut m) = world.get_mut::<Motion>(e) {
                    m.jump_queued = true;
                }
            }
            Command::Dodge { direction } => {
                let rolled = world.resource_scope(|world, land: Mut<Land>| {
                    let mut q = world.query::<(&mut Transform, &mut Motion, &mut Vitals, &Body)>();
                    match q.get_mut(world, e) {
                        Ok((mut t, mut m, mut v, b)) => movement::start_dodge(&land.0, now, &mut t, &mut m, &mut v, b, direction),
                        Err(_) => false,
                    }
                });
                if rolled {
                    world.resource_mut::<Outbox>().0.push(Event::Dodged { unit: id });
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
            Command::UseSlot { slot, .. } => use_slot(world, e, id, slot, now),
            Command::Respawn => respawn_player(world, e, id),
        }
    }
}

/// Swings basic attacks at targets in reach.
pub fn fight(world: &mut World) {
    let now = world.resource::<Clock>().time;
    let mut swings: Vec<(Entity, UnitId, UnitId, f32, f32)> = Vec::new();
    {
        let mut q = world.query::<(Entity, &Unit, &Transform, &Body, &Fighter, &Vitals)>();
        let units = world.resource::<Units>();
        for (e, u, t, b, f, v) in q.iter(world) {
            if v.dead || !f.auto_attack || now < f.next_swing {
                continue;
            }
            let Some(tid) = f.target else { continue };
            let Some(te) = units.entity(tid) else { continue };
            let (Some(tt), Some(tb), Some(tv)) = (world.get::<Transform>(te), world.get::<Body>(te), world.get::<Vitals>(te)) else {
                continue;
            };
            if tv.dead {
                continue;
            }
            let to = Vec2::new(tt.pos.x - t.pos.x, tt.pos.z - t.pos.z);
            if to.length() - b.radius - tb.radius > f.attack.range {
                continue;
            }
            swings.push((e, u.id, tid, f.attack.damage, heading(to)));
        }
    }
    for (e, src, tid, damage, facing) in swings {
        let interval = world.get::<Fighter>(e).map_or(2.0, |f| f.attack.interval);
        if let Some(mut f) = world.get_mut::<Fighter>(e) {
            f.next_swing = now + interval;
        }
        if let Some(mut t) = world.get_mut::<Transform>(e) {
            t.yaw = facing;
        }
        hold_pose(world, e, Pose::Attack, now, ATTACK_POSE);
        hit(world, src, tid, damage);
    }
}

/// Rolls a hit's size and crit, then lands it.
fn hit(world: &mut World, src: UnitId, target: UnitId, power: f32) {
    let (amount, crit) = {
        let mut rng = world.resource_mut::<Rng>();
        let crit = rng.chance(CRIT_CHANCE);
        let amount = power * rng.range(0.9, 1.1) * if crit { CRIT_POWER } else { 1.0 };
        (amount.round().max(1.0), crit)
    };
    deal_damage(world, src, target, amount, crit);
}

fn use_slot(world: &mut World, e: Entity, id: UnitId, slot: u8, now: f64) {
    let Some(f) = world.get::<Fighter>(e) else { return };
    let ready = f.cooldowns.get(slot as usize).is_some_and(|&c| now >= c);
    let power = f.attack.damage;
    let target = f.target;
    let dead = world.get::<Vitals>(e).is_none_or(|v| v.dead);
    if !ready || dead {
        return;
    }
    let (Some(t), Some(b), Some(u)) = (world.get::<Transform>(e).copied(), world.get::<Body>(e).copied(), world.get::<Unit>(e)) else {
        return;
    };
    let faction = u.faction;
    let here = Vec2::new(t.pos.x, t.pos.z);
    match slot {
        0 => {
            let Some(tid) = target else { return message(world, "You need a target.") };
            let Some(te) = world.resource::<Units>().entity(tid) else { return };
            let (Some(tt), Some(tb), Some(tv), Some(tu)) =
                (world.get::<Transform>(te), world.get::<Body>(te), world.get::<Vitals>(te), world.get::<Unit>(te))
            else {
                return;
            };
            if tv.dead || !tu.faction.is_enemy(faction) {
                return;
            }
            let to = Vec2::new(tt.pos.x, tt.pos.z) - here;
            if to.length() - b.radius - tb.radius > STRIKE_RANGE {
                return message(world, "Out of range.");
            }
            start_cooldown(world, e, 0, now + STRIKE_COOLDOWN);
            if let Some(mut tr) = world.get_mut::<Transform>(e) {
                tr.yaw = heading(to);
            }
            hold_pose(world, e, Pose::Attack, now, ATTACK_POSE);
            hit(world, id, tid, power * STRIKE_POWER);
        }
        1 => {
            start_cooldown(world, e, 1, now + SWEEP_COOLDOWN);
            hold_pose(world, e, Pose::Attack, now, ATTACK_POSE);
            let forward = t.forward();
            let victims: Vec<UnitId> = {
                let spatial = world.resource::<Spatial>();
                let units = world.resource::<Units>();
                spatial
                    .near(here, SWEEP_RANGE + 1.0)
                    .into_iter()
                    .filter(|&(vid, p)| {
                        let Some(ve) = units.entity(vid) else { return false };
                        let (Some(vu), Some(vb)) = (world.get::<Unit>(ve), world.get::<Body>(ve)) else { return false };
                        let to = p - here;
                        vu.faction.is_enemy(faction)
                            && to.length() - vb.radius <= SWEEP_RANGE
                            && to.length() > 1e-3
                            && forward.angle_to(to).abs() <= SWEEP_HALF_ANGLE
                    })
                    .map(|(vid, _)| vid)
                    .collect()
            };
            for vid in victims {
                hit(world, id, vid, power * SWEEP_POWER);
            }
        }
        _ => {}
    }
}

fn start_cooldown(world: &mut World, e: Entity, slot: usize, until: f64) {
    if let Some(mut f) = world.get_mut::<Fighter>(e) {
        f.cooldowns[slot] = until;
    }
}

fn hold_pose(world: &mut World, e: Entity, pose: Pose, now: f64, seconds: f64) {
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        a.pose = pose;
        a.since = now;
        a.hold_until = now + seconds;
    }
}

fn message(world: &mut World, text: &str) {
    world.resource_mut::<Outbox>().0.push(Event::Message { text: text.to_string() });
}

/// Lands a blow: evaded during a dodge, otherwise taken off health. Monsters (and their pack)
/// turn on whoever hit them.
pub fn deal_damage(world: &mut World, src: UnitId, target: UnitId, amount: f32, crit: bool) {
    let now = world.resource::<Clock>().time;
    let (Some(te), Some(se)) = (world.resource::<Units>().entity(target), world.resource::<Units>().entity(src)) else {
        return;
    };
    let (evaded, died) = {
        let Some(mut v) = world.get_mut::<Vitals>(te) else { return };
        if v.dead {
            return;
        }
        if now < v.invulnerable_until {
            (true, false)
        } else {
            v.health -= amount;
            v.last_combat = now;
            if v.health <= 0.0 {
                v.health = 0.0;
                v.dead = true;
                v.died_at = now;
                (false, true)
            } else {
                (false, false)
            }
        }
    };
    if let Some(mut sv) = world.get_mut::<Vitals>(se) {
        sv.last_combat = now;
    }
    world.resource_mut::<Outbox>().0.push(Event::Damage { target, source: src, amount: if evaded { 0.0 } else { amount }, crit });
    if evaded {
        return;
    }
    let attacking = world.get::<Anim>(te).is_some_and(|a| a.pose == Pose::Attack && now < a.hold_until);
    if !attacking && !died {
        hold_pose(world, te, Pose::Hit, now, HIT_POSE);
    }
    if died {
        on_death(world, te, target, Some(src), now);
        return;
    }
    rally(world, te, src);
}

/// The target and any idle pack-mates nearby go after `attacker`.
fn rally(world: &mut World, target: Entity, attacker: UnitId) {
    let (Some(t), Some(u)) = (world.get::<Transform>(target).copied(), world.get::<Unit>(target).map(|u| u.body)) else { return };
    let here = Vec2::new(t.pos.x, t.pos.z);
    let mut pack: Vec<Entity> = vec![target];
    {
        let spatial = world.resource::<Spatial>();
        let units = world.resource::<Units>();
        for (id, _) in spatial.near(here, PACK_RADIUS) {
            if let Some(e) = units.entity(id) {
                if e != target && world.get::<Unit>(e).is_some_and(|o| o.body == u) {
                    pack.push(e);
                }
            }
        }
    }
    for e in pack {
        let idle = world.get::<Brain>(e).is_some_and(|b| b.mode == Mode::Idle);
        let alive = world.get::<Vitals>(e).is_some_and(|v| !v.dead);
        if !idle || !alive {
            continue;
        }
        let now = world.resource::<Clock>().time;
        let mut q = world.query::<(&mut Brain, &mut Fighter)>();
        if let Ok((mut b, mut f)) = q.get_mut(world, e) {
            engage(&mut b, &mut f, attacker, now);
        }
    }
}

fn on_death(world: &mut World, e: Entity, id: UnitId, killer: Option<UnitId>, now: f64) {
    world.resource_mut::<Outbox>().0.push(Event::Died { unit: id, killer });
    let kind = world.get::<Unit>(e).map(|u| u.kind);
    if kind == Some(UnitKind::Monster) {
        if let Some(mut v) = world.get_mut::<Vitals>(e) {
            v.respawn_at = Some(now + MONSTER_RESPAWN);
        }
    }
    if let Some(mut a) = world.get_mut::<Anim>(e) {
        a.pose = Pose::Dead;
        a.since = now;
    }
    // Whoever was fighting it stops swinging at a corpse.
    let mut q = world.query::<(&mut Fighter, &Unit)>();
    for (mut f, u) in q.iter_mut(world) {
        if f.target == Some(id) {
            f.target = None;
            if u.kind != UnitKind::Player {
                f.auto_attack = false;
            }
        }
    }
}

fn respawn_player(world: &mut World, e: Entity, id: UnitId) {
    let Some(v) = world.get::<Vitals>(e) else { return };
    if !v.dead {
        return;
    }
    let max = v.max_health;
    let Some(home) = world.get::<Home>(e).copied() else { return };
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        *v = Vitals::new(max);
    }
    if let Some(mut t) = world.get_mut::<Transform>(e) {
        t.pos = home.pos;
        t.yaw = home.yaw;
    }
    if let Some(mut m) = world.get_mut::<Motion>(e) {
        *m = Motion::new(m.run_speed);
    }
    world.resource_mut::<Outbox>().0.push(Event::Respawned { unit: id });
}
