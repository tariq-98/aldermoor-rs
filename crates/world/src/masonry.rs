//! Masonry you can climb: dry-stone walls round the fields south of the village and the ruined hall
//! on the hill. Each piece is a prop the renderer draws and the solids it adds to the valley, from
//! the one list here, so what you see and what you stand on can't drift apart. The art build reads
//! the same list (`examples/ground_fields.rs` writes it out) and builds each prop to fill its blocks
//! and to carry its grips.

use glam::{Vec2, Vec3};

/// A box in a piece's own frame: like [`crate::Solid`], with `top` measured up from the piece's
/// origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub center: Vec2,
    pub half: Vec2,
    pub yaw: f32,
    /// How far the top stands above the piece's origin at the block's centre, or `None` for
    /// masonry too tall to climb, which blocks like a building.
    pub top: Option<f32>,
    /// How much the top rises per metre along the block's own x.
    pub rise: f32,
    /// What it is, for the art build ("gable", "door jamb"…). Empty on field walls.
    pub part: &'static str,
}

/// A grip on a piece of masonry (#249): a dressed stone standing proud of a face, held by its top
/// edge, with no room above it to climb onto: a sill, a jamb stone, a keystone. In the piece's
/// frame, its edge from `a` to `b` (x, height above the origin, z) and `out`, level and away from
/// the face. Unlike a block it stands in nobody's way: it's too small to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grip {
    pub a: Vec3,
    pub b: Vec3,
    pub out: Vec2,
    /// What it is, for the art build ("lancet sill", "keystone"…).
    pub part: &'static str,
}

/// A piece of masonry: the prop drawn at `origin` turned by `yaw` (0 faces +z), and its blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct Masonry {
    pub prop: String,
    pub origin: Vec3,
    pub yaw: f32,
    pub blocks: Vec<Block>,
    /// The grips on its faces. None on field walls.
    pub grips: Vec<Grip>,
    /// A wall's coping along its centre line, in the piece's frame: (x, top above the origin, z)
    /// at both ends, every corner and every half metre between. The tops of its blocks run
    /// straight between these points, and neighbouring pieces of a wall share their end points.
    /// Empty for the ruin.
    pub line: Vec<Vec3>,
}

impl Masonry {
    /// A point in the piece's frame (x, height above the origin, z), in the valley.
    pub fn place(&self, p: Vec3) -> Vec3 {
        let (s, c) = self.yaw.sin_cos();
        Vec3::new(self.origin.x + c * p.x + s * p.z, self.origin.y + p.y, self.origin.z - s * p.x + c * p.z)
    }

    /// A level direction in the piece's frame, in the valley.
    pub fn turn(&self, v: Vec2) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(c * v.x + s * v.y, -s * v.x + c * v.y)
    }

    /// The blocks as solids in the valley.
    pub fn solids(&self) -> impl Iterator<Item = crate::Solid> + '_ {
        let (s, c) = self.yaw.sin_cos();
        self.blocks.iter().map(move |b| crate::Solid {
            center: Vec2::new(self.origin.x + c * b.center.x + s * b.center.y, self.origin.z - s * b.center.x + c * b.center.y),
            half: b.half,
            yaw: self.yaw + b.yaw,
            top: b.top.map(|t| self.origin.y + t),
            rise: b.rise,
        })
    }
}

/// A field wall's coping stands this high above the ground beside it.
pub const FIELD_WALL_HEIGHT: f32 = 1.05;
/// The width of a field wall's top. Climbing a face lands you 0.7 to 0.8 m past it, so anything
/// narrower is vaulted rather than stood on.
pub const FIELD_WALL_WIDTH: f32 = 0.9;

