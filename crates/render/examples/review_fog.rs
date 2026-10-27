//! Review-only (uncommitted): height-fog review cameras (valley low/high, ruin from the river, horizon, straight down/up), set up like
//! review_surfaces.rs. Writes captures, never assets.
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
    // Height-fog views: low and high over the valley, the ruin seen from the river, a flat look
    // at the horizon, and near-vertical looks down and up (±89.5°, so the camera stays defined).
    let ruin = Vec3::new(-18., 11., 47.);
    let down = Vec3::new(0., 30., -30.);
    let p = 89.5f32.to_radians();
    let views: Vec<(&str, Vec3, Vec3)> = vec![
        ("valley-low", Vec3::new(5., 2.5, -45.), Vec3::new(-40., 0., -62.)),
        ("valley-high", Vec3::new(30., 45., 20.), Vec3::new(0., 0., -50.)),
        ("ruin-from-river", Vec3::new(-15., 1., -50.), ruin + Vec3::new(0., 3., 0.)),
        ("horizon", Vec3::new(0., 6., -20.), Vec3::new(-100., 6., -20.)),
        ("straight-down", down, down + Vec3::new(0., -p.sin(), p.cos())),
        ("straight-up", down, down + Vec3::new(0., p.sin(), p.cos())),
    ];
    for (name, eye, target) in views {
        // Never start under the ground: keep the eye at least 1.8 m above the terrain.
        let eye = eye.with_y(eye.y.max(world::height(eye.x, eye.z) + 1.8));
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
    Ok(())
}
