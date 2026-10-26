//! Climbing (#248): the climbing clips (#247) played by how far the sim has carried the body
//! through each move, with the hands on the hold and the feet on the wall below it.
//!
//! The clips are made on the sim's own paths (`art/blender/build_clips.py`): each key is planned
//! against the hold where it is in the world and turned into the frame of the root, which the sim
//! carries `k` of the way through its move at `k` of the clip ([`carry`]). So a clip played at the
//! share of its move the body has come ([`progress`]) keeps the hands on the hold however the
//! body's positions arrive, and a shimmy played by the distance slid along the wall
//! ([`SHIMMY_STRIDE`]) keeps them still at any speed.
//!
//! They're made on the broad human frame, which hangs with its wrists [`GRIP_DOWN`] below the hold
//! and [`GRIP_OUT`] out from the wall. Other frames reach higher or lower, nearer the wall or
//! further from it, so each is moved by the difference, measured from its own hang ([`reach`]);
//! a hold too low to hang from at full stretch lowers it further ([`raise`]). Two-bone IK then
//! closes the hands on the hold ([`grip`]) and braces the feet on the wall ([`brace`]) wherever
//! they are.
use glam::Vec3;

use crate::traverse::SHORTEST;

/// The sim hangs a body with its hands this far above its feet (`sim::climbing::HANG_DROP`), and
/// a player's feet this far out from the wall's face (their radius): the clips are made so.
pub const HANG_DROP: f32 = 1.9;
pub const FACE: f32 = 0.4;
/// A gripping wrist is this far out from the wall's face and below the hold's top, as the clips
/// close the hands on it (`edge_grip`).
pub const GRIP_OUT: f32 = 0.07;
pub const GRIP_DOWN: f32 = 0.03;
/// As the clips lift a hand off the hold, they bring it out from the wall as far, up to this.
pub const LIFT: f32 = 0.08;
/// A braced foot's ball rests this far out from the face, as the broad human's do in the hang.
pub const BALL_OUT: f32 = 0.01;
/// Each loop of the shimmy clips slides this far along the wall: the sim's shimmy speed
/// (`sim::climbing::SHIMMY_SPEED`, 1.2 m/s) over the loop's 0.4 s.
pub const SHIMMY_STRIDE: f32 = 0.48;
/// A hand holds firmly with the clip's wrist within this of where it grips, and lets go by
/// [`LET_GO`]: a hand on its way between holds plays as the clip has it.
pub const HOLD: f32 = 0.1;
pub const LET_GO: f32 = 0.25;
/// A body hanging from a hold is moved by its reach this quickly, halving what's left every this
/// many seconds, and moved back as quickly once off the wall.
pub const RAISE_HALF_LIFE: f32 = 0.05;
/// The most a frame is raised or lowered to reach the hold, and moved in toward the wall or out
/// from it, in metres.
pub const MOST_RAISE: f32 = 0.6;
pub const MOST_IN: f32 = 0.1;

/// Where the sim has a body `k` of the way through a move from `from` to `to` onto a hold, or from
/// a hang onto the top (`sim::climbing::carry`): lowering itself, out over the edge before it
/// drops; climbing, up before it goes in over the edge; across, both together, easing out.
pub fn carry(from: Vec3, to: Vec3, k: f32) -> Vec3 {
    let ease = |k: f32| 1. - (1. - k) * (1. - k);
    let k = k.clamp(0., 1.);
    let (across, up) = if to.y < from.y - 0.5 {
        (ease((k * 1.6).min(1.)), k * k)
    } else if to.y > from.y + 0.3 {
        (k * k, ease((k * 1.6).min(1.)))
    } else {
        (ease(k), ease(k))
    };
    let p = from.lerp(to, across);
    Vec3::new(p.x, from.y + (to.y - from.y) * up, p.z)
}

