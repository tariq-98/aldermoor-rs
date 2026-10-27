//! Review-only (uncommitted): TAA motion review (village and foliage pans, a fence strafe), set up like review_surfaces.rs. Writes captures, never assets.
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
    // TAA under motion: slow and fast yaw pans over the village and over a tree's foliage, and a
    // slow sideways move along a fence with sky behind it (thin edges). Every 5th frame is saved.
    let valley = world::Valley::new();
    let village = glam::Vec2::new(2., -24.);
    let tree = valley.trees.iter().map(|t| t.position).min_by(|a, b| a.distance(village).total_cmp(&b.distance(village))).unwrap();
    let at = |x: f32, z: f32, up: f32| Vec3::new(x, world::height(x, z) + up, z);
    let tree_eye = at(tree.x + 8., tree.y + 6., 2.5);
    let tree_yaw = (tree.y - tree_eye.z).atan2(tree.x - tree_eye.x).to_degrees();
    let village_eye = at(18., -6., 5.);
    let village_yaw = (-26f32 + 6.).atan2(2. - 18.).to_degrees();
    // (name, eye, start yaw in degrees, yaw per frame, pitch, sideways metres per frame)
    let runs: Vec<(&str, Vec3, f32, f32, f32, f32)> = vec![
        ("village-slow", village_eye, village_yaw - 9., 0.15, -0.12, 0.),
        ("village-fast", village_eye, village_yaw - 60., 1.0, -0.12, 0.),
        ("foliage-slow", tree_eye, tree_yaw - 9., 0.15, 0.1, 0.),
        ("foliage-fast", tree_eye, tree_yaw - 60., 1.0, 0.1, 0.),
        ("fence-strafe", at(tree.x - 3., tree.y + 9., 0.9), -90., 0., 0.12, 0.05),
    ];
    for (name, eye0, yaw0, dyaw, pitch, side) in runs {
        renderer.reset_temporal_history();
        for frame in 0..120 {
            let f = frame as f32;
            let yaw = (yaw0 + dyaw * f).to_radians();
            let eye = eye0 + Vec3::new(side * f, 0., 0.);
            let target = eye + Vec3::new(yaw.cos(), pitch, yaw.sin());
            let camera = Camera { eye, target, fov: 1. };
            renderer.scene_time = Some(f / 60.);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, f / 60.)?);
            let capture = output.join(format!("{name}-{frame:03}.png"));
            renderer.draw(camera, &draws, (frame % 5 == 4).then_some(capture.as_path()))?;
        }
        renderer.wait()?;
        println!("{name}: eye {eye0:.1}");
    }
    Ok(())
}
