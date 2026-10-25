//! The shape of the land, shared by the simulation and the renderer so the two can never disagree.
//!
//! The first map is the Brambleford valley from the visual rebuild (`aldermoor-wild/Sources/World.swift`),
//! ported exactly: the same noise, the same river and paths, and the same obstacles, including the
//! trees placed from the same seeded sequence. Distances are metres; +z is south, and a yaw of 0
//! faces +z (heading = atan2(x, z)).

use glam::{Vec2, Vec3};

mod noise;
pub use noise::{fbm, hash, mix, noise, smooth, Lcg};

/// Something you can't walk through: a box on the ground, turned by `yaw`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub center: Vec2,
    pub half: Vec2,
    pub yaw: f32,
    /// The height of a flat top you can stand on (a low wall, a ruin's broken masonry). A box
    /// with a top is part of the ground rather than an obstacle: walkers meet its sides as they
    /// meet a terrain face, so they climb onto it when it's low enough and stop when it isn't.
    /// `None` for buildings and trees, which only block.
    pub top: Option<f32>,
}

impl Solid {
    /// True when a circle of `radius` at `p` overlaps the box.
    #[inline]
    pub fn overlaps(&self, p: Vec2, radius: f32) -> bool {
        let d = p - self.center;
        let (s, c) = self.yaw.sin_cos();
        let local = Vec2::new(c * d.x - s * d.y, s * d.x + c * d.y);
        local.x.abs() < self.half.x + radius && local.y.abs() < self.half.y + radius
    }

    /// Half-size of the box's footprint once turned, for broad-phase grids.
    fn reach(&self) -> f32 {
        self.half.length()
    }
}

/// A building or landmark the renderer places and the game knows about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub position: Vec2,
    pub yaw: f32,
    pub scale: f32,
}

/// The Brambleford valley: a village in a river valley, about 560 m across.
pub struct Valley {
    pub solids: Vec<Solid>,
    pub houses: Vec<Placement>,
    pub trees: Vec<Placement>,
    grid: Grid,
    /// The solids with a top, for [`Valley::ground`], and the box round them all (most of the
    /// valley has none, and `ground` is called a lot).
    tops: Grid,
    tops_box: (Vec2, Vec2),
    heights: Heights,
}

/// The walkable square around the village, in metres from its centre.
pub const BOUNDS: f32 = 280.0;
/// Where the bridge crosses the river.
pub const BRIDGE_X: f32 = 1.5;
/// The river's surface, where the renderer draws the water.
pub const WATER_LEVEL: f32 = -0.78;
/// Half-width of the river's water, either side of [`river_z`].
pub const RIVER_HALF_WIDTH: f32 = 6.0;

const HOUSES: [(f32, f32, f32, f32); 7] = [
    (-12.0, -12.0, 0.18, 1.04),
    (17.0, -22.0, -0.2, 1.1),
    (-14.0, -36.0, 0.12, 0.95),
    (22.0, -43.0, -0.32, 0.91),
    (-33.0, -23.0, 0.65, 0.87),
    (7.0, -78.0, 0.05, 0.95),
    (27.0, -76.0, -0.4, 0.86),
];