/// Dry-stone walls along the north road and round the fields beside it, each through its corners
/// (x, z). Field gates are the gaps where one run ends and the next begins.
const FIELD_WALLS: [&[(f32, f32)]; 4] = [
    // East of the road: from the hill down to a field gate, then on to the edge of the village
    // and east along the top of the field.
    &[(12.6, 22.0), (11.2, 13.4)],
    &[(10.75, 10.4), (9.65, 6.0), (8.75, 3.4), (34.0, 3.4)],
    // West of the road: from the edge of the village up to a field gate, then on up the hill and
    // west along the hillside past the old oak.
    &[(0.6, 4.2), (1.65, 10.2)],
    &[(2.25, 13.2), (3.25, 17.4), (3.85, 21.6), (-28.0, 21.6)],
];

/// No block of field wall is longer than this; its top runs straight from end to end.
const BLOCK: f32 = 4.0;
/// No piece of wall is much longer than this, so its level of detail follows the camera closely.
const PIECE: f32 = 12.0;

/// The field walls, cut into pieces `wall.field.1`, `wall.field.2`… standing on `ground`.
pub(crate) fn field_walls(ground: impl Fn(f32, f32) -> f32) -> Vec<Masonry> {
    let mut pieces = Vec::new();
    for run in FIELD_WALLS {
        let wall = Wall::new(run, &ground);
        // Each block's top slopes straight between the coping at its ends, so a wall following a
        // slope is walked up rather than climbed a step at a time. At a corner the flatter of the
        // two stretches runs on by half a wall's width to fill it (the other's slope carried on
        // past the corner would leave a hump).
        let rise = |from: f32, to: f32| (wall.top(to) - wall.top(from)) / (to - from);
        let ends = |k: usize| {
            let n = ((wall.corner_s[k + 1] - wall.corner_s[k]) / BLOCK).ceil();
            let step = (wall.corner_s[k + 1] - wall.corner_s[k]) / n;
            (rise(wall.corner_s[k], wall.corner_s[k] + step), rise(wall.corner_s[k + 1] - step, wall.corner_s[k + 1]))
        };
        let mut blocks = Vec::new();
        for k in 0..wall.corners.len() - 1 {
            let (a, b) = (wall.corners[k], wall.corners[k + 1]);
            let (s0, len) = (wall.corner_s[k], (b - a).length());
            let dir = (b - a) / len;
            let n = (len / BLOCK).ceil() as usize;
            let back_fill = k > 0 && ends(k).0.abs() <= ends(k - 1).1.abs();
            let on_fill = k + 2 < wall.corners.len() && ends(k).1.abs() < ends(k + 1).0.abs();
            for i in 0..n {
                let (t0, t1) = (len * i as f32 / n as f32, len * (i + 1) as f32 / n as f32);
                let back = if i == 0 && back_fill { FIELD_WALL_WIDTH / 2.0 } else { 0.0 };
                let on = if i + 1 == n && on_fill { FIELD_WALL_WIDTH / 2.0 } else { 0.0 };
                let (p0, p1) = (a + dir * (t0 - back), a + dir * (t1 + on));
                let (from, to) = (s0 + t0, s0 + t1);
                let rise = rise(from, to);
                let middle = s0 + (t0 - back + t1 + on) / 2.0;
                blocks.push(Span {
                    from,
                    to,
                    center: (p0 + p1) / 2.0,
                    half: (p1 - p0).length() / 2.0,
                    yaw: -dir.y.atan2(dir.x),
                    top: wall.top(from) + rise * (middle - from),
                    rise,
                });
            }
        }
        // Pieces of about equal length, cut where blocks meet but never at a corner, which one
        // piece builds whole.
        let count = (wall.length / PIECE).ceil() as usize;
        let mut starts = vec![0];
        for k in 1..count {
            let target = wall.length * k as f32 / count as f32;
            let next = (starts[k - 1] + 1..blocks.len())
                .filter(|&i| wall.corner_s.iter().all(|c| (c - blocks[i].from).abs() > 0.5))
                .min_by(|&i, &j| (blocks[i].from - target).abs().total_cmp(&(blocks[j].from - target).abs()));
            starts.extend(next);
        }
        starts.push(blocks.len());
        for w in starts.windows(2) {
            let spans = &blocks[w[0]..w[1]];
            let (from, to) = (spans[0].from, spans[spans.len() - 1].to);
            let mid = wall.point((from + to) / 2.0);
            let origin = Vec3::new(mid.x, ground(mid.x, mid.y), mid.y);
            // The coping line: both ends, the corners and block ends between them, and every
            // half metre.
            let knots: Vec<f32> = wall.knots.iter().map(|k| k.0).filter(|&s| s > from + 0.05 && s < to - 0.05).collect();
            let mut stops = knots.clone();
            let mut s = ((from + 0.05) * 2.0).ceil() / 2.0;
            while s < to - 0.05 {
                if knots.iter().all(|&k| (k - s).abs() > 0.05) {
                    stops.push(s);
                }
                s += 0.5;
            }
            stops.sort_by(f32::total_cmp);
            let line = [from]
                .into_iter()
                .chain(stops)
                .chain([to])
                .map(|s| {
                    let p = wall.point(s);
                    Vec3::new(p.x - origin.x, wall.top(s) - origin.y, p.y - origin.z)
                })
                .collect();
            pieces.push(Masonry {
                prop: format!("wall.field.{}", pieces.len() + 1),
                origin,
                yaw: 0.0,
                blocks: spans
                    .iter()
                    .map(|b| Block {
                        center: b.center - Vec2::new(origin.x, origin.z),
                        half: Vec2::new(b.half, FIELD_WALL_WIDTH / 2.0),
                        yaw: b.yaw,
                        top: Some(b.top - origin.y),
                        rise: b.rise,
                        part: "",
                    })
                    .collect(),
                grips: Vec::new(),
                line,
            });
        }
    }
    pieces
}

