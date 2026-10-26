//! Traversal (#213): rolls, leaps, charges and climbs played by distance, as Paragon and Lyra
//! time starts, stops and landings ("distance matching"), and swimmers at the surface.
//!
//! The sim carries a body through each of these along a set path (`protocol::ForcedMove`: where
//! it starts and ends, and when) at a pace of its own: a roll starts fast and slows to a stop, a
//! climb rises up the face before it goes over the edge. Each move's clip marks the stretch that
//! covers the path with its `travel` curve (docs/ASSETS.md), the share of the path covered by each
//! moment of the clip, and the clip plays at the moment that matches the share the body has
//! covered ([`share`], [`clip_time`]). So the hands and feet land where the body really is, at
//! any pace and over any length of move, and the clip reaches the end of its stretch just as the
//! move ends. What follows the stretch (standing up out of a roll, a dash's recovery) plays on
//! after the move, by the clock.
//!
//! The clip only ever plays forward through a move ([`Progress`]), whatever the body's position
//! does between snapshots, and starts afresh with the next.
use glam::{Vec3, Vec3Swizzles};

/// The sim floats a swimmer with its feet this far under the surface (docs/MOVEMENT.md). Swim
/// clips have the surface at their origin, so a swimmer's body is raised by this much.
pub const SWIM_FLOAT: f32 = 0.5;
/// The speed the swim loop's strokes are made for, in metres a second: the sim's swim at a run
/// (0.45 of 6 m/s). Slower or faster, the strokes come slower or faster.
pub const SWIM_SPEED: f32 = 2.7;

/// Paths shorter than this, in metres, are too short to measure: their clips go by the clock.
pub const SHORTEST: f32 = 0.05;

/// The hands take hold of a climb's edge over this long, in seconds: a grab, as quick as the sim
/// lifts the body.
pub const TAKE_HOLD: f32 = 0.05;
/// And let go as the body comes over it: from this far short of the edge, in metres, until the
/// body is over it.
pub const LET_GO: f32 = 0.1;
/// The sim lifts a climber faster than it carries them across, so the shoulders rise out of reach
/// of the edge within about a tenth of a second. A hand holds it while it can reach, and lets go
/// between these shares of the arm's length; letting go, its hold halves every
/// [`RELEASE_HALF_LIFE`] seconds, so the arm eases back into the clip rather than snapping to it.
pub const REACH: [f32; 2] = [1., 1.3];
pub const RELEASE_HALF_LIFE: f32 = 0.05;
/// Climbs lower than this, in metres, are stepped up rather than pulled up, so the hands keep
/// off the top; from here to [`PULL_UP`] they hold it more and more firmly.
pub const STEP_UP: f32 = 0.4;
pub const PULL_UP: f32 = 0.7;

/// How a move's body travels its path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// Along the ground, hopping or not (a roll, a leap, a charge): how far along the line it has
    /// come.
    Along,
    /// Up a face and over its edge (a climb): the height gained and the way across, together.
    UpAndOver,
}

/// The share of the path from `from` to `to` that a body at `at` has covered, from 0 to 1, or
/// `None` if the path is too short to say.
pub fn share(path: Path, from: Vec3, to: Vec3, at: Vec3) -> Option<f32> {
    let line = (to - from).xz();
    let across = line.length();
    let along = |at: Vec3| {
        if across > 0. {
            ((at - from).xz().dot(line) / (across * across)).clamp(0., 1.)
        } else {
            0.
        }
    };
    match path {
        Path::Along => (across >= SHORTEST).then(|| along(at)),
        Path::UpAndOver => {
            let rise = to.y - from.y;
            let total = rise.abs() + across;
            (total >= SHORTEST).then(|| {
                let up = if rise.abs() > 0. {
                    ((at.y - from.y) / rise).clamp(0., 1.)
                } else {
                    0.
                };
                (up * rise.abs() + along(at) * across) / total
            })
        }
    }
}

/// Where a clip is at `share` of its move: the time its `travel` curve (keys of clip seconds and
/// the share of the path covered by then, rising) first reaches that share. Before the first
/// key's share the clip waits at its time, and past the last it holds at the last, for what
/// follows to play by the clock once the move is over.
pub fn clip_time(travel: &[[f32; 2]], share: f32) -> f32 {
    let Some(&[start, first]) = travel.first() else {
        return 0.;
    };
    if share <= first {
        return start;
    }
    for w in travel.windows(2) {
        let ([t0, s0], [t1, s1]) = (w[0], w[1]);
        if share <= s1 {
            return if s1 > s0 {
                t0 + (t1 - t0) * (share - s0) / (s1 - s0)
            } else {
                t0
            };
        }
    }
    travel[travel.len() - 1][0]
}

