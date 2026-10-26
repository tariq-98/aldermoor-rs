//! Ledges up two of the limestone cliffs (#249): bands of harder, paler stone along the strata,
//! about two metres apart, so a cliff climbs ledge by ledge as the ruin's broken walls do. The
//! rest of the cliffs stay sheer.
//!
//! Each stretch of cliff is a piece of masonry ([`Masonry`]) whose blocks are its ledges, built
//! from the drawn ground. A ledge runs back into the rock, and its front runs just out from where
//! the ground meets the ledge below: you stand on a ledge with the rock rising behind you, and the
//! ledge above overhangs it. At the top, a ledge of capstones along the edge of the terrace is the
//! last hold, and you climb onto it from below or step off it onto the terrace.
//!
//! Each ledge is one straight block, as a bed of rock runs straight across a cliff: its front is
//! one hold from end to end, so a climber shimmies the whole way along it.

use glam::{Vec2, Vec3};

use crate::masonry::{Block, Masonry};

/// A stretch of cliff with ledges up it: two points on the valley floor just out from its foot,
/// in the order that turns the way along it from `from` to `to` a quarter to the left
/// (`(-along.y, along.x)`) out of the cliff.
struct Cliff {
    prop: &'static str,
    from: Vec2,
    to: Vec2,
}

const CLIFFS: [Cliff; 2] = [
    // The west terrace's east face, north of the cottage at its foot: 16 m from the valley floor
    // up onto the terrace.
    Cliff { prop: "terrace.ledges.west", from: Vec2::new(-37.0, -5.0), to: Vec2::new(-37.0, -17.0) },
    // The east terrace's west face, south of the windmill on its edge: 26 m.
    Cliff { prop: "terrace.ledges.mill", from: Vec2::new(41.0, -100.0), to: Vec2::new(41.0, -90.0) },
];

/// The ledges' tops are this far apart at most, and at least [`crate::HANG_MIN_DROP`] so each
/// front is a hold. Hanging from one, a climb leap reaches the next (1 to 1.8 m up, and its
/// front up to 0.8 m further in); where the next is further in than that, the ledge is deep
/// enough to stand on and jump from.
pub const LEDGE_STEP: f32 = 1.8;
/// The first ledge is this far above the ground at the foot of the cliff (a jump catches it).
pub const FIRST_LEDGE: f32 = 2.8;
/// A ledge runs this far back into the rock behind it, so no gap opens between them.
const SET_IN: f32 = 0.5;
/// The capstones along the top run this far back onto the terrace.
const LIP_DEPTH: f32 = 1.5;
/// Each end of a ledge stops up to this far short of the stretch's end, each by its own amount,
/// so the ledges up a cliff don't all end in one line. The capstones run the whole way.
const RAGGED: f32 = 1.2;
/// The ground is sampled going into the cliff every this many metres along it.
const STATION: f32 = 0.5;

/// The ground going into the cliff from a point at its foot: heights every 5 cm out to 20 m in.
struct Profile(Vec<f32>);

const PROFILE_STEP: f32 = 0.05;

impl Profile {
    fn new(foot: Vec2, into: Vec2, ground: &impl Fn(f32, f32) -> f32) -> Profile {
        Profile((0..=400).map(|i| {
            let p = foot + into * (i as f32 * PROFILE_STEP);
            ground(p.x, p.y)
        }).collect())
    }

    /// The height of the terrace at the top of the cliff: where the ground levels out above its
    /// steepest part.
    fn terrace(&self) -> f32 {
        let slope = |i: usize| (self.0[i + 10] - self.0[i]) / (10.0 * PROFILE_STEP);
        let end = self.0.len() - 11;
        let steepest = (0..=end).max_by(|&a, &b| slope(a).total_cmp(&slope(b))).unwrap();
        let level = (steepest..=end).find(|&i| slope(i) < 0.6).unwrap_or(end);
        self.0[level]
    }

    /// How far in the ground first reaches `h`.
    fn reach(&self, h: f32) -> f32 {
        let i = self.0.iter().position(|&g| g >= h).unwrap_or(self.0.len() - 1).max(1);
        let (g0, g1) = (self.0[i - 1], self.0[i]);
        let k = if g1 > g0 { ((h - g0) / (g1 - g0)).clamp(0.0, 1.0) } else { 1.0 };
        (i as f32 - 1.0 + k) * PROFILE_STEP
    }
}

/// The ledges as masonry pieces `terrace.ledges.*`, built on `ground` (the drawn ground).
pub(crate) fn ledges(ground: impl Fn(f32, f32) -> f32) -> Vec<Masonry> {
    CLIFFS.iter().map(|c| cliff(c, &ground)).collect()
}

