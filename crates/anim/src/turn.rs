//! Turning on the spot (#211): the yaw the body is drawn at lags the sim's, as Lyra's root yaw
//! offset does. Under action controls the sim turns a player to the camera at once, and a remote
//! unit's yaw moves in steps between snapshots; drawn at that yaw, a standing body would spin on
//! its feet.
//!
//! The upper body faces the sim's yaw, followed closely on a spring ([`Turn::aim`]) so that a
//! sudden turn (a roll, a target picked) swings the chest round instead of snapping it. Standing,
//! the body stays where it faces and the upper body takes up the turn (the character twists the
//! spine, then the neck and head). Past [`STEP_AT`], or held past [`HOLD_AT`] for [`HOLD`], the
//! feet step round ([`Steps`]) while the body catches up, no faster than [`STEP_RATE`]. Moving or
//! acting, the body follows on a critically damped spring, so its yaw and turning speed never
//! jump.
use crate::locomotion::Spring;
use glam::Vec3;
use std::f32::consts::{LN_2, PI, TAU};

/// How far round the upper body turns from the feet before they step after it, in radians (70°).
pub const STEP_AT: f32 = 1.22;
/// A turn held past this, in radians (20°), for [`HOLD`] seconds has the feet step round too.
pub const HOLD_AT: f32 = 0.35;
pub const HOLD: f32 = 0.75;
/// How closely the upper body's aim follows the sim's yaw: the spring's half-life in seconds,
/// and the fastest it turns, in radians a second (520°/s).
pub const AIM_HALF_LIFE: f32 = 0.04;
pub const AIM_RATE: f32 = 9.;
/// Half-lives of the body's yaw following the aim, in seconds: moving or acting, and stepping
/// round on the spot.
pub const FOLLOW_HALF_LIFE: f32 = 0.08;
pub const STEP_HALF_LIFE: f32 = 0.15;
/// The fastest the body turns while its feet step round, in radians a second (230°/s).
pub const STEP_RATE: f32 = 4.;
/// The furthest the body falls behind the aim, in radians (270°): spun round faster than the feet
/// can follow for long, it is dragged.
const MAX_LAG: f32 = 4.7;
/// A step round ends once the body is this close to the aim, in radians (0.3°), turning slower
/// than this, in radians a second, and then faces the aim: the feet stepped round to it stand where
/// the clip has them.
const SETTLED: f32 = 0.005;
const SETTLED_RATE: f32 = 0.05;
/// The furthest round from the body's yaw the feet step toward, in radians (60°), so that turning
/// a long way they go round a step at a time instead of crossing.
pub const STEP_REACH: f32 = 1.05;

/// What the body is doing, which decides how its yaw follows the sim's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stance {
    /// Standing still: the feet stay planted until the turn goes too far or is held too long.
    Standing,
    /// Moving, or busy with an action, which turns the whole body: the body follows on a spring.
    Moving,
    /// Too far away to see the difference, or just appeared: the body faces the sim's yaw.
    Snap,
}

/// One body's yaw as drawn, and where its upper body faces, following the sim's yaw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turn {
    /// The body's yaw and how fast it turns, in radians and radians a second; `None` until the
    /// first update.
    yaw: Option<Spring>,
    /// Where the upper body faces, and how fast it turns.
    aim: Spring,
    /// The sim's yaw, counted on round without wrapping, so the aim and the body keep turning the
    /// way the sim turned however far behind it they fall.
    goal: f32,
    /// Seconds the turn has been held past [`HOLD_AT`], standing.
    held: f32,
    stepping: bool,
}

