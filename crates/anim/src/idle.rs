//! Crowds out of step (#217): a group spawned together would otherwise breathe in lockstep, every
//! idle loop starting with its pose. Each unit's idle starts at its own phase, taken from its id,
//! and plays a little faster or slower, so no two neighbours match and none stay matched.
//!
//! Ids handed out together are consecutive, so the phases step by the golden ratio's fraction
//! (0.618 of a loop): neighbours always start at least 0.38 of a loop apart, and no run of ids
//! lines up.

/// The golden ratio's fraction, for the phase, and the square root of two's, for the pace: two
/// steps that never line up with each other or themselves.
const PHASE_STEP: f32 = 0.618_034;
const PACE_STEP: f32 = 0.414_213_57;
/// How much faster or slower than the clip an idle plays, at most: 5%.
pub const SPREAD: f32 = 0.05;

/// Where unit `id`'s idle loop starts, as a share of the loop (0 to 1).
pub fn phase(id: u32) -> f32 {
    step(id, PHASE_STEP)
}

/// How fast unit `id`'s idle loop plays, against the clip's own pace: 0.95 to 1.05.
pub fn pace(id: u32) -> f32 {
    1. + SPREAD * (2. * step(id, PACE_STEP) - 1.)
}

/// Seconds into a loop `duration` long that unit `id` has got to, `pose_time` seconds into its
/// idle.
pub fn time(id: u32, pose_time: f32, duration: f32) -> f32 {
    phase(id) * duration + pose_time * pace(id)
}

/// The fraction of `id` steps of `step`, worked in whole numbers so that big ids lose nothing.
fn step(id: u32, step: f32) -> f32 {
    const ONE: f64 = 4_294_967_296.;
    let fixed = (step as f64 * ONE) as u32;
    id.wrapping_mul(fixed) as f32 / ONE as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How far apart two phases are round the loop, as a share of it.
    fn apart(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(1.);
        d.min(1. - d)
    }

    #[test]
    fn units_spawned_together_idle_out_of_step() {
        // A batch spawned together has ids in a row: each starts well apart from the next, and
        // within any five, no two start within a seventh of a loop of each other.
        for first in [0u32, 1, 41, 1000, 123_456, u32::MAX - 10] {
            for id in first..first.saturating_add(9) {
                assert!(apart(phase(id), phase(id + 1)) > 0.38, "{id} and the next");
            }
            for a in first..first.saturating_add(5) {
                for b in a + 1..first.saturating_add(5) {
                    assert!(apart(phase(a), phase(b)) > 0.14, "{a} and {b}");
                }
            }
        }
        // Neighbours idling side by side don't come in step over the first four loops, though
        // their paces differ (as no two stay in step, the pair that start closest included).
        let duration = 2.5;
        for id in 0..1000 {
            for t in 0..600 {
                let t = t as f32 / 60.;
                let [a, b] = [id, id + 1].map(|id| (time(id, t, duration) / duration).fract());
                assert!(apart(a, b) > 0.1, "{id} and the next in step at {t} s");
            }
        }
    }

    #[test]
    fn the_pace_stays_within_five_percent_and_differs_between_neighbours() {
        for id in 0..10_000u32 {
            let p = pace(id);
            assert!((1. - SPREAD..=1. + SPREAD).contains(&p), "{id}: {p}");
            assert!((pace(id) - pace(id + 1)).abs() > 0.004, "{id} and the next");
            assert!((0. ..1.).contains(&phase(id)));
        }
        // The same unit always gets the same.
        assert_eq!((phase(77), pace(77)), (phase(77), pace(77)));
    }
}
