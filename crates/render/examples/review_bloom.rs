//! Review-only (uncommitted): bloom review (sun, village lamps, sun-crossing pan), set up like review_surfaces.rs. Writes captures, never assets.
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
    // Bloom review: the sun (static look direction), the village and its lamps, and a fast pan
    // that carries the sun across the frame (every 5th frame saved), for halos and fireflies.
    let at = |x: f32, z: f32, up: f32| Vec3::new(x, world::height(x, z) + up, z);
    let sun = Vec3::from_array(renderer.look.sun_direction).normalize();
    let eye = at(10., -10., 3.);
    let shots: Vec<(&str, Vec3, Vec3)> = vec![
        ("sun", eye, eye + sun),
        ("village", at(18., -6., 5.), at(2., -26., 3.)),
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
    }
    let sun_yaw = sun.z.atan2(sun.x).to_degrees();
    renderer.reset_temporal_history();
    for frame in 0..90 {
        let f = frame as f32;
        let yaw = (sun_yaw - 45. + 1.0 * f).to_radians();
        let camera = Camera { eye, target: eye + Vec3::new(yaw.cos(), 0.5, yaw.sin()), fov: 1. };
        renderer.scene_time = Some(f / 60.);
        let mut draws = base();
        draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, f / 60.)?);
        let capture = output.join(format!("pan-{frame:03}.png"));
        renderer.draw(camera, &draws, (frame % 5 == 4).then_some(capture.as_path()))?;
    }
    renderer.wait()?;
    println!("bloom review done");
    Ok(())
}
