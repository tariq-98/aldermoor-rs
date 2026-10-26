//! CPU cost of animating characters: `Character::animate` (pose) and `Character::draws`
//! (skinning palettes) per character per frame, for a few typical looks and poses.
//!
//!     cargo run --release -p render --example anim_bench -- [characters] [frames] [case] [tier] [feet]
//!
//! The cases are listed in `main`; without one, it runs them all. The tiers (#215) are `full`
//! (every LOD), `half` and `quarter` (LOD 0); without one, it runs them all. The characters go
//! over the valley's ground with their feet on it (#214); `off` for the feet leaves them as the
//! clips have them, to compare. Each looks at a point sweeping round and up and down in front of
//! it (#212), as if following someone about.
use anim::Tier;
use glam::Vec3;
use protocol::{
    Action, ForcedKind, ForcedMove, HitWeight, Look, Pose, ResourceKind, Traversal, UnitCombat,
    UnitKind, UnitState,
};
use render::character::{Character, Ground, HoldAt};
use render::model::ModelCache;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

/// Counts allocations, to show that posing a character allocates nothing once it is going.
struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static COUNTING: Counting = Counting;

fn look(i: usize) -> Look {
    let gear = |items: &[(&str, &str)]| {
        items
            .iter()
            .map(|(s, id)| (s.to_string(), id.to_string()))
            .collect()
    };
    let pieces = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect();
    match i % 3 {
        0 => Look {
            art: "male".into(),
            drawn: true,
            pieces: pieces(&[
                "plate.cuirass",
                "plate.pauldrons",
                "plate.gauntlets",
                "plate.legs",
                "plate.helm_norman",
            ]),
            gear: gear(&[
                ("main_hand", "sword.arming"),
                ("off_hand", "shield.round"),
                ("back", "cape.traveller"),
            ]),
            ..Default::default()
        },
        1 => Look {
            art: "female".into(),
            drawn: true,
            pieces: pieces(&[
                "ranger.top",
                "ranger.trousers",
                "ranger.boots",
                "ranger.hood",
            ]),
            gear: gear(&[("main_hand", "bow.hunting"), ("back", "quiver.leather")]),
            ..Default::default()
        },
        _ => Look {
            art: "elf_male".into(),
            drawn: false,
            pieces: pieces(&["robe.mage"]),
            gear: gear(&[("main_hand", "staff.oak")]),
            ..Default::default()
        },
    }
}

/// The "climb" case's ledges (#213): each 1.2 m higher than the last, the first one's face 0.1 m
/// ahead of where a body starts (where the sim starts a climb), and each as far on from the last
/// as a climb over it (0.85 m) and 0.4 s running at 6 m/s take the body.
const LEDGE: f32 = 1.2;
const FACE: f32 = 0.1;
const CLIMB_EVERY: f32 = 0.85 + 0.4 * 6.;

