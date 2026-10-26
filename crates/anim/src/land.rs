//! Landing with weight (#217): the legs reach for the ground as it nears, timed by when the body
//! will touch down rather than by the clip (distance matching, as Paragon's landings do), and the
//! pelvis dips on touching down, as far as the fall was fast. A fall far enough to hurt is the
//! character's to show with its landing clip.
//!
//! [`Landing::update`] follows the body through the air and says when it touches down, how fast
//! and from how high; [`Landing::dip_for`] kicks the pelvis's spring, and [`Landing::reach`] says
//! how far the legs have reached for the ground on the way down.
use crate::locomotion::Spring;
use std::f32::consts::{E, LN_2};

/// The sim's gravity, in m/s² (`sim::movement::GRAVITY`), for when a falling body touches down.
pub const GRAVITY: f32 = 20.;
/// Falls further than this hurt, in metres (`sim::movement::SAFE_FALL`).
pub const SAFE_FALL: f32 = 4.;
/// How long before touching down the legs start reaching for the ground, in seconds.
pub const REACH: f32 = 0.2;
/// How fast the legs' reach can change, a second: faster than any fall needs (7.5 at most), so it
/// only steadies a jump in the ground beneath, as when the body passes over the edge of a ledge
/// it lands on.
pub const REACH_RATE: f32 = 10.;
/// How far the pelvis dips on touching down, in metres for each metre a second of fall speed (a
/// jump on the flat lands at 6.6), and the most it dips.
pub const DIP: f32 = 0.01;
pub const MAX_DIP: f32 = 0.15;
/// Half-life of the dip's spring, in seconds: kicked on touching down, it bottoms out after about
/// three quarters of one and is back up after about four.
pub const DIP_HALF_LIFE: f32 = 0.1;

/// Touching down: how fast the body was falling, in metres a second, and how far it fell from the
/// top of its flight, in metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touchdown {
    pub speed: f32,
    pub fell: f32,
}

/// One body's flights and landings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Landing {
    /// In the air: the highest it has been, and the fastest it has fallen.
    flight: Option<(f32, f32)>,
    dip: Spring,
    /// How far the legs have reached for the ground.
    reached: f32,
}

impl Landing {
    /// One frame of `dt` seconds: the body at height `y`, moving up at `vy` metres a second, in the
    /// air or not. Returns the touchdown on the frame it lands.
    pub fn update(&mut self, airborne: bool, y: f32, vy: f32, dt: f32) -> Option<Touchdown> {
        let touchdown = match (self.flight, airborne) {
            (Some((top, speed)), true) => {
                self.flight = Some((top.max(y), speed.max(-vy)));
                None
            }
            (None, true) => {
                self.flight = Some((y, (-vy).max(0.)));
                None
            }
            (Some((top, speed)), false) => {
                self.flight = None;
                self.reached = 0.;
                Some(Touchdown {
                    speed: speed.max(-vy).max(0.),
                    fell: (top - y).max(0.),
                })
            }
            (None, false) => None,
        };
        if self.dip != Spring::default() {
            self.dip.update(0., DIP_HALF_LIFE, dt);
            if self.dip.value.abs() < 1e-5 && self.dip.velocity.abs() < 1e-4 {
                self.dip = Spring::default();
            }
        }
        touchdown
    }

    /// Dips the pelvis for touching down at `speed` metres a second: [`DIP`] for each, up to
    /// [`MAX_DIP`].
    pub fn dip_for(&mut self, speed: f32) {
        let depth = (DIP * speed).clamp(0., MAX_DIP);
        // A critically damped spring at rest, kicked to a speed v, goes out to v / (rate e) and
        // comes back.
        self.dip.velocity -= depth * 2. * LN_2 / DIP_HALF_LIFE * E;
    }

    /// How far the pelvis is dipped, in metres (0 or less).
    pub fn dip(&self) -> f32 {
        self.dip.value.min(0.)
    }

    /// How far the legs have reached for the ground, `dt` seconds on, with the body `height`
    /// metres above it and moving up at `vy` metres a second: [`reach`], changing no faster than
    /// [`REACH_RATE`].
    pub fn reach(&mut self, height: f32, vy: f32, dt: f32) -> f32 {
        let most = REACH_RATE * dt;
        self.reached += (reach(height, vy) - self.reached).clamp(-most, most);
        self.reached
    }
}

/// How long until a body `height` metres above the ground, moving up at `vy` metres a second,
/// touches down, falling under [`GRAVITY`].
pub fn time_to_land(height: f32, vy: f32) -> f32 {
    (vy + (vy * vy + 2. * GRAVITY * height.max(0.)).sqrt()) / GRAVITY
}

