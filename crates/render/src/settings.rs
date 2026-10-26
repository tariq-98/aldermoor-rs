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
    /// Screen-space obscurance; radius and bias are world metres. Disabled by default.
    pub ao_enabled: bool,
    pub ao_radius: f32,
    pub ao_bias: f32,
    pub ao_strength: f32,
    /// Linear-HDR bloom; radius is measured in half-resolution texels.
    pub bloom_enabled: bool,
    pub bloom_threshold: f32,
    /// Soft-knee width as a fraction of threshold; zero gives a hard threshold.
    pub bloom_knee: f32,
    pub bloom_radius: f32,
    pub bloom_strength: f32,
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
            ao_enabled: false,
            ao_radius: 0.,
            ao_bias: 0.,
            ao_strength: 0.,
            bloom_enabled: false,
            bloom_threshold: 0.,
            bloom_knee: 0.5,
            bloom_radius: 0.,
            bloom_strength: 0.,
        }
    }
}
impl LookSettings {
    pub fn ao_active(&self) -> bool {
        self.ao_enabled && self.ao_strength > 0. && self.ao_radius > 0.
    }
    pub fn bloom_active(&self) -> bool {
        self.bloom_enabled && self.bloom_strength > 0. && self.bloom_radius > 0.
    }
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
                self.ao_radius,
                self.ao_bias,
                self.ao_strength,
                self.bloom_threshold,
                self.bloom_knee,
                self.bloom_radius,
                self.bloom_strength,
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
        ensure!(
            self.ao_radius >= 0.
                && self.ao_bias >= 0.
                && (0. ..=1.).contains(&self.ao_strength)
                && self.bloom_threshold >= 0.
                && (0. ..=1.).contains(&self.bloom_knee)
                && self.bloom_radius >= 0.
                && self.bloom_strength >= 0.,
            "invalid AO/bloom settings"
        );
        ensure!(
            !self.ao_enabled || self.ao_strength == 0. || self.ao_radius > 0.,
            "enabled AO with nonzero strength needs a positive radius"
        );
        ensure!(
            !self.bloom_enabled || self.bloom_strength == 0. || self.bloom_radius > 0.,
            "enabled bloom with nonzero strength needs a positive radius"
        );
        Ok(())
    }
}

#[cfg(test)]
mod post_tests {
    use super::*;

    #[test]
    fn older_looks_keep_post_disabled_and_invalid_inputs_fail() {
        let mut look: LookSettings = serde_json::from_str("{}").unwrap();
        assert!(!look.ao_active() && !look.bloom_active());
        look.validate().unwrap();
        look.ao_enabled = true;
        look.ao_strength = 1.;
        assert!(look.validate().is_err());
        look.ao_radius = 1.;
        look.validate().unwrap();
        assert!(look.ao_active());
        look.ao_bias = f32::NAN;
        assert!(look.validate().is_err());
        look.ao_bias = 0.;
        look.ao_strength = 1.01;
        assert!(look.validate().is_err());
        look.ao_strength = 0.;
        look.bloom_enabled = true;
        look.bloom_strength = 1.;
        assert!(look.validate().is_err());
        look.bloom_radius = 1.;
        look.validate().unwrap();
        assert!(look.bloom_active());
        look.bloom_threshold = -1.;
        assert!(look.validate().is_err());
        look.bloom_threshold = 0.;
        assert_eq!(look.bloom_knee, 0.5);
        for knee in [0., 0.5, 1.] {
            look.bloom_knee = knee;
            look.validate().unwrap();
        }
        for knee in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            look.bloom_knee = knee;
            assert!(look.validate().is_err());
        }
        look.bloom_knee = 0.5;
        look.bloom_strength = f32::INFINITY;
        assert!(look.validate().is_err());
    }
}
