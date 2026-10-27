//! The village's cottages as the sim knows them (#249): roofs you walk on, with the roll or cap
//! along their ridges and the chimney stacks, and the timbers up their gable ends that you climb
//! by. The art build makes each cottage from the same numbers (`build_environment.py`,
//! "Cottages"), and `cargo test -p assets` checks it still does.

use glam::{Vec2, Vec3};

use crate::masonry::{Block, Grip};
use crate::{height, Placement, Solid};

/// The walls' footprint (the plinth stands 0.2 m proud of it), their bottom and top, and how far
/// the roof runs out past the side walls (the eaves) and past the gable ends (the verges).
pub const WIDTH: f32 = 6.6;
pub const DEPTH: f32 = 5.8;
pub const WALL0: f32 = 0.6;
pub const WALL1: f32 = 3.4;
pub const EAVES: f32 = 0.55;
pub const VERGE: f32 = 0.12;

/// One of the village's cottages, in its own frame: x across, z along its ridge with its door at
/// +z, and the ground under its middle at 0. Houses scale and turn it (`Valley::houses`).
#[derive(Clone, Copy, Debug)]
pub struct Cottage {
    /// The art it's drawn with.
    pub prop: &'static str,
    /// How far the ridge rises above the walls, and how thick the roof is: thatch or tiles.
    pub rise: f32,
    pub thick: f32,
    /// The roll (thatch) or cap (tiles) along the ridge: how far either side of the ridge it
    /// meets the roof, and how far above the meeting of the slopes it stands.
    pub ridge: (f32, f32),
    /// Which side of the ridge the chimney comes up: -1 for -x, 1 for +x.
    pub chimney: f32,
}

/// The cottages, in the order houses take them (`Placement::variant`).
pub const COTTAGES: [Cottage; 3] = [
    Cottage { prop: "cottage.thatch", rise: 3.0, thick: 0.42, ridge: (0.32, 0.257), chimney: -1.0 },
    Cottage { prop: "cottage.tiles", rise: 2.5, thick: 0.14, ridge: (0.15, 0.165), chimney: 1.0 },
    Cottage { prop: "cottage.stone", rise: 2.6, thick: 0.14, ridge: (0.15, 0.165), chimney: -1.0 },
];

impl Cottage {
    /// The roof's pitch: how far it rises a metre in toward the ridge.
    pub fn slope(&self) -> f32 {
        self.rise / (WIDTH / 2.0)
    }

    /// The top of the roof over the ridge, where its slopes meet.
    pub fn ridge_top(&self) -> f32 {
        WALL1 + self.rise + self.thick / 2.0
    }

    /// Its blocks: the roof's two slopes out to the eaves and the verges, the ridge's roll or cap
    /// (two halves, each sloping down to meet the roof, gently enough to walk up), and the
    /// chimney stack under its cap. The walls stand inside the roof's slopes, which come down to
    /// the ground, so the house is ground you climb rather than a wall that only blocks.
    pub fn blocks(&self) -> [Block; 5] {
        let (slope, top) = (self.slope(), self.ridge_top());
        let (run, along) = (WIDTH / 2.0 + EAVES, DEPTH / 2.0 + VERGE);
        let (r, bump) = self.ridge;
        let steep = (bump + slope * r) / r;
        let slab = |side: f32| Block {
            center: Vec2::new(side * run / 2.0, 0.0),
            half: Vec2::new(run / 2.0, along),
            yaw: 0.0,
            top: Some(top - slope * run / 2.0),
            rise: -side * slope,
            part: "roof",
        };
        let roll = |side: f32| Block {
            center: Vec2::new(side * r / 2.0, 0.0),
            half: Vec2::new(r / 2.0, along),
            yaw: 0.0,
            top: Some(top + bump - steep * r / 2.0),
            rise: -side * steep,
            part: "ridge",
        };
        let stack = Block { center: Vec2::new(self.chimney * 1.55, -1.4), half: Vec2::splat(0.475), yaw: 0.0, top: Some(WALL1 + self.rise + 1.52), rise: 0.0, part: "chimney" };
        [slab(-1.0), slab(1.0), roll(-1.0), roll(1.0), stack]
    }

