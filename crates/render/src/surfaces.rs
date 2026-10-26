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
    #[serde(default)]
    pub screen_space: WaterScreenSpace,
}
/// Opt-in water quality. Zero defaults are neutral, not authored look values.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaterScreenSpace {
    pub enabled: bool,
    pub reflections: bool,
    pub refraction: bool,
    pub shoreline: bool,
    pub ray_distance: f32,
    pub thickness: f32,
    pub edge_fade: f32,
    pub refraction_metres: f32,
    pub shore_depth: f32,
    pub foam_strength: f32,
    pub foam_scale: f32,
}
impl WaterScreenSpace {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.ray_distance,
                self.thickness,
                self.edge_fade,
                self.refraction_metres,
                self.shore_depth,
                self.foam_strength,
                self.foam_scale
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.),
            "screen-space water inputs must be finite and nonnegative"
        );
        ensure!(
            self.edge_fade <= 0.5 && self.foam_strength <= 1.,
            "invalid water edge fade or foam strength"
        );
        ensure!(
            !self.reflections
                || (self.ray_distance > 0. && self.thickness > 0. && self.edge_fade > 0.),
            "water reflections need positive ray distance, thickness and edge fade"
        );
        ensure!(
            !self.refraction || self.refraction_metres > 0.,
            "water refraction needs a positive offset distance"
        );
        ensure!(
            !self.shoreline || (self.shore_depth > 0. && self.foam_scale > 0.),
            "water shoreline needs positive depth and foam scale"
        );
        Ok(())
    }
    pub(crate) fn pack(self, u: &mut Uniform) {
        u.water_screen = [
            self.ray_distance,
            self.thickness,
            self.edge_fade,
            self.refraction_metres,
        ];
        u.water_shore = [self.shore_depth, self.foam_strength, self.foam_scale, 0.];
        u.water_features = [
            self.enabled as u32,
            self.reflections as u32,
            self.refraction as u32,
            self.shoreline as u32,
        ];
    }
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
            w.screen_space.validate()?;
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
    pub water_screen: [f32; 4],
    pub water_shore: [f32; 4],
    pub water_features: [u32; 4],
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
            water_screen: [0.; 4],
            water_shore: [0.; 4],
            water_features: [0; 4],
        }
    }
}

#[cfg(test)]
mod water_tests {
    use super::*;
    #[test]
    fn legacy_water_has_neutral_screen_space_defaults() {
        let inputs: SurfaceAssets =
            serde_json::from_str(include_str!("../../../assets/catalog/surfaces.json")).unwrap();
        let settings = inputs.water.unwrap().screen_space;
        settings.validate().unwrap();
        let mut u = Uniform::default();
        settings.pack(&mut u);
        assert_eq!(u.water_features, [0; 4]);
        assert_eq!(u.water_screen, [0.; 4]);
        assert_eq!(u.water_shore, [0.; 4]);
    }
    #[test]
    fn water_rejects_invalid_feature_inputs() {
        for json in [
            r#"{"reflections":true}"#,
            r#"{"refraction":true}"#,
            r#"{"shoreline":true}"#,
            r#"{"edge_fade":0.6}"#,
            r#"{"foam_strength":1.1}"#,
            r#"{"thickness":-1}"#,
        ] {
            let s: WaterScreenSpace = serde_json::from_str(json).unwrap();
            assert!(s.validate().is_err(), "{json}");
        }
        assert!(serde_json::from_str::<WaterScreenSpace>(r#"{"enable":true}"#).is_err());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                WaterScreenSpace {
                    ray_distance: value,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
    }
}
