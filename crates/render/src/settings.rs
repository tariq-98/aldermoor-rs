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
    pub height_fog: HeightFogSettings,
    pub exposure: f32,
    pub grade: [[f32; 3]; 3],
    pub tone_map: ToneMap,
    pub wind_strength: f32,
}
/// Optional height profile for the existing fog colour and density. Values come from the look.
/// Disabled preserves homogeneous scene fog and leaves the sky unchanged.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HeightFogSettings {
    pub enabled: bool,
    /// World Y in metres. Density stays constant below this height.
    pub base_height: f32,
    /// Exponential decay above the base, in inverse metres. Zero is homogeneous.
    pub height_falloff: f32,
    /// Integration distance for the sky, in metres. Zero leaves the sky unchanged.
    pub sky_distance: f32,
}
impl HeightFogSettings {
    /// Frame packing contract: grade[0..3].w, then params.w. RGB/XYZ stay unchanged.
    pub fn uniform(self) -> [f32; 4] {
        [
            self.base_height,
            self.height_falloff,
            self.sky_distance,
            u32::from(self.enabled) as f32,
        ]
    }

    fn validate(self) -> Result<()> {
        ensure!(
            [self.base_height, self.height_falloff, self.sky_distance]
                .into_iter()
                .all(f32::is_finite),
            "height fog settings must be finite"
        );
        ensure!(
            self.height_falloff >= 0. && self.sky_distance >= 0.,
            "height fog falloff and sky distance must be nonnegative"
        );
        Ok(())
    }
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
            height_fog: HeightFogSettings::default(),
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
        self.height_fog.validate()?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn old_looks_keep_height_fog_disabled() {
        let look: LookSettings = serde_json::from_value(json!({"fog_density": 0.02})).unwrap();
        look.validate().unwrap();
        assert_eq!(look.height_fog, HeightFogSettings::default());
        assert_eq!(look.height_fog.uniform(), [0.; 4]);
        assert_eq!(look.fog_density, 0.02);
    }

    #[test]
    fn height_fog_round_trips_and_packs_authored_values() {
        let look: LookSettings = serde_json::from_value(json!({"height_fog": {
            "enabled": true, "base_height": -12., "height_falloff": 0.25, "sky_distance": 800.
        }}))
        .unwrap();
        look.validate().unwrap();
        assert_eq!(look.height_fog.uniform(), [-12., 0.25, 800., 1.]);
        let copy: LookSettings =
            serde_json::from_value(serde_json::to_value(&look).unwrap()).unwrap();
        assert_eq!(copy.height_fog, look.height_fog);
        let neutral: LookSettings =
            serde_json::from_value(json!({"height_fog": {"enabled": true}})).unwrap();
        neutral.validate().unwrap();
        assert_eq!(neutral.height_fog.uniform(), [0., 0., 0., 1.]);
        assert_eq!(neutral.fog_density, 0.);
    }

    #[test]
    fn invalid_height_fog_is_rejected_even_when_disabled() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for field in 0..3 {
                let mut look = LookSettings::default();
                match field {
                    0 => look.height_fog.base_height = bad,
                    1 => look.height_fog.height_falloff = bad,
                    _ => look.height_fog.sky_distance = bad,
                }
                assert!(look.validate().is_err());
            }
        }
        for value in [
            json!({"height_falloff": -0.01}),
            json!({"sky_distance": -1.}),
        ] {
            let look: LookSettings = serde_json::from_value(json!({"height_fog": value})).unwrap();
            assert!(look.validate().is_err());
        }
        assert!(
            serde_json::from_value::<LookSettings>(json!({"height_fog": {"falloff": 1.}})).is_err()
        );
    }
}