impl Valley {
    pub fn new() -> Valley {
        let mut solids = Vec::with_capacity(128);
        let houses: Vec<Placement> =
            HOUSES.iter().map(|&(x, z, yaw, scale)| Placement { position: Vec2::new(x, z), yaw, scale }).collect();
        for h in &houses {
            solids.push(Solid { center: h.position, half: Vec2::new(3.5, 3.1) * h.scale, yaw: h.yaw, top: None });
        }
        // The windmill and the shed by the north fence.
        solids.push(Solid { center: Vec2::new(49.0, -84.0), half: Vec2::splat(2.5), yaw: 0.0, top: None });
        solids.push(Solid { center: Vec2::new(-26.0, -48.0), half: Vec2::new(4.4, 3.7), yaw: 0.0, top: None });

        // Trees, from the same sequence as the renderer: candidates first, then a size and a turn
        // for each tree that fits.
        let mut r = Lcg::new(3001);
        let mut candidates = vec![
            Vec2::new(-16.0, 24.0),
            Vec2::new(21.0, 18.0),
            Vec2::new(-27.0, -4.0),
            Vec2::new(34.0, -25.0),
            Vec2::new(-39.0, -50.0),
            Vec2::new(14.0, -85.0),
        ];
        for _ in 0..115 {
            let x = r.range(-180.0, 180.0);
            let z = r.range(-180.0, 90.0);
            candidates.push(Vec2::new(x, z));
        }
        let mut trees = Vec::new();
        for p in candidates {
            if (p.y - river_z(p.x)).abs() < 12.0
                || path_distance(p.x, p.y) < 5.0
                || blocked_by(&solids, p, 3.0)
            {
                continue;
            }
            let scale = r.range(0.85, 1.6);
            let yaw = r.range(0.0, std::f32::consts::TAU);
            trees.push(Placement { position: p, yaw, scale });
            solids.push(Solid { center: p, half: Vec2::splat(scale * 0.42), yaw: 0.0, top: None });
        }
        // The bridge's parapets, either side of its deck (after the trees, which are placed
        // from the solids before them).
        for side in [-1.0, 1.0] {
            solids.push(Solid { center: Vec2::new(BRIDGE_X + side * 2.45, river_z(BRIDGE_X)), half: Vec2::new(0.2, 9.2), yaw: 0.0, top: None });
        }
        let (grid, tops) = Grid::split(&solids);
        let tops_box = tops_box(&solids);
        Valley { solids, houses, trees, grid, tops, tops_box, heights: Heights::build() }
    }

    /// Adds a solid after the valley is built (for tests, and props placed by later layers).
    pub fn add_solid(&mut self, s: Solid) {
        self.solids.push(s);
        (self.grid, self.tops) = Grid::split(&self.solids);
        self.tops_box = tops_box(&self.solids);
    }

    /// The ground a walker stands on, including the bridge deck: the terrain as it is drawn
    /// (see [`Valley::surface`]), so feet land on the triangles you see.
    pub fn ground(&self, x: f32, z: f32) -> f32 {
        let dz = z - river_z(BRIDGE_X);
        if (x - BRIDGE_X).abs() < 2.3 && dz.abs() < 9.1 {
            return 1.4 + 1.6 * (1.0 - (dz / 9.0).powf(2.0));
        }
        let p = Vec2::new(x, z);
        let mut ground = self.surface(x, z);
        let (lo, hi) = self.tops_box;
        if p.cmplt(lo).any() || p.cmpgt(hi).any() {
            return ground;
        }
        for i in self.tops.near(p, 0.0) {
            let s = &self.solids[i as usize];
            if let Some(top) = s.top
                && top > ground
                && s.overlaps(p, 0.0)
            {
                ground = top;
            }
        }
        ground
    }

    /// The terrain mesh's height at (x, z): [`height`] sampled every [`TERRAIN_STEP`] metres and
    /// joined in the same triangles the renderer draws. Much cheaper than [`height`], which
    /// evaluates the noise from scratch.
    pub fn surface(&self, x: f32, z: f32) -> f32 {
        self.heights.at(x, z)
    }

    /// True when a walker of `radius` can't stand at `p`: an obstacle or the edge of the map.
    /// The river isn't a wall: walkers swim it (see [`Valley::water_depth`]). Nor are solids with a
    /// top: they're ground, so their sides are faces (see [`Valley::ground`]).
    pub fn blocked(&self, p: Vec2, radius: f32) -> bool {
        if p.x.abs() > BOUNDS || p.y.abs() > BOUNDS {
            return true;
        }
        self.grid.near(p, radius).any(|i| self.solids[i as usize].overlaps(p, radius))
    }

    /// How deep the river is at (x, z): from its surface down to the ground (or the bridge
    /// deck, which is above it). 0 away from the river.
    pub fn water_depth(&self, x: f32, z: f32) -> f32 {
        if (z - river_z(x)).abs() > RIVER_HALF_WIDTH {
            return 0.0;
        }
        (WATER_LEVEL - self.ground(x, z)).max(0.0)
    }

    /// Moves `from` by `delta`, sliding along anything in the way (in short steps so nothing is
    /// tunnelled through).
    pub fn slide(&self, from: Vec2, delta: Vec2, radius: f32) -> Vec2 {
        let len = delta.length();
        if len < 1e-6 {
            return from;
        }
        let steps = (len / 0.2).ceil().max(1.0) as usize;
        let d = delta / steps as f32;
        let mut p = from;
        for _ in 0..steps {
            let next = p + d;
            if !self.blocked(next, radius) {
                p = next;
            } else if !self.blocked(Vec2::new(next.x, p.y), radius) {
                p.x = next.x;
            } else if !self.blocked(Vec2::new(p.x, next.y), radius) {
                p.y = next.y;
            } else {
                break;
            }
        }
        p
    }
}