/// Which way a move along the ground from `from` to `to` heads, as the sim's yaw (0 faces +z,
/// turning toward +x), or `None` if it goes nowhere.
pub fn heading(from: Vec3, to: Vec3) -> Option<f32> {
    let line = (to - from).xz();
    (line.length() >= SHORTEST).then(|| line.x.atan2(line.y))
}

/// The sim stands a climber this far past the edge, in metres.
const PAST_EDGE: f32 = 0.3;

/// Where a climb from `from` to `to` goes over the edge, by `ground` (the height at x, z): the
/// first point on the way across where the ground has risen to within 0.1 m of the top, at the
/// top's height. Without a rise to find, [`PAST_EDGE`] short of `to`.
pub fn edge(from: Vec3, to: Vec3, ground: impl Fn(f32, f32) -> f32) -> Vec3 {
    let line = (to - from).xz();
    let across = line.length();
    let dir = line.normalize_or_zero();
    let steps = (across / 0.05).ceil() as usize;
    let at = (1..=steps)
        .map(|i| from.xz() + dir * (i as f32 * 0.05).min(across))
        .find(|p| ground(p.x, p.y) >= to.y - 0.1)
        .unwrap_or(to.xz() - dir * PAST_EDGE.min(across));
    Vec3::new(at.x, to.y, at.y)
}

/// How firmly the hands hold the edge of a climb from `from` to `to` with the body at `at`,
/// `elapsed` seconds in, from 0 to 1: as firmly as the climb is high (none for a step up), taking
/// hold over [`TAKE_HOLD`], and letting go as the body comes over the edge.
pub fn grip(edge: Vec3, from: Vec3, to: Vec3, at: Vec3, elapsed: f32) -> f32 {
    let height = smoothstep(STEP_UP, PULL_UP, to.y - from.y);
    let dir = (to - from).xz().normalize_or_zero();
    let short = (edge - at).xz().dot(dir);
    height * smoothstep(0., TAKE_HOLD, elapsed) * smoothstep(0., LET_GO, short)
}

/// How much of its hold a hand keeps with the wrist's goal `distance` metres from the shoulder,
/// on an arm `length` metres long: all of it within reach, none once out of it ([`REACH`]).
pub fn within_reach(distance: f32, length: f32) -> f32 {
    1. - smoothstep(REACH[0] * length, REACH[1] * length, distance)
}

/// A hand's hold, `dt` seconds on from `held`, toward `want`: taken at once (the grip itself takes
/// hold over [`TAKE_HOLD`]), and let go of by halves ([`RELEASE_HALF_LIFE`]), then entirely once
/// under a hundredth, where it no longer shows.
pub fn ease_hold(held: f32, want: f32, dt: f32) -> f32 {
    if want >= held {
        return want;
    }
    let eased = want + (held - want) * (-dt * std::f32::consts::LN_2 / RELEASE_HALF_LIFE).exp();
    if eased < 0.01 { 0. } else { eased }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// One body's way through its moves, so that its clip only plays forward through each.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    /// When the move it follows began, in sim time (as `ForcedMove::start`).
    start: Option<f64>,
    /// Seconds of the move seen so far.
    elapsed: f32,
    /// The furthest share of the path the body has covered.
    share: f32,
}

impl Progress {
    /// Follows the move that began at `start` and lasts `duration` seconds, with the body `share`
    /// of the way along its path (`None` if the path is too short to say: then the clock
    /// decides), `dt` seconds after the last update. Returns the share to play the clip at, never
    /// less than before in the same move. The first frame of a move is a frame into it.
    pub fn update(&mut self, start: f64, duration: f32, share: Option<f32>, dt: f32) -> f32 {
        if self.start != Some(start) {
            *self = Self {
                start: Some(start),
                ..Self::default()
            };
        }
        self.elapsed += dt;
        let share = share.unwrap_or(self.elapsed / duration.max(1e-3));
        self.share = self.share.max(share.clamp(0., 1.));
        self.share
    }

