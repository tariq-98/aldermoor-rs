//! Read-only strike diagnostic: a clip's catalogue markers, then its blade tip's path at
//! 60 samples a second, to check the markers against the renderer's own skeleton solve.
//! Never modifies assets.
//!
//!     cargo run -p render --example inspect_strikes -- Sword_Overhead
use glam::{Quat, Vec3};
use render::model::{ModelCache, Property, world_matrices};
fn main() -> anyhow::Result<()> {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Sword_Overhead".into());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let cat = assets::Catalog::load(&root)?;
    let mut cache = ModelCache::default();
    let rig = cache.get(&cat.path(&cat.humanoid.body_types["male"].file))?;
    let model = cache.get(&cat.path(&cat.humanoid.animations.file))?;
    let clip = model
        .clips
        .get(&name)
        .ok_or_else(|| anyhow::anyhow!("no clip named {name}"))?;
    for meta in cat
        .humanoid
        .animations
        .clips
        .iter()
        .filter(|c| c.name == name)
    {
        for e in &meta.events {
            println!("# {} {:.3}", e.name, e.t);
        }
    }
    let names: std::collections::HashMap<_, _> = rig
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.as_str(), i))
        .collect();
    for frame in 0..=(clip.duration * 60.) as usize {
        let t = frame as f32 / 60.;
        let mut local = rig.nodes.iter().map(|n| n.rest).collect::<Vec<_>>();
        for track in &clip.tracks {
            if let Some(&i) = names.get(track.node.as_str()) {
                let value = track.sample(t);
                if matches!(track.property, Property::Rotation) {
                    local[i].rotation = Quat::from_array(value.to_array()).normalize();
                }
            }
        }
        let world = world_matrices(&rig.nodes, &local);
        let p = world[names["socket_hand_r"]].transform_point3(Vec3::Y * 0.8);
        println!("{t:.4},{:.5},{:.5},{:.5}", p.x, p.y, p.z);
    }
    Ok(())
}
