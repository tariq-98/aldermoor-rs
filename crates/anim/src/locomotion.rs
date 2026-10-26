//! Locomotion that follows the body (#110): each loop's gait measured from its feet, one stride
//! phase shared by every loop, a blend by speed that keeps the clips' own rhythm, and the hips
//! turned toward wherever the body travels.
//!
//! The loops play in place, so a planted foot slides back at the speed the clip would carry the
//! body. Each loop's natural speed is measured from its planted feet at load, and the stride moves
//! on at the body's speed over the blended stride length, which keeps planted feet where they are.
//! Facing and travel are separate under action controls (strafing, backpedalling): the hips and
//! legs turn toward the travel, forward loops play backwards to go backwards, and the character
//! turns the spine back toward the facing (Paragon, Fortnite and Lyra's orientation warping).
use glam::{Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, LN_2, PI, TAU};
use std::ops::{Add, Mul, Sub};

/// How far a strafe must swing past square before the legs change between going forward and going
/// backward, in radians (10°), so a strafe straight to the side doesn't flicker between the two.
pub const HYSTERESIS: f32 = 0.17;
/// Half-lives of the speed and hip turn the legs show, in seconds. The sim changes velocity at
/// once; the legs catch up this fast.
pub const SPEED_HALF_LIFE: f32 = 0.1;
pub const TURN_HALF_LIFE: f32 = 0.08;
/// The slowest and fastest a blend plays against its own pace. Past them stride warping (#214)
/// shortens or lengthens the steps instead: see [`Step::warp`].
pub const PACE: [f32; 2] = [0.8, 1.25];
/// From this share of a loop's own speed to this one it plays alone, a little slower or faster,
/// so a second clip is only sampled between two loops' speeds.
pub const SOLO: [f32; 2] = [0.9, 1.1];
/// A foot is planted while it slides back at more than this share of its usual speed sliding
/// back: the speed it beats a quarter of the time it slides back at all.
const PLANTED: f32 = 0.5;

/// One locomotion loop's gait, measured from its feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gait {
    /// Seconds a cycle (two steps) lasts.
    pub duration: f32,
    /// When the left and the right foot touch down, in seconds from the clip's start.
    pub contacts: [f32; 2],
    /// When they lift off again.
    pub lifts: [f32; 2],
    /// How high the ball of each foot is while it is planted, in the model's frame: the median.
    /// Running on their toes, the clips hold it higher than walking flat footed.
    pub floors: [f32; 2],
    /// Metres a cycle carries the body at the clip's own pace: how far back a planted foot slides
    /// over a whole cycle.
    pub stride: f32,
}

impl Gait {
    /// Measures a loop from where the balls of its feet (left, right) are at evenly spaced times
    /// over its `duration`, the first at the start, in the model's frame (+y up, +z ahead). A foot
    /// is planted for the longest stretch it slides back at more than [`PLANTED`] of its usual
    /// speed, touching down where that starts and lifting off where it ends. Heights are no guide
    /// to that: a sprinting foot bounces on its toes, and on a short body the foot is lowest as it
    /// pushes off. The loop's speed is the median speed planted feet slide back at. `None` if a
    /// foot never lifts, never slides back, or is higher planted than swinging (the loop goes
    /// backward).
    pub fn measure(feet: &[[Vec3; 2]], duration: f32) -> Option<Gait> {
        let n = feet.len();
        let step = duration / n as f32;
        let quantile = |mut v: Vec<f32>, q: f32| {
            v.sort_by(f32::total_cmp);
            let at = (v.len().max(1) - 1) as f32 * q;
            v.get(at as usize).copied()
        };
        let [mut contacts, mut lifts, mut floors] = [[0.; 2]; 3];
        let mut slides = Vec::with_capacity(n);
        for foot in 0..2 {
            // How fast the foot slides back from each sample to the next.
            let back: Vec<f32> = (0..n)
                .map(|k| (feet[k][foot].z - feet[(k + 1) % n][foot].z) / step)
                .collect();
            let usual = quantile(back.iter().copied().filter(|v| *v > 0.).collect(), 0.75)?;
            let planted = |k: usize| back[k % n] > usual * PLANTED;
            let (start, length) = (0..n)
                .filter(|&k| planted(k) && !planted(k + n - 1))
                .map(|k| (k, (0..n).take_while(|j| planted(k + j)).count()))
                .max_by_key(|&(_, length)| length)?;
            // A planted foot is lower, on the whole, than a swinging one.
            let height = |k: usize| feet[(start + k) % n][foot].y;
            let down = (0..length).map(height).sum::<f32>() / length as f32;
            let up = (length..n).map(height).sum::<f32>() / (n - length) as f32;
            if down >= up {
                return None;
            }
            contacts[foot] = start as f32 * step;
            lifts[foot] = ((start + length) % n) as f32 * step;
            floors[foot] = quantile((0..length).map(height).collect(), 0.5)?;
            slides.extend((0..length).map(|j| back[(start + j) % n]));
        }
        let speed = quantile(slides, 0.5)?;
        (speed > 0.).then_some(Gait {
            duration,
            contacts,
            lifts,
            floors,
            stride: speed * duration,
        })
    }

