//! Transitions between clips that never freeze the old motion (#210).
//!
//! When the clip stack switches (another clip, a layer coming or going, a clip starting over), the
//! new pose plays at once and the difference from where the old motion was heading fades out, so
//! nothing stops dead and nothing pops. There are two ways of fading it, kept to compare:
//!
//! - [`Method::Inertialize`] holds each bone's difference as an offset with a velocity of its own
//!   and settles it on a critically damped spring (Bollo, "Inertialization", GDC 2018; Holden,
//!   "Spring-It-On"). The new motion's velocity is measured on the frame after the switch, so the
//!   shown motion keeps its speed through it and bones the switch didn't change carry on untouched.
//! - [`Method::DeadBlend`] carries the old motion on with its velocity dying away and cross-fades
//!   it into the new pose (Holden, "Dead Blending", 2023; Unreal's Dead Blending node).
//!
//! Both read the old motion from the poses actually shown, so a switch that lands while another is
//! still settling starts from wherever that one had got to.
use glam::{Quat, Vec3};
use std::f32::consts::LN_2;

/// A kind of switch in the clip stack. Each settles at its own pace, set in [`Switch::half_life`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// Between standing, walking, jogging and sprinting, and the legs under an action on the move.
    Locomotion,
    /// An attack, cast, roll, jump or hit reaction starting, or an action starting over.
    Action,
    /// Back to moving or standing as an action ends.
    Recover,
    /// An upper-body layer (guard, ready, stow, a drawn weapon's idle) coming or going.
    Layer,
    /// Touching down from a jump or a fall.
    Land,
    /// A level-of-detail swap: the same motion from other clip data.
    Detail,
}

impl Switch {
    /// How fast each kind of switch settles, in seconds: the one table to tune. It is the spring's
    /// half-life in Holden's sense, so a difference with no speed of its own is down to 60% after
    /// one, 8% after three and under 1% after five. Dead blending cross-fades over
    /// [`DEAD_BLEND_SPAN`] of them.
    pub const fn half_life(self) -> f32 {
        match self {
            Switch::Locomotion => 0.09,
            Switch::Action => 0.05,
            Switch::Recover => 0.1,
            Switch::Layer => 0.07,
            Switch::Land => 0.06,
            Switch::Detail => 0.05,
        }
    }
}

/// How long dead blending cross-fades, in half-lives of the switch.
pub const DEAD_BLEND_SPAN: f32 = 2.5;
/// The furthest the old motion is carried on at a switch, in seconds, so a long frame can't fling
/// it.
const MAX_AHEAD: f32 = 1. / 30.;
/// Caps on speeds read from the shown poses, in radians and metres a second, in case a pose jumped
/// without the stack calling it a switch.
const MAX_SPIN: f32 = 40.;
const MAX_SPEED: f32 = 10.;
/// A bone has settled once its offset, plus the distance its velocity would still carry it, is
/// under these, in radians and metres.
const SETTLED_ANGLE: f32 = 1e-3;
const SETTLED_SHIFT: f32 = 1e-4;
/// Dead blending leaves alone a bone the switch moved less than this, in radians and metres.
const STILL_ANGLE: f32 = 0.01;
const STILL_SHIFT: f32 = 1e-3;
/// The quickest dead blending lets a carried-on velocity die away: its half-life, in seconds.
const CARRY_MIN: f32 = 0.05;

/// A bone's local transform as the blender reads and writes it. Scale passes through untouched.
pub trait Bone {
    fn rotation(&self) -> Quat;
    fn translation(&self) -> Vec3;
    fn set(&mut self, rotation: Quat, translation: Vec3);
}

impl Bone for (Quat, Vec3) {
    fn rotation(&self) -> Quat {
        self.0
    }
    fn translation(&self) -> Vec3 {
        self.1
    }
    fn set(&mut self, rotation: Quat, translation: Vec3) {
        *self = (rotation, translation);
    }
}

/// How a switch fades the old motion out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Method {
    /// Offsets from the new motion, settled on a critically damped spring.
    #[default]
    Inertialize,
    /// The old motion carried on and cross-faded into the new.
    DeadBlend,
    /// Every switch cuts: for checking the clip stack on its own.
    Off,
}