/// How far through a move from `from` to `to` a body at `at` is, from 0 to 1: where the move's
/// path ([`carry`]) passes nearest it, seen from above for a move mostly across (so a hop in the
/// middle, as an eject has, doesn't count). `None` if the move is too short to tell, or the body
/// is where the path stands still (climbing straight up to the next hold, the hands finishing the
/// move after the body has arrived): then the clock tells.
pub fn progress(from: Vec3, to: Vec3, at: Vec3) -> Option<f32> {
    const STEPS: usize = 32;
    if from.distance(to) < SHORTEST {
        return None;
    }
    let across = (to - from).with_y(0.).length() >= (to.y - from.y).abs();
    let seen = |p: Vec3| if across { p.with_y(0.) } else { p };
    let at = seen(at);
    let point = |i: usize| seen(carry(from, to, i as f32 / STEPS as f32));
    // The nearest of the path's points (the first, where it stands still), then the nearest point
    // on the stretches either side of it.
    let nearest = (0..=STEPS)
        .min_by(|&a, &b| {
            let (a, b) = (point(a).distance_squared(at), point(b).distance_squared(at));
            a.total_cmp(&b)
        })
        .unwrap_or(0);
    let mut best: Option<(f32, usize, f32)> = None;
    for i in [nearest.saturating_sub(1), nearest] {
        if i >= STEPS {
            continue;
        }
        let (a, b) = (point(i), point(i + 1));
        let ab = b - a;
        let t = if ab.length_squared() > 1e-12 {
            ((at - a).dot(ab) / ab.length_squared()).clamp(0., 1.)
        } else {
            0.
        };
        let d = (a + ab * t).distance_squared(at);
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, i, t));
        }
    }
    let (_, i, t) = best?;
    let k = (i as f32 + t) / STEPS as f32;
    if k >= 1. - 1e-4 {
        return Some(1.);
    }
    // Where the path stands still just ahead, the body could be anywhere along that stretch.
    let ahead = seen(carry(from, to, (k + 1. / STEPS as f32).min(1.)));
    (ahead.distance(seen(carry(from, to, k))) >= 1e-4).then_some(k)
}

/// A hold as the hands find it: a straight edge from `a` to `b` (its top, where the hands go),
/// `out` level and out from the wall, and whether the wall goes on below it for the feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: Vec3,
    pub b: Vec3,
    pub out: Vec3,
    pub braced: bool,
}

impl Edge {
    /// The point on the edge nearest `p` along it, kept on the edge.
    pub fn nearest(&self, p: Vec3) -> Vec3 {
        let ab = self.b - self.a;
        let flat = ab.with_y(0.);
        let t = if flat.length_squared() > 1e-12 {
            ((p - self.a).with_y(0.).dot(flat) / flat.length_squared()).clamp(0., 1.)
        } else {
            0.
        };
        self.a + ab * t
    }

    /// How far `p` is out from the face of the wall below the edge, level: negative inside it.
    pub fn out_from(&self, p: Vec3) -> f32 {
        (p - self.a).with_y(0.).dot(self.out)
    }
}

/// Where a hand whose wrist the clip has at `wrist` closes on `edge`, and how firmly, from 0 to 1:
/// the wrist [`GRIP_OUT`] out from the face and [`GRIP_DOWN`] below the top where the clip's hand
/// is along the edge (kept on it), lifted off as the clip lifts it, and brought out as far up to
/// [`LIFT`]. Firmly with the clip's wrist within [`HOLD`] of there, not at all past [`LET_GO`].
pub fn grip(edge: &Edge, wrist: Vec3) -> (Vec3, f32) {
    let on = edge.nearest(wrist) + edge.out * GRIP_OUT - Vec3::Y * GRIP_DOWN;
    let lift = (wrist.y - on.y).max(0.);
    let at = on + Vec3::Y * lift + edge.out * lift.min(LIFT);
    (at, 1. - smoothstep(HOLD, LET_GO, wrist.distance(at)))
}