fn cliff(c: &Cliff, ground: &impl Fn(f32, f32) -> f32) -> Masonry {
    let along = (c.to - c.from).normalize();
    let out = Vec2::new(-along.y, along.x);
    let length = (c.to - c.from).length();
    let n = (length / STATION).round().max(1.0) as usize;
    let stations: Vec<Vec2> = (0..=n).map(|j| c.from + along * (length * j as f32 / n as f32)).collect();
    let profiles: Vec<Profile> = stations.iter().map(|&p| Profile::new(p, -out, ground)).collect();
    // The ledges' heights: the first in a jump's reach of the ground at the foot wherever along
    // the cliff, the last capstones at the edge of the terrace, and the rest evenly between, no
    // further apart than a jump pulls you up.
    let foot = profiles.iter().map(|p| p.0[0]).fold(f32::MIN, f32::max);
    let lip = profiles.iter().map(Profile::terrace).fold(f32::MAX, f32::min) - 0.1;
    let first = foot + FIRST_LEDGE;
    let steps = ((lip - first) / LEDGE_STEP).ceil() as usize;
    let tops: Vec<f32> = (0..=steps).map(|i| first + (lip - first) * i as f32 / steps as f32).collect();

    let mid = (c.from + c.to) / 2.0;
    let origin = Vec3::new(mid.x, ground(mid.x, mid.y), mid.y);
    // The piece's own frame: +z out of the cliff, x along it.
    let yaw = out.x.atan2(out.y);
    let (s, cy) = yaw.sin_cos();
    let local = |p: Vec2| {
        let d = p - Vec2::new(origin.x, origin.z);
        Vec2::new(cy * d.x - s * d.y, s * d.x + cy * d.y)
    };
    let mut blocks = Vec::new();
    for (i, &top) in tops.iter().enumerate() {
        // The ground the ledge's front stands over: the ledge below's top, or the foot.
        let below = if i == 0 { top - FIRST_LEDGE } else { tops[i - 1] };
        let lip_ledge = i == tops.len() - 1;
        let set_in = if lip_ledge { LIP_DEPTH } else { SET_IN };
        // Where the ground reaches the ledge below's top all along the cliff, and where it reaches
        // this one's (from the cliff's end, `from`). The front runs straight along the first, just
        // out from all of them, so none of the cliff shows between this ledge and the one below;
        // the back runs into the rock behind the second.
        let feet: Vec<Vec2> = (0..=n).map(|j| stations[j] - c.from - out * profiles[j].reach(below)).collect();
        let heads: Vec<Vec2> = (0..=n).map(|j| stations[j] - c.from - out * (profiles[j].reach(top) + set_in)).collect();
        let dir = fit(&feet, along);
        let normal = Vec2::new(-dir.y, dir.x);
        let front = feet.iter().map(|p| p.dot(normal)).fold(f32::MIN, f32::max);
        let back = heads.iter().map(|p| p.dot(normal)).fold(f32::MAX, f32::min);
        let short = |k: i64| if lip_ledge { 0.0 } else { RAGGED * crate::hash(i as i64, k, c.from.x as i64) };
        let (start, end) = (feet[0].dot(dir) + short(0), feet[n].dot(dir) - short(1));
        let center = c.from + dir * ((start + end) / 2.0) + normal * ((front + back) / 2.0);
        blocks.push(Block {
            center: local(center),
            half: Vec2::new((end - start) / 2.0, (front - back) / 2.0),
            // Turned to run along the cliff; its own +y is out of it.
            yaw: (-dir.y).atan2(dir.x) - yaw,
            top: Some(top - origin.y),
            rise: 0.0,
            part: if lip_ledge { "lip" } else { "ledge" },
        });
    }
    Masonry { prop: c.prop.to_string(), origin, yaw, blocks, grips: Vec::new(), line: Vec::new() }
}

