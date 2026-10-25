//! How specialties level: the XP each level takes, and what a kill pays (#66, docs/PROGRESSION.md).
//!
//! Plain tables to tune. Levels 1 to 50; the climb gets 12% longer each level while a kill of
//! your own level pays 9% more, so a level takes about 6 even kills at level 1 and about 23 at 49.

use super::Rank;

/// Specialties stop here. Combat grows 6% a level (`stats::GROWTH`), so 50 is about 17 times a
/// level-1 fighter.
pub const MAX_LEVEL: u8 = 50;

/// XP from each level to the next: `XP_TO_NEXT[0]` takes level 1 to 2.
pub const XP_TO_NEXT: [u32; MAX_LEVEL as usize - 1] = [
    250, 280, 310, 350, 390, 440, 490, 550, 620, 690, 780, 870, 970, 1100, 1200, 1400, 1500, 1700, 1900, 2200, 2400, 2700, 3000, 3400, 3800, 4300,
    4800, 5300, 6000, 6700, 7500, 8400, 9400, 11000, 12000, 13000, 15000, 17000, 19000, 21000, 23000, 26000, 29000, 33000, 37000, 41000, 46000,
    51000, 58000,
];

/// What a normal monster of each level pays when it dies: `KILL_XP[0]` is level 1.
pub const KILL_XP: [u32; MAX_LEVEL as usize] = [
    40, 44, 48, 52, 56, 62, 67, 73, 80, 87, 95, 100, 110, 120, 130, 150, 160, 170, 190, 210, 220, 240, 270, 290, 320, 340, 380, 410, 450, 490, 530,
    580, 630, 690, 750, 820, 890, 970, 1100, 1200, 1300, 1400, 1500, 1600, 1800, 1900, 2100, 2300, 2500, 2700,
];

/// Elites and bosses pay this many times a normal monster of their level.
pub fn rank_multiplier(rank: Rank) -> u32 {
    match rank {
        Rank::Normal => 1,
        Rank::Elite => 4,
        Rank::Boss => 15,
    }
}

/// Monsters this many levels below you pay less, down to nothing at `GREY` below.
pub const FULL_BELOW: u8 = 5;
pub const GREY: u8 = 10;

/// XP to go from `level` to the next, or `None` at the cap.
pub fn to_next(level: u8) -> Option<u32> {
    XP_TO_NEXT.get(level.max(1) as usize - 1).copied()
}

/// What a monster of `mob_level` and `rank` pays a specialty at `level`, before it's split.
pub fn kill_xp(mob_level: u8, rank: Rank, level: u8) -> u32 {
    let base = KILL_XP[mob_level.clamp(1, MAX_LEVEL) as usize - 1] * rank_multiplier(rank);
    let below = level.saturating_sub(mob_level);
    if below <= FULL_BELOW {
        base
    } else if below >= GREY {
        0
    } else {
        base * (GREY - below) as u32 / (GREY - FULL_BELOW) as u32
    }
}