    /// Metres a second the loop carries the body at its own pace.
    pub fn speed(&self) -> f32 {
        self.stride / self.duration
    }

    /// The clip's time at a stride `phase`: 0 as the left foot touches down and 0.5 as the right
    /// does, evenly between, so every loop's feet land together whatever its timing.
    pub fn time_at(&self, phase: f32) -> f32 {
        let [left, right] = self.contacts;
        let half = (right - left).rem_euclid(self.duration);
        let phase = phase.rem_euclid(1.);
        let since = if phase < 0.5 {
            phase * 2. * half
        } else {
            half + (phase - 0.5) * 2. * (self.duration - half)
        };
        (left + since).rem_euclid(self.duration)
    }

    /// The stride phase at clip time `time`: the inverse of [`Gait::time_at`].
    pub fn phase_at(&self, time: f32) -> f32 {
        let [left, right] = self.contacts;
        let half = (right - left).rem_euclid(self.duration);
        let since = (time - left).rem_euclid(self.duration);
        if since < half {
            since / (2. * half)
        } else {
            0.5 + (since - half) / (2. * (self.duration - half))
        }
    }

    /// How much of the cycle, in phase, each foot (left, right) stays planted from touching down.
    pub fn stances(&self) -> [f32; 2] {
        [0, 1].map(|foot| (self.phase_at(self.lifts[foot]) - foot as f32 * 0.5).rem_euclid(1.))
    }

    /// Seconds of clip a whole cycle of phase would take at the pace of the step `phase` is in:
    /// a clip's steps needn't take half its cycle each, so to keep its planted feet sliding at one
    /// speed, the phase runs through each step at that step's own pace.
    pub fn span(&self, phase: f32) -> f32 {
        let half = (self.contacts[1] - self.contacts[0]).rem_euclid(self.duration);
        2. * if phase.rem_euclid(1.) < 0.5 {
            half
        } else {
            self.duration - half
        }
    }
}

/// Which two loops of `gaits` (slowest first) to blend at `speed`, and the second's weight. Near
/// a loop's own speed (within [`SOLO`] of it) that loop plays alone; so do the slowest below its
/// range and the fastest above. Between two loops' ranges the weight is the one at which the
/// blend's pace carries the body at `speed`, stride and cycle both blending with it, each clip
/// timed as at the edge of its range. So the clips keep close to the rhythm they were made with,
/// and the weight reaches 0 and 1 just where the ranges start.
pub fn blend(gaits: &[Gait], speed: f32) -> (usize, usize, f32) {
    for (i, pair) in gaits.windows(2).enumerate() {
        let [a, b] = [pair[0], pair[1]];
        if speed <= a.speed() * SOLO[1] {
            return (i, i, 0.);
        }
        if speed < b.speed() * SOLO[0] {
            // stride(w) = speed * cycle(w), both lerped from a to b.
            let [da, db] = [a.duration / SOLO[1], b.duration / SOLO[0]];
            let w = (speed * da - a.stride) / (b.stride - a.stride - speed * (db - da));
            return (i, i + 1, w.clamp(0., 1.));
        }
    }
    let last = gaits.len().saturating_sub(1);
    (last, last, 0.)
}

/// A value (a number, or a vector) that follows its target on a critically damped spring, in the
/// blender's half-life convention (see [`crate::Switch::half_life`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring<T = f32> {
    pub value: T,
    pub velocity: T,
}

impl<T> Spring<T>
where
    T: Copy + Default + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    pub fn update(&mut self, target: T, half_life: f32, dt: f32) -> T {
        let rate = 2. * LN_2 / half_life;
        let decay = (-rate * dt).exp();
        let offset = self.value - target;
        let j = self.velocity + offset * rate;
        self.value = target + (offset + j * dt) * decay;
        self.velocity = (self.velocity - j * (rate * dt)) * decay;
        self.value
    }

    pub fn snap(&mut self, value: T) {
        *self = Self {
            value,
            velocity: T::default(),
        };
    }
}