/// One bone's part in a switch: its offset from the new motion (inertialization) or the old
/// motion carried on (dead blending).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Motion {
    rotation: Quat,
    /// Angular velocity: the axis scaled by radians a second, in the parent bone's frame.
    spin: Vec3,
    translation: Vec3,
    velocity: Vec3,
    /// Dead blending: half-lives of the carried-on spin and velocity, or 0 for a bone the switch
    /// didn't move, which shows the new pose straight away.
    spin_life: f32,
    velocity_life: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Settled,
    /// A switch was taken last frame, and its offsets wait on the new motion's velocity.
    Measuring,
    Settling,
}

/// Settles each switch of one character's clip stack into the next.
#[derive(Clone, Debug, Default)]
pub struct Blender {
    method: Method,
    /// The last two poses shown, newest first, how many of them there are yet, and the time
    /// between them.
    shown: [Vec<(Quat, Vec3)>; 2],
    shown_frames: u8,
    step: f32,
    bones: Vec<Motion>,
    /// Inertialization: the new pose at the switch, held a frame to measure its velocity.
    target: Vec<(Quat, Vec3)>,
    phase: Phase,
    half_life: f32,
    /// Dead blending: time since the switch.
    time: f32,
}

impl Blender {
    pub fn new(method: Method) -> Self {
        Self {
            method,
            ..Self::default()
        }
    }

    pub fn method(&self) -> Method {
        self.method
    }

    /// Changes how switches fade. One still settling under the old method ends in a cut.
    pub fn set_method(&mut self, method: Method) {
        if method != self.method {
            self.method = method;
            self.phase = Phase::Settled;
        }
    }

    /// Whether a switch is still settling.
    pub fn blending(&self) -> bool {
        self.phase != Phase::Settled
    }

    /// Turns the clip stack's pose for this frame into the pose to show, in place. `switch` says
    /// whether the stack switched since the last frame, and how; `dt` is the time since then.
    pub fn update<B: Bone>(&mut self, pose: &mut [B], switch: Option<Switch>, dt: f32) {
        if self.bones.len() != pose.len() {
            // Another skeleton: there is no old motion to carry on.
            let rest = vec![(Quat::IDENTITY, Vec3::ZERO); pose.len()];
            *self = Self {
                shown: [rest.clone(), rest.clone()],
                bones: vec![Motion::default(); pose.len()],
                target: rest,
                ..Self::new(self.method)
            };
        }
        let dt = dt.max(1e-4);
        match switch {
            Some(kind) if self.shown_frames > 0 && self.method != Method::Off => {
                self.start(pose, kind, dt)
            }
            _ => {
                if self.phase == Phase::Measuring {
                    self.measure(pose, dt);
                }
                if self.phase == Phase::Settling {
                    self.settle(pose, dt);
                }
            }
        }
        self.remember(pose, dt);
    }

    /// Takes a switch: this frame shows the old motion carried on by `dt`, and each bone's
    /// difference from the new pose is set up to fade.
    fn start<B: Bone>(&mut self, pose: &mut [B], kind: Switch, dt: f32) {
        self.half_life = kind.half_life();
        self.time = 0.;
        let ahead = dt.min(MAX_AHEAD);
        let span = self.half_life * DEAD_BLEND_SPAN;
        for (i, bone) in pose.iter_mut().enumerate() {
            let (shown, shown_translation) = self.shown[0][i];
            let (spin, velocity) = if self.shown_frames > 1 {
                let (before, before_translation) = self.shown[1][i];
                (
                    spin_between(before, shown, self.step).clamp_length_max(MAX_SPIN),
                    ((shown_translation - before_translation) / self.step)
                        .clamp_length_max(MAX_SPEED),
                )
            } else {
                (Vec3::ZERO, Vec3::ZERO)
            };
            let rotation = (Quat::from_scaled_axis(spin * ahead) * shown).normalize();
            let translation = shown_translation + velocity * ahead;
            let (to, to_translation) = (bone.rotation(), bone.translation());
            self.bones[i] = match self.method {
                Method::DeadBlend => {
                    let turn = shortest(to * rotation.inverse()).to_scaled_axis();
                    let shift = to_translation - translation;
                    if turn.length() < STILL_ANGLE && shift.length() < STILL_SHIFT {
                        self.bones[i] = Motion::default();
                        continue;
                    }
                    Motion {
                        rotation,
                        spin,
                        translation,
                        velocity,
                        spin_life: carry_life(spin, turn, span),
                        velocity_life: carry_life(velocity, shift, span),
                    }
                }
                _ => {
                    self.target[i] = (to, to_translation);
                    Motion {
                        rotation: shortest(rotation * to.inverse()),
                        spin,
                        translation: translation - to_translation,
                        velocity,
                        ..Motion::default()
                    }
                }
            };
            bone.set(rotation, translation);
        }
        self.phase = match self.method {
            Method::DeadBlend => Phase::Settling,
            _ => Phase::Measuring,
        };
    }

