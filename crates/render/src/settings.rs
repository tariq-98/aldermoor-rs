//! Data supplied by the art owner, with deliberately neutral engine defaults.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LookSettings {
    pub sun_direction: [f32; 3],
    pub sun_colour: [f32; 3],
    pub sun_intensity: f32,
    pub ambient: [f32; 3],
    pub sky: [f32; 3],
    pub fog_colour: [f32; 3],
    pub fog_density: f32,
    pub exposure: f32,
    pub grade: [[f32; 3]; 3],
    pub tone_map: ToneMap,
    pub wind_strength: f32,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToneMap {
    #[default]
    Linear,
    Aces,
}
impl Default for LookSettings {
    fn default() -> Self {
        Self {
            sun_direction: [0., 1., 0.],
            sun_colour: [1.; 3],
            sun_intensity: 1.,
            ambient: [0.5; 3],
            sky: [0.5; 3],
            fog_colour: [0.5; 3],
            fog_density: 0.,
            exposure: 1.,
            grade: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            tone_map: ToneMap::Linear,
            wind_strength: 1.,
        }
    }
}
impl LookSettings {
    pub fn load(path: &Path) -> Result<Self> {
        let s: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        s.validate()?;
        Ok(s)
    }
    pub fn validate(&self) -> Result<()> {
        let numbers = self
            .sun_direction
            .iter()
            .chain(&self.sun_colour)
            .chain(&self.ambient)
            .chain(&self.sky)
            .chain(&self.fog_colour)
            .chain(self.grade.iter().flatten())
            .copied()
            .chain([
                self.sun_intensity,
                self.fog_density,
                self.exposure,
                self.wind_strength,
            ]);
        ensure!(
            numbers.into_iter().all(f32::is_finite),
            "look settings must be finite"
        );
        ensure!(
            glam::Vec3::from_array(self.sun_direction).length_squared() > 1e-8,
            "sun direction must be nonzero"
        );
        ensure!(
            self.sun_intensity >= 0.
                && self.fog_density >= 0.
                && self.exposure > 0.
                && self.wind_strength >= 0.,
            "invalid light/fog/exposure/wind settings"
        );
        Ok(())
    }
}
