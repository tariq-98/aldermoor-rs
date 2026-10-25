//! CPU cost of animating characters: `Character::update_combat` (pose) and `Character::draws`
//! (skinning palettes) per character per frame, for a few typical looks and poses.
//!
//!     cargo run --release -p render --example anim_bench -- [characters] [frames] [case]
//!
//! The cases are listed in `main`; without one, it runs them all.
use glam::Vec3;
use protocol::{Action, Look, Pose, ResourceKind, UnitCombat, UnitKind, UnitState};
use render::character::Character;
use render::model::ModelCache;
use std::time::Instant;

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
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let count: usize = args.next().map_or(Ok(30), |a| a.parse())?;
    let frames: usize = args.next().map_or(Ok(600), |a| a.parse())?;
    let only = args.next();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let cat = assets::Catalog::load(&root)?;
    let mut cache = ModelCache::default();
    // "idle" and "run" hold their pose throughout. "run + slash" starts the slash over every half
    // second, and "start-stop" runs and stops every half second, so those two are blending clips
    // most of the time.
    for (label, pose, speed, action) in [
        ("idle", Pose::Idle, 0., Action::None),
        ("run", Pose::Run, 6., Action::None),
        ("run + slash", Pose::Attack, 6., Action::Slash),
        ("start-stop", Pose::Run, 6., Action::None),
    ] {
        if only.as_ref().is_some_and(|o| o != label) {
            continue;
        }
        for lod in 0..3 {
            let mut characters = (0..count)
                .map(|i| Character::load_lod(&cat, &mut cache, &look(i), lod))
                .collect::<anyhow::Result<Vec<_>>>()?;
            let mut units: Vec<_> = (0..count)
                .map(|i| unit(i as u32, look(i), pose, speed))
                .collect();
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
            let (mut pose_ns, mut draw_ns, mut draws) = (0u128, 0u128, 0usize);
            for frame in 0..frames {
                let t = frame as f32 / 60.;
                let started = Instant::now();
                for (c, u) in characters.iter_mut().zip(&mut units) {
                    if label == "start-stop" {
                        let running = (t / 0.5) as usize % 2 == 1;
                        u.pose = if running { Pose::Run } else { Pose::Idle };
                        u.velocity.z = if running { speed } else { 0. };
                    }
                    let cycle = t % 0.5;
                    u.pose_time = if matches!(label, "idle" | "run") {
                        t
                    } else {
                        cycle
                    };
                    let f = fight(u.id, if action == Action::None { t } else { cycle });
                    c.update_combat(u, Some(&f), 1. / 60.);
                }
                let posed = Instant::now();
                for (c, u) in characters.iter().zip(&units) {
                    draws += c.draws(u.position, u.yaw).len();
                }
                let drawn = Instant::now();
                pose_ns += (posed - started).as_nanos();
                draw_ns += (drawn - posed).as_nanos();
            }
            let per = (frames * count) as f64 * 1000.;
            println!(
                "{label:12} lod{lod}: pose {:6.2} us, palettes {:6.2} us per character per frame ({} draws each)",
                pose_ns as f64 / per,
                draw_ns as f64 / per,
                draws / (frames * count)
            );
        }
    }
    Ok(())
}
