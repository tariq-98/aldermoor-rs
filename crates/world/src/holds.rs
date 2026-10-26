//! Handholds: the edges a climber's hands close on (docs/MOVEMENT.md, "Climbing"). Every top edge
//! of a [`Solid`] with a top that drops far enough to hang from is one, joined along straight
//! walls (the ruin's courses and coping, the cottages' eaves and verges), and so is every grip on
//! the masonry and the cottages (a sill, a keystone, a window head: [`crate::Grip`]). Cliff ledges
//! join them as the routes through the valley are built (#249).

use std::collections::HashMap;

use glam::{Vec2, Vec3, Vec3Swizzles};

use crate::{cottage, Solid, Valley};

/// An edge that drops less than this is climbed from the ground, not hung from.
pub const HANG_MIN_DROP: f32 = 1.6;
/// Edges are checked this often along their length for where the drop is deep enough.
const SAMPLE: f32 = 0.25;
/// Shorter runs than this aren't worth a grip.
const MIN_LENGTH: f32 = 0.2;
const CELL: f32 = 4.0;

/// A straight edge you can hang from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    /// Its ends, `a` to `b` running to the climber's right as they face the wall. `y` is the edge's
    /// height, where the hands go.
    pub a: Vec3,
    pub b: Vec3,
    /// Level and out of the wall: the way a climber hanging from it has their back to.
    pub out: Vec2,
    /// The wall goes on below, so the feet find it. False under an overhang (a free hang).
    pub braced: bool,
}

impl Hold {
    /// Its length along the wall, level.
    pub fn len(&self) -> f32 {
        (self.b - self.a).xz().length()
    }

    /// Level, along the wall from `a` to `b`.
    pub fn along(&self) -> Vec2 {
        (self.b - self.a).xz().normalize_or_zero()
    }

    /// The point `s` metres along from `a`, kept on the hold.
    pub fn at(&self, s: f32) -> Vec3 {
        let len = self.len();
        if len < 1e-6 {
            return self.a;
        }
        self.a.lerp(self.b, (s / len).clamp(0.0, 1.0))
    }

    /// How far along the hold `p` lies (not kept on it), and how far out from the wall.
    pub fn locate(&self, p: Vec2) -> (f32, f32) {
        let d = p - self.a.xz();
        (d.dot(self.along()), d.dot(self.out))
    }

    /// Level distance from `p` to the nearest point of the hold.
    pub fn distance(&self, p: Vec2) -> f32 {
        let (s, _) = self.locate(p);
        (self.at(s).xz() - p).length()
    }
}

