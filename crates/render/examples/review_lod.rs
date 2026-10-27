//! Review-only (uncommitted): terrain LOD review (walks across the LOD bands, slope views), set up like review_surfaces.rs. Writes captures, never assets.
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
    // Terrain LOD: walk out along the valley and back, so terrain crosses the ~64 m and ~160 m
    // LOD bands in both directions (every 5th frame saved), plus fixed views of slopes 15-50 m
    // out for self-shadow acne.
    let at = |x: f32, z: f32, up: f32| Vec3::new(x, world::height(x, z) + up, z);
    for (name, dir) in [("walk-out", 1f32), ("walk-back", -1.)] {
        renderer.reset_temporal_history();
        for frame in 0..240 {
            let f = frame as f32;
            // 0.6 m a frame over 144 m, looking along the valley towards the east cliff.
            let d = if dir > 0. { 0.6 * f } else { 144. - 0.6 * f };
            let eye = at(-60. + d, -40., 3.);
            let target = eye + Vec3::new(dir, -0.04, -0.15 * dir);
            let camera = Camera { eye, target, fov: 1. };
            renderer.scene_time = Some(f / 60.);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, f / 60.)?);
            let capture = output.join(format!("{name}-{frame:03}.png"));
            renderer.draw(camera, &draws, (frame % 5 == 4).then_some(capture.as_path()))?;
        }
        renderer.wait()?;
        println!("{name}: 240 frames");
    }
    let shots: Vec<(&str, Vec3, Vec3)> = vec![
        ("slopes-east", at(20., -60., 2.), at(55., -75., 0.)),
        ("slopes-west", at(-20., -30., 2.), at(-50., -20., 3.)),
        ("far-hills", at(0., 0., 6.), at(120., -150., 10.)),
    ];
    for (name, eye, target) in shots {
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
        println!("{name}: eye {eye:.1}");
    }
    Ok(())
}
