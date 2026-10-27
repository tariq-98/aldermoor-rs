//! Review-only (uncommitted): time-of-day review cameras (village, valley, ruin, tree; REVIEW_TIMES holds the scene time), set up like
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
    // Village, valley, ruin and tree views. REVIEW_TIMES (comma-separated seconds) holds the scene
    // at each of those times for the whole capture, for a time-of-day lapse; unset, time runs
    // from 0 as in the other reviews.
    let valley = world::Valley::new();
    let village = glam::Vec2::new(2., -24.);
    let tree = valley.trees.iter().map(|t| t.position).min_by(|a, b| a.distance(village).total_cmp(&b.distance(village))).unwrap();
    let tree_base = Vec3::new(tree.x, world::height(tree.x, tree.y), tree.y);
    let ruin = Vec3::new(-18., 11., 47.);
    let views: Vec<(&str, Vec3, Vec3)> = vec![
        ("village", Vec3::new(18., 7., -6.), Vec3::new(2., 3., -26.)),
        ("valley", Vec3::new(30., 28., 10.), Vec3::new(0., 2., -40.)),
        ("ruin", ruin + Vec3::new(14., 3., 12.), ruin),
        ("tree", tree_base + Vec3::new(7., 3., 6.), tree_base + Vec3::new(0., 2., 0.)),
    ];
    let times: Option<Vec<f32>> = std::env::var("REVIEW_TIMES").ok().map(|s| s.split(',').map(|t| t.trim().parse().unwrap()).collect());
    let only: Option<String> = std::env::var("REVIEW_VIEWS").ok();
    for (view, eye, target) in views {
        if only.as_ref().is_some_and(|o| !o.split(',').any(|v| v == view)) {
            continue;
        }
        for t in times.clone().unwrap_or(vec![-1.]) {
            let name = if t < 0. { view.to_string() } else { format!("{view}-t{t:05.2}") };
            println!("{name}: eye {eye:.1} target {target:.1}");
            let camera = Camera { eye, target, fov: 1. };
            renderer.reset_temporal_history();
            for frame in 0..60 {
                renderer.scene_time = Some(if t < 0. { frame as f32 / 60. } else { t });
                let mut draws = base();
                draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, frame as f32 / 60.)?);
                let capture = output.join(format!("{name}.png"));
                renderer.draw(camera, &draws, (frame == 59).then_some(capture.as_path()))?;
            }
            renderer.wait()?;
        }
    }
    Ok(())
}
