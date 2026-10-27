//! Review-only (uncommitted): fixed cameras square-on to the west ledge's and east cliff's
//! x- and z-facing limestone faces, set up like review_surfaces.rs. Writes captures, never assets.
use glam::{Mat4, Vec2, Vec3};
use render::{Camera, Renderer, character::Draw, model::ModelCache, scenery::Scenery};
use std::{path::PathBuf, sync::Arc};

/// The steepest point on the line from `from` to `to`, and the ground's normal there.
fn face(from: Vec2, to: Vec2) -> (Vec3, Vec3) {
    let (mut best, mut best_n) = (from, world::normal(from.x, from.y));
    for i in 0..=300 {
        let p = from.lerp(to, i as f32 / 300.);
        let n = world::normal(p.x, p.y);
        if n.y < best_n.y {
            (best, best_n) = (p, n);
        }
    }
    (Vec3::new(best.x, world::height(best.x, best.y), best.y), best_n)
}

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
    let mut written = std::collections::HashSet::new();
    // West ledge centred on (-66, -30), east cliff on (76, -85); each line crosses one face.
    for (name, from, to) in [
        ("west-ledge-plus-x", Vec2::new(-56., -50.), Vec2::new(-25., -50.)),
        ("west-ledge-plus-z", Vec2::new(-66., 15.), Vec2::new(-66., 60.)),
        ("west-ledge-minus-z", Vec2::new(-66., -95.), Vec2::new(-66., -135.)),
        ("east-cliff-minus-x", Vec2::new(66., -85.), Vec2::new(30., -85.)),
        ("east-cliff-plus-z", Vec2::new(76., -70.), Vec2::new(76., -15.)),
        ("east-cliff-minus-z", Vec2::new(76., -100.), Vec2::new(76., -150.)),
    ] {
        let (target, n) = face(from, to);
        // Name the face by the way it actually points.
        let facing = if n.x.abs() > n.z.abs() { if n.x > 0. { "plus-x" } else { "minus-x" } } else if n.z > 0. { "plus-z" } else { "minus-z" };
        let name = format!("{}-{facing}", name.rsplitn(3, '-').nth(2).unwrap());
        // Never let one capture overwrite another.
        let name = if written.insert(name.clone()) { name } else { format!("{name}-2") };
        let out = Vec2::new(n.x, n.z).normalize();
        let back = Vec2::new(target.x, target.z) + out * 30.;
        let eye = Vec3::new(back.x, (target.y + 2.).max(world::height(back.x, back.y) + 2.), back.y);
        println!("{name}: target {target:.1} normal {n:.2} eye {eye:.1}");
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
