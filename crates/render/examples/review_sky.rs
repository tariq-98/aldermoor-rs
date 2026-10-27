//! Review-only (uncommitted): fixed cameras on the sky's longitude wrap, set up like
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
    // atan2's longitude wrap faces -x (sky rotation 0). Look straight at it from high above the
    // valley at several pitches, from grazing to near the pole, wide and zoomed in, plus the
    // west ledge framing where #43 was seen.
    let eye = Vec3::new(-20., 70., -30.);
    let mut shots = Vec::new();
    for (pitch, label) in [(-4f32, "grazing"), (8., "low"), (30., "mid"), (60., "high"), (84., "pole")] {
        for (fov, zoom) in [(1f32, "wide"), (0.25, "zoom")] {
            let p = pitch.to_radians();
            let target = eye + Vec3::new(-p.cos(), p.sin(), 0.);
            shots.push((format!("seam-{label}-{zoom}"), eye, target, fov));
        }
    }
    shots.push(("west-ledge-view".into(), Vec3::new(-10.8, 5.6, -61.9), Vec3::new(-38.3, 3.6, -50.0), 1.));
    for (name, eye, target, fov) in shots {
        println!("{name}: eye {eye:.1} target {target:.1} fov {fov}");
        let camera = Camera { eye, target, fov };
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