/// How far to move a foot braced on the wall below `edge`, with its ball at `ball`, for the ball
/// to rest [`BALL_OUT`] out from the face (level, straight in or out), and how much of that to
/// take, from 0 to 1: all of it near the wall and well below the edge, none for a foot clear of
/// the wall or up at the edge (climbing onto the top), nor with no wall below (a free hang).
pub fn brace(edge: &Edge, ball: Vec3) -> (Vec3, f32) {
    if !edge.braced {
        return (Vec3::ZERO, 0.);
    }
    let below = edge.nearest(ball).y - ball.y;
    let off = edge.out_from(ball) - BALL_OUT;
    let weight = smoothstep(0.4, 0.7, below) * (1. - smoothstep(0.15, 0.35, off.abs()));
    (-edge.out * off, weight)
}

/// How far a frame must move for its hands to reach the hold where the clips put them, from its
/// wrists in its own hang (`Hang_Idle_Loop`'s first frame, in the model's frame, the feet at the
/// origin and the wall [`FACE`] ahead): up, negative for a frame that reaches higher, and in toward
/// the wall, negative for one whose arms reach further out. Kept within [`MOST_RAISE`] and
/// [`MOST_IN`].
pub fn reach(wrists: [Vec3; 2]) -> Vec3 {
    let hang = (wrists[0] + wrists[1]) / 2.;
    Vec3::new(
        0.,
        (HANG_DROP - GRIP_DOWN - hang.y).clamp(-MOST_RAISE, MOST_RAISE),
        (FACE - GRIP_OUT - hang.z).clamp(-MOST_IN, MOST_IN),
    )
}

/// How far a frame of `reach` moves hanging from a hold `drop` above its feet, which are `out` from
/// the wall's face: by its reach, lowered by however far short of [`HANG_DROP`] the hold is (the
/// sim stands a body on the ground under a hold too low to hang from at full stretch, and the
/// clips brace the feet half a metre up the wall, so there is room to sink), and in by however
/// further out than [`FACE`] the feet are.
pub fn raise(reach: Vec3, drop: f32, out: f32) -> Vec3 {
    reach + Vec3::new(0., drop - HANG_DROP, out - FACE)
}

/// What a body is doing on a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wall {
    /// Hanging from a hold, still or shimmying.
    Hanging,
    /// Lowering itself over an edge to hang from it.
    Drop,
    /// Catching a hold in the air.
    Catch,
    /// Moving from one hold to the next: across, up or down.
    Between,
    /// Climbing from a hang onto the top.
    ClimbUp,
}

/// How much of its reach a body takes `k` of the way through `what`: all of it hanging and between
/// holds, coming in as it lowers itself or catches the hold, and going as it pulls itself up onto
/// the top, the hands pushing off it.
pub fn hanging(what: Wall, k: f32) -> f32 {
    match what {
        Wall::Hanging | Wall::Between => 1.,
        Wall::Drop => smoothstep(0.3, 1., k),
        Wall::Catch => smoothstep(0., 1., k),
        Wall::ClimbUp => 1. - smoothstep(0.3, 0.6, k),
    }
}