impl Turn {
    /// Moves the aim and the body's yaw on by `dt` toward the sim's yaw `target` (radians round
    /// +y, as the sim's), and returns the body's.
    pub fn update(&mut self, target: f32, stance: Stance, dt: f32) -> f32 {
        let spring = match &mut self.yaw {
            Some(spring) if stance != Stance::Snap => spring,
            _ => {
                let still = Spring {
                    value: wrap(target),
                    velocity: 0.,
                };
                *self = Self {
                    yaw: Some(still),
                    aim: still,
                    goal: still.value,
                    ..Self::default()
                };
                return still.value;
            }
        };
        self.goal += wrap(target - self.goal);
        let was = self.aim.value;
        self.aim.update(self.goal, AIM_HALF_LIFE, dt);
        limit(&mut self.aim, was, AIM_RATE, dt);
        let aim = self.aim.value;
        if stance == Stance::Moving {
            self.stepping = false;
        }
        if stance == Stance::Moving || self.stepping {
            let was = spring.value;
            if self.stepping {
                spring.update(aim, STEP_HALF_LIFE, dt);
                // The feet only step round so fast.
                limit(spring, was, STEP_RATE, dt);
                if (spring.value - aim).abs() < SETTLED && spring.velocity.abs() < SETTLED_RATE {
                    // The last fraction of a degree at once, so that the body faces just where
                    // the feet stepped to and the upper body has no twist left to hold.
                    *spring = Spring {
                        value: aim,
                        velocity: 0.,
                    };
                    self.stepping = false;
                }
            } else {
                spring.update(aim, FOLLOW_HALF_LIFE, dt);
            }
            self.held = 0.;
        } else {
            // Standing: any turn the body had dies away, and the feet stay where they are.
            spring.velocity *= (-2. * LN_2 / FOLLOW_HALF_LIFE * dt).exp();
            spring.value += spring.velocity * dt;
            let off = (spring.value - aim).abs();
            self.held = if off > HOLD_AT { self.held + dt } else { 0. };
            self.stepping = off >= STEP_AT || self.held >= HOLD;
        }
        spring.value = aim + (spring.value - aim).clamp(-MAX_LAG, MAX_LAG);
        // Whole turns off all three, to keep the numbers small.
        let turns = (spring.value / TAU).round() * TAU;
        spring.value -= turns;
        self.aim.value -= turns;
        self.goal -= turns;
        spring.value
    }

    /// The body's yaw, once updated.
    pub fn yaw(&self) -> Option<f32> {
        self.yaw.map(|s| s.value)
    }

    /// Where the upper body faces: the sim's yaw, followed closely.
    pub fn aim(&self) -> f32 {
        wrap(self.aim.value)
    }

    /// How fast the body turns, in radians a second.
    pub fn rate(&self) -> f32 {
        self.yaw.map_or(0., |s| s.velocity)
    }

    /// How far round the aim is from the body, the short way, in radians.
    pub fn offset(&self) -> f32 {
        self.yaw.map_or(0., |s| wrap(self.aim.value - s.value))
    }

    /// Whether the feet are stepping round after the turn.
    pub fn stepping(&self) -> bool {
        self.stepping
    }

    /// The yaw the feet step toward while stepping round: the aim, or [`STEP_REACH`] round from
    /// the body toward it if that is nearer.
    pub fn step_yaw(&self) -> f32 {
        self.yaw.map_or(0., |s| {
            wrap(s.value + (self.aim.value - s.value).clamp(-STEP_REACH, STEP_REACH))
        })
    }
}

/// Keeps a spring that was at `was` from turning faster than `rate` this frame.
fn limit(spring: &mut Spring, was: f32, rate: f32, dt: f32) {
    spring.value = was + (spring.value - was).clamp(-rate * dt, rate * dt);
    spring.velocity = spring.velocity.clamp(-rate, rate);
}

/// A foot set down in the world: where it stands, and the yaw of the body it stands under as the
/// clip has it (the foot turns with that).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plant {
    pub at: Vec3,
    pub yaw: f32,
}

/// Seconds a foot takes to step, and how high it lifts, in metres: a short step less, by
/// [`LIFT_SHARE`] of how far the foot goes, counting its toe ([`TOE`] metres in front of the
/// ankle) sweeping round as it turns.
pub const SWING: f32 = 0.25;
pub const LIFT: f32 = 0.06;
pub const LIFT_SHARE: f32 = 0.4;
const TOE: f32 = 0.15;
/// How far a foot may stand from where it should, in metres and in radians (10°), before it
/// steps.
pub const STRAY: f32 = 0.03;
pub const STRAY_YAW: f32 = 0.17;
/// Once the body stops turning, a foot further than this share of [`STRAY`] and [`STRAY_YAW`]
/// from where it should stand takes one last short step.
pub const SETTLE: f32 = 0.35;
/// Then the feet ease the rest of the way onto where they should stand, no faster than this, in
/// metres and radians a second, so that the clip takes them back without a jump.
pub const EASE: f32 = 0.05;
pub const EASE_YAW: f32 = 0.3;

