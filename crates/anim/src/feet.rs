//! Feet on the ground (#214): foot placement for full-detail characters, as Unreal's foot
//! placement and the Game Animation Sample's leg IK do it. The clips are made on flat ground and
//! play at the root, which the sim keeps on the ground under the middle of the body, so on a
//! slope, a terrace or the bridge's ramps one foot would sink and the other float.
//!
//! Each foot finds the ground under it and the slope there, keeps the height above it that the
//! clip gives it (flat on it when planted, lifted mid-step), measured square to the slope, and
//! turns to the slope up to [`MAX_TILT`]. The pelvis drops on a spring as far as the lower foot
//! needs, and further while a planted leg couldn't otherwise reach its foot; then two-bone IK
//! ([`crate::ik`]) bends each leg to its foot.
//!
//! A foot planted in its stride ([`crate::Step::planted`]) holds still in the world from touching
//! down until it lifts off. The ball of the foot is what holds, so the heel still rises as the clip
//! rolls over it; letting go, the foot eases from where it was held back onto the clip. Slower or
//! faster than the loops can play ([`crate::locomotion::PACE`]), the steps shorten or lengthen
//! along the stride instead ([`crate::Step::warp`], stride warping), so the feet keep up with the
//! body.
use crate::locomotion::Spring;
use glam::{Quat, Vec2, Vec3, Vec3Swizzles};
use std::f32::consts::FRAC_PI_6;

/// The furthest a foot turns to the slope under it, in radians (30°).
pub const MAX_TILT: f32 = FRAC_PI_6;
/// How far the pelvis drops at most, and how far a foot rises or falls from where the clip has
/// it, as shares of the leg's length.
pub const MAX_DROP: f32 = 0.35;
pub const MAX_SHIFT: f32 = 0.45;
/// Half-life of the pelvis drop's spring, in seconds.
pub const DROP_HALF_LIFE: f32 = 0.05;
/// How straight a planted leg may get before the pelvis drops further for it, as a share of its
/// length, and the fastest it drops for it, in metres a second, so that it never jumps.
pub const REACH: f32 = 0.998;
pub const REACH_RATE: f32 = 2.;
/// Half-life, in seconds, of the gap a foot leaves when it lets go of where it was held, and of a
/// jump in the ground under a foot in the air (a step, a ledge, the end of the bridge): it dies
/// away on a spring, slowly at first, so the foot doesn't jump after the clip.
pub const EASE_HALF_LIFE: f32 = 0.05;
/// Half-life, in seconds, of the slope a foot turns to following the slope under it, so that it
/// turns smoothly where the ground bends (the foot or the top of a slope).
pub const TILT_HALF_LIFE: f32 = 0.04;
/// The ground rising or falling more than this under a foot between updates, beyond its slope, in
/// metres, is a step; so is the body rising or falling that much more than the slope under it.
const STEP: f32 = 0.03;
/// A planted foot holds once it is this close to the ground, in metres: easing down from a step,
/// it touches down when it gets there.
const TOUCH: f32 = 0.02;
/// A planted foot holds once the clip has the ball of the foot within [`DOWN`] metres of its
/// height planted ([`Contact::floors`]), not yet as the heel strikes with the toes up, and lets
/// go once it is [`UP`] above it as the toes push off: in between, the clips' planted feet jitter
/// up and down.
pub const DOWN: f32 = 0.012;
pub const UP: f32 = 0.025;
/// A held foot lets go too once the clip carries the ball of the foot along the body's travel at
/// more than this share of the body's speed: the toes pushing off, before they rise far. Held on
/// longer, the foot would be left far behind and race after the clip.
pub const LEAVE: f32 = 0.5;
/// How fast a held foot settles onto the ground beneath it, in metres a second, should it touch
/// down above or below it (easing in from a step).
const SETTLE_RATE: f32 = 0.4;
/// Seconds that placement takes to fade in or out, easing in and out.
pub const FADE: f32 = 0.15;
/// A held foot further than this share of its leg's length from where the clip has it lets go.
pub const STRETCH: f32 = 0.25;
/// The body moving further than this between updates, in metres, jumped (a teleport): the feet
/// let go at once.
pub const JUMP: f32 = 1.5;
/// The body further than this above the ground under it, in metres, is in the air or afloat.
pub const AIRBORNE: f32 = 0.2;
/// The shortest and longest that stride warping makes the steps, as shares of the clips'.
pub const WARP: [f32; 2] = [0.5, 1.6];
/// How far either side of a foot the slope is measured, in metres: about half a foot's length.
const SPAN: f32 = 0.1;

