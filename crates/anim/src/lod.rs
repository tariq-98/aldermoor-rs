//! Animation level of detail (#215): how often each character's pose is rebuilt, by how much it
//! matters to the player, so that a crowd fits a CPU budget. It works the way Unreal's animation
//! budget allocator and update rate optimisation do: the most important characters rebuild every
//! frame, the next every other frame with the frames between blended, and the rest every fourth
//! frame, held. Rebuilds are spread over the frames so the cost stays flat, and while the frame's
//! animation time is over budget, fewer characters get the higher rates.

/// How often a character's pose is rebuilt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// Every frame, with cape springs and the feet stepping round when turning on the spot.
    #[default]
    Full,
    /// Every other frame, shown between rebuilds by blending the last two poses.
    Half,
    /// Every fourth frame, held between rebuilds.
    Quarter,
}
impl Tier {
    /// Frames from one rebuild to the next.
    pub fn period(self) -> u64 {
        match self {
            Tier::Full => 1,
            Tier::Half => 2,
            Tier::Quarter => 4,
        }
    }
    /// Whether a character rebuilds on `frame`. `slot` spreads characters over the frames of a
    /// period, so that about as many rebuild on each: a unit id does.
    pub fn due(self, frame: u64, slot: u64) -> bool {
        frame.wrapping_add(slot).is_multiple_of(self.period())
    }
}

/// How many characters rebuild every frame, and how many every other frame, when the budget
/// allows: the art plan's crowd tiers. The rest rebuild every fourth frame.
pub const FULL: usize = 8;
pub const HALF: usize = 16;
/// Animation CPU time a frame may take, in milliseconds: a third of the crowd's 3 ms
/// (docs/PERFORMANCE.md).
pub const BUDGET_MS: f32 = 1.;

/// What decides how much a character matters to the player.
#[derive(Clone, Copy, Debug)]
pub struct Importance {
    /// The player, whoever they target and whoever targets them, which come before everyone else.
    pub involved: bool,
    /// Its distance from the camera, squared.
    pub distance: f32,
    /// Its tier last frame, which it keeps against a character a little nearer, so that two at
    /// about the same distance don't trade tiers back and forth.
    pub tier: Tier,
}