/// How far the legs have reached for the ground: from 0 [`REACH`] seconds before touching down to
/// 1 on it, easing in and out; 0 while the body still rises.
pub fn reach(height: f32, vy: f32) -> f32 {
    if vy >= 0. {
        return 0.;
    }
    let w = (1. - time_to_land(height, vy) / REACH).clamp(0., 1.);
    w * w * (3. - 2. * w)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1. / 60.;

    /// The sim's jump: up at 6.6 m/s from `from` metres above where it lands, frame by frame
    /// until it touches down. Each frame's height and vertical speed.
    fn jump(from: f32) -> Vec<(f32, f32)> {
        let mut frames = vec![];
        for f in 1.. {
            let t = f as f32 * DT;
            let y = from + 6.6 * t - 0.5 * GRAVITY * t * t;
            if y <= 0. {
                break;
            }
            frames.push((y, 6.6 - GRAVITY * t));
        }
        frames.push((0., 0.));
        frames
    }

    #[test]
    fn touching_down_says_how_fast_and_how_far_it_fell() {
        for from in [0., 2., 5.] {
            let mut landing = Landing::default();
            landing.update(false, from, 0., DT);
            let mut touchdowns = vec![];
            for (y, vy) in jump(from) {
                touchdowns.extend(landing.update(y > 0., y, vy, DT));
            }
            let [t] = touchdowns[..] else {
                panic!("{touchdowns:?}")
            };
            // From the top of the arc (1.09 m up) to the ground, and as fast as that falls.
            let fell = from + 6.6 * 6.6 / (2. * GRAVITY);
            assert!((t.fell - fell).abs() < 0.05, "from {from}: fell {}", t.fell);
            let speed = (2. * GRAVITY * fell).sqrt();
            assert!(
                (t.speed - speed).abs() < 0.5,
                "from {from}: {} m/s",
                t.speed
            );
        }
    }

    #[test]
    fn the_dip_goes_with_the_fall_speed_and_comes_back() {
        let mut deepest = vec![];
        for speed in [3., 6.6, 10., 30.] {
            let mut landing = Landing::default();
            landing.dip_for(speed);
            let mut dips = vec![];
            for _ in 0..60 {
                landing.update(false, 0., 0., DT);
                dips.push(landing.dip());
            }
            let low = dips.iter().copied().fold(0., f32::min);
            // As deep as the fall speed says, up to the most...
            let want = (DIP * speed).min(MAX_DIP);
            assert!(
                (-low - want).abs() < 0.03 * want,
                "{speed} m/s dipped {low}"
            );
            // ...going down at a fraction of the speed it fell at, then easing back up without
            // bouncing, and settling.
            let bottom = dips.iter().position(|d| *d == low).unwrap();
            assert!(
                -dips[0] < 0.6 * want && -dips[0] / DT < speed,
                "{speed} m/s: {}",
                dips[0]
            );
            for (i, pair) in dips.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                assert!(b <= 0., "{speed} m/s: came up past where it was");
                assert!(
                    if i < bottom { b <= a } else { b >= a },
                    "{speed} m/s: bounced"
                );
            }
            assert_eq!(landing, Landing::default());
            deepest.push(-low);
        }
        assert!(deepest.windows(2).all(|w| w[0] < w[1] || w[1] == MAX_DIP));
    }

    #[test]
    fn the_legs_reach_for_the_ground_by_when_it_lands() {
        // From the top of a jump the body takes a third of a second to land.
        assert!((time_to_land(1.089, 0.) - 0.33).abs() < 0.005);
        assert!((time_to_land(0., -6.6)).abs() < 1e-6);
        // Rising, the legs don't reach; falling, they start REACH before touching down and are
        // all the way there on it, without a jump.
        let frames = jump(3.);
        let air = &frames[..frames.len() - 1];
        let mut last = 0.;
        for (i, &(y, vy)) in air.iter().enumerate() {
            let w = reach(y, vy);
            let left = (air.len() - i) as f32 * DT;
            if vy > 0. || left > REACH + DT {
                assert_eq!(w, 0., "{left} s out");
            }
            assert!(w >= last && w - last < 0.3, "{left} s out: {last} to {w}");
            last = w;
        }
        assert!(last > 0.95, "only reached {last} on touching down");
        // Following the body, a landing on the flat reaches just as fast...
        let mut landing = Landing::default();
        for &(y, vy) in air {
            assert!((landing.reach(y, vy, DT) - reach(y, vy)).abs() < 1e-5);
        }
        // ...and running and jumping up onto a ledge 80 cm high, which the body passes over on
        // the way down 21 cm above it, the legs don't jump down to it (they would by 0.63).
        let ground = |x: f32| if x < 2.5 { 0. } else { 0.8 };
        let (mut landing, mut last, mut most) = (Landing::default(), 0., 0f32);
        for f in 1.. {
            let t = f as f32 * DT;
            let (x, y, vy) = (6. * t, 6.6 * t - 0.5 * GRAVITY * t * t, 6.6 - GRAVITY * t);
            if y <= ground(x) {
                break;
            }
            landing.update(true, y, vy, DT);
            let w = landing.reach(y - ground(x), vy, DT);
            most = most.max((reach(y - ground(x), vy) - last).abs());
            assert!((w - last).abs() <= REACH_RATE * DT + 1e-6, "{last} to {w}");
            last = w;
        }
        assert!(most > 0.5, "the ground under the body only jumped {most}");
        assert!(last > 0.5, "only reached {last} landing on the ledge");
        // Touched down, it starts again from nothing.
        landing.update(false, 0.8, 0., DT);
        assert_eq!(landing.reach(0.05, -5., DT), REACH_RATE * DT);
    }
}