    /// Its grips, up each gable end, each within a climb of the one below: the heads of the
    /// windows and the lintel over the door, the wall plate along the top of the wall, the gable
    /// window's sill and head (on the front), and the collar across the gable, from which the
    /// verge is in reach. Each stands out at least as far as the verge, so a body hanging from it
    /// is clear of the roof's edge above it (in the sim the roof is solid down to the ground).
    pub fn grips(&self) -> [Grip; 10] {
        let face = |part, x0: f32, x1: f32, y: f32, proud: f32, out: f32| Grip {
            a: Vec3::new(x0, y, out * (DEPTH / 2.0 + proud)),
            b: Vec3::new(x1, y, out * (DEPTH / 2.0 + proud)),
            out: Vec2::new(0.0, out),
            part,
        };
        let window = WALL1 + self.rise * 0.24;
        let collar = WALL1 + self.rise * 0.48 + 0.07;
        [
            face("window head", -2.655, -1.445, WALL0 + 2.145, 0.12, 1.0),
            face("window head", 1.445, 2.655, WALL0 + 2.145, 0.12, 1.0),
            face("lintel", -0.75, 0.75, WALL0 + 2.27, 0.16, 1.0),
            face("wall plate", -3.36, 3.36, WALL1, 0.12, 1.0),
            face("gable window sill", -0.43, 0.43, window - 0.275, 0.12, 1.0),
            face("gable window head", -0.43, 0.43, window + 0.395, 0.12, 1.0),
            face("collar", -1.716, 1.716, collar, 0.12, 1.0),
            face("window head", 0.295, 1.505, WALL0 + 2.145, 0.12, -1.0),
            face("wall plate", -3.36, 3.36, WALL1, 0.12, -1.0),
            face("collar", -1.716, 1.716, collar, 0.12, -1.0),
        ]
    }
}

/// The cottage a house is.
pub fn of(house: &Placement) -> &'static Cottage {
    &COTTAGES[house.variant % COTTAGES.len()]
}

/// A point in a cottage's frame, in the valley for `house`: scaled, turned and stood on the ground
/// where the renderer stands it (`world::height` at its middle).
pub fn place(house: &Placement, p: Vec3) -> Vec3 {
    let (s, c) = house.yaw.sin_cos();
    let q = Vec2::new(p.x, p.z) * house.scale;
    let base = height(house.position.x, house.position.y);
    Vec3::new(house.position.x + c * q.x + s * q.y, base + p.y * house.scale, house.position.y - s * q.x + c * q.y)
}

/// A level direction in a cottage's frame, in the valley for `house`.
pub fn turn(house: &Placement, v: Vec2) -> Vec2 {
    let (s, c) = house.yaw.sin_cos();
    Vec2::new(c * v.x + s * v.y, -s * v.x + c * v.y)
}