    /// Seconds of the current move seen so far.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// When the move it follows began, once it has followed one.
    pub fn start(&self) -> Option<f64> {
        self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1. / 60.;

    /// Where the sim has a body `k` of the way through a move (docs/MOVEMENT.md): along the
    /// ground easing out, with a hop of `arc` in the middle; a climb up the face first, then
    /// over.
    fn sim(path: Path, from: Vec3, to: Vec3, arc: f32, k: f32) -> Vec3 {
        match path {
            Path::Along => {
                let eased = 1. - (1. - k) * (1. - k);
                from.lerp(to, eased) + Vec3::Y * (k * std::f32::consts::PI).sin() * arc
            }
            Path::UpAndOver => {
                let up = 1. - (1. - (k * 1.6).min(1.)).powi(2);
                let p = from.lerp(to, k * k);
                Vec3::new(p.x, from.y + (to.y - from.y) * up, p.z)
            }
        }
    }

    /// The traversal clips' curves, as the catalogue has them.
    const ROLL: [[f32; 2]; 3] = [[0.1, 0.], [0.43, 0.5], [0.87, 1.]];
    const CLIMB: [[f32; 2]; 2] = [[0., 0.], [0.667, 1.]];

    #[test]
    fn clips_play_forward_and_end_their_stretch_as_the_move_ends() {
        let from = Vec3::new(2., 1., -3.);
        // A roll (5 m in 0.45 s), a leap, a long charge, and climbs low and high.
        let moves = [
            (Path::Along, Vec3::new(4., 0., 3.), 0.45, 0.15, &ROLL[..]),
            (Path::Along, Vec3::new(0., 0., -6.), 0.3, 0.4, &ROLL[..]),
            (Path::Along, Vec3::new(-14., 0., 5.), 0.9, 0.3, &ROLL[..]),
            (
                Path::UpAndOver,
                Vec3::new(0.3, 0.6, 0.5),
                0.6,
                0.,
                &CLIMB[..],
            ),
            (
                Path::UpAndOver,
                Vec3::new(-0.5, 1.5, 1.2),
                0.6,
                0.,
                &CLIMB[..],
            ),
        ];
        for (path, to, duration, arc, travel) in moves {
            let to = from + to;
            let mut progress = Progress::default();
            let mut last = travel[0][0];
            let frames = (duration / DT).round() as usize;
            for f in 0..=frames {
                let at = sim(path, from, to, arc, f as f32 / frames as f32);
                let s = progress.update(7.5, duration, share(path, from, to, at), DT);
                let t = clip_time(travel, s);
                assert!(
                    t >= last,
                    "{path:?} to {to}: back from {last} to {t} at frame {f}"
                );
                last = t;
            }
            let end = travel[travel.len() - 1][0];
            assert!(
                (last - end).abs() < 1e-4,
                "{path:?} to {to} ended at {last}, not {end}"
            );
        }
    }

    #[test]
    fn the_clip_keeps_pace_with_the_body() {
        // The roll covers half its path while the clip reaches the hands touching down, and it is
        // fastest at the start, as the body is.
        let (from, to) = (Vec3::ZERO, Vec3::new(0., 0., 5.));
        let at = |k| sim(Path::Along, from, to, 0.15, k);
        let time = |k| clip_time(&ROLL, share(Path::Along, from, to, at(k)).unwrap());
        let halfway = 1. - 0.5f32.sqrt();
        assert!((time(halfway) - 0.43).abs() < 1e-4, "{}", time(halfway));
        assert!(time(0.1) - time(0.) > 2. * (time(1.) - time(0.9)));
        // Climbing, the height gained counts as much as the way across.
        let (top, base) = (Vec3::new(0., 1.2, 0.8), Vec3::ZERO);
        let s = share(Path::UpAndOver, base, top, Vec3::new(0., 1.2, 0.)).unwrap();
        assert!((s - 0.6).abs() < 1e-5, "{s}");
    }

    #[test]
    fn a_move_too_short_to_measure_goes_by_the_clock() {
        let here = Vec3::new(1., 0., 1.);
        assert_eq!(share(Path::Along, here, here + Vec3::X * 0.01, here), None);
        assert_eq!(heading(here, here), None);
        let mut progress = Progress::default();
        let shares: Vec<_> = (1..=27)
            .map(|_| progress.update(3., 0.45, None, DT))
            .collect();
        assert!((shares[0] - DT / 0.45).abs() < 1e-6, "{}", shares[0]);
        assert!((shares[26] - 1.).abs() < 1e-5, "{}", shares[26]);
        // A climb straight up still measures.
        assert!(share(Path::UpAndOver, here, here + Vec3::Y, here + Vec3::Y * 0.5).is_some());
    }

    #[test]
    fn the_next_move_starts_afresh_and_a_body_jumping_back_doesnt_wind_the_clip_back() {
        let mut progress = Progress::default();
        assert_eq!(progress.start(), None);
        assert_eq!(progress.update(1., 0.45, Some(0.6), DT), 0.6);
        assert_eq!(progress.update(1., 0.45, Some(0.55), DT), 0.6);
        assert!((progress.elapsed() - 2. * DT).abs() < 1e-6);
        assert_eq!(progress.update(2., 0.45, Some(0.1), DT), 0.1);
        assert_eq!(progress.elapsed(), DT);
        assert_eq!(progress.start(), Some(2.));
    }

    #[test]
    fn curves_hold_outside_their_keys_and_skip_flat_stretches() {
        assert_eq!(clip_time(&ROLL, -0.2), 0.1);
        assert_eq!(clip_time(&ROLL, 1.3), 0.87);
        assert!((clip_time(&ROLL, 0.75) - 0.65).abs() < 1e-5);
        let flat = [[0., 0.], [0.2, 0.5], [0.4, 0.5], [0.6, 1.]];
        assert_eq!(clip_time(&flat, 0.5), 0.2);
        assert!((clip_time(&flat, 0.75) - 0.5).abs() < 1e-5);
        assert_eq!(clip_time(&[], 0.5), 0.);
    }

    #[test]
    fn headings_are_the_sims_yaw() {
        let at = Vec3::new(3., 2., 1.);
        let east = heading(at, at + Vec3::X).unwrap();
        assert!((east - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(heading(at, at + Vec3::Z).unwrap().abs() < 1e-6);
    }

    #[test]
    fn hands_find_the_edge_hold_it_and_let_go_coming_over_it() {
        // A wall 1.2 m high, its face 0.6 m ahead of the climber; the sim stands them 0.3 m past
        // its edge.
        let ground = |_x: f32, z: f32| if z >= 0.6 { 1.2 } else { 0. };
        let (from, to) = (Vec3::ZERO, Vec3::new(0., 1.2, 0.9));
        let at = edge(from, to, ground);
        assert!((at - Vec3::new(0., 1.2, 0.6)).length() < 0.051, "{at}");
        // Without a rise to find, 0.3 m short of where they stand.
        let flat = edge(from, to, |_, _| 0.);
        assert!((flat - Vec3::new(0., 1.2, 0.6)).length() < 1e-5, "{flat}");
        let hold = |body: Vec3, elapsed| grip(at, from, to, body, elapsed);
        assert_eq!(hold(from, 0.), 0.);
        let taking = hold(from, DT);
        assert!(taking > 0. && taking < 0.5, "{taking}");
        assert_eq!(hold(from + Vec3::Y * 0.5, TAKE_HOLD), 1.);
        let coming = hold(Vec3::new(0., 1.2, 0.55), 0.4);
        assert!(coming > 0. && coming < 0.6, "{coming}");
        assert_eq!(hold(Vec3::new(0., 1.2, 0.6), 0.5), 0.);
        // A step up keeps the hands off it.
        assert_eq!(grip(at, from, Vec3::new(0., 0.35, 0.9), from, 0.3), 0.);
    }

    #[test]
    fn hands_hold_while_they_reach_and_ease_off_once_they_dont() {
        assert_eq!(within_reach(0.5, 0.6), 1.);
        assert_eq!(within_reach(0.59, 0.6), 1.);
        let partly = within_reach(0.7, 0.6);
        assert!(partly > 0. && partly < 1., "{partly}");
        assert_eq!(within_reach(0.8, 0.6), 0.);
        // Taken at once, let go of by halves.
        assert_eq!(ease_hold(0.2, 0.9, DT), 0.9);
        let held = ease_hold(1., 0., RELEASE_HALF_LIFE);
        assert!((held - 0.5).abs() < 1e-5, "{held}");
        let mut held = 1.;
        for _ in 0..18 {
            let next = ease_hold(held, 0., DT);
            assert!(next < held && next > 0.);
            held = next;
        }
        assert!(held < 0.02, "{held}");
        // And then all of it.
        for _ in 0..6 {
            held = ease_hold(held, 0., DT);
        }
        assert_eq!(held, 0.);
    }
}
