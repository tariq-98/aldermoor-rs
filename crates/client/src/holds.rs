//! The hold a climber hangs from (#248), found in the client's own copy of the valley's holds
//! (`world::Valley::holds`) from where the sim has the body, for the renderer to close the hands on
//! it and brace the feet on the wall below (`render::character::HoldAt`).
//!
//! The snapshot says where a body is, not which hold it has, but the sim hangs a body at a point
//! fixed by its hold (`sim::climbing::hang_point`): out from the wall by the body's radius, and
//! [`HANG_DROP`] below the hold, or less where the ground comes up to the feet. So the hold is
//! the one that point hangs from.
//!
//! The renderer asks while it poses, for every climber every frame, so the holds are filed by the
//! squares of ground a body could hang from each over: one search finds a body's square and the
//! holds in it, without allocating or hashing.

use std::{ops::RangeInclusive, sync::Arc};

use anim::climb::{FACE, HANG_DROP};
use glam::{Vec2, Vec3, Vec3Swizzles};
use world::{Hold, Valley};

/// How far a hold is above the feet of a body hanging from it: [`HANG_DROP`], or down to the
/// sim's climbing height where the ground comes up to the feet (it lets go below that).
const RISE: RangeInclusive<f32> = 1.4..=1.95;
/// How far out from the wall's face the feet are: a body's radius.
const OUT: RangeInclusive<f32> = 0.1..=0.8;
/// How far past either end of a hold a body may be and still hang from it (the hands stop short
/// of the ends).
const PAST_END: f32 = 0.1;
/// The squares' size, in metres.
const SQUARE: f32 = 2.;

/// The valley's holds, filed by the squares of ground a body could hang from each over.
pub struct Holds {
    valley: Arc<Valley>,
    /// Each square with any holds, sorted, and where its holds start in `ids` (the next square's
    /// start ends them).
    squares: Vec<((i32, i32), usize)>,
    ids: Vec<u32>,
}

impl Holds {
    pub fn new(valley: Arc<Valley>) -> Holds {
        // Each hold goes in every square its span, widened by as far out as a body hangs, touches.
        let reach = Vec2::splat(OUT.end().max(PAST_END));
        let mut filed: Vec<((i32, i32), u32)> = Vec::new();
        for (i, h) in valley.holds.iter().enumerate() {
            let (lo, hi) = (
                h.a.xz().min(h.b.xz()) - reach,
                h.a.xz().max(h.b.xz()) + reach,
            );
            let ((x0, z0), (x1, z1)) = (square(lo), square(hi));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    filed.push(((x, z), i as u32));
                }
            }
        }
        filed.sort_unstable();
        let mut squares: Vec<((i32, i32), usize)> = Vec::new();
        for (n, &(at, _)) in filed.iter().enumerate() {
            if squares.last().is_none_or(|s| s.0 != at) {
                squares.push((at, n));
            }
        }
        let ids = filed.into_iter().map(|(_, i)| i).collect();
        Holds {
            valley,
            squares,
            ids,
        }
    }

    /// The hold a body with its feet at `p` hangs from: of those it could be hanging from, the
    /// one the sim would hang it nearest to there from. `None` if it hangs from none.
    pub fn hung_from(&self, p: Vec3) -> Option<Hold> {
        let s = (self.squares)
            .binary_search_by_key(&square(p.xz()), |s| s.0)
            .ok()?;
        let end = self.squares.get(s + 1).map_or(self.ids.len(), |n| n.1);
        let mut ground = None;
        let mut best: Option<(f32, &Hold)> = None;
        for &i in &self.ids[self.squares[s].1..end] {
            let h = self.valley.holds.get(i);
            // Too high or too low to hang from anywhere along it.
            if h.a.y.max(h.b.y) - p.y < *RISE.start() || h.a.y.min(h.b.y) - p.y > *RISE.end() {
                continue;
            }
            let (along, out) = h.locate(p.xz());
            let hands = h.at(along);
            let rise = hands.y - p.y;
            if !OUT.contains(&out)
                || !RISE.contains(&rise)
                || along < -PAST_END
                || along > h.len() + PAST_END
            {
                continue;
            }
            // How far the body is from where the sim would hang it from this hold: [`FACE`] out
            // from the wall, between the hold's ends, and the feet [`HANG_DROP`] below the hands
            // or on the ground where that's higher. Where the ground props the feet up, two holds
            // can hang a body at the same point (a gable's wall plate, and the verge over its
            // end), so a hundredth of how far the arms are from full stretch picks the one hung
            // from at full stretch, and is too little to count anywhere else.
            let ground = *ground.get_or_insert_with(|| self.valley.ground(p.x, p.z));
            let feet = (hands.y - HANG_DROP).max(ground);
            let off = (out - FACE).abs()
                + (along - along.clamp(0., h.len())).abs()
                + (feet - p.y).abs()
                + (rise - HANG_DROP).abs() / 100.;
            if best.is_none_or(|b| off < b.0) {
                best = Some((off, h));
            }
        }
        best.map(|(_, h)| *h)
    }
}

fn square(p: Vec2) -> (i32, i32) {
    ((p.x / SQUARE).floor() as i32, (p.y / SQUARE).floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hang_in_the_valley_finds_its_hold() {
        // Wherever the sim would hang a player along each of the valley's holds, the hold comes
        // back; and not standing on its top, nor a stride back from its wall. On its top another
        // hold may stand in reach above (the bellcote's, over the gable's coping), so a body there
        // could as well be hanging from that one; the renderer doesn't ask about a body on top.
        let valley = Arc::new(Valley::new());
        let holds = Holds::new(Arc::clone(&valley));
        assert!(!valley.holds.is_empty());
        for h in valley.holds.iter() {
            for at in [0.2, h.len() / 2., h.len() - 0.2] {
                let at = at.clamp(0., h.len());
                let hang = sim::climbing::hang_point(&valley, h, at, 0.4);
                assert_eq!(holds.hung_from(hang), Some(*h), "hanging at {hang}");
                let top = h.at(at) - Vec3::new(h.out.x, 0., h.out.y) * 0.3;
                assert_ne!(holds.hung_from(top), Some(*h), "standing on top at {top}");
                let back = hang + Vec3::new(h.out.x, 0., h.out.y) * 0.6;
                assert_eq!(holds.hung_from(back), None, "standing back at {back}");
            }
        }
    }
}