/// The house's solids in the valley.
pub fn solids(house: &Placement) -> impl Iterator<Item = Solid> + '_ {
    let base = height(house.position.x, house.position.y);
    of(house).blocks().into_iter().map(move |b| {
        let c = place(house, Vec3::new(b.center.x, 0.0, b.center.y));
        Solid { center: Vec2::new(c.x, c.z), half: b.half * house.scale, yaw: house.yaw + b.yaw, top: b.top.map(|t| base + t * house.scale), rise: b.rise, roof: None }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roofs_are_walked_up_and_their_eaves_caught_in_a_jump() {
        for c in &COTTAGES {
            // Shallower than 2:1, the steepest ground walked (`sim::movement::MAX_SLOPE`), ridge and
            // all.
            let [slab, _, roll, ..] = c.blocks();
            assert!(slab.rise < 2.0 && roll.rise < 2.0, "{} pitches {} and {}", c.prop, slab.rise, roll.rise);
            // The eaves at most 3.4 m up (a jump's reach, 2.1 m of arm and 1.1 m of jump, with 0.3 m to
            // catch), and too high to step onto.
            let eave = slab.top.unwrap() + slab.rise * (-(WIDTH / 2.0 + EAVES) - slab.center.x);
            assert!((2.6..=3.4).contains(&eave), "{}'s eaves are {eave} m up", c.prop);
            // The ridge's roll meets the roof where it ends.
            let meets = |b: &Block, x: f32| b.top.unwrap() + b.rise * (x - b.center.x);
            let r = c.ridge.0;
            assert!((meets(&slab, -r) - meets(&roll, -r)).abs() < 1e-4, "{}'s ridge stands off its roof", c.prop);
        }
    }

    #[test]
    fn every_grip_stands_out_at_least_as_far_as_the_verge() {
        for c in &COTTAGES {
            for g in c.grips() {
                assert!(g.a.z == g.b.z && g.a.z.abs() >= DEPTH / 2.0 + VERGE - 1e-6, "{}'s {} is in the roof", c.prop, g.part);
            }
        }
    }

    /// The sim's reach, from `sim::climbing`: hand over hand 0.3 to 1 m up and 0.5 m to the side, a
    /// climb leap 1 to 1.8 m up and 0.8 m to the side; the hands 2.1 m above the feet, which hang
    /// 1.9 m below them; a jump 1.1 m high, and 0.3 m to catch.
    fn in_reach(dy: f32, side: f32) -> bool {
        ((0.3..=1.0).contains(&dy) && side <= 0.5) || ((1.0..=1.8).contains(&dy) && side <= 0.8)
    }

    #[test]
    fn every_cottages_gable_ends_climb_hold_by_hold_to_the_verge() {
        let v = crate::Valley::new();
        let mut banks = 0;
        for (i, h) in v.houses.iter().enumerate() {
            let c = of(h);
            let base = height(h.position.x, h.position.y);
            for out in [1.0, -1.0] {
                let face = turn(h, Vec2::new(0.0, out));
                // This end's holds: its grips and its verges, in the valley.
                let local = |p: Vec3| {
                    let d = Vec2::new(p.x, p.z) - h.position;
                    let (s, c) = h.yaw.sin_cos();
                    Vec2::new(c * d.x - s * d.y, s * d.x + c * d.y) / h.scale
                };
                let on_end = |p: Vec3| {
                    let q = local(p);
                    (q.y * out - DEPTH / 2.0).abs() < 0.2 && q.x.abs() <= WIDTH / 2.0 + EAVES + 0.01
                };
                let holds: Vec<_> = v.holds.iter().filter(|g| g.out.dot(face) > 0.99 && on_end(g.a) && on_end(g.b)).collect();
                // Where the hands can go on each, every 0.1 m short of its ends, that a body can hang
                // from: clear of everything, and with the ground well below.
                let hangs = |g: &crate::Hold| -> Vec<Vec3> {
                    let n = (g.len() / 0.1) as usize;
                    (0..=n)
                        .map(|k| g.at(0.2 + (g.len() - 0.4).max(0.0) * k as f32 / n.max(1) as f32))
                        .filter(|q| {
                            let spot = Vec2::new(q.x, q.z) + g.out * 0.4;
                            let feet = (q.y - 1.9).max(v.ground(spot.x, spot.y));
                            v.ground(spot.x, spot.y) < q.y - 1.5 && !v.blocked(spot, 0.36) && v.off_faces(spot, 0.36, feet + 1.5) == spot
                        })
                        .collect()
                };
                let level: Vec<_> = holds.iter().filter(|g| (g.a.y - g.b.y).abs() < 0.01).collect();
                let verges = holds.len() - level.len();
                assert!(level.len() >= 3 && verges >= 2, "house {i} ({}): {} grips and {verges} verges on its {out} end", c.prop, level.len());
                // The lowest grip is caught in a jump from the ground below it (3.5 m at most: the
                // hands 2.1 m above the feet at the top of a 1.1 m jump, and 0.3 m to catch), unless
                // the house stands over a bank there: the back of the one by the river.
                let lowest = level.iter().min_by(|a, b| a.a.y.total_cmp(&b.a.y)).unwrap();
                let below = |q: &Vec3| {
                    let spot = Vec2::new(q.x, q.z) + lowest.out * 0.4;
                    q.y - v.ground(spot.x, spot.y)
                };
                let from = hangs(lowest);
                let banked = from.iter().all(|q| q.y - below(q) < base - 0.5);
                if banked {
                    banks += 1;
                } else {
                    assert!(from.iter().any(|q| below(q) <= 3.4), "house {i} ({}): its lowest grip, {} m up, is out of a jump's reach", c.prop, (lowest.a.y - base) / h.scale);
                }
                // Every grip has another hold above it in reach, a grip or the verge.
                for g in &level {
                    let from = hangs(g);
                    assert!(!from.is_empty(), "house {i} ({}): nowhere to hang from its grip {} m up", c.prop, (g.a.y - base) / h.scale);
                    let next = holds.iter().filter(|o| !std::ptr::eq(**o, **g)).flat_map(|o| hangs(o)).any(|q| {
                        from.iter().any(|p| in_reach(q.y - p.y, (Vec2::new(q.x, q.z) - Vec2::new(p.x, p.z)).length()))
                    });
                    assert!(next, "house {i} ({}): nothing in reach above its grip {} m up", c.prop, (g.a.y - base) / h.scale);
                }
            }
        }
        assert_eq!(banks, 1, "only the back of the house by the river stands over a bank");
    }
}
