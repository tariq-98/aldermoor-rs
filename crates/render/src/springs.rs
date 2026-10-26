//! Fixed 120 Hz cape chains and capsule collision, following workshop/src/springs.js.
use crate::character::Character;
use glam::{Mat4, Quat, Vec3, Vec3A};
use serde_json::Value;
struct Joint {
    bone: usize,
    /// The joint whose bone is this one's parent, if any.
    parent: Option<usize>,
    /// Bones under this one that aren't joints, which follow it each time it turns.
    others: Vec<usize>,
    axis: Vec3,
    length: f32,
    tail: Vec3A,
    previous: Vec3A,
}
struct Capsule {
    bone: usize,
    a: Vec3,
    b: Vec3,
    radius: f32,
}
pub(crate) struct SpringBones {
    /// Parents before children, so each joint turns from where its parent has just turned to.
    joints: Vec<Joint>,
    capsules: Vec<Capsule>,
    stiffness: f32,
    drag: f32,
    gravity: f32,
    radius: f32,
    acc: f32,
    ready: bool,
    last_origin: Vec3,
    /// How much of the springs' swing shows, which fades in and out over `FADE` as they turn on
    /// and off with the character's detail (#215).
    weight: f32,
    /// Each joint's world rotation once it has turned, this step.
    turned: Vec<Quat>,
    /// The capsules where they are this frame: one end, the way to the other end, one over its
    /// length squared, and how near a tail may come. `Vec3A`, as every tail of every step checks
    /// every capsule, and four lanes do a vector's sums at once.
    placed: Vec<(Vec3A, Vec3A, f32, f32)>,
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
                    parent: None,
                    others: Vec::new(),
                    axis: tail.normalize(),
                    length: tail.length(),
                    tail: Vec3A::ZERO,
                    previous: Vec3A::ZERO,
                });
            }
        }
        let depth = |mut i: usize| {
            let mut d = 0;
            while let Some(p) = c.nodes[i].parent {
                d += 1;
                i = p;
            }
            d
        };
        joints.sort_by_key(|j| depth(j.bone));
        let bones: Vec<_> = joints.iter().map(|j| j.bone).collect();
        for j in &mut joints {
            j.parent = c.nodes[j.bone]
                .parent
                .and_then(|p| bones.iter().position(|&b| b == p));
            j.others = (0..c.nodes.len())
                .filter(|&i| c.nodes[i].parent == Some(j.bone) && !bones.contains(&i))
                .collect();
        }
        let num =
            |key: &str, fallback: f32| spec[key].as_f64().map(|v| v as f32).unwrap_or(fallback);
        Self {
            turned: vec![Quat::IDENTITY; joints.len()],
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
            weight: 1.,
            placed: Vec::new(),
        }
    }
    /// Steps the chains on by `dt` and turns their bones after them, `on` at full detail (#215).
    /// Off, they fade out and then cost nothing, leaving the bones as the pose has them.
    pub fn update(&mut self, c: &mut Character, root: Mat4, dt: f32, on: bool) {
        const FADE: f32 = 0.5;
        let fade = dt.max(0.) / FADE;
        self.weight = if on {
            (self.weight + fade).min(1.)
        } else {
            (self.weight - fade).max(0.)
        };
        if self.weight == 0. {
            // Turned back on, the tails start again from where the bones are.
            self.ready = false;
            return;
        }
        let inverse = root.inverse();
        let origin = root.w_axis.truncate();
        if !self.ready || origin.distance(self.last_origin) > 10. {
            if !self.ready && on {
                self.weight = fade.min(1.);
            }
            for j in &mut self.joints {
                let m = root * c.world[j.bone];
                j.tail = m.transform_point3a((j.axis * j.length).into());
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
        self.placed.clear();
        self.placed.extend(self.capsules.iter().map(|p| {
            let m = root * c.world[p.bone];
            let a = m.transform_point3a(p.a.into());
            let ab = m.transform_point3a(p.b.into()) - a;
            (
                a,
                ab,
                1. / ab.length_squared().max(1e-12),
                p.radius + self.radius,
            )
        }));
        let keep = (1. - self.drag.clamp(0., 1.)).powf(STEP * 60.);
        for step in 0..steps.max(1) {
            for (k, j) in self.joints.iter_mut().enumerate() {
                // Each joint turns from its rest under its parent as the parent stands now, so only
                // its own world matrix needs building: its children build theirs from it in turn.
                let parent = c.nodes[j.bone].parent;
                let above = parent.map_or(Mat4::IDENTITY, |p| c.world[p]);
                let above_rotation = match j.parent {
                    Some(p) => self.turned[p],
                    None => above.to_scale_rotation_translation().1,
                };
                let local = &mut c.local[j.bone];
                local.rotation = c.nodes[j.bone].rest.rotation;
                let at_rest = above_rotation * local.rotation;
                let from = at_rest * j.axis;
                let head =
                    root.transform_point3a(above.transform_point3a(local.translation.into()));
                let rest = root.transform_vector3a(from.into());
                if steps > 0 {
                    let mut next =
                        j.tail + (j.tail - j.previous) * keep + rest * self.stiffness * STEP
                            - Vec3A::Y * self.gravity * STEP;
                    next = head + (next - head).normalize_or(rest) * j.length;
                    for &(a, ab, inverse_length, reach) in &self.placed {
                        let t = ((next - a).dot(ab) * inverse_length).clamp(0., 1.);
                        let closest = a + ab * t;
                        let delta = next - closest;
                        if delta.length_squared() < reach * reach {
                            next = closest + delta.normalize_or(Vec3A::NEG_Z) * reach;
                            next = head + (next - head).normalize_or(rest) * j.length;
                        }
                    }
                    j.previous = j.tail;
                    j.tail = next;
                }
                let to = Vec3::from(inverse.transform_vector3a(j.tail - head));
                let world = if to.length_squared() < 1e-10 {
                    at_rest
                } else {
                    let world =
                        (Quat::from_rotation_arc(from, to.normalize()) * at_rest).normalize();
                    let world = if self.weight < 1. {
                        at_rest.slerp(world, self.weight)
                    } else {
                        world
                    };
                    local.rotation = (above_rotation.inverse() * world).normalize();
                    world
                };
                self.turned[k] = world;
                c.world[j.bone] = above * local.matrix();
                for &i in &j.others {
                    c.update_chain(i);
                }
            }
            if step == 0 && steps == 0 {
                break;
            }
        }
    }
}
