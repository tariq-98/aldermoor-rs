use std::sync::Arc;
// Draw only the catalogue's supplied placements. No decorative scatter is generated here.
use crate::{
    Camera,
    character::Draw,
    model::ModelCache,
    visibility::{Bounds, lod_for_height, screen_height},
};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
#[derive(Clone)]
struct Instance {
    prop: assets::Prop,
    transform: Mat4,
    lod: Option<usize>,
}
pub struct Scenery {
    instances: Vec<Instance>,
    pub colliders: Vec<(Mat4, Bounds)>,
}
impl Scenery {
    pub fn new(catalog: &assets::Catalog, cache: &mut ModelCache) -> Result<Self> {
        let env = catalog
            .environment
            .as_ref()
            .context("environment catalogue is required")?;
        let v = world::Valley::new();
        let mut scene = Self {
            instances: Vec::new(),
            colliders: Vec::new(),
        };
        let mut place = |id: &str, position: Vec3, yaw: f32, scale: f32| -> Result<()> {
            let prop = env
                .props
                .iter()
                .find(|p| p.id == id)
                .with_context(|| format!("missing scenery {id}"))?
                .clone();
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                Quat::from_rotation_y(yaw),
                position,
            );
            if matches!(prop.kind.as_str(), "house" | "landmark")
                && let Some([w, d]) = prop.footprint
            {
                scene.colliders.push((
                    transform,
                    Bounds {
                        center: Vec3::Y * prop.height * 0.5,
                        half: Vec3::new(w, prop.height, d) * 0.5,
                    },
                ));
            }
            scene.instances.push(Instance {
                prop,
                transform,
                lod: None,
            });
            Ok(())
        };
        ensure!(
            !env.placements.houses.is_empty() && !env.placements.trees.is_empty(),
            "placement families cannot be empty"
        );
        for (i, p) in v.houses.iter().enumerate() {
            place(
                &env.placements.houses[i % env.placements.houses.len()],
                Vec3::new(
                    p.position.x,
                    world::height(p.position.x, p.position.y),
                    p.position.y,
                ),
                p.yaw,
                p.scale,
            )?;
        }
        for (i, p) in v.trees.iter().enumerate() {
            place(
                &env.placements.trees[i % env.placements.trees.len()],
                Vec3::new(
                    p.position.x,
                    world::height(p.position.x, p.position.y),
                    p.position.y,
                ),
                p.yaw,
                p.scale,
            )?;
        }
        place(
            &env.placements.bridge,
            Vec3::new(world::BRIDGE_X, 0., world::river_z(world::BRIDGE_X)),
            0.,
            1.,
        )?;
        for p in env
            .placements
            .landmarks
            .iter()
            .chain(&env.placements.dressing)
        {
            place(
                &p.prop,
                Vec3::new(p.at[0], world::height(p.at[0], p.at[1]), p.at[1]),
                p.yaw,
                p.scale,
            )?;
        }
        // Load the supplied levels before play; changing LOD does not block a frame on disk I/O.
        for instance in &scene.instances {
            for lod in 0..=2 {
                cache.get_lod(&catalog.path(&instance.prop.file), lod)?;
            }
            for p in &instance.prop.parts {
                cache.get(&catalog.path(&p.file))?;
            }
        }
        Ok(scene)
    }
    pub fn len(&self) -> usize {
        self.instances.len()
    }
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
    pub fn draws(
        &mut self,
        catalog: &assets::Catalog,
        cache: &mut ModelCache,
        camera: Camera,
        height: u32,
        time: f32,
    ) -> Result<Vec<Draw>> {
        let mut out = Vec::new();
        for (i, instance) in self.instances.iter_mut().enumerate() {
            let origin = instance.transform.w_axis.truncate();
            let scale = instance.transform.x_axis.length();
            let depth = (origin - camera.eye).dot((camera.target - camera.eye).normalize());
            let pixels = screen_height(
                instance.prop.height * scale,
                depth.abs(),
                camera.fov,
                height,
            );
            let lod = if depth < 0. {
                2
            } else {
                lod_for_height(pixels, instance.lod)
            };
            instance.lod = Some(lod);
            let model = cache.get_lod(&catalog.path(&instance.prop.file), lod)?;
            for (pi, p) in model.primitives.iter().enumerate() {
                out.push(Draw {
                    dye: None,
                    base_map: None,
                    instance: 0x4000_0000_0000_0000 | ((i as u64) << 24) | pi as u64,
                    model: model.clone(),
                    primitive: pi,
                    transform: instance.transform * model.rest_world[p.node],
                    palette: Arc::from([]),
                    hidden: 0,
                    base: p.material.base,
                    morph: [0.; 2],
                });
            }
            for (j, part) in instance.prop.parts.iter().enumerate() {
                let model = cache.get(&catalog.path(&part.file))?;
                let spin = part.spin.as_ref().map_or(Quat::IDENTITY, |s| {
                    Quat::from_axis_angle(
                        Vec3::from_array(s.axis).normalize(),
                        time * s.rpm * std::f32::consts::TAU / 60.,
                    )
                });
                let transform =
                    instance.transform * Mat4::from_rotation_translation(spin, part.at.into());
                for (pi, p) in model.primitives.iter().enumerate() {
                    out.push(Draw {
                        dye: None,
                        base_map: None,
                        instance: 0x4000_0000_0000_0000
                            | ((i as u64) << 24)
                            | (((j + 1) as u64) << 16)
                            | pi as u64,
                        model: model.clone(),
                        primitive: pi,
                        transform: transform * model.rest_world[p.node],
                        palette: Arc::from([]),
                        hidden: 0,
                        base: p.material.base,
                        morph: [0.; 2],
                    });
                }
            }
        }
        Ok(out)
    }
    pub fn camera_eye(&self, target: Vec3, desired: Vec3, radius: f32) -> Vec3 {
        let mut t = 1f32;
        for (m, bounds) in &self.colliders {
            let inverse = m.inverse();
            let local_start = inverse.transform_point3(target);
            let local_end = inverse.transform_point3(desired);
            let r = radius / m.x_axis.length();
            if let Some(hit) = segment_box(local_start, local_end, bounds.padded(r)) {
                t = t.min((hit - 0.01).max(0.));
            }
        }
        // A target already inside a collision proxy must not collapse the view matrix.
        // Interior geometry will eventually provide more precise collision volumes.
        let delta = desired - target;
        if delta.length_squared() < 1e-8 {
            return target + Vec3::Z * 0.05;
        }
        target.lerp(desired, t.max((0.05 / delta.length()).min(1.)))
    }
}
pub fn segment_box(from: Vec3, to: Vec3, bounds: Bounds) -> Option<f32> {
    let delta = to - from;
    let lo = bounds.center - bounds.half;
    let hi = bounds.center + bounds.half;
    let (mut enter, mut exit) = (0f32, 1f32);
    for axis in 0..3 {
        if delta[axis].abs() < 1e-8 {
            if from[axis] < lo[axis] || from[axis] > hi[axis] {
                return None;
            }
        } else {
            let a = (lo[axis] - from[axis]) / delta[axis];
            let b = (hi[axis] - from[axis]) / delta[axis];
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
            if enter > exit {
                return None;
            }
        }
    }
    Some(enter)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_segment_stops_at_wall_and_misses_above_roof() {
        let b = Bounds {
            center: Vec3::new(0., 2., 0.),
            half: Vec3::new(3., 2., 3.),
        };
        assert_eq!(
            segment_box(Vec3::new(0., 2., 6.), Vec3::new(0., 2., 0.), b),
            Some(0.5)
        );
        assert!(segment_box(Vec3::new(0., 6., 6.), Vec3::new(0., 6., 0.), b).is_none());
        let scenery = Scenery {
            instances: Vec::new(),
            colliders: vec![(Mat4::IDENTITY, b)],
        };
        let target = b.center;
        let eye = scenery.camera_eye(target, target + Vec3::Z * 6., 0.25);
        assert!(eye.is_finite() && eye.distance(target) >= 0.049);
    }
}
