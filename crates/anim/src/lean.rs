//! Bodies lean with what they do (#217): forward as they speed up, back as they stop, and into
//! turns, the way a runner's weight shifts. The small procedural layer Overgrowth and Fortnite put
//! on top of their clips, which play upright whatever the body does.
//!
//! [`Lean::update`] follows the body's velocity over the ground on a critically damped spring. The
//! spring's own rate of change is the body's acceleration, smoothed, and the body tips toward it
//! by a share of the angle that would balance it (as a runner leans into a bend at
//! `atan(a / g)`), within caps that grow with speed, so a walker barely leans and a runner fully.
//! The tilt follows that on a spring of its own, so it builds and goes without a jump whether the
//! sim ramps the speed up (#195) or changes it at once.
use crate::locomotion::Spring;
use glam::Vec2;

/// The furthest the body leans forward or back, and to either side, in radians (8° and 10°), at
/// [`FULL_SPEED`] and faster; slower, the caps shrink in step with the speed.
pub const MAX_PITCH: f32 = 0.14;
pub const MAX_BANK: f32 = 0.175;
/// The speed at which the caps apply in full, in metres a second: a jog.
pub const FULL_SPEED: f32 = 5.;
/// How much of the angle that balances the acceleration the body leans.
pub const SHARE: f32 = 0.35;
/// The acceleration a body balances against, in m/s²: the real world's, since this is about how
/// a body holds itself, not how far the sim lets it jump.
const G: f32 = 9.81;
/// Half-lives in seconds: of the spring the velocity follows, whose rate of change is the
/// acceleration, and of the tilt following the lean that balances it (and going when the lean is
/// held: in the air, rolling, down).
pub const VELOCITY_HALF_LIFE: f32 = 0.1;
pub const TILT_HALF_LIFE: f32 = 0.07;

/// How one body leans, following its velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lean {
    /// `None` until the first update, which takes the velocity as it is rather than as a start
    /// from standing.
    velocity: Option<Spring<Vec2>>,
    tilt: Spring<Vec2>,
}

impl Lean {
    /// One frame of `dt` seconds for a body moving at `velocity` over the ground (x and z) and
    /// facing `yaw` round +y (from +z toward +x), with `free` of the lean showing (0 lets it go).
    /// Returns the tilt in the model's frame: how far the body leans toward +x (the bank, into a
    /// turn toward +x) and toward +z (the pitch, forward), in radians.
    pub fn update(&mut self, velocity: Vec2, yaw: f32, free: f32, dt: f32) -> Vec2 {
        let spring = self.velocity.get_or_insert(Spring {
            value: velocity,
            velocity: Vec2::ZERO,
        });
        // Going steadily, and upright: there's nothing to lean toward, so nothing to work out.
        if spring.value == velocity
            && spring.velocity == Vec2::ZERO
            && self.tilt == Spring::default()
        {
            return Vec2::ZERO;
        }
        settle(spring, velocity, VELOCITY_HALF_LIFE, dt);
        let (accel, speed) = (
            spring.velocity,
            spring.value.length().max(velocity.length()),
        );
        // Into the body's frame: the way it faces is +z, the side it turns toward +x.
        let (sin, cos) = yaw.sin_cos();
        let side = accel.x * cos - accel.y * sin;
        let ahead = accel.x * sin + accel.y * cos;
        let scale = (speed / FULL_SPEED).min(1.) * free.clamp(0., 1.);
        let lean = |a: f32, most: f32| ((a / G).atan() * SHARE).clamp(-most, most) * scale;
        let target = Vec2::new(lean(side, MAX_BANK), lean(ahead, MAX_PITCH));
        settle(&mut self.tilt, target, TILT_HALF_LIFE, dt)
    }

    /// Whether it leans no way at all, and won't until the velocity changes.
    pub fn settled(&self) -> bool {
        self.velocity.is_none_or(|s| s.velocity == Vec2::ZERO) && self.tilt == Spring::default()
    }
}