/// One leg as the clip has it this frame, in the model's frame (+y up, with the flat ground the
/// clips were made on at y = 0): its hip, knee, ankle and ball joints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    pub hip: Vec3,
    pub knee: Vec3,
    pub ankle: Vec3,
    pub ball: Vec3,
}

/// How the feet meet the ground this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// Whether each foot (left, right) is planted in its stride, to hold still in the world while
    /// the ball of the foot is down, and how high the clip has the ball of each planted foot, in
    /// the model's frame ([`crate::Step::floors`]).
    pub planted: [bool; 2],
    pub floors: [f32; 2],
    /// The line the legs stride along, in the model's frame (a unit vector), and how much longer
    /// than the clips' the steps must be along it ([`crate::Step::warp`]).
    pub stride: Vec3,
    pub warp: f32,
}

impl Contact {
    /// Standing or acting on the spot: on the ground, with nothing held.
    pub const STILL: Contact = Contact {
        planted: [false; 2],
        floors: [0.; 2],
        stride: Vec3::Z,
        warp: 1.,
    };
}

/// Where one leg goes this frame, in the model's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    /// Where the ankle reaches to, from the hip as dropped with the pelvis.
    pub ankle: Vec3,
    /// The turn to give the foot, on the left of its rotation in the model's frame.
    pub tilt: Quat,
}

/// What foot placement does to the pose this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// How far the pelvis drops, in metres (0 or less).
    pub drop: f32,
    /// The left leg and the right.
    pub legs: [Placed; 2],
}

/// One character's feet on the ground: the pelvis drop, each foot's hold, and how much of it all
/// shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feet {
    /// How far the pelvis drops below where the clip has it, in metres (0 or less).
    drop: Spring,
    /// How much of the placement shows, fading in on the ground and out off it.
    weight: f32,
    feet: [Foot; 2],
    /// Where the body was last update, and the slope under it, to tell a jump (a teleport) or a
    /// step up or down from moving.
    last: Option<(Vec3, Vec2)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Foot {
    /// Where the ball of the foot is held in the world while planted, and the height it settles
    /// to on the ground there.
    held: Option<(Vec3, f32)>,
    /// How far the ankle is from where it should be, in the world, dying away on a spring: the
    /// gap left by letting go of where it was held, or by a step in the ground.
    gap: Spring<Vec3>,
    /// Let go while still planted, stretched too far: it holds again only once it has lifted.
    stretched: bool,
    /// Where the clip had the ball of the foot last update, stride warped, in the model's frame:
    /// to tell how fast the clip carries it over the ground.
    ball: Option<Vec3>,
    /// The ground under the ankle last update, and its slope there (rise over run along x and z),
    /// to spot steps; and the slope the foot turns to, following it.
    ground: Option<(Vec3, Vec2)>,
    slope: Vec2,
    /// Where the ankle goes from where the clip has it, in the world, and the turn to the slope:
    /// kept as they were while off the ground, fading out.
    offset: Vec3,
    tilt: Quat,
}