/// One block of a field wall: where it runs along the wall, its box, and its top at its centre.
struct Span {
    from: f32,
    to: f32,
    center: Vec2,
    half: f32,
    yaw: f32,
    top: f32,
    rise: f32,
}

/// A run of field wall: its corners, and the coping's height at every block's ends (knots: how
/// far along the wall, and the height), with straight runs between.
struct Wall {
    corners: Vec<Vec2>,
    corner_s: Vec<f32>,
    length: f32,
    knots: Vec<(f32, f32)>,
}

impl Wall {
    /// The coping at each knot follows the ground averaged over three metres, so it doesn't copy
    /// every bump.
    fn new(run: &[(f32, f32)], ground: impl Fn(f32, f32) -> f32) -> Wall {
        let corners: Vec<Vec2> = run.iter().map(|&(x, z)| Vec2::new(x, z)).collect();
        let mut corner_s = vec![0.0];
        for w in corners.windows(2) {
            corner_s.push(corner_s.last().unwrap() + (w[1] - w[0]).length());
        }
        let length = *corner_s.last().unwrap();
        let mut wall = Wall { corners, corner_s, length, knots: Vec::new() };
        let ground_at = |s: f32| {
            let p = wall.point(s);
            ground(p.x, p.y)
        };
        let mut knots = vec![];
        for k in 0..wall.corners.len() - 1 {
            let (s0, s1) = (wall.corner_s[k], wall.corner_s[k + 1]);
            let n = ((s1 - s0) / BLOCK).ceil() as usize;
            for i in 0..=n {
                let s = s0 + (s1 - s0) * i as f32 / n as f32;
                if i == 0 && k > 0 {
                    continue;
                }
                let samples: Vec<f32> = (-6..=6).map(|j| ground_at((s + j as f32 * 0.25).clamp(0.0, length))).collect();
                knots.push((s, samples.iter().sum::<f32>() / samples.len() as f32 + FIELD_WALL_HEIGHT));
            }
        }
        wall.knots = knots;
        wall
    }

