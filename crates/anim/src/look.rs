//! Heads and chests follow the aim (#212): the two procedural layers third-person games put on top
//! of their clips. An aim offset pitches the spine with the aim, so the arms and the bow, wand or
//! staff they hold point where the crosshair does; a look-at turns the head, and a little of the
//! chest, toward what matters: a target, or a player stopping by. Both follow on critically damped
//! springs, within limits, and fade off while a swing's clip owns the head.
//!
//! [`Gaze::update`] takes where to look relative to the way the upper body faces (after #211's
//! turn) and gives the turn for each bone of the chain from the bottom of the spine to the head:
//! the yaw round the vertical and the pitch about the line across the shoulders, in the model's
//! frame. Each bone carries its share, so the head ends up turned by all of it.
use crate::locomotion::Spring;
use std::f32::consts::{PI, TAU};

/// The chain, from the bottom of the spine to the head: `spine_01`, `spine_02`, `spine_03`,
/// `neck_01` and `Head`.
pub const CHAIN: usize = 5;
/// How the bones share the pitch. Aiming a weapon, the spine takes all of it, so the arms and what
/// they hold point the way the head looks; just looking, the neck and head take nearly all of it.
pub const AIM_PITCH: [f32; CHAIN] = [0.3, 0.3, 0.4, 0., 0.];
pub const LOOK_PITCH: [f32; CHAIN] = [0., 0.05, 0.1, 0.35, 0.5];
/// How the bones share a turn to the side: a little of the chest (a fifth), the rest the neck and
/// head.
pub const LOOK_YAW: [f32; CHAIN] = [0., 0.05, 0.15, 0.3, 0.5];
/// The furthest round the gaze turns from where the upper body faces, in radians (70°). A point
/// further round than [`LET_GO`] (110°) is let go of, and the gaze comes back ahead, rather than
/// swinging across from one side to the other as the point passes behind.
pub const MAX_YAW: f32 = 1.22;
pub const LET_GO: f32 = 1.92;
/// The furthest up and down the gaze pitches, in radians: aiming, 50° up and 45° down (a bow up a
/// cliff, or down from the bridge); just looking, 40° either way.
pub const AIM_UP: f32 = 0.87;
pub const AIM_DOWN: f32 = 0.79;
pub const LOOK_LIMIT: f32 = 0.7;
/// Half-lives of the springs, in seconds: aiming follows closely, so shots go where the crosshair
/// is, and looking turns at a person's pace. Then taking up aiming or letting it go, and fading
/// out for a swing and back.
pub const AIM_HALF_LIFE: f32 = 0.05;
pub const LOOK_HALF_LIFE: f32 = 0.15;
pub const FADE_HALF_LIFE: f32 = 0.1;

/// Where to look, relative to the way the upper body faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    /// Round from the facing (toward +x) and up from level, in radians.
    pub yaw: f32,
    pub pitch: f32,
    /// Whether a weapon aims there (the spine pitches), rather than only the head looking.
    pub aiming: bool,
}

/// The turn for each bone of the chain, in radians: round the vertical (toward +x), and up about
/// the line across the shoulders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turns {
    pub yaw: [f32; CHAIN],
    pub pitch: [f32; CHAIN],
}

impl Turns {
    /// The whole turn, which the head ends up with.
    pub fn total(&self) -> (f32, f32) {
        (self.yaw.iter().sum(), self.pitch.iter().sum())
    }
}

/// Where one character looks, following where it wants to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gaze {
    yaw: Spring,
    pitch: Spring,
    /// How much it aims (1) rather than looks (0), and how much of the gaze shows (0 while a swing
    /// owns the head).
    aiming: Spring,
    weight: Spring,
}

