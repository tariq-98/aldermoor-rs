//! Optional material inputs supplied by Claude. Absent inputs leave neutral placeholders.
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::path::Path;
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceAssets {
    pub terrain: Option<Terrain>,
    pub water: Option<Water>,
    pub sky: Option<Sky>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrain {
    pub layers: Vec<Layer>,
    pub weights: String,
    pub bounds: [f32; 4],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub colour: String,
    pub normal: String,
    pub uv_metres: f32,
    pub roughness: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Water {
    pub normal: String,
    pub uv_metres: f32,
    pub normal_strength: f32,
    pub flow: [f32; 2],
    pub deep_colour: [f32; 3],
    pub shallow_colour: [f32; 3],
    pub absorption: [f32; 3],
    pub roughness: f32,
    pub opacity: f32,
    pub reflection_strength: f32,
    pub ior: f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sky {
    pub texture: String,
    pub rotation_radians: f32,
    pub intensity: f32,
}
impl SurfaceAssets {
    pub fn load(path: &Path) -> Result<Self> {
        let inputs: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        if let Some(t) = &inputs.terrain {
            ensure!(
                (1..=4).contains(&t.layers.len())
                    && t.bounds.iter().all(|v| v.is_finite())
                    && t.bounds[2] > t.bounds[0]
                    && t.bounds[3] > t.bounds[1],
                "invalid terrain layer/bounds inputs"
            );
            for l in &t.layers {
                ensure!(
                    l.uv_metres.is_finite()
                        && l.uv_metres > 0.
                        && l.roughness.is_finite()
                        && (0.0..=1.).contains(&l.roughness),
                    "invalid terrain layer sampling"
                );
            }
        }
        if let Some(w) = &inputs.water {
            ensure!(
                w.uv_metres.is_finite()
                    && w.uv_metres > 0.
                    && w.ior.is_finite()
                    && w.ior > 1.
                    && (0.0..=1.).contains(&w.opacity)
                    && (0.0..=1.).contains(&w.roughness),
                "invalid water inputs"
            );
            ensure!(
                w.deep_colour
                    .iter()
                    .chain(&w.shallow_colour)
                    .chain(&w.absorption)
                    .chain(&w.flow)
                    .chain([w.normal_strength, w.reflection_strength].iter())
                    .all(|v| v.is_finite()),
                "water inputs must be finite"
            );
        }
        if let Some(s) = &inputs.sky {
            ensure!(
                s.rotation_radians.is_finite() && s.intensity.is_finite() && s.intensity >= 0.,
                "invalid sky inputs"
            );
        }
        Ok(inputs)
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniform {
    pub bounds: [f32; 4],
    pub scales: [f32; 4],
    pub roughness: [f32; 4],
    pub water_deep: [f32; 4],
    pub water_shallow: [f32; 4],
    pub water_absorption: [f32; 4],
    pub water_flow: [f32; 4],
    pub sky: [f32; 4],
    pub flags: [u32; 4],
}
impl Default for Uniform {
    fn default() -> Self {
        Self {
            bounds: [0., 0., 1., 1.],
            scales: [0.; 4],
            roughness: [1.; 4],
            water_deep: [0.; 4],
            water_shallow: [0.; 4],
            water_absorption: [0.; 4],
            water_flow: [0.; 4],
            sky: [0.; 4],
            flags: [0; 4],
        }
    }
}
