//! Read-only timing diagnostic for unmarked vendor clips. Never modifies assets.
use glam::{Quat, Vec3};
use render::model::{ModelCache, Property, world_matrices};
fn main() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let cat = assets::Catalog::load(&root)?;
    let mut cache = ModelCache::default();
    let rig = cache.get(&cat.path(&cat.humanoid.body_types["male"].file))?;
    let model = cache.get(&cat.path(&cat.humanoid.animations.file))?;
    let clip = &model.clips["Sword_Heavy_Combo"];
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
