//! Review-only (uncommitted): far-cascade cache review (views, fast turn, cut, band walk, windmill, moving sun), set up like review_surfaces.rs. Writes captures, never assets.
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
    // Far-cascade cache review. A run is a list of per-frame cameras plus the frames to save.
    // REVIEW_SUN_SPEED (seconds of day per frame) advances scene_time faster for a moving sun.
    let at = |x: f32, z: f32, up: f32| Vec3::new(x, world::height(x, z) + up, z);
    let sun_speed: f32 = std::env::var("REVIEW_SUN_SPEED").ok().map_or(1. / 60., |s| s.parse().unwrap());
    let look = |eye: Vec3, yaw: f32, pitch: f32| (eye, eye + Vec3::new(yaw.to_radians().cos(), pitch, yaw.to_radians().sin()));
    let ruin = Vec3::new(-18., 11., 47.);
    let village_eye = at(18., -6., 5.);
    let vy = (-26f32 + 6.).atan2(2. - 18.).to_degrees();
    let hold = |eye: Vec3, target: Vec3, n: usize| vec![(eye, target); n];
    let mut runs: Vec<(String, Vec<(Vec3, Vec3)>, Vec<usize>)> = vec![
        ("valley".into(), hold(at(30., -10., 28.), at(0., -50., 2.), 60), vec![59]),
        ("village".into(), hold(village_eye, at(2., -26., 3.), 60), vec![59]),
        ("ruin".into(), hold(ruin + Vec3::new(33., 8., 33.), ruin, 60), vec![59]),
        ("windmill-far".into(), hold(at(-10., -20., 4.), at(60., -100., 25.), 90), vec![59, 69, 79, 89]),
    ];
    // Fast turn: 4 degrees a frame for 60 frames after 30 settled frames.
    let turn: Vec<_> = (0..90).map(|f| look(village_eye, vy + if f < 30 { 0. } else { 4. * (f - 30) as f32 }, -0.1)).collect();
    runs.push(("turn".into(), turn, (30..90).step_by(6).collect()));
    // Camera cut: 40 frames on the village, then a hard cut to the ruin; save the first frames after it.
    let mut cut = hold(village_eye, at(2., -26., 3.), 40);
    cut.extend(hold(ruin + Vec3::new(30., 6., 34.), ruin, 40));
    runs.push(("cut".into(), cut, vec![39, 40, 41, 42, 44, 48, 55, 79]));
    // Walk back 1 m every 4 frames through the ~44-50 m band from the ruin.
    let walk: Vec<_> = (0..120).map(|f| {
        let e = ruin + Vec3::new(30., 6., 34.) + Vec3::new(0.6, 0., 0.68) * (f as f32 / 4.);
        (e.with_y(e.y.max(world::height(e.x, e.z) + 1.8)), ruin)
    }).collect();
    runs.push(("walk".into(), walk, (9..120).step_by(10).collect()));
    for (name, cams, saves) in runs {
        renderer.reset_temporal_history();
        for (frame, (eye, target)) in cams.iter().enumerate() {
            let camera = Camera { eye: *eye, target: *target, fov: 1. };
            let t = frame as f32 * sun_speed;
            renderer.scene_time = Some(t);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, t)?);
            let capture = output.join(format!("{name}-{frame:03}.png"));
            renderer.draw(camera, &draws, saves.contains(&frame).then_some(capture.as_path()))?;
        }
        renderer.wait()?;
        println!("{name}: {} frames", cams.len());
    }
    Ok(())
}