/// A raise `dt` seconds on from `now` toward `want`, halving the difference every
/// [`RAISE_HALF_LIFE`].
pub fn ease_raise(now: Vec3, want: Vec3, dt: f32) -> Vec3 {
    let left = (-dt * std::f32::consts::LN_2 / RAISE_HALF_LIFE).exp();
    let eased = want + (now - want) * left;
    if eased.distance(want) < 1e-4 {
        want
    } else {
        eased
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wall facing +z with its top 3 m up, from x 0 to 2.
    const EDGE: Edge = Edge {
        a: Vec3::new(0., 3., 0.),
        b: Vec3::new(2., 3., 0.),
        out: Vec3::Z,
        braced: true,
    };

    #[test]
    fn the_body_on_its_path_says_how_far_through_the_move_it_is() {
        // Lowering itself over an edge, climbing onto a top, hopping across to the next hold and
        // catching one a little below: at every point of the sim's path, the share comes back.
        let moves = [
            (Vec3::new(1., 3., -0.3), Vec3::new(1., 1.1, 0.4)),
            (Vec3::new(1., 1.1, 0.4), Vec3::new(1., 3., -0.5)),
            (Vec3::new(1., 1.1, 0.4), Vec3::new(1.5, 1.15, 0.4)),
            (Vec3::new(1.2, 1.3, 0.6), Vec3::new(1., 1.1, 0.4)),
        ];
        for (from, to) in moves {
            for i in 0..=40 {
                let k = i as f32 / 40.;
                let got = progress(from, to, carry(from, to, k)).unwrap();
                assert!((got - k).abs() < 0.01, "{from} to {to}: {got} at {k}");
            }
        }
    }

    #[test]
    fn a_hop_across_reads_by_how_far_across_the_body_has_come() {
        // A side eject (`sim::climbing`): 2 m along the wall and 0.6 m up, hopping 0.35 m in the
        // middle of the move, which the path doesn't know of.
        let (from, to) = (Vec3::new(1., 1.1, 0.4), Vec3::new(3., 1.7, 0.4));
        for i in 0..=40 {
            let k = i as f32 / 40.;
            let hop = Vec3::Y * (k * std::f32::consts::PI).sin() * 0.35;
            let got = progress(from, to, carry(from, to, k) + hop).unwrap();
            assert!((got - k).abs() < 0.01, "{got} at {k}");
        }
    }

    #[test]
    fn a_body_off_its_path_by_a_little_still_reads_close() {
        // Drawn between two snapshots, the body cuts the path's corners a little.
        let (from, to) = (Vec3::new(1., 3., -0.3), Vec3::new(1., 1.1, 0.4));
        for i in 1..20 {
            let (k0, k1) = (i as f32 / 20., (i + 1) as f32 / 20.);
            let between = carry(from, to, k0).lerp(carry(from, to, k1), 0.5);
            let got = progress(from, to, between).unwrap();
            assert!(
                got > k0 - 0.01 && got < k1 + 0.01,
                "{got} between {k0} and {k1}"
            );
        }
    }

    #[test]
    fn where_the_path_stands_still_the_clock_tells() {
        // Straight up to the next hold: the body arrives with five eighths of the move done, and
        // the hands finish it.
        let (from, to) = (Vec3::new(1., 1., 0.4), Vec3::new(1., 1.8, 0.4));
        let early = progress(from, to, carry(from, to, 0.3)).unwrap();
        assert!((early - 0.3).abs() < 0.01, "{early}");
        assert_eq!(progress(from, to, to), None);
        assert_eq!(progress(from, to, carry(from, to, 0.8)), None);
        // And a move too short to measure.
        assert_eq!(progress(from, from + Vec3::X * 0.01, from), None);
    }

    #[test]
    fn hands_close_on_the_edge_where_the_clip_has_them_along_it() {
        // The clip's wrist a little low and out: it closes on the edge where it is along it.
        let (at, firm) = grip(&EDGE, Vec3::new(0.7, 2.93, 0.12));
        assert!((at - Vec3::new(0.7, 2.97, 0.07)).length() < 1e-5, "{at}");
        assert_eq!(firm, 1.);
        // Lifted as the clip lifts it, and brought out as far.
        let (at, firm) = grip(&EDGE, Vec3::new(0.7, 3.03, 0.13));
        assert!((at - Vec3::new(0.7, 3.03, 0.13)).length() < 1e-5, "{at}");
        assert_eq!(firm, 1.);
        // Past an end, on the end.
        let (at, _) = grip(&EDGE, Vec3::new(2.1, 2.97, 0.07));
        assert!((at.x - 2.).abs() < 1e-5, "{at}");
        // On its way to the next hold up, half a metre short of it, it lets go.
        let (_, firm) = grip(&EDGE, Vec3::new(0.7, 2.47, 0.07));
        assert_eq!(firm, 0.);
        let (_, partly) = grip(&EDGE, Vec3::new(0.7, 2.8, 0.1));
        assert!(partly > 0. && partly < 1., "{partly}");
    }

    #[test]
    fn feet_brace_on_the_wall_below_and_not_up_at_the_edge() {
        let ball = Vec3::new(1., 1.5, 0.15);
        let (by, firm) = brace(&EDGE, ball);
        assert!(((ball + by).z - BALL_OUT).abs() < 1e-5, "{}", (ball + by).z);
        assert!((by.x, by.y) == (0., 0.), "{by}");
        assert_eq!(firm, 1.);
        // Inside the wall, pushed out of it.
        let (by, _) = brace(&EDGE, Vec3::new(1., 1.5, -0.05));
        assert!((by.z - (BALL_OUT + 0.05)).abs() < 1e-5, "{by}");
        // Clear of the wall, at the edge, or with no wall below: left alone.
        assert_eq!(brace(&EDGE, Vec3::new(1., 1.5, 0.5)).1, 0.);
        assert_eq!(brace(&EDGE, Vec3::new(1., 2.8, 0.1)).1, 0.);
        let free = Edge {
            braced: false,
            ..EDGE
        };
        assert_eq!(brace(&free, ball), (Vec3::ZERO, 0.));
    }

    #[test]
    fn short_frames_rise_to_the_hold_and_tall_ones_sink_to_it() {
        let at = |y: f32, z: f32| [Vec3::new(0.2, y, z), Vec3::new(-0.2, y, z)];
        let grip = FACE - GRIP_OUT;
        assert!(reach(at(HANG_DROP - GRIP_DOWN, grip)).length() < 1e-6);
        let short = reach(at(1.4, grip - 0.06));
        assert!(short.distance(Vec3::new(0., HANG_DROP - GRIP_DOWN - 1.4, 0.06)) < 1e-5);
        let tall = reach(at(1.95, grip + 0.02));
        assert!(tall.distance(Vec3::new(0., -0.08, -0.02)) < 1e-5, "{tall}");
        assert_eq!(reach(at(0.5, 0.)), Vec3::new(0., MOST_RAISE, MOST_IN));
    }

    #[test]
    fn under_a_low_hold_a_frame_sinks_by_what_the_hold_lacks() {
        let r = Vec3::new(0., 0.1, 0.02);
        assert_eq!(raise(r, HANG_DROP, FACE), r);
        assert!(raise(r, 1.6, FACE).distance(Vec3::new(0., -0.2, 0.02)) < 1e-6);
        // Hanging further out from the wall, it comes in by as much.
        assert!(raise(r, HANG_DROP, 0.5).distance(Vec3::new(0., 0.1, 0.12)) < 1e-6);
    }

    #[test]
    fn the_raise_comes_in_lowering_and_catching_and_goes_climbing_up() {
        assert_eq!(hanging(Wall::Hanging, 0.), 1.);
        assert_eq!(hanging(Wall::Between, 0.5), 1.);
        assert_eq!(hanging(Wall::Drop, 0.2), 0.);
        assert_eq!(hanging(Wall::Drop, 1.), 1.);
        assert!(hanging(Wall::Catch, 0.5) > 0. && hanging(Wall::Catch, 0.5) < 1.);
        assert_eq!(hanging(Wall::ClimbUp, 0.2), 1.);
        assert_eq!(hanging(Wall::ClimbUp, 0.7), 0.);
        // Eased: half of it in a half-life, all of it in the end.
        let want = Vec3::new(0., 0.4, 0.06);
        let half = ease_raise(Vec3::ZERO, want, RAISE_HALF_LIFE);
        assert!(half.distance(want / 2.) < 1e-5, "{half}");
        let mut raise = Vec3::ZERO;
        for _ in 0..60 {
            raise = ease_raise(raise, want, 1. / 60.);
        }
        assert_eq!(raise, want);
    }
}