    /// The point `s` metres along the wall.
    fn point(&self, s: f32) -> Vec2 {
        let k = self.corner_s.iter().rposition(|&c| c <= s).unwrap_or(0).min(self.corners.len() - 2);
        let (a, b) = (self.corners[k], self.corners[k + 1]);
        a.lerp(b, ((s - self.corner_s[k]) / (b - a).length()).clamp(0.0, 1.0))
    }

    /// The top of the coping `s` metres along.
    fn top(&self, s: f32) -> f32 {
        let i = self.knots.iter().position(|k| k.0 >= s - 1e-4).unwrap_or(self.knots.len() - 1).max(1);
        let (a, b) = (self.knots[i - 1], self.knots[i]);
        a.1 + (b.1 - a.1) * ((s - a.0) / (b.0 - a.0)).clamp(0.0, 1.0)
    }
}

/// Where the ruined hall stands, on the flat top of the hill south-west of the village, and its
/// turn: its gable (+z in its own frame) faces north, toward the village.
pub const RUIN_AT: Vec2 = Vec2::new(-18.0, 47.0);
pub const RUIN_YAW: f32 = std::f32::consts::PI;

/// The hall in its own frame: 7.2 m across (x) and 11.4 m long (z), walls 0.95 m thick. The east
/// wall breaks down in steps from the gable to the rear, each one a climb from the last, to a
/// lookout under the gable; the rest has fallen to low walls and stumps. The gable's buttresses
/// and the door jambs stand too tall to climb.
///
/// The gable climbs (#249): from the lookout a jump reaches its raked coping, which you walk up to
/// the empty bellcote on its apex, and a line of grips climbs its outer face from the lancet's
/// sill, up the window's east jamb and over its arch to the oculus and the bellcote
/// ([`RUIN_GRIPS`]). The gable's blocks are its two raked halves, their tops the coping's top, and
/// the bellcote between them, over the window.
const RUIN: [Block; 21] = [
    raked("gable", -3.6, -0.75, GABLE_RIDGE, GABLE_RAKE),
    raked("bellcote", -0.75, 0.0, BELLCOTE_RIDGE, BELLCOTE_RAKE),
    raked("bellcote", 0.0, 0.75, BELLCOTE_RIDGE, BELLCOTE_RAKE),
    raked("gable", 0.75, 3.6, GABLE_RIDGE, GABLE_RAKE),
    block("buttress", -3.1, 6.15, 0.5, 0.45, None),
    block("buttress", 3.1, 6.15, 0.5, 0.45, None),
    // The east wall (−x, east once turned), stepping down from the gable to the rear.
    block("east wall", -3.125, 3.575, 0.475, 1.175, Some(4.2)),
    block("east wall", -3.125, 1.5, 0.475, 0.9, Some(2.9)),
    block("east wall", -3.125, -0.4, 0.475, 1.0, Some(1.85)),
    block("east wall", -3.125, -2.45, 0.475, 1.05, Some(1.1)),
    block("east wall", -3.125, -4.6, 0.475, 1.1, Some(0.5)),
    // The west wall, with the door between two jambs that still carry its arch.
    block("door jamb", 3.125, 3.425, 0.475, 1.325, None),
    block("door jamb", 3.125, -0.4, 0.475, 0.9, None),
    block("west wall", 3.125, -2.45, 0.475, 1.15, Some(1.25)),
    block("west wall", 3.125, -4.65, 0.475, 1.05, Some(0.6)),
    // The rear wall, breached in the middle.
    block("rear wall", -1.75, -5.225, 0.9, 0.475, Some(1.4)),
    block("rear wall", 1.75, -5.225, 0.9, 0.475, Some(0.8)),
    // Stones fallen from the walls: steps up.
    Block { center: Vec2::new(-1.3, -1.6), half: Vec2::new(0.6, 0.45), yaw: 0.35, top: Some(0.55), rise: 0.0, part: "fallen" },
    Block { center: Vec2::new(4.6, -1.0), half: Vec2::new(0.55, 0.4), yaw: -0.5, top: Some(0.45), rise: 0.0, part: "fallen" },
    Block { center: Vec2::new(-4.45, -3.3), half: Vec2::new(0.5, 0.42), yaw: 0.2, top: Some(0.5), rise: 0.0, part: "fallen" },
    Block { center: Vec2::new(0.4, -6.6), half: Vec2::new(0.55, 0.45), yaw: -0.3, top: Some(0.4), rise: 0.0, part: "fallen" },
];