/// How far to turn the hips from the facing for travel `travel` radians round from it (toward
/// +x), and whether the legs go backward. The legs go forward until the travel is more than
/// [`HYSTERESIS`] past square to the facing, and once going backward, keep going backward until
/// it is that far inside square again (`backward` says which way they went last).
pub fn hip_turn(travel: f32, backward: bool) -> (f32, bool) {
    let edge = FRAC_PI_2 + if backward { -HYSTERESIS } else { HYSTERESIS };
    let backward = wrap(travel).abs() > edge;
    let turn = if backward { travel - PI } else { travel };
    (wrap(turn), backward)
}

fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// What a character shows of its locomotion loops on one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    /// The two loops to blend, as indices into the gaits, each with its clip time.
    pub from: (usize, f32),
    pub to: (usize, f32),
    /// How much of `to` the blend takes.
    pub weight: f32,
    /// How far round the vertical to turn the hips from the facing, in radians toward +x.
    pub turn: f32,
    /// Whether each foot (left, right) is planted, by where the stride is in the blend's stances,
    /// and how high the blend holds the ball of each foot while it is ([`Gait::floors`]).
    pub planted: [bool; 2],
    pub floors: [f32; 2],
    /// How much longer than the clips' the steps must be to carry the body at its speed: 1 while
    /// the blend's pace keeps up, less below its slowest pace and more above its fastest.
    pub warp: f32,
}

/// One character's place in its locomotion: the stride phase, and the speed and hip turn its legs
/// show, which follow the body's on springs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stride {
    /// Where the cycle is: 0 as the left foot touches down, 0.5 as the right does.
    pub phase: f32,
    speed: Spring,
    turn: Spring,
    /// Whether the legs go backward, while moving.
    backward: Option<bool>,
}

impl Stride {
    /// Moves the stride on by `dt` for a body travelling at `velocity` metres a second, in the
    /// facing's frame (`x` along the model's +x, `y` along +z, ahead), over `gaits` (slowest first,
    /// at least one). Straight after [`Stride::stop`], the legs take the body's speed and
    /// direction at once, since whatever came before was another clip that the blender settles.
    pub fn advance(&mut self, gaits: &[Gait], velocity: Vec2, dt: f32) -> Step {
        let travel = velocity.x.atan2(velocity.y);
        let (turn, backward) = hip_turn(travel, self.backward == Some(true));
        if self.backward.is_some() {
            self.speed.update(velocity.length(), SPEED_HALF_LIFE, dt);
            self.turn.update(turn, TURN_HALF_LIFE, dt);
        } else {
            self.speed.snap(velocity.length());
            self.turn.snap(turn);
        }
        self.backward = Some(backward);
        let speed = self.speed.value.max(0.);
        let (a, b, w) = blend(gaits, speed);
        let [ga, gb] = [gaits[a], gaits[b]];
        // Metres and clip seconds a cycle takes at the pace of the step the feet are in.
        let span = [ga, gb].map(|g| g.span(self.phase));
        let reach = ga.speed() * span[0] + (gb.speed() * span[1] - ga.speed() * span[0]) * w;
        let span = span[0] + (span[1] - span[0]) * w;
        let cycles = (speed / reach).clamp(PACE[0] / span, PACE[1] / span);
        // The legs carry the body along their own line: ahead when the hips face the travel,
        // backward when they face away, and slower while they swing round from one to the other.
        let along = (travel - self.turn.value).cos();
        self.phase = (self.phase + cycles * along * dt).rem_euclid(1.);
        let [sa, sb] = [ga.stances(), gb.stances()];
        let planted = [0, 1].map(|foot| {
            let stance = sa[foot] + (sb[foot] - sa[foot]) * w;
            (self.phase - foot as f32 * 0.5).rem_euclid(1.) < stance
        });
        Step {
            from: (a, ga.time_at(self.phase)),
            to: (b, gb.time_at(self.phase)),
            weight: w,
            turn: self.turn.value,
            planted,
            floors: [0, 1].map(|foot| ga.floors[foot] + (gb.floors[foot] - ga.floors[foot]) * w),
            warp: speed / (reach * cycles),
        }
    }