impl Feet {
    /// Places the feet for one frame of `dt` seconds: the body at `root` turned `yaw` round +y,
    /// its `legs` (left, right) as the clips have them, and the `ground`'s height at any (x, z).
    /// `contact` is `None` in the air, swimming, rolling or below full detail, which fades the
    /// placement out; the body far enough above the ground ([`AIRBORNE`]) counts as in the air
    /// too. Returns `None` once it has faded out: the pose stays as the clips have it.
    pub fn update(
        &mut self,
        root: Vec3,
        yaw: f32,
        legs: [Leg; 2],
        contact: Option<Contact>,
        ground: impl Fn(f32, f32) -> f32,
        dt: f32,
    ) -> Option<Placement> {
        let (under, slope) = sample(&ground, root.x, root.z);
        let moved = self
            .last
            .replace((root, slope))
            .map(|(last, s)| (root - last, s));
        let jumped = moved.is_some_and(|(by, _)| by.length() > JUMP);
        let contact = contact.filter(|_| root.y - under < AIRBORNE);
        if let (Some((by, s)), Some(_), false) = (moved, contact, jumped) {
            // The body stepped up or down: the pelvis stays where it was in the world and
            // settles from there, rather than dipping twice.
            let step = by.y - s.dot(by.xz());
            if step.abs() > STEP {
                self.drop.value -= step;
            }
        }
        let fade = if contact.is_some() { dt } else { -dt } / FADE;
        self.weight = (self.weight + fade).clamp(0., 1.);
        if self.weight == 0. {
            *self = Feet {
                last: self.last,
                ..Feet::default()
            };
            return None;
        }
        let turn = Quat::from_rotation_y(yaw);
        let travel = moved.map_or(Vec3::ZERO, |(by, _)| by.with_y(0.));
        let lengths = legs.map(|l| l.hip.distance(l.knee) + l.knee.distance(l.ankle));
        for (i, ((leg, foot), length)) in legs.iter().zip(&mut self.feet).zip(lengths).enumerate() {
            if jumped {
                *foot = Foot::default();
            }
            let clip = root + turn * leg.ankle;
            let Some(contact) = contact else {
                // Off the ground: nothing holds, and the feet keep where they were going.
                foot.held = None;
                foot.ground = None;
                foot.ball = None;
                continue;
            };
            // Stride warping moves the whole foot along the stride, from the root.
            let warp = contact.warp.clamp(WARP[0], WARP[1]);
            let along = contact.stride * (leg.ankle.dot(contact.stride) * (warp - 1.));
            let at = clip + turn * along;
            // How far the clip carried the ball of the foot over the ground (leaving out the
            // body's turn, which holding is there to undo), and whether it is still down.
            let ball = leg.ball + along;
            let carried = foot
                .ball
                .replace(ball)
                .map_or(Vec3::ZERO, |last| travel + turn * (ball - last));
            let leaving = carried.dot(travel) > LEAVE * travel.length_squared();
            let lift = if foot.held.is_some() { UP } else { DOWN };
            let floor = contact.floors[i];
            let planted = contact.planted[i] && leg.ball.y < floor + lift && !leaving;
            let (g, slope) = sample(&ground, at.x, at.z);
            match foot.ground {
                Some((last, s)) => {
                    // The ground jumped under the foot: it eases onto the new height.
                    let jump = g - last.y - s.dot(at.xz() - last.xz());
                    if jump.abs() > STEP {
                        foot.gap.value.y -= jump;
                    }
                }
                None => foot.slope = slope,
            }
            foot.ground = Some((Vec3::new(at.x, g, at.z), slope));
            foot.slope += (slope - foot.slope) * (1. - (-dt / TILT_HALF_LIFE).exp2());
            foot.tilt = tilt(foot.slope);
            let up = foot.tilt * Vec3::Y;
            // The height the clip gives the ankle above its flat ground, square to the slope.
            let shift = MAX_SHIFT * length;
            let height = (g + leg.ankle.y / up.y).clamp(clip.y - shift, clip.y + shift);
            let free = Vec3::new(at.x, height, at.z);
            let ball = foot.tilt * (turn * (leg.ball - leg.ankle));
            if !planted {
                foot.stretched = false;
            }
            let target = match foot.held {
                Some((mut held, rest)) => {
                    let most = SETTLE_RATE * dt;
                    held.y += (rest - held.y).clamp(-most, most);
                    let ankle = held - ball;
                    let stretched = ankle.distance(free) > STRETCH * length;
                    if planted && !stretched {
                        foot.held = Some((held, rest));
                    } else {
                        foot.held = None;
                        foot.stretched = stretched;
                        foot.gap.snap(ankle - free);
                    }
                    ankle
                }
                None if planted && !foot.stretched && foot.gap.value.length() < TOUCH => {
                    // Touching down: it holds just where it is, and settles onto the ground.
                    let ankle = free + foot.gap.value;
                    let under = ankle + ball;
                    let rest = ground(under.x, under.z) + floor / up.y;
                    foot.held = Some((under, rest));
                    foot.gap = Spring::default();
                    ankle
                }
                None => free + foot.gap.update(Vec3::ZERO, EASE_HALF_LIFE, dt),
            };
            foot.offset = target - clip;
        }
        // The pelvis drops as far as the lower foot goes, on its spring...
        let deepest = -MAX_DROP * (lengths[0] + lengths[1]) / 2.;
        let lowest = self.feet[0].offset.y.min(self.feet[1].offset.y);
        let was = self.drop.value;
        self.drop
            .update(lowest.clamp(deepest, 0.), DROP_HALF_LIFE, dt);
        // ...and further, quickly, while a planted leg is too straight to reach its foot.
        for ((leg, foot), length) in legs.iter().zip(&self.feet).zip(lengths) {
            if foot.held.is_none() {
                continue;
            }
            let hip = root + turn * leg.hip;
            let target = root + turn * leg.ankle + foot.offset;
            let [flat, reach] = [(target - hip).xz().length_squared(), REACH * length];
            if flat < reach * reach {
                let need = target.y - hip.y + (reach * reach - flat).sqrt();
                let to = need.max(was - REACH_RATE * dt).max(deepest);
                if to < self.drop.value {
                    self.drop.value = to;
                    self.drop.velocity = self.drop.velocity.min((to - was) / dt.max(1e-4));
                }
            }
        }
        let w = self.weight * self.weight * (3. - 2. * self.weight);
        let back = turn.inverse();
        Some(Placement {
            drop: self.drop.value * w,
            legs: [0, 1].map(|i| Placed {
                ankle: legs[i].ankle + back * self.feet[i].offset * w,
                tilt: Quat::IDENTITY.slerp(back * self.feet[i].tilt * turn, w),
            }),
        })
    }