impl Gaze {
    /// One frame of `dt` seconds, looking toward `want`, or ahead when `None`. `free` is how much
    /// the clips leave the head free: 1 standing, moving or loosing a shot, 0 while a swing's clip
    /// owns it, which fades the gaze out.
    pub fn update(&mut self, want: Option<Aim>, free: f32, dt: f32) -> Turns {
        // Faded right out, it stops following altogether, and fading back in it starts where it
        // now looks, rather than sweeping across from where it looked before.
        let out = self.faded_out();
        if out && free <= 0. {
            return Turns::default();
        }
        let aiming = f32::from(u8::from(want.is_some_and(|w| w.aiming)));
        if out {
            self.aiming.snap(aiming);
        }
        let aiming = settle(&mut self.aiming, aiming, FADE_HALF_LIFE, dt).clamp(0., 1.);
        let weight = settle(&mut self.weight, free.clamp(0., 1.), FADE_HALF_LIFE, dt).clamp(0., 1.);
        let up = LOOK_LIMIT + (AIM_UP - LOOK_LIMIT) * aiming;
        let down = LOOK_LIMIT + (AIM_DOWN - LOOK_LIMIT) * aiming;
        let (yaw, pitch) = match want.filter(|w| wrap(w.yaw).abs() < LET_GO) {
            Some(w) => (
                wrap(w.yaw).clamp(-MAX_YAW, MAX_YAW),
                w.pitch.clamp(-down, up),
            ),
            None => (0., 0.),
        };
        if out {
            self.yaw.snap(yaw);
            self.pitch.snap(pitch);
        }
        let half_life = LOOK_HALF_LIFE + (AIM_HALF_LIFE - LOOK_HALF_LIFE) * aiming;
        let yaw = settle(&mut self.yaw, yaw, half_life, dt).clamp(-MAX_YAW, MAX_YAW);
        let pitch = settle(&mut self.pitch, pitch, half_life, dt).clamp(-down, up);
        let mut turns = Turns::default();
        if weight == 0. || yaw == 0. && pitch == 0. {
            return turns;
        }
        for i in 0..CHAIN {
            let share = LOOK_PITCH[i] + (AIM_PITCH[i] - LOOK_PITCH[i]) * aiming;
            turns.yaw[i] = LOOK_YAW[i] * yaw * weight;
            turns.pitch[i] = share * pitch * weight;
        }
        turns
    }

    /// How much of the gaze shows, from 0 to 1.
    pub fn weight(&self) -> f32 {
        self.weight.value.clamp(0., 1.)
    }

    /// Whether it has faded right out, so that while it's held, an update does nothing.
    pub fn faded_out(&self) -> bool {
        self.weight == Spring::default()
    }
}

/// Moves `spring` toward `target`, and once it is there, stops it on it: settled ahead or faded
/// out, the gaze turns nothing at all, and costs nothing.
fn settle(spring: &mut Spring, target: f32, half_life: f32, dt: f32) -> f32 {
    if spring.value == target && spring.velocity == 0. {
        return target;
    }
    let value = spring.update(target, half_life, dt);
    if (value - target).abs() < 1e-5 && spring.velocity.abs() < 1e-4 {
        spring.snap(target);
    }
    spring.value
}

fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1. / 60.;

    /// Runs `gaze` for `frames` frames toward `want` with `free` of it showing, and returns each
    /// frame's turns.
    fn run(gaze: &mut Gaze, want: Option<Aim>, free: f32, frames: usize) -> Vec<Turns> {
        (0..frames).map(|_| gaze.update(want, free, DT)).collect()
    }

    fn aim(yaw_degrees: f32, pitch_degrees: f32, aiming: bool) -> Option<Aim> {
        Some(Aim {
            yaw: yaw_degrees.to_radians(),
            pitch: pitch_degrees.to_radians(),
            aiming,
        })
    }

    #[test]
    fn the_limits_hold() {
        // However far round, up or down the point is, the head turns no further than the limits,
        // even on the way there.
        for (yaw, pitch, aiming, most_up, most_down) in [
            (100., 80., true, AIM_UP, AIM_DOWN),
            (-100., -80., true, AIM_UP, AIM_DOWN),
            (100., 80., false, LOOK_LIMIT, LOOK_LIMIT),
            (-100., -80., false, LOOK_LIMIT, LOOK_LIMIT),
        ] {
            let mut gaze = Gaze::default();
            let frames = run(&mut gaze, aim(yaw, pitch, aiming), 1., 120);
            for t in &frames {
                let (y, p) = t.total();
                assert!(
                    y.abs() <= MAX_YAW + 1e-5,
                    "{yaw}°: turned {}°",
                    y.to_degrees()
                );
                assert!(
                    p <= most_up + 1e-5 && p >= -most_down - 1e-5,
                    "{pitch}°: pitched {}°",
                    p.to_degrees()
                );
            }
            // ...and it gets there.
            let (y, p) = frames.last().unwrap().total();
            assert!((y.abs() - MAX_YAW).abs() < 0.01, "{yaw}°: settled at {y}");
            let edge = if pitch > 0. { most_up } else { most_down };
            assert!((p.abs() - edge).abs() < 0.01, "{pitch}°: settled at {p}");
        }
        // Within them, it turns all the way.
        let mut gaze = Gaze::default();
        let (y, p) = run(&mut gaze, aim(-40., 25., false), 1., 120)
            .last()
            .unwrap()
            .total();
        assert!((y.to_degrees() + 40.).abs() < 0.1 && (p.to_degrees() - 25.).abs() < 0.1);
    }

    #[test]
    fn the_pitch_splits_over_the_chain_as_specified() {
        for (aiming, shares) in [(true, AIM_PITCH), (false, LOOK_PITCH)] {
            let mut gaze = Gaze::default();
            let t = *run(&mut gaze, aim(0., 30., aiming), 1., 180)
                .last()
                .unwrap();
            let total = t.total().1;
            assert!((total.to_degrees() - 30.).abs() < 0.05, "{total}");
            for (i, share) in shares.into_iter().enumerate() {
                assert!(
                    (t.pitch[i] - share * total).abs() < 1e-4,
                    "aiming {aiming}: bone {i} pitched {} of {total}",
                    t.pitch[i]
                );
            }
        }
        // Aiming, the spine takes all of it (the arms point the way the head looks); looking, the
        // neck and head take nearly all of it.
        assert_eq!(AIM_PITCH[..3].iter().sum::<f32>(), 1.);
        assert!(LOOK_PITCH[3..].iter().sum::<f32>() >= 0.85);
        for shares in [AIM_PITCH, LOOK_PITCH, LOOK_YAW] {
            assert!((shares.iter().sum::<f32>() - 1.).abs() < 1e-6);
        }
        // A look round to the side turns the chest a little (a fifth), the neck and head the rest.
        let mut gaze = Gaze::default();
        let t = *run(&mut gaze, aim(50., 0., false), 1., 180).last().unwrap();
        let chest: f32 = t.yaw[..3].iter().sum();
        assert!((chest / t.total().0 - 0.2).abs() < 1e-4);
    }

    #[test]
    fn the_look_fades_out_during_a_swing_and_back_after() {
        let mut gaze = Gaze::default();
        let want = aim(60., -20., true);
        let start = gaze_size(run(&mut gaze, want, 1., 60).last().unwrap());
        // A swing's clip owns the head for half a second: the gaze fades out, smoothly...
        let swing = run(&mut gaze, want, 0., 30);
        let mut last = start;
        for t in &swing {
            let now = gaze_size(t);
            assert!(now <= last + 1e-6, "it turned back mid-swing");
            assert!(
                last - now < 0.1 * start,
                "it dropped {} in a frame",
                last - now
            );
            last = now;
        }
        assert!(
            last < 0.03 * start,
            "{last} of {start} left after the swing"
        );
        // ...and comes back once the swing is over.
        let after = run(&mut gaze, want, 1., 60);
        assert!((gaze_size(after.last().unwrap()) - start).abs() < 0.01 * start);
    }

    #[test]
    fn faded_right_out_it_stops_and_fades_back_in_where_it_now_looks() {
        let mut gaze = Gaze::default();
        run(&mut gaze, aim(60., 0., false), 1., 60);
        // Held for good (a long rally, or quarter detail), it fades right out and stops...
        let held = run(&mut gaze, aim(60., 0., false), 0., 120);
        assert_eq!(*held.last().unwrap(), Turns::default());
        assert!(gaze.faded_out());
        let still = gaze;
        run(&mut gaze, aim(-60., 20., true), 0., 30);
        assert_eq!(gaze, still, "it followed the point while faded out");
        // ...and fades back in already turned the way it now looks, rather than sweeping across
        // from where it looked before.
        let back = run(&mut gaze, aim(-60., 20., true), 1., 60);
        for t in &back {
            assert!(
                t.total().0 <= 0. && t.total().1 >= 0.,
                "it swept across: {t:?}"
            );
        }
        let (y, p) = back.last().unwrap().total();
        assert!((y.to_degrees() + 60.).abs() < 0.1 && (p.to_degrees() - 20.).abs() < 0.1);
    }

    /// How far the head is turned, all told.
    fn gaze_size(t: &Turns) -> f32 {
        let (y, p) = t.total();
        y.hypot(p)
    }

    #[test]
    fn the_gaze_never_jumps() {
        // The point jumps about, passes behind and goes; the head follows without jumping: at
        // most 25° a frame, about as far as the aim's spring turns when the point jumps 130°.
        let mut gaze = Gaze::default();
        let mut was = Turns::default();
        let script = [
            (aim(0., 0., false), 30),
            (aim(65., 10., false), 40),
            (aim(-65., -30., true), 40),
            // Behind, one way and then the other: it lets go and comes back ahead.
            (aim(150., 0., true), 40),
            (aim(-150., 0., true), 40),
            (None, 40),
            (aim(30., 45., true), 40),
        ];
        for (want, frames) in script {
            for t in run(&mut gaze, want, 1., frames) {
                let (dy, dp) = (t.total().0 - was.total().0, t.total().1 - was.total().1);
                assert!(dy.hypot(dp) < 25f32.to_radians(), "jumped {dy}, {dp}");
                was = t;
            }
        }
        // Behind, it looks ahead.
        let mut gaze = Gaze::default();
        let t = *run(&mut gaze, aim(170., 20., false), 1., 120)
            .last()
            .unwrap();
        assert!(gaze_size(&t) < 1e-3);
    }
}
