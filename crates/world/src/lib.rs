//! The shape of the land, shared by the simulation and the renderer so the two can never disagree.
//!
//! The first map is the Brambleford valley from the visual rebuild (`aldermoor-wild/Sources/World.swift`),
//! ported exactly: the same noise, the same river and paths, and the same obstacles, including the
//! trees placed from the same seeded sequence. Distances are metres; +z is south, and a yaw of 0
//! faces +z (heading = atan2(x, z)).

use glam::{Vec2, Vec3};

mod holds;
mod masonry;
mod noise;
pub use holds::{Hold, Holds, HANG_MIN_DROP};
pub use masonry::{Block, Masonry, FIELD_WALL_HEIGHT, FIELD_WALL_WIDTH, RUIN_AT, RUIN_YAW};
pub use noise::{fbm, hash, mix, noise, smooth, Lcg};

/// Something you can't walk through: a box on the ground, turned by `yaw`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub center: Vec2,
    pub half: Vec2,
    pub yaw: f32,
    /// The height of a top you can stand on (a low wall, a ruin's broken masonry), at the box's
    /// centre. A box with a top is part of the ground rather than an obstacle: walkers meet its
    /// sides as they meet a terrain face, so they climb onto it when it's low enough and stop when
    /// it isn't. `None` for buildings and trees, which only block.
    pub top: Option<f32>,
    /// How much the top rises per metre along the box's own x: a wall following a slope, which
    /// you walk up rather than climb a step at a time. 0 for a flat top.
    pub rise: f32,
}

impl Solid {
    /// True when a circle of `radius` at `p` overlaps the box.
    #[inline]
    pub fn overlaps(&self, p: Vec2, radius: f32) -> bool {
        let local = self.local(p);
        local.x.abs() < self.half.x + radius && local.y.abs() < self.half.y + radius
    }

    /// The height of the top at `p`, if the box has one and `p` is on it.
    #[inline]
    pub fn top_at(&self, p: Vec2) -> Option<f32> {
        let top = self.top?;
        if (p - self.center).length_squared() > self.half.length_squared() {
            return None;
        }
        let local = self.local(p);
        (local.x.abs() < self.half.x && local.y.abs() < self.half.y).then_some(top + self.rise * local.x)
    }

    /// `p` in the box's own frame.
    #[inline]
    fn local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(c * d.x - s * d.y, s * d.x + c * d.y)
    }

    /// A direction in the box's own frame, turned back into the world's.
    #[inline]
    fn unturn(&self, v: Vec2) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(c * v.x + s * v.y, -s * v.x + c * v.y)
    }

    /// The highest its top goes, if it has one.
    #[inline]
    fn top_max(&self) -> Option<f32> {
        Some(self.top? + self.rise.abs() * self.half.x)
    }

    /// Whether a circle of `radius` at `p` might touch the box: a cheap test before
    /// [`Solid::overlaps`].
    #[inline]
    fn near(&self, p: Vec2, radius: f32) -> bool {
        (p - self.center).length_squared() < (self.reach() + radius).powi(2)
    }

    /// The height of the top where the box comes nearest `p`, if it has one.
    #[inline]
    fn top_nearest(&self, p: Vec2) -> Option<f32> {
        Some(self.top? + self.rise * self.local(p).x.clamp(-self.half.x, self.half.x))
    }

    /// Where a circle of `radius` at `p`, overlapping the box, stops touching it: moved straight
    /// out through the side it's nearest.
    fn push_out(&self, p: Vec2, radius: f32) -> Vec2 {
        let local = self.local(p);
        // A hair past touching, so the turn back into the world's frame can't leave it overlapping.
        let reach = self.half + Vec2::splat(radius + 1e-4);
        let out = reach - local.abs();
        let moved = if out.x < out.y { Vec2::new(reach.x.copysign(local.x), local.y) } else { Vec2::new(local.x, reach.y.copysign(local.y)) };
        p + self.unturn(moved - local)
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
    /// Which of its family's art the renderer draws, kept when placements before it are dropped
    /// so a tree doesn't change look when another one goes.
    pub variant: usize,
}