/// The grips up the gable's outer face, from the lancet's sill to the bellcote's, each within a
/// climb (1 m) or a climb leap (1.8 m) of the one below. The sill stands 0.12 m proud of the face,
/// the corbelled stones above it 0.15 m, and the bell's sill 0.16 m proud of the bellcote, which
/// is set back 0.08 m.
const RUIN_GRIPS: [Grip; 7] = [
    grip("lancet sill", -0.75, 0.75, 2.6, 0.12),
    grip("jamb stone", -0.95, -0.57, 3.55, 0.15),
    grip("jamb stone", -0.95, -0.57, 4.45, 0.15),
    grip("label stop", -0.95, -0.57, 5.3, 0.15),
    grip("keystone", -0.25, 0.25, 6.75, 0.15),
    grip("oculus sill", -0.45, 0.45, 7.2, 0.15),
    grip("bell sill", -0.3, 0.3, 8.95, 0.08),
];

/// A grip on the gable's outer face from x0 to x1, `y` up and standing `proud` of the face.
const fn grip(part: &'static str, x0: f32, x1: f32, y: f32, proud: f32) -> Grip {
    Grip { a: Vec3::new(x0, y, GABLE_FACE + proud), b: Vec3::new(x1, y, GABLE_FACE + proud), out: Vec2::new(0.0, 1.0), part }
}

/// The gable wall runs along x at z = 5.225, 0.95 m thick: its outer face, toward the village.
const GABLE_Z: f32 = 5.225;
const GABLE_FACE: f32 = GABLE_Z + 0.475;
/// The gable's coping: the top of its stones over the middle (were the bellcote not there), and
/// how much it rises a metre toward the middle (3 m up over 3.6 m, from the eaves at 6 m to the
/// apex at 9 m, the coping 0.276 m above the masonry).
const GABLE_RIDGE: f32 = 9.276;
const GABLE_RAKE: f32 = 3.0 / 3.6;
/// The bellcote's own small coping, from its eaves 0.75 m either side of the apex up to its peak.
const BELLCOTE_RIDGE: f32 = 10.796;
const BELLCOTE_RAKE: f32 = 0.65 / 0.75;

/// A stretch of the gable from x0 to x1 whose top is a coping rising `rake` a metre toward x = 0,
/// where it would reach `ridge`.
const fn raked(part: &'static str, x0: f32, x1: f32, ridge: f32, rake: f32) -> Block {
    let x = (x0 + x1) / 2.0;
    let (away, rise) = if x < 0.0 { (-x, rake) } else { (x, -rake) };
    Block { center: Vec2::new(x, GABLE_Z), half: Vec2::new((x1 - x0) / 2.0, 0.475), yaw: 0.0, top: Some(ridge - away * rake), rise, part }
}

const fn block(part: &'static str, x: f32, z: f32, hx: f32, hz: f32, top: Option<f32>) -> Block {
    Block { center: Vec2::new(x, z), half: Vec2::new(hx, hz), yaw: 0.0, top, rise: 0.0, part }
}