    /// The body stopped walking (it stands, acts in place, jumps, rolls or fell): the next
    /// [`Stride::advance`] starts from its motion as it is. The phase carries on from where it was.
    pub fn stop(&mut self) {
        self.backward = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loop whose feet plant for `stance` of the cycle, sliding back at `speed`, the left foot
    /// touching down at `left` seconds and the right half a cycle later.
    fn feet(duration: f32, left: f32, stance: f32, speed: f32, samples: usize) -> Vec<[Vec3; 2]> {
        let foot = |t: f32| {
            let since = (t - left).rem_euclid(duration) / duration;
            let reach = speed * duration * stance / 2.;
            if since < stance {
                Vec3::new(0., 0.03, reach - speed * since * duration)
            } else {
                // Swinging forward, lifted clear of the ground.
                let swing = (since - stance) / (1. - stance);
                Vec3::new(
                    0.,
                    0.08 + 0.15 * (swing * PI).sin(),
                    -reach + 2. * reach * swing,
                )
            }
        };
        (0..samples)
            .map(|k| {
                let t = duration * k as f32 / samples as f32;
                [foot(t), foot(t - duration / 2.)]
            })
            .collect()
    }

    /// Gaits like the male body's walk, jog and sprint, each step half a cycle.
    fn loops() -> [Gait; 3] {
        [
            (1.333, 0.95, 0.11, 0.02),
            (0.933, 6.1, 0.04, 0.04),
            (0.667, 9.4, 0.03, 0.03),
        ]
        .map(|(duration, speed, left, floor)| Gait {
            duration,
            contacts: [left, left + duration / 2.],
            lifts: [left + duration * 0.4, left + duration * 0.9],
            floors: [floor; 2],
            stride: speed * duration,
        })
    }

    #[test]
    fn measuring_finds_touch_downs_and_the_speed_a_planted_foot_slides_back() {
        for (duration, left, stance, speed) in [(1.333, 0.1, 0.6, 1.2), (0.7, 0.05, 0.3, 6.)] {
            let gait = Gait::measure(&feet(duration, left, stance, speed, 96), duration).unwrap();
            let sample = duration / 96.;
            assert!((gait.contacts[0] - left).abs() <= sample, "{gait:?}");
            assert!(
                (gait.contacts[1] - left - duration / 2.).abs() <= sample,
                "{gait:?}"
            );
            assert!((gait.speed() - speed).abs() < 0.01 * speed, "{gait:?}");
            // Each foot lifts off once it has been planted for its stance, the ball as high
            // through it as on touching down.
            for (foot, down) in [left, left + duration / 2.].into_iter().enumerate() {
                let lift = (down + stance * duration).rem_euclid(duration);
                let off = (gait.lifts[foot] - lift + duration / 2.).rem_euclid(duration);
                assert!((off - duration / 2.).abs() <= 2. * sample, "{gait:?}");
                assert!((gait.floors[foot] - 0.03).abs() < 1e-4, "{gait:?}");
            }
        }
        // Feet that never leave the ground, or slide forward, are not a forward loop.
        let still = vec![[Vec3::ZERO; 2]; 32];
        assert_eq!(Gait::measure(&still, 1.), None);
        let back: Vec<_> = feet(1., 0.1, 0.5, 2., 64)
            .into_iter()
            .map(|f| f.map(|p| p * Vec3::new(1., 1., -1.)))
            .collect();
        assert_eq!(Gait::measure(&back, 1.), None);
    }

    #[test]
    fn every_loop_lands_its_feet_on_the_same_phases() {
        for gait in loops() {
            assert!((gait.time_at(0.) - gait.contacts[0]).abs() < 1e-5);
            assert!((gait.time_at(0.5) - gait.contacts[1]).abs() < 1e-5);
            assert!((gait.time_at(1.) - gait.contacts[0]).abs() < 1e-5);
            for k in 0..20 {
                let phase = k as f32 / 20.;
                assert!((gait.phase_at(gait.time_at(phase)) - phase).abs() < 1e-4);
            }
            // Time runs on steadily through the cycle and round the end of the clip.
            let mut last = gait.time_at(0.);
            for k in 1..=200 {
                let t = gait.time_at(k as f32 / 200.);
                let step = (t - last).rem_euclid(gait.duration);
                assert!(step > 0. && step < 0.02 * gait.duration, "{gait:?} at {k}");
                last = t;
            }
        }
    }

    #[test]
    fn each_step_plays_at_its_own_pace() {
        // The left foot's step takes 0.4 s of clip and the right's 0.6 s: at the loop's own speed,
        // the clip still plays at one second a second through both.
        let gait = Gait {
            duration: 1.,
            contacts: [0.1, 0.5],
            lifts: [0.35, 0.85],
            floors: [0.03; 2],
            stride: 2.,
        };
        let mut stride = Stride::default();
        let dt = 1. / 60.;
        let mut step = stride.advance(&[gait], Vec2::new(0., 2.), dt);
        for _ in 0..120 {
            let (phase, time) = (stride.phase, step.from.1);
            step = stride.advance(&[gait], Vec2::new(0., 2.), dt);
            if (phase < 0.5) == (stride.phase < 0.5) && stride.phase > phase {
                let played = (step.from.1 - time).rem_euclid(1.);
                assert!((played - dt).abs() < 1e-4, "{played} s at phase {phase}");
            }
        }
    }

    #[test]
    fn each_foot_is_planted_through_its_stance_and_steps_warp_past_the_pace() {
        let gaits = loops();
        let dt = 1. / 60.;
        // Within the loops' paces: planted from touching down (phase 0 and 0.5) through the
        // stance, 40% of the cycle, and the steps as long as the clips'.
        let mut stride = Stride::default();
        for _ in 0..120 {
            let step = stride.advance(&gaits, Vec2::new(0., 6.), dt);
            assert!((step.warp - 1.).abs() < 1e-4, "{step:?}");
            assert_eq!(step.floors, gaits[1].floors);
            for (foot, down) in [0., 0.5].into_iter().enumerate() {
                let since = (stride.phase - down).rem_euclid(1.);
                if (since - 0.4).abs() > 0.02 {
                    assert_eq!(step.planted[foot], since < 0.4, "{foot} at {since}");
                }
            }
        }
        // Between two loops, the balls of the feet are held between the two loops' heights.
        let step = Stride::default().advance(&gaits, Vec2::new(0., 3.), dt);
        let floor = step.floors[0];
        assert!(
            floor > 0.021 && floor < 0.039 && step.floors[1] == floor,
            "{step:?}"
        );
        // Slower than the walk can play, and faster than the sprint, the steps shorten and
        // lengthen to carry the body.
        for (speed, gait) in [(0.4, gaits[0]), (14., gaits[2])] {
            let mut stride = Stride::default();
            let mut step = stride.advance(&gaits, Vec2::new(0., speed), dt);
            for _ in 0..60 {
                step = stride.advance(&gaits, Vec2::new(0., speed), dt);
            }
            let pace = (speed / gait.speed()).clamp(PACE[0], PACE[1]);
            assert!(
                (step.warp - speed / (gait.speed() * pace)).abs() < 1e-3,
                "{step:?}"
            );
        }
    }

    #[test]
    fn a_loop_plays_alone_near_its_own_speed_and_blends_between() {
        let gaits = loops();
        // How much of the pose each loop makes at `speed`.
        let shares = |speed: f32| {
            let (a, b, w) = blend(&gaits, speed);
            let mut shares = [0.; 3];
            shares[a] += 1. - w;
            shares[b] += w;
            shares
        };
        assert_eq!(shares(0.5), [1., 0., 0.]);
        assert_eq!(shares(12.), [0., 0., 1.]);
        for (i, gait) in gaits.iter().enumerate() {
            for pace in [SOLO[0], 1., SOLO[1]] {
                let speed = gait.speed() * pace;
                assert!(
                    (shares(speed)[i] - 1.).abs() < 1e-4,
                    "loop {i} at {speed} m/s"
                );
            }
        }
        let mut last = shares(0.5);
        for k in 0..=400 {
            let speed = 0.5 + 10. * k as f32 / 400.;
            let (a, b, w) = blend(&gaits, speed);
            if a != b {
                // The pair's stride over its cycle, each timed as at the edge of its range.
                let [ga, gb] = [gaits[a], gaits[b]];
                let stride = ga.stride + (gb.stride - ga.stride) * w;
                let [da, db] = [ga.duration / SOLO[1], gb.duration / SOLO[0]];
                let cycle = da + (db - da) * w;
                assert!(
                    (stride / cycle - speed).abs() < 1e-4,
                    "{speed} m/s: {a} {b} {w}"
                );
            }
            // The blend moves steadily from the walk to the sprint as speed goes up, without
            // jumps where a loop's range starts or ends.
            let now = shares(speed);
            let mean = |s: [f32; 3]| s[1] + 2. * s[2];
            assert!(mean(now) >= mean(last) - 1e-6, "{speed} m/s");
            assert!(
                mean(now) - mean(last) < 0.03,
                "{speed} m/s: {last:?} to {now:?}"
            );
            last = now;
        }
    }

    #[test]
    fn the_hips_turn_toward_the_travel_and_go_backward_past_square() {
        let deg = |d: f32| d.to_radians();
        // Going forward: forward until 10° past square, backward beyond, turned toward the travel.
        for (travel, turn, backward) in [
            (0., 0., false),
            (45., 45., false),
            (-45., -45., false),
            (90., 90., false),
            (-95., -95., false),
            (105., -75., true),
            (135., -45., true),
            (-135., 45., true),
            (180., 0., true),
        ] {
            let (t, b) = hip_turn(deg(travel), false);
            assert_eq!(b, backward, "{travel}°");
            assert!(
                (t - deg(turn)).abs() < 1e-4,
                "{travel}°: {}",
                t.to_degrees()
            );
        }
        // Swinging round, the legs hold their way until the travel is 10° past square.
        let sweep = |from: f32, to: f32, mut was: bool| {
            let mut flipped = None;
            for k in 0..=200 {
                let travel = from + (to - from) * k as f32 / 200.;
                let (_, b) = hip_turn(deg(travel), was);
                if b != was && flipped.is_none() {
                    flipped = Some(travel);
                }
                was = b;
            }
            flipped.unwrap()
        };
        assert!((sweep(0., 180., false) - 100.).abs() < 1.);
        assert!((sweep(180., 0., true) - 80.).abs() < 1.);
        assert!((sweep(0., -180., false) + 100.).abs() < 1.);
    }

    #[test]
    fn each_direction_turns_the_hips_and_runs_the_stride_its_way() {
        let gaits = loops();
        for travel in (0..8).map(|k| k as f32 * PI / 4. + 0.01) {
            let velocity = Vec2::new(travel.sin(), travel.cos()) * 6.;
            let mut stride = Stride::default();
            let mut step = stride.advance(&gaits, velocity, 1. / 60.);
            let mut moved = 0.;
            for _ in 0..30 {
                let phase = stride.phase;
                step = stride.advance(&gaits, velocity, 1. / 60.);
                moved += wrap((stride.phase - phase) * TAU) / TAU;
            }
            let (turn, backward) = hip_turn(travel, false);
            assert!((step.turn - turn).abs() < 1e-4, "{}°", travel.to_degrees());
            assert_eq!(moved < 0., backward, "{}°: {moved}", travel.to_degrees());
            // Half a second at 6 m/s covers half a second's worth of the blend's stride.
            let (a, b, w) = blend(&gaits, 6.);
            let stride = gaits[a].stride + (gaits[b].stride - gaits[a].stride) * w;
            assert!(
                (moved.abs() - 0.5 * 6. / stride).abs() < 1e-3,
                "{}°: {moved}",
                travel.to_degrees()
            );
        }
    }

    #[test]
    fn switching_speed_and_direction_never_restarts_the_stride() {
        let gaits = loops();
        let mut stride = Stride::default();
        let dt = 1. / 60.;
        // Walk, run, sprint, strafe, backpedal, turn round and walk again, all in jumps.
        let legs = [
            (0., 2.5),
            (0., 6.),
            (0., 9.),
            (90., 6.),
            (180., 3.6),
            (-100., 6.),
            (10., 9.),
            (0., 2.5),
        ];
        let mut last = stride.advance(&gaits, Vec2::new(0., 2.5), dt);
        let mut phase = stride.phase;
        for (travel, speed) in legs.map(|(t, s): (f32, f32)| (t.to_radians(), s)) {
            let velocity = Vec2::new(travel.sin(), travel.cos()) * speed;
            for _ in 0..40 {
                let step = stride.advance(&gaits, velocity, dt);
                let moved = wrap((stride.phase - phase) * TAU) / TAU;
                // At most the fastest loop's quickest pace: no jumps.
                assert!(moved.abs() <= PACE[1] / 0.667 * dt + 1e-5, "{moved}");
                // The hips swing round quickly but steadily, never in one go: every jump here
                // is 70° or more.
                assert!(
                    (step.turn - last.turn).abs() < 0.2,
                    "{step:?} after {last:?}"
                );
                phase = stride.phase;
                last = step;
            }
        }
    }
}