/// The Brambleford valley: a village in a river valley, about 560 m across.
#[derive(Clone)]
pub struct Valley {
    pub solids: Vec<Solid>,
    pub houses: Vec<Placement>,
    pub trees: Vec<Placement>,
    /// The field walls and the ruin: what the renderer draws, and the solids at the end of
    /// `solids` that you climb onto (or that stop you).
    pub masonry: Vec<Masonry>,
    /// The edges you can hang from (docs/MOVEMENT.md, "Climbing").
    pub holds: Holds,
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
            HOUSES.iter().enumerate().map(|(variant, &(x, z, yaw, scale))| Placement { position: Vec2::new(x, z), yaw, scale, variant }).collect();
        for h in &houses {
            solids.push(Solid { center: h.position, half: Vec2::new(3.5, 3.1) * h.scale, yaw: h.yaw, top: None, rise: 0.0 });
        }
        // The windmill and the shed by the north fence.
        solids.push(Solid { center: Vec2::new(49.0, -84.0), half: Vec2::splat(2.5), yaw: 0.0, top: None, rise: 0.0 });
        solids.push(Solid { center: Vec2::new(-26.0, -48.0), half: Vec2::new(4.4, 3.7), yaw: 0.0, top: None, rise: 0.0 });

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
        // The art each tree had before trees left cliff faces (#40), so the rest keep theirs.
        let mut variant = 0;
        for p in candidates {
            if (p.y - river_z(p.x)).abs() < 12.0
                || path_distance(p.x, p.y) < 5.0
                || blocked_by(&solids, p, 3.0)
            {
                continue;
            }
            let scale = r.range(0.85, 1.6);
            let yaw = r.range(0.0, std::f32::consts::TAU);
            let art = variant;
            variant += 1;
            // Not out of a cliff face. Checked after the draws so the trees that stay keep their
            // size and turn.
            if too_steep_for_a_tree(p) {
                continue;
            }
            trees.push(Placement { position: p, yaw, scale, variant: art });
            solids.push(Solid { center: p, half: Vec2::splat(scale * 0.42), yaw: 0.0, top: None, rise: 0.0 });
        }
        // The bridge's parapets, either side of its deck (after the trees, which are placed
        // from the solids before them).
        for side in [-1.0, 1.0] {
            solids.push(Solid { center: Vec2::new(BRIDGE_X + side * 2.45, river_z(BRIDGE_X)), half: Vec2::new(0.2, 9.2), yaw: 0.0, top: None, rise: 0.0 });
        }
        // Then the walls and the ruin, standing on the drawn ground.
        let heights = Heights::build();
        let mut masonry = masonry::field_walls(|x, z| heights.at(x, z));
        masonry.push(masonry::ruin(|x, z| heights.at(x, z)));
        solids.extend(masonry.iter().flat_map(Masonry::solids));
        let (grid, tops) = Grid::split(&solids);
        let tops_box = tops_box(&solids);
        let mut valley = Valley { solids, houses, trees, masonry, holds: Holds::default(), grid, tops, tops_box, heights };
        valley.holds = Holds::build(&valley);
        valley
    }

    /// Adds a solid after the valley is built (for tests, and props placed by later layers).
    pub fn add_solid(&mut self, s: Solid) {
        self.solids.push(s);
        (self.grid, self.tops) = Grid::split(&self.solids);
        self.tops_box = tops_box(&self.solids);
        self.holds = Holds::build(self);
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
            if let Some(top) = self.solids[i as usize].top_at(p) {
                ground = ground.max(top);
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

    /// Where a walker of `radius` at `p` stands once it's off the sides of the solids with a top
    /// higher than `above` (a face too tall to step onto): a radius short of them, as obstacles
    /// stop it (#216). `p` when it touches none.
    pub fn off_faces(&self, mut p: Vec2, radius: f32, above: f32) -> Vec2 {
        let (lo, hi) = self.tops_box;
        if p.cmplt(lo - Vec2::splat(radius)).any() || p.cmpgt(hi + Vec2::splat(radius)).any() {
            return p;
        }
        // Twice, for an inner corner, where moving off one face can put the body on the other.
        for _ in 0..2 {
            let mut moved = false;
            for i in self.tops.near(p, radius) {
                let s = &self.solids[i as usize];
                // Most solids near are low walls, or not that near: ruled out before turning `p`
                // into their frame.
                if s.top_max().is_none_or(|top| top <= above) || !s.near(p, radius) {
                    continue;
                }
                if s.overlaps(p, radius) && s.top_nearest(p).is_some_and(|top| top > above) {
                    p = s.push_out(p, radius);
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        p
    }

    /// The wall a walker of `radius` at `p` going along `dir` (a unit vector) meets within
    /// `reach`, if it's one it can't step onto: a solid with a top higher than `above`, or an
    /// obstacle at least 2 m across and 0.6 m deep (a building's wall, not a tree or a parapet).
    /// The wall's outward normal, and how far the body is short of touching it.
    pub fn wall_ahead(&self, p: Vec2, dir: Vec2, radius: f32, reach: f32, above: f32) -> Option<(Vec2, f32)> {
        let probe = p + dir * reach;
        let mut best: Option<(f32, Vec2)> = None;
        for i in self.grid.near(probe, radius).chain(self.tops.near(probe, radius)) {
            let s = &self.solids[i as usize];
            if s.top_max().is_some_and(|top| top <= above) || !s.near(probe, radius) || !s.overlaps(probe, radius) || s.top_nearest(p).is_some_and(|top| top <= above) {
                continue;
            }
            // The side the body is in front of, and how far short of it.
            let local = s.local(p);
            let short = local.abs() - s.half - Vec2::splat(radius);
            let (normal, gap, across, deep) = if short.x > short.y {
                (Vec2::new(local.x.signum(), 0.0), short.x, s.half.y, s.half.x)
            } else {
                (Vec2::new(0.0, local.y.signum()), short.y, s.half.x, s.half.y)
            };
            if s.top.is_none() && (across < 1.0 || deep < 0.3) {
                continue;
            }
            if best.is_none_or(|b| gap < b.0) {
                best = Some((gap, s.unturn(normal)));
            }
        }
        best.map(|(gap, normal)| (normal, gap.max(0.0)))
    }

    /// The first top of a solid that a walker at `from` going along `dir` (a unit vector) meets
    /// within `dist`: a point on it halfway through (or `dist` along, if that comes first), and
    /// its height there. Finds walls too thin for [`Valley::ground`]'s samples to catch.
    pub fn top_ahead(&self, from: Vec2, dir: Vec2, dist: f32) -> Option<(Vec2, f32)> {
        let mut best: Option<(f32, Vec2, f32)> = None;
        for i in self.tops.near(from, dist.min(1.5)) {
            let s = &self.solids[i as usize];
            let Some(top) = s.top else { continue };
            let (o, d) = (s.local(from), s.local(from + dir) - s.local(from));
            let (mut t0, mut t1) = (0.0f32, dist);
            for (o, d, h) in [(o.x, d.x, s.half.x), (o.y, d.y, s.half.y)] {
                if d.abs() < 1e-6 {
                    if o.abs() >= h {
                        t1 = -1.0;
                    }
                    continue;
                }
                let (a, b) = ((-h - o) / d, (h - o) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
            if t0 < t1 && best.is_none_or(|b| t0 < b.0) {
                let t = (t0 + t1) / 2.0;
                let p = from + dir * t;
                best = Some((t0, p, top + s.rise * s.local(p).x));
            }
        }
        best.map(|(_, p, h)| (p, h))
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

/// Trees don't grow where the ground is steeper than this, anywhere under the tree.
const TREE_SLOPE_LIMIT_DEGREES: f32 = 40.0;

/// Whether the ground around `p` (out to about a trunk and its roots) is too steep for a tree:
/// a cliff face, or the lip of one.
fn too_steep_for_a_tree(p: Vec2) -> bool {
    let flattest = TREE_SLOPE_LIMIT_DEGREES.to_radians().cos();
    [(0.0, 0.0), (1.5, 0.0), (-1.5, 0.0), (0.0, 1.5), (0.0, -1.5)].iter().any(|(dx, dz)| normal(p.x + dx, p.y + dz).y < flattest)
}

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
#[derive(Clone)]
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
#[derive(Clone)]
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
        // 106 from the Swift sequence, less the 19 on cliff faces (#40), which still take their
        // draws so the rest keep their sizes and turns.
        assert_eq!(v.trees.len(), 87);
        // 9 buildings, 87 trees, the bridge's two parapets, then the walls and the ruin.
        let masonry: usize = v.masonry.iter().map(|m| m.blocks.len()).sum();
        assert_eq!(v.solids.len(), 98 + masonry);
        let last = v.solids[95];
        assert!((last.center - Vec2::new(-52.010445, -2.722763)).length() < 1e-3);
        assert!((last.half.x - 0.3776364).abs() < 1e-4);
        // Each tree keeps the art it had before the cliff trees went: its place in the 106.
        assert!(v.trees.windows(2).all(|w| w[0].variant < w[1].variant));
        assert!(v.trees.last().unwrap().variant < 106);
        assert!(v.houses.iter().enumerate().all(|(i, h)| h.variant == i));
    }

    #[test]
    fn no_tree_grows_out_of_a_cliff() {
        let v = Valley::new();
        let flattest = TREE_SLOPE_LIMIT_DEGREES.to_radians().cos();
        for t in &v.trees {
            let n = normal(t.position.x, t.position.y);
            assert!(n.y >= flattest, "a tree at {} stands on a {:.0}° slope", t.position, n.y.acos().to_degrees());
        }
        // The west ledge's east face, where #40 saw one.
        assert!(!v.trees.iter().any(|t| t.position.distance(Vec2::new(-39.0, -50.0)) < 1.0));
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

    /// A wall 3 m tall, 0.9 m deep and 6 m long by the road, turned a little off the grid.
    fn with_a_tall_wall() -> (Valley, Solid, f32) {
        let mut v = Valley::new();
        let ground = v.ground(8.0, 12.0);
        let wall = Solid { center: Vec2::new(8.0, 12.0), half: Vec2::new(0.45, 3.0), yaw: 0.3, top: Some(ground + 3.0), rise: 0.0 };
        v.add_solid(wall);
        (v, wall, ground)
    }

    #[test]
    fn faces_too_tall_to_step_onto_stop_walkers_a_body_short() {
        let (v, wall, ground) = with_a_tall_wall();
        // A body overlapping the wall's west face moves straight out, to a radius from it.
        let into = wall.center + wall.unturn(Vec2::new(-0.6, 0.5));
        let out = v.off_faces(into, 0.4, ground + 1.5);
        let local = wall.local(out);
        assert!((local.x + 0.85).abs() < 1e-3 && (local.y - 0.5).abs() < 1e-3, "moved to {local} in the wall's frame");
        assert!(!wall.overlaps(out, 0.4));
        // A top low enough to step onto is ground, not a face: the body stays.
        assert_eq!(v.off_faces(into, 0.4, ground + 3.5), into);
        // And so does one clear of it.
        let clear = wall.center + wall.unturn(Vec2::new(-1.0, 0.0));
        assert_eq!(v.off_faces(clear, 0.4, ground + 1.5), clear);
    }

    #[test]
    fn a_wall_ahead_is_a_face_too_tall_to_step_onto_or_a_buildings_side() {
        let (v, wall, ground) = with_a_tall_wall();
        // 0.1 m short of the wall's west face, heading into it.
        let at = wall.center + wall.unturn(Vec2::new(-0.95, 0.0));
        let into = wall.unturn(Vec2::X);
        let (normal, gap) = v.wall_ahead(at, into, 0.4, 0.25, ground + 1.5).expect("no wall ahead");
        assert!(normal.dot(-into) > 0.999 && (gap - 0.1).abs() < 1e-3, "{normal} {gap}");
        // Not when it's low enough to step onto, out of reach, or off to the side.
        assert!(v.wall_ahead(at, into, 0.4, 0.25, ground + 3.5).is_none());
        assert!(v.wall_ahead(at - into, into, 0.4, 0.25, ground + 1.5).is_none());
        assert!(v.wall_ahead(at, wall.unturn(Vec2::Y), 0.4, 0.25, ground + 1.5).is_none());
        // A cottage's side is a wall; a tree and the bridge's parapet aren't.
        let side = |s: &Solid| (s.center + s.unturn(Vec2::new(0.0, -(s.half.y + 0.45))), s.unturn(Vec2::Y));
        let cottage = v.solids[0];
        let (p, d) = side(&cottage);
        assert!(v.wall_ahead(p, d, 0.4, 0.25, f32::INFINITY).is_some(), "no wall at the cottage");
        let tree = v.solids.iter().find(|s| s.top.is_none() && s.half.x < 0.7).unwrap();
        let (p, d) = side(tree);
        assert!(v.wall_ahead(p, d, 0.4, 0.25, f32::INFINITY).is_none(), "a tree is a wall");
        let parapet = v.solids.iter().find(|s| s.top.is_none() && s.half.x == 0.2).unwrap();
        let p = parapet.center + Vec2::new(-0.65, 0.0);
        assert!(v.wall_ahead(p, Vec2::X, 0.4, 0.25, f32::INFINITY).is_none(), "the parapet is a wall");
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