/// The ruined hall, standing on the ground at its middle.
pub(crate) fn ruin(ground: impl Fn(f32, f32) -> f32) -> Masonry {
    Masonry {
        prop: "ruin.hall".into(),
        origin: Vec3::new(RUIN_AT.x, ground(RUIN_AT.x, RUIN_AT.y), RUIN_AT.y),
        yaw: RUIN_YAW,
        blocks: RUIN.to_vec(),
        grips: RUIN_GRIPS.to_vec(),
        line: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{path_distance, Solid, Valley, BOUNDS};

    /// The sim climbs a face that tops out within this of your feet (`sim::movement::CLIMB_HEIGHT`).
    const CLIMB: f32 = 1.5;

    fn corners(s: &Solid) -> [Vec2; 4] {
        let (sn, c) = s.yaw.sin_cos();
        let (x, z) = (Vec2::new(c, -sn) * s.half.x, Vec2::new(sn, c) * s.half.y);
        [s.center - x - z, s.center + x - z, s.center + x + z, s.center - x + z]
    }

    #[test]
    fn field_walls_stand_about_a_metre_proud_and_can_be_walked_along() {
        let v = Valley::new();
        for m in v.masonry.iter().filter(|m| m.prop.starts_with("wall.field.")) {
            for s in m.solids() {
                assert!(s.half.y * 2.0 >= 0.85, "{} is too narrow to stand on", m.prop);
                // Walking along it is walking up a slope, never a climb.
                assert!(s.rise.abs() < 0.3, "{} rises {} a metre", m.prop, s.rise);
                let (sn, c) = s.yaw.sin_cos();
                for i in -10..=10 {
                    let p = s.center + Vec2::new(c, -sn) * (s.half.x * 0.99 * i as f32 / 10.0);
                    let proud = s.top_at(p).unwrap() - v.surface(p.x, p.y);
                    assert!((0.85..1.25).contains(&proud), "{} stands {proud} m proud at {p}", m.prop);
                    // Where two stretches meet, the corner may stand a little above a sloping one.
                    assert!((v.ground(p.x, p.y) - s.top_at(p).unwrap()).abs() < 0.1, "{}'s top at {p} isn't the ground", m.prop);
                }
            }
            // The drawn coping is the blocks' tops (its ends lie on the blocks' edges).
            for q in &m.line {
                let p = Vec2::new(m.origin.x + q.x, m.origin.z + q.z);
                let top = m
                    .solids()
                    .filter_map(|s| {
                        let l = s.local(p);
                        (l.x.abs() <= s.half.x + 0.01 && l.y.abs() <= s.half.y + 0.01).then(|| s.top.unwrap() + s.rise * l.x)
                    })
                    .fold(f32::MIN, f32::max);
                assert!((top - (m.origin.y + q.y)).abs() < 0.06, "{}'s coping at {p} is off its blocks", m.prop);
            }
        }
    }

    #[test]
    fn masonry_stands_clear_of_roads_buildings_trees_and_water() {
        let v = Valley::new();
        let others = &v.solids[..v.solids.len() - v.masonry.iter().map(|m| m.blocks.len()).sum::<usize>()];
        for m in &v.masonry {
            for s in m.solids() {
                for p in corners(&s).into_iter().chain([s.center]) {
                    assert!(p.x.abs() < BOUNDS && p.y.abs() < BOUNDS, "{} leaves the map at {p}", m.prop);
                    assert!(path_distance(p.x, p.y) > 3.3, "{} stands on the road at {p}", m.prop);
                    assert_eq!(v.water_depth(p.x, p.y), 0.0, "{} stands in the river at {p}", m.prop);
                    assert!(!others.iter().any(|o| o.overlaps(p, 0.5)), "{} runs into a house or a tree at {p}", m.prop);
                }
            }
        }
    }

    #[test]
    fn the_ruin_climbs_in_steps_to_a_lookout_and_on_up_its_gable() {
        let v = Valley::new();
        let ruin = v.masonry.iter().find(|m| m.prop == "ruin.hall").expect("the ruin");
        // The east wall, from the rear to the gable: each step a climb from the one before.
        let mut steps: Vec<_> = ruin.blocks.iter().filter(|b| b.part == "east wall").collect();
        steps.sort_by(|a, b| a.center.y.total_cmp(&b.center.y));
        let mut from = 0.0;
        for b in &steps {
            let top = b.top.expect("the east wall has tops");
            assert!(top - from <= CLIMB - 0.1, "a {} m step up the east wall", top - from);
            assert!(b.half.x * 2.0 >= 0.85, "too narrow to stand on");
            from = top;
        }
        assert!(from > 4.0, "the lookout is {from} m up");
        // Over the lookout, the gable's coping is too tall to climb but in reach of a jump (a
        // mantle reaches 2.2 m above the feet at the top of a 1.1 m jump).
        let lookout = ruin.blocks.iter().find(|b| b.top == Some(from)).unwrap();
        let top = |b: &Block, x: f32| b.top.unwrap() + b.rise * (x - b.center.x);
        let gable = ruin.blocks.iter().find(|b| b.part == "gable" && b.center.x < 0.0).unwrap();
        let over = top(gable, lookout.center.x) - from;
        assert!(over > CLIMB && over < 3.2, "the coping is {over} m over the lookout");
        // It rises to the bellcote, whose top is the highest point of the ruin.
        let halves: Vec<_> = ruin.blocks.iter().filter(|b| b.part == "gable").collect();
        assert_eq!(halves.len(), 2);
        let peak = ruin.blocks.iter().filter(|b| b.part == "bellcote").map(|b| top(b, 0.0)).fold(f32::MIN, f32::max);
        for b in ruin.blocks.iter().filter(|b| b.part != "bellcote") {
            for x in [b.center.x - b.half.x, b.center.x + b.half.x] {
                assert!(b.top.is_none_or(|_| top(b, x) < peak), "{} stands above the bellcote", b.part);
            }
        }
        // Where the coping meets the bellcote, the gable and the bellcote's tops are continuous
        // with their neighbours' across the apex.
        for g in &halves {
            let inner = if g.center.x < 0.0 { g.center.x + g.half.x } else { g.center.x - g.half.x };
            assert!((top(g, inner) - (9.276 - 0.75 * 3.0 / 3.6)).abs() < 1e-3);
        }
    }

    #[test]
    fn a_line_of_grips_climbs_the_gables_face_each_in_reach_of_the_last() {
        let v = Valley::new();
        let ruin = v.masonry.iter().find(|m| m.prop == "ruin.hall").expect("the ruin");
        let face = 5.225 + 0.475;
        for g in &ruin.grips {
            // Proud of the face, so a hand can close on it, and level along it.
            assert!(g.a.z - face >= 0.05 && g.a.z == g.b.z && g.a.y == g.b.y, "{} isn't proud of the face", g.part);
            assert!(g.out == Vec2::Y && (g.b.x - g.a.x) > 0.35, "{} faces the wrong way or is too short", g.part);
        }
        // The first is caught in a jump from the ground (the hands reach 2.1 m above the feet, and
        // a jump 1.1 m), and each after it is a climb (1 m) or a climb leap (1.8 m) above the last.
        assert!(ruin.grips[0].a.y <= 2.1 + 1.1, "the lowest grip is out of a jump's reach");
        for w in ruin.grips.windows(2) {
            let up = w[1].a.y - w[0].a.y;
            assert!(up > 0.0 && up <= 1.8, "{} is {up} m above the grip below", w[1].part);
        }
        let prev = ruin.grips.last().unwrap().a.y;
        assert_eq!(ruin.grips.first().unwrap().part, "lancet sill");
        // The last is a climb leap below the bellcote's coping.
        let coping = ruin.blocks.iter().filter(|b| b.part == "bellcote").map(|b| b.top.unwrap() + b.rise * -b.center.x).fold(f32::MIN, f32::max);
        assert!(coping - 0.2 - prev <= 1.8, "the bellcote is {} m above the bell's sill", coping - prev);
    }

}
