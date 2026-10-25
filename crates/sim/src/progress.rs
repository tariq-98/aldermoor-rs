//! Specialty XP and levels (#66, docs/PROGRESSION.md).
//!
//! Each specialty levels on its own. A monster that dies pays its full XP to every player who did
//! at least a tenth of the fight (damage to it, healing on those it fought, damage taken from it;
//! #201), split between the specialties they fought with by how much each did; nothing comes from
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

/// What each player has done in a fight with a monster, by specialty, until it dies: damage
/// dealt to it, healing on those it's fighting, and damage taken from it.
#[derive(Component, Clone, Debug, Default)]
pub struct Credit(pub Vec<(UnitId, Specialty, f32)>);

/// The share of a fight a player needs for a kill's XP and their own loot.
pub const SHARE_FLOOR: f32 = 0.1;

impl Credit {
    pub fn add(&mut self, id: UnitId, spec: Specialty, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        match self.0.iter_mut().find(|(u, s, _)| *u == id && *s == spec) {
            Some(c) => c.2 += amount,
            None => self.0.push((id, spec, amount)),
        }
    }

    /// What one player did, all specialties together.
    pub fn of(&self, id: UnitId) -> f32 {
        self.0.iter().filter(|c| c.0 == id).map(|c| c.2).sum()
    }

    /// The players who did at least [`SHARE_FLOOR`] of the fight, in the order they joined it.
    pub fn earners(&self) -> Vec<UnitId> {
        let total: f32 = self.0.iter().map(|c| c.2).sum();
        let mut out: Vec<UnitId> = Vec::new();
        for &(id, _, _) in &self.0 {
            if !out.contains(&id) && total > 0.0 && self.of(id) >= total * SHARE_FLOOR {
                out.push(id);
            }
        }
        out
    }
}

/// The player `e` earns for, and with which specialty: itself, or a summon's owner.
pub(crate) fn earner(world: &World, e: Entity) -> Option<(UnitId, Specialty)> {
    let p = match world.get::<Summoned>(e) {
        Some(s) => world.resource::<Units>().entity(s.owner)?,
        None => e,
    };
    world.get::<PlayerControl>(p)?;
    Some((world.get::<Unit>(p)?.id, world.get::<Fighter>(p)?.specialty?))
}

/// Counts `amount` toward `who`'s share of `monster`, if one is a player (or a player's summon)
/// and the other a monster that pays out.
pub(crate) fn credit(world: &mut World, who: Entity, monster: Entity, amount: f32) {
    if amount <= 0.0 || world.get::<Credit>(monster).is_none() {
        return;
    }
    let Some((id, spec)) = earner(world, who) else { return };
    if let Some(mut c) = world.get_mut::<Credit>(monster) {
        c.add(id, spec, amount);
    }
}

/// Pays out a monster that just died: everyone who did a fair share of the fight gets its full
/// XP, split between the specialties they fought it with by how much each did. Summoned
/// monsters pay nothing.
pub(crate) fn pay_out(world: &mut World, dead: Entity) {
    let Some(credit) = world.get_mut::<Credit>(dead).map(|mut c| std::mem::take(&mut *c)) else { return };
    if credit.0.is_empty() || world.get::<Summoned>(dead).is_some() {
        return;
    }
    let Some(rank) = world.get::<Brain>(dead).and_then(|b| catalog().mob(b.mob)).map(|m| m.rank) else { return };
    let mob_level = world.get::<Unit>(dead).map_or(1, |u| u.level);
    for id in credit.earners() {
        let Some(p) = world.resource::<Units>().entity(id) else { continue };
        let total = credit.of(id);
        for &(_, spec, n) in credit.0.iter().filter(|c| c.0 == id) {
            let level = world.get::<Progress>(p).map_or(1, |pr| pr.level(spec));
            let amount = (kill_xp(mob_level, rank, level) as f64 * (n / total) as f64).round() as u32;
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