    /// Inertialization, the frame after a switch: now that the new motion's velocity is known,
    /// gives each offset the velocity that keeps the shown motion at the old motion's speed.
    fn measure<B: Bone>(&mut self, pose: &[B], dt: f32) {
        for ((m, &(from, from_translation)), bone) in
            self.bones.iter_mut().zip(&self.target).zip(pose)
        {
            let spin = spin_between(from, bone.rotation(), dt).clamp_length_max(MAX_SPIN);
            let velocity =
                ((bone.translation() - from_translation) / dt).clamp_length_max(MAX_SPEED);
            // What shows is the offset turning the new motion, so its angular velocity is the
            // offset's plus the new motion's turned by the offset. The spring runs on the offset's
            // rotation vector, which changes at a rate of its own.
            m.spin = log_rate(m.rotation.to_scaled_axis(), m.spin - m.rotation * spin);
            m.velocity -= velocity;
        }
        self.phase = Phase::Settling;
    }

    fn settle<B: Bone>(&mut self, pose: &mut [B], dt: f32) {
        match self.method {
            Method::Inertialize => {
                // The exact step of a critically damped spring (Holden's
                // `decay_spring_damper_exact`), with `rate` half its damping.
                let rate = 2. * LN_2 / self.half_life;
                let decay = (-rate * dt).exp();
                let mut settling = false;
                for (m, bone) in self.bones.iter_mut().zip(pose.iter_mut()) {
                    if *m == Motion::default() {
                        continue;
                    }
                    let offset = m.rotation.to_scaled_axis();
                    let j = m.spin + offset * rate;
                    let offset = (offset + j * dt) * decay;
                    m.spin = (m.spin - j * (rate * dt)) * decay;
                    let k = m.velocity + m.translation * rate;
                    m.translation = (m.translation + k * dt) * decay;
                    m.velocity = (m.velocity - k * (rate * dt)) * decay;
                    if offset.length() + m.spin.length() * self.half_life < SETTLED_ANGLE
                        && m.translation.length() + m.velocity.length() * self.half_life
                            < SETTLED_SHIFT
                    {
                        *m = Motion::default();
                        continue;
                    }
                    settling = true;
                    m.rotation = Quat::from_scaled_axis(offset);
                    bone.set(
                        m.rotation * bone.rotation(),
                        bone.translation() + m.translation,
                    );
                }
                if !settling {
                    self.phase = Phase::Settled;
                }
            }
            Method::DeadBlend => {
                self.time += dt;
                let fade = smoothstep(self.time / (self.half_life * DEAD_BLEND_SPAN));
                for (m, bone) in self.bones.iter_mut().zip(pose.iter_mut()) {
                    if m.spin_life == 0. {
                        continue;
                    }
                    m.spin *= (-LN_2 * dt / m.spin_life).exp();
                    m.velocity *= (-LN_2 * dt / m.velocity_life).exp();
                    m.rotation = (Quat::from_scaled_axis(m.spin * dt) * m.rotation).normalize();
                    m.translation += m.velocity * dt;
                    bone.set(
                        m.rotation.slerp(bone.rotation(), fade),
                        m.translation.lerp(bone.translation(), fade),
                    );
                }
                if fade >= 1. {
                    self.phase = Phase::Settled;
                }
            }
            Method::Off => self.phase = Phase::Settled,
        }
    }

    fn remember<B: Bone>(&mut self, pose: &[B], dt: f32) {
        self.shown.swap(0, 1);
        for (kept, bone) in self.shown[0].iter_mut().zip(pose) {
            *kept = (bone.rotation(), bone.translation());
        }
        self.step = dt;
        self.shown_frames = (self.shown_frames + 1).min(2);
    }
}

/// The same rotation as `q`, with the sign that turns the short way round.
fn shortest(q: Quat) -> Quat {
    if q.w < 0. { -q } else { q }
}

/// The angular velocity that turns `from` into `to` in `dt` seconds.
fn spin_between(from: Quat, to: Quat, dt: f32) -> Vec3 {
    shortest(to * from.inverse()).to_scaled_axis() / dt
}