/// Moves `spring` toward `target`, and once it is there, stops it on it: a steady pace leans no
/// way at all, and costs nothing.
fn settle(spring: &mut Spring<Vec2>, target: Vec2, half_life: f32, dt: f32) -> Vec2 {
    if spring.value != target || spring.velocity != Vec2::ZERO {
        spring.update(target, half_life, dt);
        if spring.value.distance_squared(target) < 1e-10 && spring.velocity.length() < 1e-4 {
            spring.snap(target);
        }
    }
    spring.value
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1. / 60.;

    /// Runs `lean` for `frames` frames at `velocity` (a function of the frame), facing the way it
    /// goes (+z standing), and returns each frame's tilt.
    fn run(lean: &mut Lean, frames: usize, velocity: impl Fn(usize) -> Vec2) -> Vec<Vec2> {
        (0..frames)
            .map(|f| {
                let v = velocity(f);
                let yaw = if v == Vec2::ZERO { 0. } else { v.x.atan2(v.y) };
                lean.update(v, yaw, 1., DT)
            })
            .collect()
    }

    /// The furthest one way or the other that `pick` of the tilts goes.
    fn most(tilts: &[Vec2], pick: impl Fn(Vec2) -> f32) -> f32 {
        tilts
            .iter()
            .map(|t| pick(*t))
            .fold(0., |a, b| if b.abs() > a.abs() { b } else { a })
    }

    /// Running round at `speed` toward +x (or -x with `toward` -1) at `rate` radians a second.
    fn circling(speed: f32, rate: f32, toward: f32) -> impl Fn(usize) -> Vec2 {
        move |f| {
            let way = rate * f as f32 * DT;
            Vec2::new(toward * way.sin(), way.cos()) * speed
        }
    }

    #[test]
    fn it_leans_forward_speeding_up_back_stopping_and_into_turns() {
        // Standing, then running off ahead (+z): forward.
        let mut lean = Lean::default();
        let tilts = run(&mut lean, 60, |f| {
            if f < 10 { Vec2::ZERO } else { Vec2::Y * 6. }
        });
        let pitch = most(&tilts, |t| t.y);
        assert!(pitch > 0.05, "leaned {pitch} speeding up");
        assert!(
            most(&tilts, |t| t.x).abs() < 1e-6,
            "it banked running straight"
        );
        // Running, then stopping: back.
        let tilts = run(&mut lean, 60, |_| Vec2::ZERO);
        assert!(
            most(&tilts, |t| t.y) < -0.05,
            "it didn't lean back stopping"
        );
        // Running round toward +x: into the turn, so toward +x; the other way, the other way.
        for toward in [1., -1.] {
            let mut lean = Lean::default();
            let bank = run(&mut lean, 120, circling(6., 1.2, toward))
                .last()
                .unwrap()
                .x;
            assert!(
                bank * toward > 0.05,
                "banked {bank} turning toward {toward}"
            );
        }
        // Facing +x, speeding up along +x is still forward.
        let quarter = std::f32::consts::FRAC_PI_2;
        let mut lean = Lean::default();
        lean.update(Vec2::ZERO, quarter, 1., DT);
        let tilts: Vec<_> = (0..20)
            .map(|_| lean.update(Vec2::X * 6., quarter, 1., DT))
            .collect();
        assert!(most(&tilts, |t| t.y) > 0.05 && most(&tilts, |t| t.x).abs() < 1e-4);
    }

    #[test]
    fn the_caps_hold_and_shrink_with_speed() {
        // Setting off, stopping dead and spinning round as tight as can be: never past the caps,
        // and right up to them.
        for speed in [3., 5., 9.] {
            let mut lean = Lean::default();
            let mut tilts = run(&mut lean, 40, |f| {
                if f < 5 { Vec2::ZERO } else { Vec2::Y * speed }
            });
            tilts.extend(run(&mut lean, 40, |_| Vec2::ZERO));
            tilts.extend(run(&mut lean, 120, circling(speed, 4., 1.)));
            let scale = (speed / FULL_SPEED).min(1.);
            let (pitch, bank) = (most(&tilts, |t| t.y), most(&tilts, |t| t.x));
            assert!(
                pitch.abs() <= MAX_PITCH * scale + 1e-6,
                "{speed}: pitched {pitch}"
            );
            assert!(
                bank.abs() <= MAX_BANK * scale + 1e-6,
                "{speed}: banked {bank}"
            );
            assert!(
                pitch.abs() > 0.9 * MAX_PITCH * scale,
                "{speed}: only pitched {pitch}"
            );
            assert!(
                bank.abs() > 0.9 * MAX_BANK * scale,
                "{speed}: only banked {bank}"
            );
        }
        // Setting off at a walk, it barely leans.
        let mut lean = Lean::default();
        let tilts = run(&mut lean, 40, |f| {
            if f < 5 { Vec2::ZERO } else { Vec2::Y * 1.4 }
        });
        assert!(most(&tilts, |t| t.y) < 0.3 * MAX_PITCH);
    }

    #[test]
    fn a_ramp_and_a_jump_in_speed_both_lean_smoothly_and_settle() {
        // At once (today's sim) or over a quarter second (#195's ramp): the lean comes and goes
        // without a jump, and once the speed holds it is gone and costs nothing.
        for ramp in [0, 15] {
            let mut lean = Lean::default();
            let speed = |f: usize| 6. * ((f as f32 - 10.) / ramp.max(1) as f32).clamp(0., 1.);
            let tilts = run(&mut lean, 120, |f| Vec2::Y * speed(f));
            for pair in tilts.windows(2) {
                assert!(
                    pair[0].distance(pair[1]) < 0.02,
                    "ramp {ramp}: jumped {} to {}",
                    pair[0],
                    pair[1]
                );
            }
            assert!(most(&tilts, |t| t.y) > 0.5 * MAX_PITCH, "ramp {ramp}");
            assert_eq!(*tilts.last().unwrap(), Vec2::ZERO);
            assert!(lean.settled());
        }
    }

    #[test]
    fn it_lets_go_smoothly_and_takes_a_first_velocity_as_it_is() {
        // A body first seen running doesn't lean as if it had just set off.
        let mut lean = Lean::default();
        assert_eq!(lean.update(Vec2::Y * 6., 0., 1., DT), Vec2::ZERO);
        assert!(lean.settled());
        // Leaning into a start, then held (in the air): it comes upright smoothly.
        let mut lean = Lean::default();
        run(&mut lean, 5, |_| Vec2::ZERO);
        let leaning = run(&mut lean, 8, |_| Vec2::Y * 6.).last().unwrap().y;
        assert!(leaning > 0.05);
        let held: Vec<_> = (0..60)
            .map(|_| lean.update(Vec2::Y * 6., 0., 0., DT))
            .collect();
        let mut last = leaning;
        for t in &held {
            assert!((t.y - last).abs() < 0.02, "{last} to {}", t.y);
            assert!(t.y < leaning + 0.01, "it leaned further, to {}", t.y);
            last = t.y;
        }
        assert_eq!(*held.last().unwrap(), Vec2::ZERO);
    }
}
