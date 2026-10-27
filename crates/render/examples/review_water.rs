//! Review-only (uncommitted): river water review (bank and bridge views, refraction, foam, edge-fade pan, timings), set up like review_surfaces.rs. Writes captures, never assets.
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
    // River water: bank and bridge views up and down the river, refraction in the shallows,
    // shoreline foam, and a pan from the bridge for screen-edge fade. Prints GPU and water pass
    // times (by name, so this also builds where those passes don't exist).
    let rz = world::river_z;
    let at = |x: f32, z: f32, up: f32| Vec3::new(x, world::height(x, z) + up, z);
    let water = |x: f32| Vec3::new(x, world::WATER_LEVEL, rz(x));
    let bridge = Vec3::new(world::BRIDGE_X, 4.6, rz(world::BRIDGE_X));
    let bank = at(20., rz(20.) + 11., 1.7);
    let views: Vec<(&str, Vec3, Vec3)> = vec![
        ("bank-upstream", bank, water(-25.)),
        ("bank-downstream", bank, water(65.)),
        ("bridge-upstream", bridge, water(-40.)),
        ("bridge-downstream", bridge, water(45.)),
        ("refraction-shallows", at(-10., rz(-10.) + 7., 1.2), water(-10.) + Vec3::new(0., -0.4, 3.)),
        ("foam-shore", at(30., rz(30.) + 9., 2.), water(30.) + Vec3::new(0., 0., 5.5)),
    ];
    let names = render::GPU_PASS_NAMES;
    let report = |name: &str, frames: Vec<render::GpuFrame>| {
        let med = |mut v: Vec<f64>| { if v.is_empty() { return f64::NAN; } v.sort_by(f64::total_cmp); v[v.len() / 2] };
        let gpu = med(frames.iter().map(|f| f.gpu_ms).collect());
        let mut line = format!("TIMING {name} gpu_ms={gpu:.3}");
        for pass in ["scene_sky_opaque_water_transparent", "water_transparent", "water_screen_space", "water_input_copies"] {
            if let Some(i) = names.iter().position(|n| *n == pass) {
                let v: Vec<f64> = frames.iter().filter_map(|f| f.passes[i].map(|p| p.vertex_ms + p.fragment_ms)).collect();
                line += &format!(" {pass}={:.3}(n={})", med(v.clone()), v.len());
            }
        }
        println!("{line}");
    };
    for (name, eye, target) in views {
        let camera = Camera { eye, target, fov: 1. };
        renderer.reset_temporal_history();
        for frame in 0..90 {
            renderer.scene_time = Some(frame as f32 / 60.);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, frame as f32 / 60.)?);
            let capture = output.join(format!("{name}.png"));
            renderer.draw(camera, &draws, (frame == 59).then_some(capture.as_path()))?;
            if frame == 30 { renderer.take_completed_frames().for_each(drop); }
        }
        renderer.wait()?;
        report(name, renderer.take_completed_frames().collect());
    }
    renderer.reset_temporal_history();
    for frame in 0..90 {
        let f = frame as f32;
        let yaw = (150f32 + 0.5 * f).to_radians();
        let camera = Camera { eye: bridge, target: bridge + Vec3::new(yaw.cos(), -0.12, yaw.sin()), fov: 1. };
        renderer.scene_time = Some(f / 60.);
        let mut draws = base();
        draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, f / 60.)?);
        let capture = output.join(format!("pan-{frame:03}.png"));
        renderer.draw(camera, &draws, (frame % 10 == 9).then_some(capture.as_path()))?;
    }
    renderer.wait()?;
    println!("pan: 90 frames");
    Ok(())
}