/// The feet stepping round while the body turns on the spot: one at a time, each from where it
/// stands to where it should, the foot on the side of the turn first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steps {
    /// Left and right, where each last stood.
    planted: [Plant; 2],
    /// The foot in the air, and how far through its step (0 to 1).
    swing: Option<(usize, f32)>,
}

impl Steps {
    /// Starts with the feet (left, right) planted where they stand.
    pub fn new(planted: [Plant; 2]) -> Self {
        Self {
            planted,
            swing: None,
        }
    }

    /// Moves the feet on by `dt` toward `rest`, where each should stand, and returns where each
    /// is now: a foot in the air partway between where it stood and where it is going, lifted
    /// off the ground. Once the body has stopped turning (`settle`), the feet step closer in and
    /// then ease onto `rest`.
    pub fn update(&mut self, rest: [Plant; 2], settle: bool, dt: f32) -> [Plant; 2] {
        if self.swing.is_none() {
            // How far out each foot is, as a share of how far it may be.
            let stray = [0, 1].map(|i| {
                let [from, to] = [self.planted[i], rest[i]];
                (from.at.distance(to.at) / STRAY).max(wrap(to.yaw - from.yaw).abs() / STRAY_YAW)
            });
            // Turning left (toward +x) the left foot opens the turn; turning right, the right.
            let lead = if wrap(rest[0].yaw - self.planted[0].yaw) >= 0. {
                0
            } else {
                1
            };
            let foot = if stray[lead] * 1.25 >= stray[1 - lead] {
                lead
            } else {
                1 - lead
            };
            if stray[foot] > if settle { SETTLE } else { 1. } {
                self.swing = Some((foot, 0.));
            } else if settle {
                for (plant, to) in self.planted.iter_mut().zip(rest) {
                    let gap = to.at - plant.at;
                    plant.at += gap.clamp_length_max(EASE * dt);
                    let [turn, most] = [wrap(to.yaw - plant.yaw), EASE_YAW * dt];
                    plant.yaw = wrap(plant.yaw + turn.clamp(-most, most));
                }
            }
        }
        let mut feet = self.planted;
        if let Some((foot, along)) = &mut self.swing {
            *along = (*along + dt / SWING).min(1.);
            let eased = *along * *along * (3. - 2. * *along);
            let [from, to] = [self.planted[*foot], rest[*foot]];
            let travel = from.at.distance(to.at) + TOE * wrap(to.yaw - from.yaw).abs();
            let lift = LIFT.min(travel * LIFT_SHARE);
            feet[*foot] = Plant {
                at: from.at.lerp(to.at, eased) + Vec3::Y * lift * (PI * *along).sin(),
                yaw: wrap(from.yaw + wrap(to.yaw - from.yaw) * eased),
            };
            if *along >= 1. {
                self.planted[*foot] = to;
                self.swing = None;
            }
        }
        feet
    }

    /// Whether both feet stand on the ground where `rest` says, near enough (2 mm and half a
    /// degree) for the clip to take them back unseen.
    pub fn settled(&self, rest: [Plant; 2]) -> bool {
        self.swing.is_none()
            && (0..2).all(|i| {
                self.planted[i].at.distance(rest[i].at) <= 0.002
                    && wrap(rest[i].yaw - self.planted[i].yaw).abs() <= 0.01
            })
    }

    /// The foot in the air, if one is.
    pub fn swinging(&self) -> Option<usize> {
        self.swing.map(|(foot, _)| foot)
    }
}

fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1. / 60.;

    fn degrees(radians: f32) -> f32 {
        radians.to_degrees()
    }

    /// Runs `turn` for `seconds` standing, with the sim's yaw at `yaw(t)`; returns each frame's
    /// body yaw, aim, and whether the feet stepped round then.
    fn stand(turn: &mut Turn, seconds: f32, yaw: impl Fn(f32) -> f32) -> Vec<(f32, f32, bool)> {
        (0..(seconds / DT).round() as usize)
            .map(|k| {
                let body = turn.update(yaw(k as f32 * DT), Stance::Standing, DT);
                (body, turn.aim(), turn.stepping())
            })
            .collect()
    }

    fn standing_at(yaw: f32) -> Turn {
        let mut turn = Turn::default();
        turn.update(yaw, Stance::Snap, DT);
        turn
    }

    #[test]
    fn standing_the_body_stays_put_while_the_turn_builds_up() {
        let mut turn = standing_at(0.3);
        // The camera swings 50° left over half a second.
        let frames = stand(&mut turn, 0.7, |t| 0.3 + 0.87 * (t / 0.5).min(1.));
        assert!(frames.iter().all(|f| (f.0 - 0.3).abs() < 1e-6 && !f.2));
        assert!(
            (turn.offset() - 0.87).abs() < 2e-3,
            "{}°",
            degrees(turn.offset())
        );
        // A small turn stays with the upper body however long it's held.
        let mut turn = standing_at(0.);
        let frames = stand(&mut turn, 3., |_| 0.3);
        assert!(frames.iter().all(|f| f.0.abs() < 1e-6 && !f.2));
        assert!((turn.offset() - 0.3).abs() < 1e-5);
    }

    #[test]
    fn past_the_limit_the_feet_step_round_until_the_body_catches_up() {
        let mut turn = standing_at(0.);
        // The camera swings 100° left over half a second, then holds.
        let frames = stand(&mut turn, 2., |t| 1.75 * (t / 0.5).min(1.));
        let first = frames.iter().position(|f| f.2).expect("never stepped");
        // The feet step as the upper body's turn reaches 70°, not before.
        let off = |f: &(f32, f32, bool)| wrap(f.1 - f.0);
        assert!(off(&frames[first]) >= STEP_AT && off(&frames[first - 1]) < STEP_AT);
        assert!(frames[..first].iter().all(|f| f.0.abs() < 1e-6));
        // Then the body turns no faster than the feet can go, and settles facing the aim.
        for pair in frames.windows(2) {
            assert!((pair[1].0 - pair[0].0).abs() <= STEP_RATE * DT + 1e-5);
        }
        let last = frames.iter().rposition(|f| f.2).unwrap();
        assert!(last < frames.len() - 1, "still stepping");
        assert!((turn.yaw().unwrap() - 1.75).abs() < SETTLED);
    }

    #[test]
    fn a_turn_held_a_moment_steps_round_too() {
        let mut turn = standing_at(0.);
        let frames = stand(&mut turn, 2., |_| -0.6);
        let first = frames.iter().position(|f| f.2).expect("never stepped");
        // Past 20° within a frame or two (the aim swings round), and held.
        let past = frames.iter().position(|f| f.1 < -HOLD_AT).unwrap();
        let held = (first - past) as f32 * DT;
        assert!((held - HOLD).abs() < 2. * DT, "stepped after {held}s");
        assert!((turn.yaw().unwrap() + 0.6).abs() < SETTLED);
        assert!(!turn.stepping());
    }

    #[test]
    fn a_quick_half_turn_swings_the_chest_and_steps_round_one_way() {
        for target in [PI - 0.01, -PI + 0.01] {
            let mut turn = standing_at(0.);
            let frames = stand(&mut turn, 2.5, |_| target);
            let mut last = (0., 0.);
            for &(body, aim, _) in &frames {
                // Never back the other way; the body no faster than the feet, and the chest
                // swinging round instead of snapping.
                let [turned, swung] = [wrap(body - last.0), wrap(aim - last.1)];
                assert!(turned * target.signum() >= -1e-5 && turned.abs() <= STEP_RATE * DT + 1e-5);
                assert!(swung * target.signum() >= -1e-5 && swung.abs() <= AIM_RATE * DT + 1e-5);
                last = (body, aim);
            }
            assert!(wrap(turn.yaw().unwrap() - target).abs() < SETTLED);
            assert!(!turn.stepping());
        }
    }

    #[test]
    fn moving_the_body_follows_snapshots_twenty_times_a_second_without_jumps() {
        let mut turn = Turn::default();
        turn.update(0., Stance::Snap, DT);
        // The sim's yaw as a client sees it in snapshots 50 ms apart, held for three frames each:
        // a turn speeding up to 180°/s, held, then a 90° flick (a roll's facing snaps like that).
        let sim = |t: f32| {
            let t = (t / 0.05).floor() * 0.05;
            if t < 1. {
                PI / 2. * t * t
            } else if t < 1.5 {
                PI / 2.
            } else {
                PI
            }
        };
        let (mut last_yaw, mut last_rate) = (0., 0.);
        let mut worst = [(0f32, 0f32); 2];
        for k in 0..(2.5 / DT) as usize {
            let t = k as f32 * DT;
            let yaw = turn.update(sim(t), Stance::Moving, DT);
            let rate = wrap(yaw - last_yaw) / DT;
            let w = &mut worst[(t >= 1.5) as usize];
            *w = (w.0.max(rate.abs() * DT), w.1.max((rate - last_rate).abs()));
            (last_yaw, last_rate) = (yaw, rate);
        }
        // Turning, the snapshots step 0.16 rad at the end: the body turns smoothly through them.
        assert!(worst[0].0 < 0.06, "turned {} rad in a frame", worst[0].0);
        assert!(
            worst[0].1 < 0.6,
            "turning speed jumped {} rad/s",
            worst[0].1
        );
        // The flick's 1.57 rad comes in over frames.
        assert!(worst[1].0 < 0.2, "turned {} rad in a frame", worst[1].0);
        assert!(wrap(last_yaw - PI).abs() < 0.01);
    }

    #[test]
    fn snapping_faces_the_sim_at_once() {
        let mut turn = standing_at(0.);
        stand(&mut turn, 0.2, |_| 1.);
        assert!((turn.update(2.5, Stance::Snap, DT) - 2.5).abs() < 1e-6);
        assert!(turn.offset().abs() < 1e-6);
        assert!(!turn.stepping());
    }

    /// Where a body facing `yaw` at the origin stands its feet, 9 cm either side.
    fn feet(yaw: f32) -> [Plant; 2] {
        let side = Vec3::new(yaw.cos(), 0., -yaw.sin());
        [1., -1.].map(|s| Plant {
            at: side * 0.09 * s,
            yaw,
        })
    }

    #[test]
    fn the_feet_step_one_at_a_time_the_lead_foot_first_and_end_where_they_should() {
        for (to, lead) in [(1.3, 0), (-1.3, 1), (PI * 0.9, 0)] {
            let mut turn = standing_at(0.);
            let mut steps = Steps::new(feet(0.));
            // The feet start stepping once the aim has swung past the limit.
            while !turn.stepping() {
                turn.update(to, Stance::Standing, DT);
                assert_eq!(turn.yaw(), Some(0.));
            }
            let (mut order, mut last, mut last_swing) = (Vec::new(), feet(0.), None);
            for _ in 0..300 {
                turn.update(to, Stance::Standing, DT);
                let settle = !turn.stepping();
                let now = steps.update(feet(turn.step_yaw()), settle, DT);
                let swing = steps.swinging();
                for i in 0..2 {
                    // A foot on the ground stays where it is, or once the turn is over eases
                    // onto where it should stand.
                    if swing != Some(i) && last_swing != Some(i) {
                        let most = if settle { EASE * DT } else { 0. };
                        let moved = now[i].at.distance(last[i].at);
                        assert!(moved < most + 1e-6, "turning to {to}: moved {moved}");
                    }
                }
                if let Some(i) = swing.filter(|&i| last_swing != Some(i)) {
                    order.push(i);
                }
                (last, last_swing) = (now, swing);
                if !turn.stepping() && steps.settled(feet(turn.step_yaw())) {
                    break;
                }
            }
            assert_eq!(order.first(), Some(&lead), "turning to {to}: {order:?}");
            assert!(order.windows(2).all(|w| w[0] != w[1]), "{order:?}");
            assert!(steps.settled(feet(to)), "turning to {to}");
            assert!(!turn.stepping());
        }
    }
}