impl Default for Valley {
    fn default() -> Self {
        Self::new()
    }
}

/// The box round every solid with a top (empty, min above max, when there are none).
fn tops_box(solids: &[Solid]) -> (Vec2, Vec2) {
    solids.iter().filter(|s| s.top.is_some()).fold((Vec2::INFINITY, Vec2::NEG_INFINITY), |(lo, hi), s| {
        let r = Vec2::splat(s.reach());
        (lo.min(s.center - r), hi.max(s.center + r))
    })
}

fn blocked_by(solids: &[Solid], p: Vec2, radius: f32) -> bool {
    if (p.y - river_z(p.x)).abs() < 6.5 && (p.x - BRIDGE_X).abs() > 2.5 {
        return true;
    }
    solids.iter().any(|s| s.top.is_none() && s.overlaps(p, radius))
}

/// The river's centre line: z for a given x.
#[inline]
pub fn river_z(x: f32) -> f32 {
    -58.0 + (x * 0.021).sin() * 7.0
}

/// Terrain height at (x, z), before the bridge.
pub fn height(x: f32, z: f32) -> f32 {
    let mut h = 4.0 + (fbm(x * 0.018, z * 0.018, 41) - 0.5) * 7.0 + (x * 0.025 + z * 0.012).sin() * 2.0;
    h += (-((z - 37.0) / 32.0).powf(2.0)).exp() * 7.0;
    let village = (-(x * x / 2400.0 + (z + 24.0).powf(2.0) / 1500.0)).exp();
    h = mix(h, 3.8 + 0.008 * x + 0.015 * (z + 24.0), village * 0.86);
    let rd = (z - river_z(x)).abs();
    h = mix(h, -1.55, 1.0 - smooth(4.0, 13.0, rd));
    let hills = smooth(80.0, 175.0, Vec2::new(x * 0.85, z + 30.0).length());
    let ridge = fbm(x * 0.009, z * 0.009, 15) * 28.0 + 8.0 + (x * 0.014).sin() * 7.0;
    let mut rise = hills * ridge;
    // Eroded limestone terraces frame the settlement.
    let ledge = (-(((x + 66.0) / 25.0).powf(2.0) + ((z + 30.0) / 65.0).powf(2.0))).exp();
    rise += smooth(0.23, 0.42, ledge) * 17.0 + smooth(0.67, 0.84, ledge) * 5.0;
    let east = (-(((x - 76.0) / 30.0).powf(2.0) + ((z + 85.0) / 50.0).powf(2.0))).exp();
    rise += smooth(0.26, 0.45, east) * 26.0;
    // The river has cut a gorge through the high ground: its bed and banks stay as they are in the
    // valley, and the walls flare wider the more rock there is to cut, so they stay climbable-steep
    // rather than sheer.
    h + rise * smooth(GORGE_FLOOR, GORGE_FLOOR + 4.0 + 0.8 * rise, rd)
}

/// Half-width of the river's bed and banks where it cuts through the hills: everything nearer
/// the centre line than this is valley floor, never hilltop.
const GORGE_FLOOR: f32 = 9.0;

/// The terrain's surface normal at (x, z).
pub fn normal(x: f32, z: f32) -> Vec3 {
    Vec3::new(height(x - 0.5, z) - height(x + 0.5, z), 1.0, height(x, z - 0.5) - height(x, z + 0.5)).normalize()
}

/// Distance to the nearest road, street or the village square.
pub fn path_distance(x: f32, z: f32) -> f32 {
    let main = (x - (4.0 + (z * 0.045).sin() * 5.0)).abs();
    let side = (z - (-24.0 + (x * 0.065).sin() * 3.0)).abs();
    let street = if x.abs() < 48.0 { side } else { 999.0 };
    let square = Vec2::new(x - 2.0, z + 24.0).length() - 5.0;
    main.min(street).min(square + 2.0)
}

/// Spacing of the terrain mesh's vertices, in metres. The renderer builds its mesh on this grid.
pub const TERRAIN_STEP: f32 = 2.0;
const HEIGHTS_SIDE: usize = (BOUNDS * 2.0 / TERRAIN_STEP) as usize + 1;

