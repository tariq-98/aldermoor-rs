//! Read-only camera reviews of the supplied valley materials; writes captures, never assets.
use glam::{Mat4, Vec3};
use render::{Camera, Renderer, character::Draw, model::ModelCache, scenery::Scenery};
use std::{path::PathBuf, sync::Arc};
fn main() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/render-review");
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
    for (name, eye, target) in [
        (
            "supplied-river",
            Vec3::new(12., 9., -41.),
            Vec3::new(2., -0.8, -59.),
        ),
        (
            "supplied-limestone",
            Vec3::new(-26., 13., -18.),
            Vec3::new(-55., 10., -30.),
        ),
    ] {
        let camera = Camera {
            eye,
            target,
            fov: 1.,
        };
        renderer.reset_temporal_history();
        for frame in 0..60 {
            renderer.scene_time = Some(frame as f32 / 60.);
            let mut draws = base();
            draws.extend(scenery.draws(&catalog, &mut cache, camera, 1100, frame as f32 / 60.)?);
            let capture = output.join(format!("{name}.png"));
            renderer.draw(camera, &draws, (frame == 59).then_some(capture.as_path()))?;
        }
        renderer.wait()?;
        println!("{}", output.join(format!("{name}.png")).display());
    }
    Ok(())
}
