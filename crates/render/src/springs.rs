//! Fixed 120 Hz cape chains and capsule collision, following workshop/src/springs.js.
use crate::character::Character;
use glam::{Mat4, Vec3};
use serde_json::Value;
struct Joint {
    bone: usize,
    axis: Vec3,
    length: f32,
    tail: Vec3,
    previous: Vec3,
}
struct Capsule {
    bone: usize,
    a: Vec3,
    b: Vec3,
    radius: f32,
}
pub(crate) struct SpringBones {
    joints: Vec<Joint>,
    capsules: Vec<Capsule>,
    stiffness: f32,
    drag: f32,
    gravity: f32,
    radius: f32,
    acc: f32,
    ready: bool,
    last_origin: Vec3,
}
impl SpringBones {
    pub fn new(c: &Character, spec: &Value, colliders: &[assets::Collider]) -> Self {
        let mut joints = Vec::new();
        for chain in spec["chains"].as_array().into_iter().flatten() {
            let bones: Vec<_> = chain
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| c.names.get(n.as_str()?).copied())
                .collect();
            for (i, &bone) in bones.iter().enumerate() {
                let mut tail = if let Some(child) = bones.get(i + 1) {
                    c.nodes[*child].rest.translation
                } else {
                    let p = c.nodes[bone].rest.translation;
                    if p.length_squared() > 1e-10 {
                        p.normalize() * p.length().min(0.08)
                    } else {
                        Vec3::new(0., -0.07, 0.)
                    }
                };
                if tail.length_squared() < 1e-10 {
                    tail = Vec3::new(0., -0.07, 0.)
                }
                joints.push(Joint {
                    bone,
                    axis: tail.normalize(),
                    length: tail.length(),
                    tail: Vec3::ZERO,
                    previous: Vec3::ZERO,
                });
            }
        }
        let num =
            |key: &str, fallback: f32| spec[key].as_f64().map(|v| v as f32).unwrap_or(fallback);
        Self {
            joints,
            capsules: colliders
                .iter()
                .filter_map(|p| {
                    Some(Capsule {
                        bone: *c.names.get(&p.bone)?,
                        a: p.a.into(),
                        b: p.b.into(),
                        radius: p.radius,
                    })
                })
                .collect(),
            stiffness: num("stiffness", 0.6),
            drag: num("drag", 0.4),
            gravity: num("gravity", 1.),
            radius: num("radius", 0.02),
            acc: 0.,
            ready: false,
            last_origin: Vec3::ZERO,
        }
    }
    pub fn update(&mut self, c: &mut Character, root: Mat4, dt: f32) {
        let inverse = root.inverse();
        let origin = root.w_axis.truncate();
        if !self.ready || origin.distance(self.last_origin) > 10. {
            for j in &mut self.joints {
                let m = root * c.world[j.bone];
                j.tail = m.transform_point3(j.axis * j.length);
                j.previous = j.tail;
            }
            self.ready = true;
            self.acc = 0.;
        }
        self.last_origin = origin;
        const STEP: f32 = 1. / 120.;
        self.acc = (self.acc + dt.max(0.)).min(STEP * 8.);
        let steps = (self.acc / STEP).floor() as usize;
        self.acc -= steps as f32 * STEP;
        let capsules: Vec<_> = self
            .capsules
            .iter()
            .map(|p| {
                let m = root * c.world[p.bone];
                (m.transform_point3(p.a), m.transform_point3(p.b), p.radius)
            })
            .collect();
        for step in 0..steps.max(1) {
            for j in &mut self.joints {
                c.local[j.bone].rotation = c.nodes[j.bone].rest.rotation;
                c.update_chain(j.bone);
                let m = root * c.world[j.bone];
                let head = m.w_axis.truncate();
                let rest = m.transform_vector3(j.axis).normalize();
                if steps > 0 {
                    let keep = (1. - self.drag.clamp(0., 1.)).powf(STEP * 60.);
                    let mut next =
                        j.tail + (j.tail - j.previous) * keep + rest * self.stiffness * STEP
                            - Vec3::Y * self.gravity * STEP;
                    next = head + (next - head).normalize_or(rest) * j.length;
                    for (a, b, r) in &capsules {
                        let ab = *b - *a;
                        let t =
                            ((next - *a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0., 1.);
                        let closest = *a + ab * t;
                        let delta = next - closest;
                        let distance = delta.length();
                        if distance < r + self.radius {
                            next = closest + delta.normalize_or(Vec3::NEG_Z) * (r + self.radius);
                            next = head + (next - head).normalize_or(rest) * j.length;
                        }
                    }
                    j.previous = j.tail;
                    j.tail = next;
                }
                c.turn_bone(
                    j.bone,
                    inverse.transform_vector3(rest),
                    inverse.transform_vector3(j.tail - head),
                    1.,
                );
            }
            if step == 0 && steps == 0 {
                break;
            }
        }
    }
}