/// [`height`] at every terrain vertex, from (-BOUNDS, -BOUNDS) in rows of increasing z.
struct Heights {
    samples: Vec<f32>,
}

impl Heights {
    fn build() -> Heights {
        let mut samples = Vec::with_capacity(HEIGHTS_SIDE * HEIGHTS_SIDE);
        for j in 0..HEIGHTS_SIDE {
            for i in 0..HEIGHTS_SIDE {
                samples.push(height(i as f32 * TERRAIN_STEP - BOUNDS, j as f32 * TERRAIN_STEP - BOUNDS));
            }
        }
        Heights { samples }
    }

    /// Each grid square is split along its (x + 1, z)–(x, z + 1) diagonal, as the mesh's
    /// triangles are. Outside the grid, the nearest edge.
    #[inline]
    fn at(&self, x: f32, z: f32) -> f32 {
        let last = (HEIGHTS_SIDE - 1) as f32;
        let gx = ((x + BOUNDS) / TERRAIN_STEP).clamp(0.0, last);
        let gz = ((z + BOUNDS) / TERRAIN_STEP).clamp(0.0, last);
        let (i, j) = ((gx as usize).min(HEIGHTS_SIDE - 2), (gz as usize).min(HEIGHTS_SIDE - 2));
        let (u, v) = (gx - i as f32, gz - j as f32);
        let h = |di: usize, dj: usize| self.samples[(j + dj) * HEIGHTS_SIDE + i + di];
        if u + v <= 1.0 {
            h(0, 0) + (h(1, 0) - h(0, 0)) * u + (h(0, 1) - h(0, 0)) * v
        } else {
            h(1, 1) + (h(0, 1) - h(1, 1)) * (1.0 - u) + (h(1, 0) - h(1, 1)) * (1.0 - v)
        }
    }
}

/// Obstacles bucketed into 16 m cells, so a collision check looks at a handful, not all of them.
struct Grid {
    cells: Vec<Vec<u16>>,
}

const CELL: f32 = 16.0;
const SIDE: usize = ((BOUNDS * 2.0 / CELL) as usize) + 2;

impl Grid {
    /// One grid of the solids that block, and one of those with a top.
    fn split(solids: &[Solid]) -> (Grid, Grid) {
        (Grid::build(solids, false), Grid::build(solids, true))
    }

    fn build(solids: &[Solid], tops: bool) -> Grid {
        let mut cells = vec![Vec::new(); SIDE * SIDE];
        for (i, s) in solids.iter().enumerate().filter(|(_, s)| s.top.is_some() == tops) {
            let r = s.reach() + 1.5;
            let (x0, z0) = Self::cell(s.center - Vec2::splat(r));
            let (x1, z1) = Self::cell(s.center + Vec2::splat(r));
            for z in z0..=z1 {
                for x in x0..=x1 {
                    cells[z * SIDE + x].push(i as u16);
                }
            }
        }
        Grid { cells }
    }

    #[inline]
    fn cell(p: Vec2) -> (usize, usize) {
        let f = |v: f32| (((v + BOUNDS) / CELL).floor().max(0.0) as usize).min(SIDE - 1);
        (f(p.x), f(p.y))
    }

    /// Obstacles that could touch a circle at `p` (radius up to 1.5 m).
    fn near(&self, p: Vec2, radius: f32) -> impl Iterator<Item = u16> + '_ {
        debug_assert!(radius <= 1.5);
        let (x, z) = Self::cell(p);
        self.cells[z * SIDE + x].iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values from the Swift original (aldermoor-wild), computed on 2026-10-23, except where the
    // river now cuts through the west ledge (#41): (-40, -60) sat 0.76 m up on the buried river.
    // The river no longer blocks (#65: you swim it), so (-40, -60) in it isn't blocked either.
    const SPOTS: [(f32, f32, f32, f32, f32, bool); 9] = [
        (0.0, 0.0, 4.9790773, -58.0, 4.0, false),
        (6.0, 28.0, 9.586473, -57.12033, 2.7604523, false),
        (-40.0, -60.0, -1.55, -63.2125, 34.453495, false),
        (120.0, -20.0, 9.364937, -53.923687, 115.06778, false),
        (-70.0, -30.0, 23.01629, -64.96447, 69.12138, false),
        (76.0, -85.0, 31.035648, -51.002224, 68.842804, false),
        (1.5, -58.0, -1.5499997, -57.779537, 0.03453374, false),
        (33.3, -71.2, 3.6076798, -53.494225, 28.988165, false),
        (-150.0, 60.0, 23.259926, -57.94115, 156.1369, false),
    ];

