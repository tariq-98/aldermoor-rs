//! Topology comes from the simulation; supplied surface catalogues provide terrain and water materials.
use crate::{model::*, visibility::Bounds};
use glam::{Mat4, Quat, Vec3};
use std::{collections::HashMap, sync::Arc};
pub fn valley() -> Arc<Model> {
    let mut primitives = Vec::new();
    let step = world::TERRAIN_STEP;
    let n = (world::BOUNDS * 2. / step) as usize;
    let cells = 32;
    for cz in (0..n).step_by(cells) {
        for cx in (0..n).step_by(cells) {
            let nx = (n - cx).min(cells);
            let nz = (n - cz).min(cells);
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            for z in 0..=nz {
                for x in 0..=nx {
                    let x = (cx + x) as f32 * step - world::BOUNDS;
                    let z = (cz + z) as f32 * step - world::BOUNDS;
                    let mut v = Vertex::plain(
                        Vec3::new(x, world::height(x, z), z),
                        world::normal(x, z),
                        [1.; 4],
                    );
                    v.uv = [x, z, 0., 0.];
                    vertices.push(v);
                }
            }
            for z in 0..nz {
                for x in 0..nx {
                    let a = (z * (nx + 1) + x) as u32;
                    let b = a + (nx + 1) as u32;
                    indices.extend([a, b, a + 1, a + 1, b, b + 1]);
                }
            }
            let bounds =
                Bounds::from_points(vertices.iter().map(|v| Vec3::from_slice(&v.position[..3])));
            primitives.push(Primitive {
                vertices,
                indices,
                bounds,
                material: Material {
                    name: "__terrain_placeholder".into(),
                    base: [0.5, 0.5, 0.5, 1.],
                    roughness: 1.,
                    ..Default::default()
                },
                node: 0,
                skin: None,
                morphs: Vec::new(),
                morph_names: Vec::new(),
            });
        }
    }
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for x in -140..=140 {
        let x = x as f32 * 2.;
        for side in [-1., 1.] {
            let z = world::river_z(x) + side * 6.;
            let mut vertex = Vertex::plain(Vec3::new(x, -0.78, z), Vec3::Y, [1.; 4]);
            vertex.uv = [x, z, (-0.78 - world::height(x, z)).max(0.), 0.];
            vertices.push(vertex);
        }
    }
    for x in 0..280u32 {
        let i = x * 2;
        indices.extend([i, i + 1, i + 2, i + 2, i + 1, i + 3]);
    }
    let bounds = Bounds::from_points(vertices.iter().map(|v| Vec3::from_slice(&v.position[..3])));
    primitives.push(Primitive {
        vertices,
        indices,
        bounds,
        material: Material {
            name: "__water_placeholder".into(),
            base: [0.5, 0.5, 0.5, 1.],
            roughness: 1.,
            ..Default::default()
        },
        node: 0,
        skin: None,
        morphs: Vec::new(),
        morph_names: Vec::new(),
    });
    Arc::new(Model {
        id: next_model_id(),
        key: "world:neutral-topology".into(),
        nodes: vec![Node {
            name: "world".into(),
            parent: None,
            rest: Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        }],
        rest_world: vec![Mat4::IDENTITY],
        skins: Vec::new(),
        primitives,
        clips: HashMap::new(),
    })
}
