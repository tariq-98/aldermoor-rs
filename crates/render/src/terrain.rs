//! Topology comes from the simulation; supplied surface catalogues provide terrain and water materials.
use crate::{model::*, visibility::Bounds};
use glam::{Mat4, Quat, Vec3};
use std::{collections::HashMap, sync::Arc};
const CHUNK_CELLS: usize = 32;

pub fn valley() -> Arc<Model> {
    let mut primitives = Vec::new();
    let step = world::TERRAIN_STEP;
    let n = (world::BOUNDS * 2. / step) as usize;
    let cells = CHUNK_CELLS;
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

/// Half/quarter-resolution interiors use the original world-height/normal vertices.
/// Keep every 2 m perimeter segment at all levels. Boundary cells fan into that
/// perimeter, so even a full-to-quarter neighbour has identical edge topology.
/// No skirts, neighbour lookup, duplicated vertices or per-frame mesh rebuilds.
pub(crate) fn lod_indices(chunk: usize, level: usize) -> Vec<u32> {
    assert!(level <= 2);
    let n = (world::BOUNDS * 2. / world::TERRAIN_STEP) as usize;
    let side = n.div_ceil(CHUNK_CELLS);
    assert!(chunk < side * side);
    let nx = (n - (chunk % side) * CHUNK_CELLS).min(CHUNK_CELLS);
    let nz = (n - (chunk / side) * CHUNK_CELLS).min(CHUNK_CELLS);
    let stride = 1 << level;
    let index = |x, z| (z * (nx + 1) + x) as u32;
    let mut indices = Vec::new();
    for z in (0..nz).step_by(stride) {
        for x in (0..nx).step_by(stride) {
            let right = (x + stride).min(nx);
            let bottom = (z + stride).min(nz);
            let a = index(x, z);
            let b = index(x, bottom);
            let c = index(right, z);
            let d = index(right, bottom);
            if level == 0 || (x > 0 && z > 0 && right < nx && bottom < nz) {
                indices.extend([a, b, c, c, b, d]);
                continue;
            }
            // Clockwise in xz gives upward-facing triangles. Only the chunk's
            // outer sides need the fine edge samples; internal edges stay coarse.
            let mut edge = vec![a];
            if x == 0 {
                edge.extend((z + 1..bottom).map(|ez| index(x, ez)));
            }
            edge.push(b);
            if bottom == nz {
                edge.extend((x + 1..right).map(|ex| index(ex, bottom)));
            }
            edge.push(d);
            if right == nx {
                edge.extend((z + 1..bottom).rev().map(|ez| index(right, ez)));
            }
            edge.push(c);
            if z == 0 {
                edge.extend((x + 1..right).rev().map(|ex| index(ex, z)));
            }
            let center = index((x + right) / 2, (z + bottom) / 2);
            for i in 0..edge.len() {
                indices.extend([center, edge[i], edge[(i + 1) % edge.len()]]);
            }
        }
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn boundary(indices: &[u32]) -> Vec<(u32, u32)> {
        let mut edges = BTreeMap::new();
        for t in indices.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                *edges.entry((a.min(b), a.max(b))).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&count| count == 1 || count == 2));
        edges
            .into_iter()
            .filter_map(|(edge, count)| (count == 1).then_some(edge))
            .collect()
    }

    #[test]
    fn every_lod_covers_each_chunk_and_has_the_same_fine_boundary() {
        let valley = valley();
        for (chunk, p) in valley
            .primitives
            .iter()
            .enumerate()
            .filter(|(_, p)| p.material.name == "__terrain_placeholder")
        {
            assert_eq!(lod_indices(chunk, 0), p.indices);
            let full_boundary = boundary(&p.indices);
            let mut previous_count = p.indices.len();
            for level in 1..=2 {
                let indices = lod_indices(chunk, level);
                assert!(indices.len() < previous_count);
                previous_count = indices.len();
                assert_eq!(
                    boundary(&indices),
                    full_boundary,
                    "chunk {chunk}, LOD {level}"
                );
                let mut area = 0.;
                for t in indices.chunks_exact(3) {
                    let [a, b, c] = [t[0], t[1], t[2]]
                        .map(|i| Vec3::from_slice(&p.vertices[i as usize].position[..3]));
                    let twice_area = (b - a).cross(c - a).y;
                    assert!(twice_area > 0., "degenerate or reversed triangle");
                    area += twice_area * 0.5;
                }
                assert_eq!(area, 4. * p.bounds.half.x * p.bounds.half.z);
            }
        }
    }

    #[test]
    fn neighbouring_edges_match_positions_normals_and_uvs_at_all_lod_pairs() {
        let valley = valley();
        let n = (world::BOUNDS * 2. / world::TERRAIN_STEP) as usize;
        let side = n.div_ceil(CHUNK_CELLS);
        for chunk in 0..side * side {
            for (neighbour, axis) in [(chunk + 1, 0), (chunk + side, 2)] {
                if (axis == 0 && chunk % side == side - 1) || neighbour >= side * side {
                    continue;
                }
                let p = &valley.primitives[chunk];
                let limit = p
                    .vertices
                    .iter()
                    .map(|v| v.position[axis])
                    .fold(f32::NEG_INFINITY, f32::max);
                let segments = |chunk_id: usize, level| {
                    let mesh = &valley.primitives[chunk_id];
                    let mut segments: Vec<_> = boundary(&lod_indices(chunk_id, level))
                        .into_iter()
                        .filter_map(|(a, b)| {
                            let a = mesh.vertices[a as usize];
                            let b = mesh.vertices[b as usize];
                            (a.position[axis] == limit && b.position[axis] == limit)
                                .then_some((a, b))
                        })
                        .collect();
                    segments.sort_by(|(a, _), (b, _)| {
                        a.position[2 - axis].total_cmp(&b.position[2 - axis])
                    });
                    segments
                };
                for a in 0..=2 {
                    for b in 0..=2 {
                        let left = segments(chunk, a);
                        let right = segments(neighbour, b);
                        assert!(!left.is_empty());
                        assert_eq!(left.len(), right.len());
                        for ((a, b), (c, d)) in left.into_iter().zip(right) {
                            for (u, v) in [(a, c), (b, d)] {
                                assert_eq!(u.position, v.position);
                                assert_eq!(u.normal, v.normal);
                                assert_eq!(u.uv, v.uv);
                            }
                        }
                    }
                }
            }
        }
    }
}