/// Every hold in a valley, with a grid to find the ones near a point.
#[derive(Clone, Default)]
pub struct Holds {
    list: Vec<Hold>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

impl Holds {
    /// The holds along the tops of `valley`'s solids. Built once the valley's ground is.
    pub fn build(valley: &Valley) -> Holds {
        let mut list = Vec::new();
        for s in valley.solids.iter().filter(|s| s.top.is_some()) {
            for side in 0..4 {
                edge_holds(valley, s, side, &mut list);
            }
        }
        let grips = valley.masonry.iter().flat_map(|m| m.grips.iter().map(|g| (m.place(g.a), m.place(g.b), m.turn(g.out))));
        let timbers = valley.houses.iter().flat_map(|h| cottage::of(h).grips().map(|g| (cottage::place(h, g.a), cottage::place(h, g.b), cottage::turn(h, g.out))));
        for (mut a, mut b, out) in grips.chain(timbers) {
            // Run to the right of a climber facing the wall.
            if (b - a).xz().dot(Vec2::new(out.y, -out.x)) < 0.0 {
                std::mem::swap(&mut a, &mut b);
            }
            list.push(Hold { a, b, out, braced: true });
        }
        join(&mut list);
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, h) in list.iter().enumerate() {
            let (lo, hi) = (h.a.xz().min(h.b.xz()), h.a.xz().max(h.b.xz()));
            let ((x0, z0), (x1, z1)) = (cell(lo), cell(hi));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    cells.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        Holds { list, cells }
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn get(&self, i: u32) -> &Hold {
        &self.list[i as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Hold> + '_ {
        self.list.iter()
    }

    /// The holds that come within `r` of `p` (level distance), each once, nearest first.
    pub fn near(&self, p: Vec2, r: f32) -> Vec<u32> {
        let ((x0, z0), (x1, z1)) = (cell(p - Vec2::splat(r)), cell(p + Vec2::splat(r)));
        let mut found: Vec<(f32, u32)> = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &i in self.cells.get(&(x, z)).into_iter().flatten() {
                    let d = self.list[i as usize].distance(p);
                    if d <= r && !found.iter().any(|f| f.1 == i) {
                        found.push((d, i));
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.into_iter().map(|(_, i)| i).collect()
    }
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// The runs along one side of a box's top (0..4: +x, -x, +z, -z in its own frame) with a drop
/// deep enough to hang from, nothing taller standing on the edge, and room for a body outside.
fn edge_holds(valley: &Valley, s: &Solid, side: usize, list: &mut Vec<Hold>) {
    let Some(top) = s.top else { return };
    let (hx, hz) = (s.half.x, s.half.y);
    let (normal, from, to) = match side {
        0 => (Vec2::X, Vec2::new(hx, -hz), Vec2::new(hx, hz)),
        1 => (Vec2::NEG_X, Vec2::new(-hx, -hz), Vec2::new(-hx, hz)),
        2 => (Vec2::Y, Vec2::new(-hx, hz), Vec2::new(hx, hz)),
        _ => (Vec2::NEG_Y, Vec2::new(-hx, -hz), Vec2::new(hx, -hz)),
    };
    let (sn, c) = s.yaw.sin_cos();
    let turn = |v: Vec2| Vec2::new(c * v.x + sn * v.y, -sn * v.x + c * v.y);
    let out = turn(normal);
    let point = |l: Vec2| {
        let w = s.center + turn(l);
        Vec3::new(w.x, top + s.rise * l.x, w.y)
    };
    let (mut a, mut b) = (point(from), point(to));
    // Run to the right of a climber facing the wall.
    if (b - a).xz().dot(Vec2::new(out.y, -out.x)) < 0.0 {
        std::mem::swap(&mut a, &mut b);
    }
    let len = (b - a).xz().length();
    let n = (len / SAMPLE).ceil().max(1.0) as usize;
    let ok = |k: usize| {
        let q = a.lerp(b, k as f32 / n as f32);
        let edge = q.xz();
        let (outside, inside) = (edge + out * 0.3, edge - out * 0.05);
        let drop = q.y - valley.ground(outside.x, outside.y);
        let exposed = valley.ground(inside.x, inside.y) <= q.y + 0.05;
        drop >= HANG_MIN_DROP && exposed && !valley.blocked(edge + out * 0.45, 0.3)
    };
    let mut start = None;
    for k in 0..=n + 1 {
        match (k <= n && ok(k), start) {
            (true, None) => start = Some(k),
            (false, Some(k0)) => {
                let (p, q) = (a.lerp(b, k0 as f32 / n as f32), a.lerp(b, (k - 1) as f32 / n as f32));
                if (q - p).xz().length() >= MIN_LENGTH {
                    list.push(Hold { a: p, b: q, out, braced: true });
                }
                start = None;
            }
            _ => {}
        }
    }
}

/// Joins holds that carry straight on from one another (a wall built of several boxes), so a
/// climber shimmies along one hold rather than hopping between pieces.
fn join(list: &mut Vec<Hold>) {
    let mut i = 0;
    while i < list.len() {
        let mut merged = false;
        for j in 0..list.len() {
            if i == j {
                continue;
            }
            let (h, g) = (list[i], list[j]);
            let straight = h.out.dot(g.out) > 0.999
                && h.along().dot(g.along()) > 0.999
                && ((h.b - h.a).y / h.len().max(1e-6) - (g.b - g.a).y / g.len().max(1e-6)).abs() < 0.01;
            if straight && (h.b - g.a).length() < 0.05 {
                list[i].b = g.b;
                list.remove(j);
                merged = true;
                break;
            }
        }
        if merged {
            i = 0;
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box with its top `rise` metres above the ground at its middle, standing on the road by
    /// the field gate east of the village.
    fn with_box(center: Vec2, half: Vec2, rise: f32) -> (Valley, f32) {
        let mut v = Valley::new();
        let top = v.ground(center.x, center.y) + rise;
        v.add_solid(Solid { center, half, yaw: 0.0, top: Some(top), rise: 0.0 });
        (v, top)
    }

    fn holds_at(v: &Valley, top: f32) -> Vec<Hold> {
        v.holds.iter().filter(|h| (h.a.y - top).abs() < 1e-3).copied().collect()
    }

    #[test]
    fn a_tall_box_has_a_hold_along_each_side_of_its_top() {
        let (v, top) = with_box(Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 3.4);
        let holds = holds_at(&v, top);
        assert_eq!(holds.len(), 4, "{holds:?}");
        for h in &holds {
            let long = (h.out.x.abs() - 1.0).abs() < 1e-4;
            assert!((h.len() - if long { 6.0 } else { 0.9 }).abs() < 1e-3, "{h:?}");
            assert!(h.braced);
            // Running to the right of a climber facing the wall.
            assert!(h.along().dot(Vec2::new(h.out.y, -h.out.x)) > 0.99);
        }
    }

    #[test]
    fn a_low_box_has_no_holds_and_a_box_against_a_taller_one_loses_that_side() {
        // (Short, as the road slopes: a long wall's downhill end stands taller.)
        let (v, top) = with_box(Vec2::new(8.0, 12.0), Vec2::new(0.45, 1.0), 1.2);
        assert!(holds_at(&v, top).is_empty(), "a 1.2 m wall is climbed onto, not hung from");
        // A 2.4 m step against the 3.4 m wall's west face: no hold on its east side, where the
        // taller wall stands, and the taller wall keeps its own hold above the step.
        let (mut v, high) = with_box(Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 3.4);
        let step = high - 1.0;
        v.add_solid(Solid { center: Vec2::new(7.1, 12.0), half: Vec2::new(0.45, 3.0), yaw: 0.0, top: Some(step), rise: 0.0 });
        let low = holds_at(&v, step);
        assert!(!low.iter().any(|h| h.out.x > 0.5), "a hold facing the taller wall: {low:?}");
        assert!(low.iter().any(|h| h.out.x < -0.5), "lost the step's west hold");
        assert!(holds_at(&v, high).iter().all(|h| h.out.x > -0.5), "the taller wall's west hold drops only 1 m onto the step");
    }

    #[test]
    fn boxes_in_a_straight_line_make_one_hold() {
        let mut v = Valley::new();
        let top = v.ground(8.0, 12.0) + 3.4;
        for z in [10.0, 12.0, 14.0] {
            v.add_solid(Solid { center: Vec2::new(8.0, z), half: Vec2::new(0.45, 1.0), yaw: 0.0, top: Some(top), rise: 0.0 });
        }
        let west: Vec<Hold> = holds_at(&v, top).into_iter().filter(|h| h.out.x < -0.5).collect();
        assert_eq!(west.len(), 1, "{west:?}");
        assert!((west[0].len() - 6.0).abs() < 1e-3);
    }

    #[test]
    fn near_finds_holds_by_distance() {
        let (v, top) = with_box(Vec2::new(8.0, 12.0), Vec2::new(0.45, 3.0), 3.4);
        let near = v.holds.near(Vec2::new(7.0, 12.0), 0.6);
        assert_eq!(near.len(), 1);
        let h = v.holds.get(near[0]);
        assert!(h.out.x < -0.99 && (h.a.y - top).abs() < 1e-3);
        assert!(v.holds.near(Vec2::new(5.0, 12.0), 0.6).is_empty());
    }

    #[test]
    fn top_ahead_finds_a_wall_too_thin_for_the_ground_samples() {
        let (v, top) = with_box(Vec2::new(8.0, 12.0), Vec2::new(0.2, 2.0), 1.0);
        let (p, h) = v.top_ahead(Vec2::new(7.7, 12.0), Vec2::X, 0.6).expect("missed the wall");
        assert!((p - Vec2::new(8.0, 12.0)).length() < 0.01 && (h - top).abs() < 1e-4, "{p} {h}");
        assert!(v.top_ahead(Vec2::new(7.7, 12.0), -Vec2::X, 0.6).is_none());
    }
}