/// The way along a straight line through the points `p`, fitted by least squares: `along`
/// turned as far as they slant across it.
fn fit(p: &[Vec2], along: Vec2) -> Vec2 {
    let out = Vec2::new(-along.y, along.x);
    let k = p.len() as f32;
    let (t0, o0) = (p.iter().map(|q| q.dot(along)).sum::<f32>() / k, p.iter().map(|q| q.dot(out)).sum::<f32>() / k);
    let (mut tt, mut to) = (0.0, 0.0);
    for q in p {
        let (t, o) = (q.dot(along) - t0, q.dot(out) - o0);
        tt += t * t;
        to += t * o;
    }
    (along + out * (to / tt)).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Valley;

    /// The ledge pieces in the valley.
    fn pieces(v: &Valley) -> Vec<&Masonry> {
        v.masonry.iter().filter(|m| m.prop.starts_with("terrace.ledges.")).collect()
    }

    #[test]
    fn each_cliff_climbs_ledge_by_ledge_from_its_foot_onto_the_terrace() {
        let v = Valley::new();
        let ledges = pieces(&v);
        assert_eq!(ledges.len(), 2);
        for m in ledges {
            let mut tops: Vec<f32> = m.blocks.iter().map(|b| b.top.unwrap()).collect();
            tops.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
            assert!(tops.len() >= 7, "{}: {} ledges", m.prop, tops.len());
            for w in tops.windows(2) {
                // A jump from one pulls you up onto the next, and each front is a hold.
                let step = w[1] - w[0];
                assert!((crate::HANG_MIN_DROP..=LEDGE_STEP + 1e-3).contains(&step), "{}: ledges {step} m apart", m.prop);
            }
            for b in &m.blocks {
                let top = m.origin.y + b.top.unwrap();
                let (front, out) = front_of(m, b);
                for x in (-10..=10).map(|k| k as f32 / 10.0 * 0.95 * b.half.x) {
                    let edge = front(x);
                    // A ledge, not a crack: its top reaches 0.15 m or more in front of where the
                    // ground rises above it (where the cliff is nearly sheer). Deep enough (half a
                    // metre), you stand on it.
                    let ledge = (0..40).map(|k| edge - out * (k as f32 * 0.05)).take_while(|q| v.surface(q.x, q.y) < top).count();
                    assert!(ledge as f32 * 0.05 >= 0.15 || b.part == "lip", "{} at {edge}: only {} m of ledge at {top} m", m.prop, ledge as f32 * 0.05);
                    // Its front is a hold: the next ledge down, or the foot, is far enough below.
                    let below = v.ground(edge.x + out.x * 0.3, edge.y + out.y * 0.3);
                    assert!(top - below >= crate::HANG_MIN_DROP - 1e-3, "{} at {edge}: a {} m drop below the ledge at {top} m", m.prop, top - below);
                }
            }
            // The first ledge is caught in a jump from the ground at the cliff's foot, and the last
            // is the edge of the terrace, which you step off onto.
            let lowest = m.origin.y + tops[0];
            let highest = m.origin.y + tops[tops.len() - 1];
            for lip in m.blocks.iter().filter(|b| b.part == "lip") {
                // Walking in from the capstones' front, onto the terrace: never a step up too steep
                // to walk (2:1), nor down.
                let (front, out) = front_of(m, lip);
                let mut last = highest;
                for k in 1..=12 {
                    let p = front(0.0) - out * (k as f32 * 0.25);
                    let g = v.ground(p.x, p.y);
                    assert!(g - last <= 2.0 * 0.25 + 1e-3 && g >= highest - 1e-3, "{}: off the capstones at {highest} m, the ground goes from {last} to {g} m", m.prop);
                    last = g;
                }
            }
            let (front, out) = front_of(m, &m.blocks[0]);
            let foot = front(0.0) + out * 0.5;
            let foot = Vec3::new(foot.x, 0.0, foot.y);
            // (3.5 m at most: the hands 2.1 m above the feet at the top of a 1.1 m jump, and 0.3 m to catch.)
            assert!(lowest - v.surface(foot.x, foot.z) <= 3.4, "{}: the first ledge is {} m up", m.prop, lowest - v.surface(foot.x, foot.z));
        }
    }

    #[test]
    fn every_ledge_front_is_a_hold() {
        let v = Valley::new();
        for m in pieces(&v) {
            for b in &m.blocks {
                let top = m.origin.y + b.top.unwrap();
                let (front, out) = front_of(m, b);
                for x in (-4..=4).map(|k| k as f32 / 4.0 * 0.9 * b.half.x) {
                    let edge = front(x);
                    let held = v.holds.iter().any(|h| (h.a.y - top).abs() < 1e-3 && h.out.dot(out) > 0.999 && h.distance(edge) < 0.01);
                    assert!(held, "{}: no hold along the ledge at {top} m, at {edge}", m.prop);
                }
            }
        }
    }

    /// A ledge block's front in the valley, `x` metres along it from its middle, and the way out
    /// of the cliff there.
    fn front_of<'a>(m: &'a Masonry, b: &'a Block) -> (impl Fn(f32) -> Vec2 + 'a, Vec2) {
        let (s, c) = b.yaw.sin_cos();
        let (along, out) = (Vec2::new(c, -s), Vec2::new(s, c));
        let front = move |x: f32| {
            let p = b.center + along * x + out * b.half.y;
            let w = m.place(Vec3::new(p.x, 0.0, p.y));
            Vec2::new(w.x, w.z)
        };
        (front, m.turn(out))
    }
}