/// How fast the rotation vector `x` changes as its rotation turns at angular velocity `spin` (in
/// the parent's frame): the inverse of SO(3)'s left Jacobian at `x`, applied to `spin`.
fn log_rate(x: Vec3, spin: Vec3) -> Vec3 {
    let angle = x.length();
    let along = if angle < 1e-2 {
        1. / 12.
    } else if angle < 3. {
        1. / (angle * angle) - (1. + angle.cos()) / (2. * angle * angle.sin())
    } else {
        // Near half a turn the rotation vector is ill-defined; take it as turning straight.
        return spin;
    };
    let turned = x.cross(spin);
    spin - 0.5 * turned + along * x.cross(turned)
}

/// How fast dead blending lets a carried-on `speed` die away: slowly enough to coast to the new
/// pose, `gap` away, when it heads that way, and quickly when it heads anywhere else.
fn carry_life(speed: Vec3, gap: Vec3, span: f32) -> f32 {
    let toward = speed.dot(gap.normalize_or_zero());
    if toward <= 0. {
        CARRY_MIN
    } else {
        (gap.length() * LN_2 / toward).clamp(CARRY_MIN, span.max(CARRY_MIN))
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    /// A bone turning steadily at `spin` from `start`, and sliding at `velocity` from `origin`.
    #[derive(Clone, Copy)]
    struct Steady {
        start: Quat,
        spin: Vec3,
        origin: Vec3,
        velocity: Vec3,
    }
    impl Steady {
        fn turning(start: Quat, spin: Vec3) -> Self {
            Self {
                start,
                spin,
                origin: Vec3::ZERO,
                velocity: Vec3::ZERO,
            }
        }
        /// Turning at `spin`, passing `pose` at `at` seconds.
        fn through(pose: Quat, spin: Vec3, at: f32) -> Self {
            Self::turning(Quat::from_scaled_axis(-spin * at) * pose, spin)
        }
        fn at(&self, t: f32) -> (Quat, Vec3) {
            (
                Quat::from_scaled_axis(self.spin * t) * self.start,
                self.origin + self.velocity * t,
            )
        }
    }

    /// Plays `clips` through a blender, one bone each, switching from clip `n - 1` to clip `n`
    /// at `switches[n - 1]` seconds, and returns the shown poses frame by frame.
    fn play(
        method: Method,
        kind: Switch,
        clips: &[&[Steady]],
        switches: &[f32],
        dt: f32,
        seconds: f32,
    ) -> Vec<Vec<(Quat, Vec3)>> {
        let mut blender = Blender::new(method);
        let mut shown = Vec::new();
        let mut playing = 0;
        for frame in 0..(seconds / dt).round() as usize {
            let t = frame as f32 * dt;
            let switch = switches
                .get(playing)
                .is_some_and(|&at| t >= at - dt * 0.5)
                .then(|| {
                    playing += 1;
                    kind
                });
            let mut pose: Vec<_> = clips[playing].iter().map(|c| c.at(t)).collect();
            blender.update(&mut pose, switch, dt);
            shown.push(pose);
        }
        shown
    }

    fn angle(a: Quat, b: Quat) -> f32 {
        shortest(b * a.inverse()).to_scaled_axis().length()
    }

    /// Angular velocity of bone `bone` over each frame of `shown`.
    fn spins(shown: &[Vec<(Quat, Vec3)>], bone: usize, dt: f32) -> Vec<Vec3> {
        shown
            .windows(2)
            .map(|w| spin_between(w[0][bone].0, w[1][bone].0, dt))
            .collect()
    }

    /// The largest change from one frame to the next in bone 0's angular velocity and in the
    /// velocity of its translation, between `from` and `to` seconds.
    fn roughness(shown: &[Vec<(Quat, Vec3)>], dt: f32, from: f32, to: f32) -> [f32; 2] {
        let frames = (from / dt).round() as usize - 2..(to / dt).round() as usize;
        let spins: Vec<_> = spins(&shown[frames.clone()], 0, dt);
        let speeds: Vec<_> = shown[frames]
            .windows(2)
            .map(|w| (w[1][0].1 - w[0][0].1) / dt)
            .collect();
        [spins, speeds].map(|v| v.windows(2).map(|w| w[1].distance(w[0])).fold(0., f32::max))
    }

    const BLENDING: [Method; 2] = [Method::Inertialize, Method::DeadBlend];

    /// Jogging one way, then at half a second a clip 70° and 5 cm away moving another way, as an
    /// arm might when an attack starts.
    fn swing() -> [Steady; 2] {
        [
            Steady {
                origin: Vec3::new(0., 0., -0.25),
                velocity: Vec3::new(0., 0., 0.5),
                ..Steady::through(Quat::IDENTITY, Vec3::new(0., 4., 0.), 0.5)
            },
            Steady {
                origin: Vec3::new(0.05, 0.15, 0.),
                velocity: Vec3::new(0., -0.3, 0.),
                ..Steady::through(
                    Quat::from_rotation_z(70f32.to_radians()),
                    Vec3::new(-3., 0., 0.),
                    0.5,
                )
            },
        ]
    }

    #[test]
    fn a_switch_keeps_the_pose_and_its_velocity() {
        let [from, to] = swing();
        for method in BLENDING {
            // At 60 fps the switch frame carries the old motion on, and nothing pops.
            let dt = 1. / 60.;
            let shown = play(
                method,
                Switch::Locomotion,
                &[&[from], &[to]],
                &[0.5],
                dt,
                1.,
            );
            let at = 30;
            let carried = from.at(at as f32 * dt);
            assert!(
                angle(shown[at][0].0, carried.0) < 1e-4,
                "{method:?}: the switch frame shows the old motion carried on"
            );
            assert!(shown[at][0].1.distance(carried.1) < 1e-5);
            let jump = angle(from.at(0.5).0, to.at(0.5).0);
            let worst = shown
                .windows(2)
                .map(|w| angle(w[0][0].0, w[1][0].0))
                .fold(0., f32::max);
            assert!(
                worst < jump / 4.,
                "{method:?}: a frame turned {worst} rad of a {jump} rad jump"
            );
            // Four times the frame rate, a quarter of the change in velocity from frame to frame
            // across the switch: it is continuous. A pop or a jump in velocity wouldn't shrink.
            let rough = |dt: f32| {
                let shown = play(
                    method,
                    Switch::Locomotion,
                    &[&[from], &[to]],
                    &[0.5],
                    dt,
                    0.7,
                );
                roughness(&shown, dt, 0.5, 0.6)
            };
            let ([spin, speed], [fine_spin, fine_speed]) = (rough(1. / 240.), rough(1. / 960.));
            assert!(
                fine_spin < 0.4 * spin && fine_speed < 0.4 * speed,
                "{method:?}: velocity changes {spin} and {speed} at 240 fps, {fine_spin} and \
                 {fine_speed} at 960"
            );
        }
        // The same switch as a cut jumps 70° in a frame.
        let shown = play(
            Method::Off,
            Switch::Locomotion,
            &[&[from], &[to]],
            &[0.5],
            1. / 60.,
            1.,
        );
        assert!(angle(shown[29][0].0, shown[30][0].0) > 1.);
    }

    #[test]
    fn offsets_settle_in_a_few_half_lives() {
        let half_life = Switch::Recover.half_life();
        let still = |angle: f32| Steady::turning(Quat::from_rotation_x(angle), Vec3::ZERO);
        let dt = 1. / 240.;
        let shown = play(
            Method::Inertialize,
            Switch::Recover,
            &[&[still(FRAC_PI_2)], &[still(0.)]],
            &[0.1],
            dt,
            1.,
        );
        let left = |half_lives: f32| {
            let frame = ((0.1 + half_lives * half_life) / dt).round() as usize;
            angle(shown[frame][0].0, Quat::IDENTITY).to_degrees()
        };
        assert!(left(1.) > 45., "about 60% is left after one half-life");
        assert!(
            left(3.) < 9.,
            "a 90° jump is under 10% after three half-lives"
        );
        assert!(left(5.) < 1., "and under 1° after five");
        assert_eq!(left(8.), 0., "and settled after eight");
    }

    #[test]
    fn three_switches_inside_100ms_stay_continuous() {
        // Each clip comes in about 50° from where the one before it would be.
        let switches = [0.5, 0.533, 0.567];
        let clips = [
            Steady::turning(Quat::IDENTITY, Vec3::new(0., 5., 0.)),
            Steady::through(
                Quat::from_rotation_x(0.9),
                Vec3::new(-4., 0., 0.),
                switches[0],
            ),
            Steady::through(
                Quat::from_rotation_z(-0.8),
                Vec3::new(0., 0., 6.),
                switches[1],
            ),
            Steady::through(
                Quat::from_rotation_y(-0.6),
                Vec3::new(3., 3., 0.),
                switches[2],
            ),
        ];
        let clips = clips.each_ref().map(std::slice::from_ref);
        let dt = 1. / 60.;
        let steps = |shown: &[Vec<(Quat, Vec3)>]| -> Vec<f32> {
            shown
                .windows(2)
                .map(|w| angle(w[0][0].0, w[1][0].0))
                .collect()
        };
        let cut = steps(&play(
            Method::Off,
            Switch::Action,
            &clips,
            &switches,
            dt,
            1.,
        ));
        let pop = cut.iter().copied().fold(0., f32::max);
        for method in BLENDING {
            for kind in [Switch::Action, Switch::Locomotion] {
                let shown = play(method, kind, &clips, &switches, dt, 1.5);
                let steps = steps(&shown);
                // Each switch frame carries on the motion shown the frame before.
                for at in [30, 32, 34] {
                    assert!(
                        (steps[at - 1] - steps[at - 2]).abs() < 1e-3,
                        "{method:?} {kind:?}: frame {at} turned {} after {}",
                        steps[at - 1],
                        steps[at - 2]
                    );
                }
                let worst = steps.iter().copied().fold(0., f32::max);
                assert!(
                    worst < pop / 2.,
                    "{method:?} {kind:?}: a frame turned {worst} rad, cuts pop {pop}"
                );
                let last = shown.last().unwrap()[0].0;
                assert!(
                    angle(last, clips[3][0].at(1.5 - dt).0) < 1e-3,
                    "and it settles"
                );
                let rough = |dt: f32| {
                    roughness(
                        &play(method, kind, &clips, &switches, dt, 0.8),
                        dt,
                        0.5,
                        0.67,
                    )[0]
                };
                let (spin, fine) = (rough(1. / 240.), rough(1. / 960.));
                assert!(
                    fine < 0.4 * spin,
                    "{method:?} {kind:?}: velocity changes {spin} at 240 fps, {fine} at 960"
                );
            }
        }
    }

    #[test]
    fn bones_a_switch_leaves_alone_carry_on_untouched() {
        // The legs keep jogging while the arm switches clips.
        let [arm, swung] = swing();
        let leg = Steady {
            start: Quat::from_rotation_x(0.3),
            spin: Vec3::new(6., 0., 0.),
            origin: Vec3::new(0., 0.9, 0.),
            velocity: Vec3::new(0., 0.2, 0.),
        };
        let dt = 1. / 60.;
        for method in BLENDING {
            let shown = play(
                method,
                Switch::Layer,
                &[&[arm, leg], &[swung, leg]],
                &[0.5],
                dt,
                1.,
            );
            for (frame, pose) in shown.iter().enumerate() {
                let (rotation, translation) = leg.at(frame as f32 * dt);
                assert!(
                    angle(pose[1].0, rotation) < 1e-3 && pose[1].1.distance(translation) < 1e-4,
                    "{method:?}: the leg strayed at frame {frame}"
                );
            }
        }
    }

    #[test]
    fn a_settled_blender_shows_the_clips_as_they_are() {
        let [from, to] = swing();
        for method in BLENDING {
            let mut blender = Blender::new(method);
            let dt = 1. / 60.;
            for frame in 0..120 {
                let t = frame as f32 * dt;
                let clip = if frame < 30 { from } else { to };
                let mut pose = [clip.at(t)];
                blender.update(&mut pose, (frame == 30).then_some(Switch::Action), dt);
                if frame > 70 {
                    assert!(!blender.blending(), "{method:?} still blending at {frame}");
                    assert_eq!(pose[0], clip.at(t));
                }
            }
        }
    }

    #[test]
    fn the_first_frame_and_a_new_skeleton_have_nothing_to_blend_from() {
        let mut blender = Blender::new(Method::Inertialize);
        let mut pose = [(Quat::from_rotation_y(1.), Vec3::X)];
        blender.update(&mut pose, Some(Switch::Action), 1. / 60.);
        assert_eq!(pose[0], (Quat::from_rotation_y(1.), Vec3::X));
        let mut bigger = [(Quat::IDENTITY, Vec3::ZERO); 3];
        blender.update(&mut bigger, Some(Switch::Action), 1. / 60.);
        assert_eq!(bigger, [(Quat::IDENTITY, Vec3::ZERO); 3]);
        assert!(!blender.blending());
    }
}
