//! Review-only (uncommitted): shadow review cameras (valley, village, ruin, trees, cascade-band walks), set up like
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
    // Valley, village and ruin views, a close tree (leaf cut-outs), and two slow walks that carry
    // shadows through the cascade blend bands at about 13-15 m and 44-50 m view depth.
    let valley = world::Valley::new();
    let village = glam::Vec2::new(2., -24.);
    let tree = valley.trees.iter().map(|t| t.position).min_by(|a, b| a.distance(village).total_cmp(&b.distance(village))).unwrap();
    let ground = |x: f32, z: f32| world::height(x, z);
    let tree_base = Vec3::new(tree.x, ground(tree.x, tree.y), tree.y);
    let ruin = Vec3::new(-18., 11., 47.);
    let mut shots: Vec<(String, Vec3, Vec3)> = vec![
        ("valley".into(), Vec3::new(30., 28., 10.), Vec3::new(0., 2., -40.)),
        ("village".into(), Vec3::new(18., 7., -6.), Vec3::new(2., 3., -26.)),
        ("ruin-near".into(), ruin + Vec3::new(14., 3., 12.), ruin),
        ("ruin-far".into(), ruin + Vec3::new(33., 8., 33.), ruin),
        ("tree-close".into(), tree_base + Vec3::new(7., 3., 6.), tree_base + Vec3::new(0., 2., 0.)),
    ];
    // Walks: the eye backs away 1 m per shot along its view, so each prop crosses a band.
    for (walk, eye, target) in [("walk-village", Vec3::new(10., 4.5, -12.), Vec3::new(0., 2., -30.)), ("walk-ruin", ruin + Vec3::new(30., 6., 34.), ruin)] {
        let back = (eye - target).with_y(0.).normalize();
        for step in 0..6 {
            let e = eye + back * step as f32;
            let e = e.with_y(e.y.max(ground(e.x, e.z) + 1.8));
            shots.push((format!("{walk}-{step}"), e, target));
        }
    }
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
    Ok(())
}
