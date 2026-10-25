//! Specialty XP and levels (#66, docs/PROGRESSION.md).
//!
//! Each specialty levels on its own. A monster that dies pays XP to every player who used skills
//! on it, split between the specialties they used in proportion to how often; nothing comes from
//! monsters that don't die. Quests give a pool the player assigns to any specialty. The equipped
//! specialty's level is the one that fights.

use bevy_ecs::prelude::*;
use protocol::{Event, UnitId};

use crate::combat::{emit, tell};
use crate::content::progression::{kill_xp, to_next, MAX_LEVEL};
use crate::content::{catalog, Specialty};
use crate::{stats, Brain, Fighter, PlayerControl, Summoned, Unit, Units, Vitals};

/// A player's level and XP in every specialty, and quest XP not yet given to one.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Progress {
    pub levels: [u8; 6],
    /// XP into the current level, toward the next.
    pub xp: [u32; 6],
    pub unassigned: u32,
}

impl Progress {
    pub fn new(specialty: Specialty, level: u8) -> Progress {
        let mut levels = [1; 6];
        levels[specialty.index()] = level.clamp(1, MAX_LEVEL);
        Progress { levels, xp: [0; 6], unassigned: 0 }
    }

    pub fn level(&self, s: Specialty) -> u8 {
        self.levels[s.index()]
    }

    /// Adds XP to one specialty. Returns the levels it rose through (none at the cap, where XP
    /// stops counting).
    pub fn add(&mut self, s: Specialty, amount: u32) -> Vec<u8> {
        let i = s.index();
        let mut ups = Vec::new();
        let mut left = amount;
        while let Some(need) = to_next(self.levels[i]) {
            let room = need - self.xp[i];
            if left < room {
                self.xp[i] += left;
                return ups;
            }
            left -= room;
            self.xp[i] = 0;
            self.levels[i] += 1;
            ups.push(self.levels[i]);
        }
        ups
    }
}

/// Who has used skills on a monster, with which specialty, and how often, until it dies.
#[derive(Component, Clone, Debug, Default)]
pub struct Credit(pub Vec<(UnitId, Specialty, u32)>);

/// Notes that `user` used a skill on `target`. Summons count for their owner.
pub(crate) fn note_use(world: &mut World, user: Entity, target: Entity) {
    let player = match world.get::<Summoned>(user) {
        Some(s) => world.resource::<Units>().entity(s.owner),
        None => Some(user),
    };
    let Some(p) = player.filter(|&p| world.get::<PlayerControl>(p).is_some()) else { return };
    let (Some(id), Some(spec)) = (world.get::<Unit>(p).map(|u| u.id), world.get::<Fighter>(p).and_then(|f| f.specialty)) else { return };
    let Some(mut credit) = world.get_mut::<Credit>(target) else { return };
    match credit.0.iter_mut().find(|(u, s, _)| *u == id && *s == spec) {
        Some(c) => c.2 += 1,
        None => credit.0.push((id, spec, 1)),
    }
}

/// Pays out a monster that just died: each player who used skills on it gets its XP, split
/// between their specialties by use. Summoned monsters pay nothing.
pub(crate) fn pay_out(world: &mut World, dead: Entity) {
    let Some(credit) = world.get_mut::<Credit>(dead).map(|mut c| std::mem::take(&mut c.0)) else { return };
    if credit.is_empty() || world.get::<Summoned>(dead).is_some() {
        return;
    }
    let Some(rank) = world.get::<Brain>(dead).and_then(|b| catalog().mob(b.mob)).map(|m| m.rank) else { return };
    let mob_level = world.get::<Unit>(dead).map_or(1, |u| u.level);
    // In the order players first touched it, so the payout (and its events) is deterministic.
    let mut players: Vec<UnitId> = Vec::new();
    for (id, _, _) in &credit {
        if !players.contains(id) {
            players.push(*id);
        }
    }
    for id in players {
        let Some(p) = world.resource::<Units>().entity(id) else { continue };
        let uses: Vec<(Specialty, u32)> = credit.iter().filter(|c| c.0 == id).map(|c| (c.1, c.2)).collect();
        let total: u32 = uses.iter().map(|u| u.1).sum();
        for (spec, n) in uses {
            let level = world.get::<Progress>(p).map_or(1, |pr| pr.level(spec));
            let amount = (kill_xp(mob_level, rank, level) as u64 * n as u64 / total as u64) as u32;
            give(world, p, spec, amount);
        }
    }
}

/// Adds XP to a specialty, levelling it up (and the player's numbers, if it's equipped).
pub(crate) fn give(world: &mut World, e: Entity, spec: Specialty, amount: u32) {
    if amount == 0 {
        return;
    }
    let Some(ups) = world.get_mut::<Progress>(e).map(|mut p| p.add(spec, amount)) else { return };
    let Some(&level) = ups.last() else { return };
    let unit = world.get::<Unit>(e).map_or(0, |u| u.id);
    for &l in &ups {
        emit(world, Event::LevelUp { unit, specialty: spec.key().to_string(), level: l });
    }
    tell(world, e, &format!("{} reached level {level}.", catalog().specialty(spec).name));
    if world.get::<Fighter>(e).and_then(|f| f.specialty) == Some(spec) {
        set_level(world, e, spec, level);
    }
}

/// Fights at `level` of `spec`: health, power and armour from the level, keeping the share of
/// health the unit had.
pub(crate) fn set_level(world: &mut World, e: Entity, spec: Specialty, level: u8) {
    let (s, health) = stats::for_specialty(catalog().specialty(spec), level);
    if let Some(mut st) = world.get_mut::<crate::Stats>(e) {
        *st = s;
    }
    if let Some(mut v) = world.get_mut::<Vitals>(e) {
        let share = if v.max_health > 0.0 { v.health / v.max_health } else { 1.0 };
        v.max_health = health;
        if !v.dead {
            v.health = (health * share).max(1.0).min(health);
        }
    }
    if let Some(mut u) = world.get_mut::<Unit>(e) {
        u.level = level;
    }
}

/// Gives `amount` of a player's quest XP to `spec`. Returns false if they don't have that much.
pub(crate) fn assign(world: &mut World, e: Entity, spec: Specialty, amount: u32) -> bool {
    let Some(mut p) = world.get_mut::<Progress>(e) else { return false };
    if amount == 0 || p.unassigned < amount {
        return false;
    }
    p.unassigned -= amount;
    give(world, e, spec, amount);
    true
}