/// Ranks characters by importance and gives each its tier, keeping the frame's animation time
/// inside `BUDGET_MS` by shrinking the higher tiers while it's over.
#[derive(Clone, Debug)]
pub struct Crowd {
    /// The frame's animation time, smoothed, in milliseconds.
    spent: f32,
    /// The share of `FULL` and `HALF` the budget allows, from an eighth to all of them.
    share: f32,
    order: Vec<(bool, f32, usize)>,
    tiers: Vec<Tier>,
}
impl Default for Crowd {
    fn default() -> Self {
        Self {
            spent: 0.,
            share: 1.,
            order: Vec::new(),
            tiers: Vec::new(),
        }
    }
}
impl Crowd {
    /// Ranks this frame's characters, the involved first and then the nearest, and returns each
    /// one's tier, in the order given.
    pub fn assign(&mut self, characters: impl IntoIterator<Item = Importance>) -> &[Tier] {
        self.order.clear();
        self.order
            .extend(characters.into_iter().enumerate().map(|(i, c)| {
                // A character holds its tier against one up to a fifth nearer (full) or a tenth
                // (half).
                let hold = match c.tier {
                    Tier::Full => 0.64,
                    Tier::Half => 0.81,
                    Tier::Quarter => 1.,
                };
                (c.involved, c.distance * hold, i)
            }));
        self.order
            .sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
        self.tiers.clear();
        self.tiers.resize(self.order.len(), Tier::Quarter);
        for (rank, &(_, _, i)) in self.order.iter().enumerate() {
            self.tiers[i] = self.tier(rank);
        }
        &self.tiers
    }
    /// The tier for the character ranked `rank`, 0 the most important.
    pub fn tier(&self, rank: usize) -> Tier {
        let full = ((FULL as f32 * self.share).round() as usize).max(1);
        let half = (HALF as f32 * self.share).round() as usize;
        if rank < full {
            Tier::Full
        } else if rank < full + half {
            Tier::Half
        } else {
            Tier::Quarter
        }
    }
    /// Takes in how long animating took this frame, in milliseconds. While the smoothed time is
    /// over the budget the higher tiers shrink each frame, and under three quarters of it they
    /// grow back, more slowly.
    pub fn spent(&mut self, ms: f32) {
        self.spent += (ms - self.spent) * 0.1;
        if self.spent > BUDGET_MS {
            self.share = (self.share - 0.05).max(0.125);
        } else if self.spent < BUDGET_MS * 0.75 {
            self.share = (self.share + 0.01).min(1.);
        }
    }
    /// The frame's animation time, smoothed, in milliseconds.
    pub fn smoothed(&self) -> f32 {
        self.spent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(distance: f32) -> Importance {
        Importance {
            involved: false,
            distance: distance * distance,
            tier: Tier::Quarter,
        }
    }
    #[test]
    fn rebuilds_spread_evenly_over_the_frames() {
        for tier in [Tier::Full, Tier::Half, Tier::Quarter] {
            for frame in 0..8 {
                let due = (0..100).filter(|&slot| tier.due(frame, slot)).count();
                let even = 100 / tier.period() as usize;
                assert_eq!(due, even, "{tier:?} on frame {frame}");
            }
            // Each character rebuilds once a period.
            let rebuilds = (0..8).filter(|&frame| tier.due(frame, 7)).count();
            assert_eq!(rebuilds, 8 / tier.period() as usize);
        }
    }
    #[test]
    fn the_involved_come_first_then_the_nearest() {
        let mut crowd = Crowd::default();
        // 40 characters a metre apart, the furthest of them fighting the player.
        let mut characters: Vec<_> = (0..40).map(|i| at(i as f32 + 1.)).collect();
        characters[39].involved = true;
        let tiers = crowd.assign(characters).to_vec();
        assert_eq!(tiers[39], Tier::Full);
        assert_eq!(tiers.iter().filter(|t| **t == Tier::Full).count(), FULL);
        assert_eq!(tiers.iter().filter(|t| **t == Tier::Half).count(), HALF);
        assert!(tiers[..FULL - 1].iter().all(|t| *t == Tier::Full));
        assert_eq!(tiers[FULL - 1], Tier::Half);
        let quarter = |t: &Tier| *t == Tier::Quarter;
        assert!(tiers[FULL + HALF - 1..39].iter().all(quarter));
    }
    #[test]
    fn a_character_keeps_its_tier_against_one_a_little_nearer() {
        let mut crowd = Crowd::default();
        let mut characters: Vec<_> = (0..FULL + 1).map(|i| at(i as f32 + 1.)).collect();
        let last = characters.len() - 1;
        characters[last - 1].tier = Tier::Full;
        // The last full one is 5% further now than the first half one: it stays full.
        characters[last - 1].distance = ((last + 1) as f32 * 1.05).powi(2);
        let tiers = crowd.assign(characters.clone()).to_vec();
        assert_eq!((tiers[last - 1], tiers[last]), (Tier::Full, Tier::Half));
        // 30% further, and they trade.
        characters[last - 1].distance = ((last + 1) as f32 * 1.3).powi(2);
        let tiers = crowd.assign(characters).to_vec();
        assert_eq!((tiers[last - 1], tiers[last]), (Tier::Half, Tier::Full));
    }
    #[test]
    fn over_budget_fewer_get_the_higher_rates_and_they_come_back_under_it() {
        let mut crowd = Crowd::default();
        let count = |crowd: &mut Crowd, tier| {
            let tiers = crowd.assign((0..64).map(|i| at(i as f32)));
            tiers.iter().filter(|t| **t == tier).count()
        };
        for _ in 0..120 {
            crowd.spent(3.);
        }
        assert_eq!(count(&mut crowd, Tier::Full), 1);
        assert_eq!(count(&mut crowd, Tier::Half), 2);
        for _ in 0..600 {
            crowd.spent(0.3);
        }
        assert_eq!(count(&mut crowd, Tier::Full), FULL);
        assert_eq!(count(&mut crowd, Tier::Half), HALF);
        // Between three quarters of the budget and all of it, nothing changes.
        crowd.spent = 0.9;
        crowd.share = 0.5;
        crowd.spent(0.9);
        assert_eq!(crowd.share, 0.5);
    }
}
