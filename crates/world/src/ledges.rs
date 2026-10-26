//! Ledges up two of the limestone cliffs (#249): bands of harder, paler stone along the strata,
//! about two metres apart, so a cliff climbs ledge by ledge as the ruin's broken walls do. The
//! rest of the cliffs stay sheer.
//!
//! Each stretch of cliff is a piece of masonry ([`Masonry`]) whose blocks are its ledges, built
//! from the drawn ground. A ledge runs back into the rock, and its front stands right above where
//! the ground meets the ledge below: you stand on a ledge with the rock rising behind you, and the
//! ledge above overhangs it. At the top, a ledge of capstones along the edge of the terrace is the
//! last hold, and you climb onto it from below or step off it onto the terrace.

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
/// front within 0.5 m: true where the cliff is steeper than 3.6:1, and where it isn't, the
/// ledge is deep enough to stand on and jump from).
pub const LEDGE_STEP: f32 = 1.8;
/// The first ledge is this far above the ground at the foot of the cliff (a jump catches it).
pub const FIRST_LEDGE: f32 = 2.8;
/// A ledge runs this far back into the rock behind it, so no gap opens between them.
const SET_IN: f32 = 0.5;
/// The capstones along the top run this far back onto the terrace.
const LIP_DEPTH: f32 = 1.5;
/// Each block of ledge covers about this much of the cliff's length.
const SEGMENT: f32 = 3.0;

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
    let n = (length / SEGMENT).round().max(1.0) as usize;
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
        for j in 0..n {
            let (a, b) = (&profiles[j], &profiles[j + 1]);
            let front = (a.reach(below) + b.reach(below)) / 2.0;
            let back = (a.reach(top) + b.reach(top)) / 2.0 + if lip_ledge { LIP_DEPTH } else { SET_IN };
            let (p0, p1) = (stations[j], stations[j + 1]);
            let center = (p0 + p1) / 2.0 - out * ((front + back) / 2.0);
            blocks.push(Block {
                center: local(center),
                half: Vec2::new((p1 - p0).length() / 2.0, (back - front) / 2.0),
                yaw: 0.0,
                top: Some(top - origin.y),
                rise: 0.0,
                part: if lip_ledge { "lip" } else { "ledge" },
            });
        }
    }
    Masonry { prop: c.prop.to_string(), origin, yaw, blocks, grips: Vec::new(), line: Vec::new() }
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
            let out = m.turn(Vec2::Y);
            for b in &m.blocks {
                let top = m.origin.y + b.top.unwrap();
                // Room to stand on every ledge with the rock behind: its top reaches 0.45 m or more
                // in front of where the ground rises above it.
                let front = |x: f32| m.place(Vec3::new(b.center.x + x, 0.0, b.center.y + b.half.y));
                for x in [-0.8, 0.0, 0.8].map(|k| k * b.half.x) {
                    let p = front(x);
                    let edge = Vec2::new(p.x, p.z);
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
                let mut last = highest;
                for k in 1..=12 {
                    let p = m.place(Vec3::new(lip.center.x, 0.0, lip.center.y + lip.half.y - k as f32 * 0.25));
                    let g = v.ground(p.x, p.z);
                    assert!(g - last <= 2.0 * 0.25 + 1e-3 && g >= highest - 1e-3, "{}: off the capstones at {highest} m, the ground goes from {last} to {g} m", m.prop);
                    last = g;
                }
            }
            let foot = m.place(Vec3::new(m.blocks[0].center.x, 0.0, m.blocks[0].center.y + m.blocks[0].half.y + 0.5));
            // (3.5 m at most: the hands 2.1 m above the feet at the top of a 1.1 m jump, and 0.3 m to catch.)
            assert!(lowest - v.surface(foot.x, foot.z) <= 3.4, "{}: the first ledge is {} m up", m.prop, lowest - v.surface(foot.x, foot.z));
        }
    }

    #[test]
    fn every_ledge_front_is_a_hold() {
        let v = Valley::new();
        for m in pieces(&v) {
            let out = m.turn(Vec2::Y);
            for b in &m.blocks {
                let top = m.origin.y + b.top.unwrap();
                let mid = m.place(Vec3::new(b.center.x, b.top.unwrap(), b.center.y + b.half.y));
                let held = v.holds.iter().any(|h| (h.a.y - top).abs() < 1e-3 && h.out.dot(out) > 0.99 && h.distance(Vec2::new(mid.x, mid.z)) < 0.05);
                assert!(held, "{}: no hold along the ledge at {top} m, {mid}", m.prop);
            }
        }
    }
}
