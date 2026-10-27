//! Review-only (uncommitted): terrain blend views at cliff bases and a gentle slope, plus a
//! moving pan and walk, set up like review_surfaces.rs. Writes captures, never assets.
use glam::{Mat4, Vec3};
use render::{Camera, Renderer, character::Draw, model::ModelCache, scenery::Scenery};
use std::{path::PathBuf, sync::Arc};

fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let output = PathBuf::from(std::env::var("REVIEW_OUT")?);
    std::fs::create_dir_all(&output)?;
    let catalog = assets::Catalog::load(&root)?;
    let mut cache = ModelCache::default();
    let mut scenery = Scenery::new(&catalog, &mut cache)?;
    let terrain = render::terrain::valley();
    let base = || {
        (0..terrain.primitives.len())
            .map(|primitive| Draw {
                model: terrain.clone(),
                primitive,
                instance: 0x8000_0000_0000_0000 | primitive as u64,
                transform: Mat4::IDENTITY,
                palette: Arc::from([]),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
                dye: None,
                base_map: None,
            })
            .collect::<Vec<_>>()
    };
    let mut renderer = Renderer::new(1800, 1100, 0.65)?;
    renderer.look = render::settings::LookSettings::load(&root.join("catalog/look.json"))?;
    renderer.load_surface_assets(&root.join("catalog/surfaces.json"), &root)?;
    // Cliff-base blends (grass, earth and rock), a gentle slope, then a slow pan and a walk with
    // the camera moving every frame under TAA, saving every 10th frame.
    let at = |x: f32, z: f32, up: f32| Vec3::new(x, world::height(x, z) + up, z);
    let shots: Vec<(&str, Vec3, Vec3)> = vec![
        ("east-cliff-base", at(25., -85., 2.), at(45.7, -85., 1.)),
        ("west-ledge-base", at(-20., -55., 2.), at(-38.3, -50., 1.)),
        ("east-cliff-north-base", at(76., -5., 2.), at(76., -31.7, 1.)),
        ("gentle-slope", at(40., 10., 2.), at(70., -10., 0.)),
    ];
    for (name, eye, target) in shots {
        println!("{name}: eye {eye:.1} target {target:.1}");
        let camera = Camera { eye, target, fov: 1. };
        renderer.reset_temporal_history();
        for frame in 0..60 {
            renderer.scene_time = Some(frame as f32 / 60.);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, frame as f32 / 60.)?);
            let capture = output.join(format!("{name}.png"));
            renderer.draw(camera, &draws, (frame == 59).then_some(capture.as_path()))?;
        }
        renderer.wait()?;
    }
    // Pan: yaw sweeps 0.25° a frame across the east cliff's base. Walk: the eye moves 0.08 m a
    // frame along the west ledge's base, looking at the face.
    let pan_eye = at(25., -85., 2.);
    let walk_eye = |f: f32| at(-22., -62. + 0.08 * f, 2.);
    for (name, frames) in [("pan", 120), ("walk", 120)] {
        renderer.reset_temporal_history();
        for frame in 0..frames {
            let f = frame as f32;
            let (eye, target) = if name == "pan" {
                let yaw = (-12. + 0.25 * f).to_radians();
                (pan_eye, pan_eye + Vec3::new(yaw.cos(), -0.08, yaw.sin()))
            } else {
                let e = walk_eye(f);
                (e, e + Vec3::new(-1., -0.1, 0.25))
            };
            let camera = Camera { eye, target, fov: 1. };
            renderer.scene_time = Some(f / 60.);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, f / 60.)?);
            let capture = output.join(format!("{name}-{frame:03}.png"));
            renderer.draw(camera, &draws, (frame % 10 == 9).then_some(capture.as_path()))?;
        }
        renderer.wait()?;
        println!("{name}: {frames} frames");
    }
    Ok(())
}