fn unit(id: u32, look: Look, pose: Pose, speed: f32) -> UnitState {
    UnitState {
        id,
        kind: UnitKind::Player,
        body: "player".into(),
        name: format!("bench{id}"),
        level: 1,
        position: Vec3::ZERO,
        velocity: Vec3::new(0., 0., speed),
        yaw: 0.,
        pose,
        pose_time: 0.,
        health: 100.,
        max_health: 100.,
        hostile: false,
        target: None,
        look: look.into(),
        traversal: Default::default(),
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let count: usize = args.next().map_or(Ok(30), |a| a.parse())?;
    let frames: usize = args.next().map_or(Ok(600), |a| a.parse())?;
    let only = args.next().filter(|case| case != "all");
    let full = [(0, Tier::Full), (1, Tier::Full), (2, Tier::Full)];
    let runs = match args.next().as_deref() {
        None => [&full[..], &[(0, Tier::Half), (0, Tier::Quarter)]].concat(),
        Some("full") => full.to_vec(),
        Some("half") => vec![(0, Tier::Half)],
        Some("quarter") => vec![(0, Tier::Quarter)],
        Some(other) => anyhow::bail!("the tier is full, half or quarter, not {other}"),
    };
    let feet = match args.next().as_deref() {
        None | Some("on") => true,
        Some("off") => false,
        Some(other) => anyhow::bail!("the feet are on or off, not {other}"),
    };
    let valley = Arc::new(world::Valley::new());
    let ground: Ground = {
        let valley = Arc::clone(&valley);
        Arc::new(move |x, z| valley.ground(x, z))
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let cat = assets::Catalog::load(&root)?;
    let mut cache = ModelCache::default();
    // "idle", "walk", "run" and "strafe" hold their pose throughout: "walk" blends the walk and
    // jog loops, and "strafe" runs square to the facing, the hips turned toward the travel and the
    // chest turned back. "turn" stands looking 80° either way and back every two seconds, as a
    // player sweeping the camera round, so the chest twists and the feet step round on every
    // sweep. "slash" and "run + slash" start the slash over every half second, and "start-stop"
    // runs and stops every half second, so those three are blending clips most of the time, and
    // the body leans setting off and stopping (#217). "circle" runs round, leaning into the turn,
    // and "jump" runs and jumps every second, the legs reaching for the ground on the way down
    // and the pelvis dipping on landing. "run + hits" runs taking a blow every half second (#218),
    // from all round and light, heavy and big in turn, so the body is flinching most of the time.
    // "run + rolls" runs and rolls to one side and then the other every second, and "climb" climbs
    // a ledge every second and runs on over its top to the next (#213), so both are carried
    // through moves about half the time, then recover from them; "swim" swims ahead. "hang" hangs
    // from a hold on a wall (#248), shimmying right, then left, then hanging still, in turn every
    // 0.8 s, its hands on the hold and its feet on the wall.
    for (label, pose, speed, action) in [
        ("idle", Pose::Idle, 0., Action::None),
        ("turn", Pose::Idle, 0., Action::None),
        ("walk", Pose::Walk, 2.5, Action::None),
        ("run", Pose::Run, 6., Action::None),
        ("strafe", Pose::Run, 6., Action::None),
        ("slash", Pose::Attack, 0., Action::Slash),
        ("run + slash", Pose::Attack, 6., Action::Slash),
        ("start-stop", Pose::Run, 6., Action::None),
        ("circle", Pose::Run, 6., Action::None),
        ("jump", Pose::Run, 6., Action::None),
        ("run + hits", Pose::Run, 6., Action::None),
        ("run + rolls", Pose::Run, 6., Action::None),
        ("climb", Pose::Run, 6., Action::None),
        ("swim", Pose::Run, 2.7, Action::None),
        ("hang", Pose::Hang, 0., Action::None),
    ] {
        if only.as_ref().is_some_and(|o| o != label) {
            continue;
        }
        for &(lod, tier) in &runs {
            let mut characters = (0..count)
                .map(|i| Character::load_lod(&cat, &mut cache, &look(i), lod))
                .collect::<anyhow::Result<Vec<_>>>()?;
            // Side by side on the slopes west of the village, going north over the terraces.
            let mut units: Vec<_> = (0..count)
                .map(|i| {
                    let mut u = unit(i as u32, look(i), pose, speed);
                    u.position = Vec3::new(
                        -75. + 1.5 * (i % 20) as f32,
                        0.,
                        -70. - 3. * (i / 20) as f32,
                    );
                    if label == "swim" {
                        u.traversal = Traversal::Swimming;
                    }
                    if label == "hang" {
                        u.traversal = Traversal::Hanging;
                    }
                    u
                })
                .collect();
            if label == "strafe" {
                for u in &mut units {
                    u.velocity = Vec3::new(speed, 0., 0.);
                }
            }
            // Climbing, each goes up a flight of ledges: one every `CLIMB_EVERY` from just ahead
            // of where it starts, each `LEDGE` higher than the last.
            let grounds: Vec<Ground> = (units.iter())
                .map(|u| match label {
                    "climb" => {
                        let (valley, start) = (Arc::clone(&valley), u.position.z + FACE);
                        Arc::new(move |x: f32, z: f32| {
                            let ledges = ((z - start) / CLIMB_EVERY).floor() + 1.;
                            valley.ground(x, z) + LEDGE * ledges.max(0.)
                        }) as Ground
                    }
                    _ => Arc::clone(&ground),
                })
                .collect();
            for ((c, u), ground) in characters.iter_mut().zip(&mut units).zip(&grounds) {
                c.set_tier(tier);
                c.set_ground(Arc::clone(ground));
                c.foot_placement = feet;
                u.position.y = ground(u.position.x, u.position.z);
                if label == "hang" {
                    // Hanging 1 m off the ground from a wall 0.4 m ahead, 6 m long.
                    u.position.y += 1.;
                    let at = u.position;
                    let holds: HoldAt = Arc::new(move |p: Vec3| {
                        let near = (p.x - at.x).abs() <= 3. && (p - at).with_x(0.).length() < 0.3;
                        near.then_some(world::Hold {
                            a: at + Vec3::new(3., 1.9, 0.4),
                            b: at + Vec3::new(-3., 1.9, 0.4),
                            out: glam::Vec2::new(0., -1.),
                            braced: true,
                        })
                    });
                    c.set_holds(holds);
                }
            }
            let mut moves: Vec<Option<ForcedMove>> = vec![None; count];
            let fight = |id: u32, t: f32| UnitCombat {
                id,
                action,
                action_time: t,
                cast: None,
                auras: vec![],
                resource_kind: ResourceKind::Rage,
                resource: 0.,
                max_resource: 100.,
                guarding: false,
                downed: false,
                defiance: None,
            };
            let (mut pose_ns, mut draw_ns, mut draws, mut allocations) = (0u128, 0u128, 0usize, 0);
            let mut rebuilds = 0;
            for frame in 0..frames {
                let t = frame as f32 / 60.;
                let before = ALLOCATIONS.load(Relaxed);
                let started = Instant::now();
                let each = characters.iter_mut().zip(&mut units).zip(&mut moves);
                for (((c, u), m), under) in each.zip(&grounds) {
                    if label == "start-stop" {
                        let running = (t / 0.5) as usize % 2 == 1;
                        u.pose = if running { Pose::Run } else { Pose::Idle };
                        u.velocity.z = if running { speed } else { 0. };
                    }
                    if label == "turn" {
                        u.yaw = 1.4 * (std::f32::consts::PI * t).sin();
                    }
                    if label == "circle" {
                        u.yaw = 1.2 * t + u.id as f32;
                        u.velocity = Vec3::new(u.yaw.sin(), 0., u.yaw.cos()) * speed;
                    }
                    let cycle = t % 0.5;
                    if label == "run + hits" {
                        // The sim holds `Pose::Hit` for a quarter of a second after a blow, and
                        // the hitstops are `content::combat::HITSTOP`'s.
                        let blow = frame / 30;
                        if frame % 30 == 0 {
                            let (weight, hitstop) = [
                                (HitWeight::Light, 0.04),
                                (HitWeight::Heavy, 0.08),
                                (HitWeight::Big, 0.12),
                            ][(blow + u.id as usize) % 3];
                            c.hit(u.yaw + 2.4 * blow as f32 + u.id as f32, weight, hitstop);
                        }
                        u.pose = if cycle < 0.25 { Pose::Hit } else { Pose::Run };
                    }
                    if label == "hang" {
                        u.velocity.x = [-1.2, 1.2, 0.][(t / 0.8) as usize % 3];
                    }
                    let steady = ["idle", "turn", "walk", "run", "strafe", "swim", "hang"];
                    u.pose_time = if steady.contains(&label) { t } else { cycle };
                    // Up at 6.6 m/s under the sim's gravity, then running till the next second.
                    let (mut up, rise) = (0., 6.6 / anim::land::GRAVITY);
                    if label == "jump" {
                        let at = t % 1.;
                        up = (6.6 * at - 0.5 * anim::land::GRAVITY * at * at).max(0.);
                        (u.pose, u.pose_time) = match at {
                            at if at < rise => (Pose::Jump, at),
                            at if up > 0. => (Pose::Fall, at - rise),
                            at => (Pose::Run, at - 2. * rise),
                        };
                        u.velocity.y = if up > 0. {
                            6.6 - anim::land::GRAVITY * at
                        } else {
                            0.
                        };
                    }
                    // Carried through a move (#213) as the sim carries a body: a roll 5 m to one
                    // side, fastest at the start with a 0.15 m hop, or a climb up the ledge just
                    // ahead and over its top, rising before it goes over. Then running on.
                    let mut carried = None;
                    if matches!(label, "run + rolls" | "climb") {
                        let climb = label == "climb";
                        let (kind, duration) = match climb {
                            true => (ForcedKind::Climb, 0.6),
                            false => (ForcedKind::Roll, 0.45),
                        };
                        let at = t % 1.;
                        if frame % 60 == 0 {
                            let side = [1., -1.][(frame / 60 + u.id as usize) % 2];
                            let to = match climb {
                                true => u.position + Vec3::new(0., LEDGE, 0.85),
                                false => u.position + Vec3::X * 5. * side,
                            };
                            *m = Some(ForcedMove {
                                kind,
                                from: u.position,
                                to,
                                start: t as f64,
                                duration,
                            });
                        }
                        match *m {
                            Some(mv) if at < duration => {
                                let k = at / duration;
                                let there = if climb {
                                    let up = 1. - (1. - (k * 1.6).min(1.)).powi(2);
                                    mv.from.lerp(mv.to, k * k).with_y(mv.from.y + LEDGE * up)
                                } else {
                                    let eased = 1. - (1. - k) * (1. - k);
                                    let hop = (k * std::f32::consts::PI).sin() * 0.15;
                                    mv.from.lerp(mv.to, eased) + Vec3::Y * hop
                                };
                                u.velocity = (there - u.position) * 60.;
                                u.pose = if climb { Pose::Jump } else { Pose::Dodge };
                                (u.pose_time, u.traversal) = (at, Traversal::Forced(mv));
                                carried = Some(there);
                            }
                            _ => {
                                // The sim lets go where the move ends.
                                if let Traversal::Forced(mv) = u.traversal {
                                    u.position = mv.to;
                                }
                                (u.pose, u.pose_time) = (Pose::Run, at - duration);
                                (u.velocity, u.traversal) = (Vec3::Z * speed, Traversal::Ground);
                            }
                        }
                    }
                    let f = fight(u.id, if action == Action::None { t } else { cycle });
                    match carried {
                        Some(there) => u.position = there,
                        None if label == "hang" => u.position += u.velocity / 60.,
                        None => {
                            u.position += u.velocity.with_y(0.) / 60.;
                            u.position.y = under(u.position.x, u.position.z) + up;
                        }
                    }
                    let phase = t + u.id as f32;
                    let round = u.yaw + 1.2 * (0.7 * phase).sin();
                    c.look_at(Some(
                        u.position
                            + Vec3::new(
                                6. * round.sin(),
                                1.6 + 1.5 * (0.5 * phase).sin(),
                                6. * round.cos(),
                            ),
                    ));
                    rebuilds += c.animate(u, Some(&f), 1. / 60., frame as u64) as usize;
                }
                let posed = Instant::now();
                // The first frames set up buffers that later frames reuse.
                if frame >= 10 {
                    allocations += ALLOCATIONS.load(Relaxed) - before;
                }
                for (c, u) in characters.iter().zip(&units) {
                    draws += c.draws(u.position, c.body_yaw()).len();
                }
                let drawn = Instant::now();
                pose_ns += (posed - started).as_nanos();
                draw_ns += (drawn - posed).as_nanos();
            }
            let per = (frames * count) as f64 * 1000.;
            let posed = (frames.saturating_sub(10) * count).max(1) as f64;
            let run = match tier {
                Tier::Full => format!("lod{lod}"),
                Tier::Half => "half".into(),
                Tier::Quarter => "quarter".into(),
            };
            println!(
                "{label:12} {run:7}: pose {:6.2} us, palettes {:6.2} us per character per frame ({:.2} rebuilds, {} draws each, {:.2} allocations a pose)",
                pose_ns as f64 / per,
                draw_ns as f64 / per,
                rebuilds as f64 / (frames * count) as f64,
                draws / (frames * count),
                allocations as f64 / posed
            );
        }
    }
    Ok(())
}
