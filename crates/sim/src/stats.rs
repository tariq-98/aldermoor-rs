//! The numbers that set the pace of a fight.
//!
//! Health and power grow by the same share every level, so the edge a few levels give is the
//! same at level 5 as at level 25: about 1.8 times the staying power five levels up (each side's
//! health and damage both grow 34%), and over three times ten levels up. Skill (dodging, timed
//! blocks, interrupts, aim, position) has room to beat the first and not the second
//! (docs/DESIGN.md, "Progression and player skill").

use crate::content::{MobDef, SpecialtyDef};
use crate::Stats;

/// Health and power grow 6% a level.
pub const GROWTH: f32 = 1.06;
/// A level-1 fighter's health, power and armour (before specialty or monster multipliers).
pub const BASE_HEALTH: f32 = 200.0;
pub const BASE_POWER: f32 = 30.0;
pub const BASE_ARMOR: f32 = 100.0;
pub const CRIT_CHANCE: f32 = 0.05;
pub const CRIT_MULTIPLIER: f32 = 1.5;
/// The global cooldown most skills share.
pub const GCD: f64 = 1.0;
/// Seconds a downed unit lasts before bleeding out (less if hurt).
pub const DOWNED_TIME: f64 = 18.0;
/// The window in which repeated crowd control diminishes.
pub const DR_WINDOW: f64 = 18.0;
/// Crowd control on an elite or boss chips this much defiance per second it would have held
/// them (a knockback counts as 1.5 seconds), on top of a skill's own defiance damage.
pub const DEFIANCE_PER_SECOND: f32 = 60.0;
/// Seconds a broken defiance bar takes to come back.
pub const DEFIANCE_BACK: f64 = 9.0;

/// How much stronger a level is than level 1.
pub fn scale(level: u8) -> f32 {
    GROWTH.powi(level.max(1) as i32 - 1)
}

/// The share of a physical blow armour stops, against an attacker of `attacker_level`.
pub fn armor_reduction(armor: f32, attacker_level: u8) -> f32 {
    let a = armor.max(0.0);
    (a / (a + 2.0 * BASE_ARMOR * scale(attacker_level))).min(0.75)
}

/// Full, half, quarter, then immune until the window passes.
pub fn diminish(count: u8) -> f64 {
    match count {
        0 => 1.0,
        1 => 0.5,
        2 => 0.25,
        _ => 0.0,
    }
}

pub fn for_specialty(def: &SpecialtyDef, level: u8) -> (Stats, f32) {
    let k = scale(level);
    (
        Stats { power: BASE_POWER * k * def.power, armor: BASE_ARMOR * k * def.armor, crit: CRIT_CHANCE, haste: 0.0 },
        BASE_HEALTH * k * def.health,
    )
}

pub fn for_mob(def: &MobDef, level: u8) -> (Stats, f32) {
    let k = scale(level);
    (
        Stats { power: BASE_POWER * k * def.power, armor: BASE_ARMOR * k * def.armor, crit: CRIT_CHANCE, haste: 0.0 },
        BASE_HEALTH * k * def.health,
    )
}