    /// How much of the placement shows: 1 on the ground at full detail, fading to 0 off it (before
    /// easing).
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// Whether each foot (left, right) is held where it was planted.
    pub fn held(&self) -> [bool; 2] {
        self.feet.map(|f| f.held.is_some())
    }
}

/// The ground's height at (x, z) and its slope there (rise over run along x and z). Along each
/// axis the slope is the gentler side's, or level where the two sides disagree, so a step or the
/// foot of a wall doesn't tip the foot over.
fn sample(ground: &impl Fn(f32, f32) -> f32, x: f32, z: f32) -> (f32, Vec2) {
    let g = ground(x, z);
    let gentler = |a: f32, b: f32| {
        if a * b <= 0. {
            0.
        } else if a.abs() < b.abs() {
            a
        } else {
            b
        }
    };
    let slope = Vec2::new(
        gentler(ground(x + SPAN, z) - g, g - ground(x - SPAN, z)),
        gentler(ground(x, z + SPAN) - g, g - ground(x, z - SPAN)),
    ) / SPAN;
    (g, slope)
}

/// The turn that stands a foot on ground of `slope` (rise over run along x and z), up to
/// [`MAX_TILT`].
fn tilt(slope: Vec2) -> Quat {
    let normal = Vec3::new(-slope.x, 1., -slope.y).normalize();
    let axis = Vec3::Y.cross(normal);
    let sin = axis.length();
    if sin < 1e-6 {
        return Quat::IDENTITY;
    }
    Quat::from_axis_angle(axis / sin, sin.atan2(normal.y).min(MAX_TILT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ik::two_bone;
    use std::f32::consts::{FRAC_PI_2, PI};

    const DT: f32 = 1. / 60.;
    /// A leg like the male body's.
    const THIGH: f32 = 0.43;
    const CALF: f32 = 0.45;
    const HIP: f32 = 0.88;
    /// The sole's heel and toe and the ball joint, from the ankle, the foot flat as the clips have
    /// it planted: the sole on the clip's ground with the ankle 8 cm up.
    const HEEL: Vec3 = Vec3::new(0., -0.08, -0.06);
    const TOE: Vec3 = Vec3::new(0., -0.08, 0.17);
    const BALL: Vec3 = Vec3::new(0., -0.06, 0.13);
    /// A walk like the clips' in the model's frame: a cycle a second, each foot planted for 60% of
    /// it sliding back at 1 m/s, the left touching down at phase 0 and the right at 0.5.
    const SPEED: f32 = 1.;
    const STANCE: f32 = 0.6;

    /// Where the knee of a leg from `hip` to `ankle` is, bending forward.
    fn knee(hip: Vec3, ankle: Vec3) -> Vec3 {
        let d = hip.distance(ankle).min(THIGH + CALF - 1e-4);
        let line = (ankle - hip).normalize();
        let cos = (THIGH * THIGH + d * d - CALF * CALF) / (2. * THIGH * d);
        let ahead = (Vec3::Z - line * line.z).normalize();
        hip + line * THIGH * cos + ahead * THIGH * (1. - cos * cos).max(0.).sqrt()
    }

    /// The walk's legs (left, right) at `phase`, and whether each foot is planted: the planted
    /// feet jitter up to `bounce` metres off the ground, as the run clips' do.
    fn legs(phase: f32, bounce: f32) -> ([Leg; 2], [bool; 2]) {
        let leg = |side: usize| {
            let x = if side == 0 { 0.1 } else { -0.1 };
            let since = (phase - side as f32 * 0.5).rem_euclid(1.);
            let reach = SPEED * STANCE / 2.;
            let (z, lift) = if since < STANCE {
                (reach - SPEED * since, bounce * (since * 50.).sin().abs())
            } else {
                let swing = (since - STANCE) / (1. - STANCE);
                (-reach + 2. * reach * swing, 0.12 * (swing * PI).sin())
            };
            let ankle = Vec3::new(x, 0.08 + lift, z);
            let hip = Vec3::new(x * 0.9, HIP, 0.);
            let leg = Leg {
                hip,
                knee: knee(hip, ankle),
                ankle,
                ball: ankle + BALL,
            };
            (leg, since < STANCE)
        };
        let [(left, l), (right, r)] = [leg(0), leg(1)];
        ([left, right], [l, r])
    }

    /// What a frame shows: the pelvis drop, and per foot whether the clip has it planted and where
    /// its heel, toe and ball are in the world.
    #[derive(Debug)]
    struct Shown {
        drop: f32,
        planted: [bool; 2],
        heel: [Vec3; 2],
        toe: [Vec3; 2],
        ball: [Vec3; 2],
    }

    /// A body walking the clip over the ground, facing `yaw`, its planted feet jittering up to
    /// `bounce` metres off the ground.
    struct Walker {
        root: Vec3,
        yaw: f32,
        phase: f32,
        bounce: f32,
        feet: Feet,
    }

    impl Walker {
        fn new(ground: impl Fn(f32, f32) -> f32, yaw: f32) -> Self {
            Walker {
                root: Vec3::new(0.3, ground(0.3, 0.2), 0.2),
                yaw,
                phase: 0.,
                bounce: 0.,
                feet: Feet::default(),
            }
        }

        /// One frame: the body goes `speed` metres a second the way it faces, turning `spin`
        /// radians a second, while the clip plays on at its own pace. `hold` says whether the
        /// planted feet hold, `warp` how much longer the steps are, and `on` whether the body is
        /// on the ground.
        fn step(
            &mut self,
            ground: impl Fn(f32, f32) -> f32 + Copy,
            speed: f32,
            spin: f32,
            hold: bool,
            warp: f32,
            on: bool,
        ) -> Shown {
            self.yaw += spin * DT;
            let turn = Quat::from_rotation_y(self.yaw);
            self.root += turn * Vec3::Z * speed * DT;
            self.root.y = ground(self.root.x, self.root.z);
            self.phase = (self.phase + DT).rem_euclid(1.);
            let (legs, planted) = legs(self.phase, self.bounce);
            let contact = on.then_some(Contact {
                planted: planted.map(|p| p && hold),
                floors: [0.08 + BALL.y; 2],
                stride: Vec3::Z,
                warp,
            });
            let placement = self
                .feet
                .update(self.root, self.yaw, legs, contact, ground, DT);
            let mut shown = Shown {
                drop: placement.map_or(0., |p| p.drop),
                planted,
                heel: [Vec3::ZERO; 2],
                toe: [Vec3::ZERO; 2],
                ball: [Vec3::ZERO; 2],
            };
            for (s, leg) in legs.iter().enumerate() {
                // The leg bends to its foot with the hip dropped, as the character does it.
                let (ankle, tilt) = match placement {
                    Some(p) => {
                        let drop = Vec3::Y * p.drop;
                        let [hip, knee, ankle] =
                            [leg.hip + drop, leg.knee + drop, leg.ankle + drop];
                        let line = (ankle - hip).normalize();
                        let pole = (knee - hip) - line * (knee - hip).dot(line);
                        let (upper, lower) = two_bone(hip, knee, ankle, p.legs[s].ankle, pole);
                        let knee_now = hip + upper * (knee - hip);
                        (knee_now + lower * (ankle - knee), p.legs[s].tilt)
                    }
                    None => (leg.ankle, Quat::IDENTITY),
                };
                let at = |offset: Vec3| self.root + turn * (ankle + tilt * offset);
                shown.heel[s] = at(HEEL);
                shown.toe[s] = at(TOE);
                shown.ball[s] = at(BALL);
            }
            shown
        }
    }

    fn slope(rise: f32) -> impl Fn(f32, f32) -> f32 + Copy {
        move |_: f32, z: f32| rise * z
    }

    #[test]
    fn on_a_30_degree_slope_the_soles_meet_the_ground_through_each_contact() {
        let rise = MAX_TILT.tan();
        let ground = slope(rise);
        // Up, down, across both ways and slantwise.
        for yaw in [0., PI, FRAC_PI_2, -FRAC_PI_2, 0.6, 2.5] {
            let mut walker = Walker::new(ground, yaw);
            let mut worst = 0f32;
            for frame in 0..300 {
                let shown = walker.step(ground, SPEED, 0., true, 1., true);
                for s in (0..2).filter(|&s| frame >= 30 && shown.planted[s]) {
                    for p in [shown.heel[s], shown.toe[s]] {
                        // Square to the slope.
                        let off = (p.y - ground(p.x, p.z)) * MAX_TILT.cos();
                        worst = worst.max(off.abs());
                    }
                }
            }
            assert!(
                worst < 0.01,
                "facing {yaw}: a sole {worst} m off the ground"
            );
        }
    }

    #[test]
    fn a_held_foot_moves_under_2_mm_through_its_contact() {
        // The body going faster or slower than the clip and turning, so that its planted feet
        // would skate: on level ground, up a gentle slope and on a 30° one; and on level ground
        // with the clip's planted feet jittering 2 cm up and down, as the run clips' do, which
        // mustn't let go of them.
        for (rise, pace, spin, bounce) in [
            (0., 1.15, 0.5, 0.),
            (0.3, 0.85, -0.5, 0.),
            (MAX_TILT.tan(), 1.1, 0.3, 0.),
            (0., 1.15, -0.3, 0.02),
        ] {
            let ground = slope(rise);
            let mut walker = Walker::new(ground, 0.4);
            walker.bounce = bounce;
            let mut starts: [Option<Vec3>; 2] = [None; 2];
            let [mut worst, mut settled] = [0f32; 2];
            let [mut planted, mut holding] = [0; 2];
            for frame in 0..400 {
                let shown = walker.step(ground, SPEED * pace, spin, true, 1., true);
                for (s, start) in starts.iter_mut().enumerate() {
                    if frame < 30 || !shown.planted[s] {
                        *start = None;
                        continue;
                    }
                    // From touching down (the jittering ball once it is near enough the ground)
                    // until the clip lifts the foot, whether or not it still holds.
                    planted += 1;
                    if start.is_none() && walker.feet.feet[s].held.is_none() {
                        continue;
                    }
                    holding += 1;
                    let from = *start.get_or_insert(shown.ball[s]);
                    worst = worst.max(shown.ball[s].xz().distance(from.xz()));
                    settled = settled.max((shown.ball[s].y - from.y).abs());
                }
            }
            // Touching down above the ground, it settles onto it.
            let settle = if bounce > 0. { DOWN } else { 0. };
            assert!(
                worst < 0.002 && settled < 0.002 + settle,
                "rise {rise}, bounce {bounce}: a held foot moved {worst} m, and {settled} m up or down"
            );
            assert!(
                holding as f32 > 0.85 * planted as f32,
                "rise {rise}, bounce {bounce}: held {holding} of {planted} planted frames"
            );
        }
    }

    #[test]
    fn the_pelvis_drop_is_continuous() {
        // Level ground, up a 30° slope, over its top, down a ledge 30 cm high and on; then
        // standing, off the ground (a jump), landing and walking off.
        let ground = |_: f32, z: f32| {
            let up = (z - 1.5).clamp(0., 2.) * MAX_TILT.tan();
            up - if z > 5. { 0.3 } else { 0. }
        };
        let mut walker = Walker::new(ground, 0.);
        let mut last = walker.step(ground, SPEED, 0., true, 1., true);
        let mut pelvis = walker.root.y + last.drop;
        let mut deepest = 0f32;
        for frame in 0..720 {
            let (speed, on) = match frame {
                0..420 => (SPEED, true),
                420..480 => (0., true),
                480..520 => (0., false),
                _ => (SPEED, true),
            };
            let shown = walker.step(ground, speed, 0., speed > 0., 1., on);
            // The pelvis, dropped, moves no faster than 2 m/s, though the body drops 30 cm at
            // once at the ledge: there the drop takes up the step, and settles from there.
            let now = walker.root.y + shown.drop;
            let change = (now - pelvis).abs();
            assert!(
                change < 0.035,
                "frame {frame}: the pelvis jumped {change} m"
            );
            let step = (walker.root.z - 5.).abs() < 0.01;
            assert!(
                step || (shown.drop - last.drop).abs() < 0.035,
                "frame {frame}: the drop jumped from {} to {}",
                last.drop,
                shown.drop
            );
            pelvis = now;
            for s in 0..2 {
                // Nor do the feet jump, at the ledge or anywhere else: a foot swinging forward
                // and up the slope goes about 5 cm a frame itself.
                let moved = shown.ball[s].distance(last.ball[s]);
                assert!(moved < 0.08, "frame {frame}: foot {s} jumped {moved} m");
            }
            deepest = deepest.min(shown.drop);
            last = shown;
        }
        // It did drop: on the slope the lower foot is well below the body.
        assert!(deepest < -0.1, "{deepest}");
        assert!(deepest >= -MAX_DROP * (THIGH + CALF));
    }

    #[test]
    fn slower_than_the_clips_play_the_steps_shorten_so_the_feet_keep_up() {
        // At 60% of the clip's speed, steps 60% as long slide back as fast as the body goes: a
        // planted foot stays where it is without being held.
        let ground = slope(0.);
        let mut walker = Walker::new(ground, 0.8);
        let mut last = walker.step(ground, 0.6 * SPEED, 0., false, 0.6, true);
        for frame in 0..240 {
            let shown = walker.step(ground, 0.6 * SPEED, 0., false, 0.6, true);
            for s in (0..2).filter(|&s| frame > 30 && shown.planted[s] && last.planted[s]) {
                let moved = shown.ball[s].distance(last.ball[s]);
                assert!(moved < 1e-3, "frame {frame}: foot {s} slid {moved} m");
            }
            last = shown;
        }
    }

    #[test]
    fn off_the_ground_placement_fades_out_and_comes_back_on_landing() {
        let ground = slope(MAX_TILT.tan());
        let mut walker = Walker::new(ground, 0.);
        for _ in 0..60 {
            walker.step(ground, SPEED, 0., true, 1., true);
        }
        assert_eq!(walker.feet.weight(), 1.);
        // Off the ground (in the air, rolling, below full detail), it fades out over FADE.
        let frames = (FADE / DT).ceil() as usize;
        for k in 0..frames + 1 {
            let shown = walker.step(ground, SPEED, 0., true, 1., false);
            assert_eq!(shown.drop == 0., k == frames, "{k}");
        }
        assert_eq!(walker.feet.weight(), 0.);
        // Well above the ground counts as off it, whatever the caller says.
        let (legs, planted) = legs(0.2, 0.);
        let contact = Contact {
            planted,
            ..Contact::STILL
        };
        let mut feet = Feet::default();
        for _ in 0..frames + 1 {
            assert_eq!(
                feet.update(
                    Vec3::new(0., 1., 0.),
                    0.,
                    legs,
                    Some(contact),
                    slope(0.),
                    DT
                ),
                None
            );
        }
        // Back on the ground, it fades in again.
        for k in 1..=frames {
            walker.step(ground, SPEED, 0., true, 1., true);
            assert!((walker.feet.weight() - (k as f32 * DT / FADE).min(1.)).abs() < 1e-4);
        }
    }

    #[test]
    fn a_jump_lets_go_of_the_feet_at_once() {
        let ground = slope(0.);
        let mut walker = Walker::new(ground, 0.);
        for _ in 0..40 {
            walker.step(ground, SPEED * 1.15, 0., true, 1., true);
        }
        assert!(walker.feet.feet.iter().any(|f| f.held.is_some()));
        // Taken 10 m away, nothing is held back where it was.
        walker.root.z += 10.;
        let shown = walker.step(ground, SPEED, 0., true, 1., true);
        for s in 0..2 {
            assert!(shown.ball[s].distance(walker.root) < 1., "{shown:?}");
        }
    }
}