    #[test]
    fn heights_rivers_and_paths_match_the_original() {
        let v = Valley::new();
        for (x, z, h, r, p, b) in SPOTS {
            assert!((height(x, z) - h).abs() < 1e-3, "height at ({x}, {z}): {} vs {h}", height(x, z));
            assert!((river_z(x) - r).abs() < 1e-4, "river at {x}");
            assert!((path_distance(x, z) - p).abs() < 1e-3, "path at ({x}, {z})");
            assert_eq!(v.blocked(Vec2::new(x, z), 0.3), b, "blocked at ({x}, {z})");
        }
    }

    #[test]
    fn the_river_runs_open_across_the_whole_valley() {
        // Through the hills too (#41): the bed is under water everywhere, and nothing the river
        // blocks rises above the water's edge, so there is no invisible wall on a hilltop.
        let mut x = -BOUNDS;
        while x <= BOUNDS {
            let r = river_z(x);
            assert!(height(x, r) < -1.5, "bed at x = {x} is {}", height(x, r));
            for d in [-6.5, -4.0, 4.0, 6.5] {
                assert!(height(x, r + d) < 0.0, "bank at ({x}, {}) is {}", r + d, height(x, r + d));
            }
            x += 0.5;
        }
        // The hills still stand either side of the gorge.
        assert!(height(-66.0, river_z(-66.0) + 40.0) > 20.0);
        assert!(height(76.0, river_z(76.0) - 40.0) > 30.0);
    }

    #[test]
    fn the_same_trees_grow_in_the_same_places() {
        let v = Valley::new();
        assert_eq!(v.trees.len(), 106);
        // 9 buildings, 106 trees, then the bridge's two parapets.
        assert_eq!(v.solids.len(), 117);
        let last = v.solids[114];
        assert!((last.center - Vec2::new(-52.010445, -2.722763)).length() < 1e-3);
        assert!((last.half.x - 0.3776364).abs() < 1e-4);
    }

    #[test]
    fn the_ground_is_the_drawn_terrain() {
        let v = Valley::new();
        // On a vertex it is the height itself; between vertices it stays on the triangles, so it
        // never strays far from the true height.
        for (x, z) in [(0.0, 0.0), (-66.0, -30.0), (76.0, -84.0), (-BOUNDS, -BOUNDS), (BOUNDS, BOUNDS)] {
            assert!((v.surface(x, z) - height(x, z)).abs() < 1e-4, "vertex ({x}, {z})");
        }
        let mut worst: f32 = 0.0;
        let mut x = -BOUNDS;
        while x <= BOUNDS {
            let mut z = -BOUNDS;
            while z <= BOUNDS {
                worst = worst.max((v.surface(x, z) - height(x, z)).abs());
                z += 0.37;
            }
            x += 0.37;
        }
        assert!(worst < 2.0, "the mesh strays {worst} m from the height field");
        // Each square splits along the same diagonal as the renderer's triangles: halfway along
        // it, the surface is the mean of its two ends.
        let (x0, z0) = (10.0, 20.0);
        let mid = v.surface(x0 + 1.0, z0 + 1.0);
        let ends = (height(x0 + 2.0, z0) + height(x0, z0 + 2.0)) / 2.0;
        assert!((mid - ends).abs() < 1e-4, "{mid} vs {ends}");
    }

    #[test]
    fn walls_stop_walkers_and_the_bridge_crosses_the_river() {
        let v = Valley::new();
        // Walking north into the first cottage stops at its wall.
        let start = Vec2::new(-12.0, 0.0);
        let end = v.slide(start, Vec2::new(0.0, -20.0), 0.4);
        assert!(end.y > -9.5 && end.y < -5.0, "stopped at {end}");
        // The river is deep in the middle and dry on the bridge.
        assert!(v.water_depth(20.0, river_z(20.0)) > 0.7);
        assert_eq!(v.water_depth(20.0, river_z(20.0) + 8.0), 0.0);
        assert_eq!(v.water_depth(BRIDGE_X, river_z(BRIDGE_X)), 0.0);
        assert!(v.ground(BRIDGE_X, river_z(BRIDGE_X)) > 2.9);
    }
}
