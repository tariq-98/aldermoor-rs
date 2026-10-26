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
use protocol::{Action, HitWeight, Look, Pose, ResourceKind, UnitCombat, UnitKind, UnitState};
use render::character::{Character, Ground};
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
        aim_pitch: 0,
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
    ] {
        if only.as_ref().is_some_and(|o| o != label) {
            continue;
        }
        for &(lod, tier) in &runs {
            let mut characters = (0..count)
                .map(|i| Character::load_lod(&cat, &mut cache, &look(i), lod))
                .collect::<anyhow::Result<Vec<_>>>()?;
            for c in &mut characters {
                c.set_tier(tier);
                c.set_ground(Arc::clone(&ground));
                c.foot_placement = feet;
            }
            // Side by side on the slopes west of the village, going north over the terraces.
            let mut units: Vec<_> = (0..count)
                .map(|i| {
                    let mut u = unit(i as u32, look(i), pose, speed);
                    u.position = Vec3::new(
                        -75. + 1.5 * (i % 20) as f32,
                        0.,
                        -70. - 3. * (i / 20) as f32,
                    );
                    u
                })
                .collect();
            if label == "strafe" {
                for u in &mut units {
                    u.velocity = Vec3::new(speed, 0., 0.);
                }
            }
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
                for (c, u) in characters.iter_mut().zip(&mut units) {
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
                    u.pose_time = if matches!(label, "idle" | "turn" | "walk" | "run" | "strafe") {
                        t
                    } else {
                        cycle
                    };
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
                    let f = fight(u.id, if action == Action::None { t } else { cycle });
                    u.position += u.velocity.with_y(0.) / 60.;
                    u.position.y = valley.ground(u.position.x, u.position.z) + up;
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
