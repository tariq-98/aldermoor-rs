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
    /// Optional art-authored cycle, evaluated against simulation seconds. None preserves
    /// the static look; omitted keyframe channels inherit that look without new art values.
    pub time_of_day: Option<TimeOfDay>,
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
            time_of_day: None,
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
        if let Some(cycle) = &self.time_of_day {
            cycle.validate(self)?;
        }
        Ok(())
    }
}

/// Phase is a fraction of a day in [0, 1). Duration and starting phase are authored,
/// not engine look defaults. An empty list is a static placeholder.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeOfDay {
    pub duration_seconds: f64,
    #[serde(default)]
    pub offset: f64,
    pub keyframes: Vec<TimeOfDayKeyframe>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeOfDayKeyframe {
    pub phase: f64,
    /// Direction towards the light: azimuth 0 = +X, 90 = +Z; elevation 90 = up.
    /// Azimuth follows the shortest arc between keys, including across 360 degrees.
    pub sun_azimuth_degrees: Option<f32>,
    pub sun_elevation_degrees: Option<f32>,
    pub sun_colour: Option<[f32; 3]>,
    pub sun_intensity: Option<f32>,
    pub ambient: Option<[f32; 3]>,
    /// Fallback environment colour. Authored sky textures retain their own colours.
    pub sky: Option<[f32; 3]>,
    /// Multiplier for the existing sky texture, its prefiltered IBL and water reflection.
    /// Omission means identity (1), including for legacy static looks.
    pub sky_texture_intensity: Option<f32>,
    pub fog_colour: Option<[f32; 3]>,
    pub fog_density: Option<f32>,
    pub exposure: Option<f32>,
}

/// Only the evaluated lighting channels: no keyframe allocation or clone per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeOfDayLighting {
    pub sun_direction: [f32; 3],
    pub sun_colour: [f32; 3],
    pub sun_intensity: f32,
    pub ambient: [f32; 3],
    pub sky: [f32; 3],
    pub sky_texture_intensity: f32,
    pub fog_colour: [f32; 3],
    pub fog_density: f32,
    pub exposure: f32,
}

impl LookSettings {
    /// Call validate after loading/changing the look, before sampling. The shipping client
    /// supplies Snapshot.time via scene_time; fixed scene times also freeze this cycle.
    pub fn lighting_at(&self, seconds: f64) -> TimeOfDayLighting {
        let base = TimeOfDayLighting {
            sun_direction: self.sun_direction,
            sun_colour: self.sun_colour,
            sun_intensity: self.sun_intensity,
            ambient: self.ambient,
            sky: self.sky,
            sky_texture_intensity: 1.,
            fog_colour: self.fog_colour,
            fog_density: self.fog_density,
            exposure: self.exposure,
        };
        let Some(cycle) = &self.time_of_day else {
            return base;
        };
        if cycle.keyframes.is_empty() {
            return base;
        }
        // Reduce seconds before division to keep the phase bounded on long sessions.
        let phase = if seconds.is_finite() {
            (seconds.rem_euclid(cycle.duration_seconds) / cycle.duration_seconds + cycle.offset)
                .rem_euclid(1.)
        } else {
            cycle.offset
        };
        let next = cycle.keyframes.partition_point(|key| key.phase <= phase);
        let a = &cycle.keyframes[(next + cycle.keyframes.len() - 1) % cycle.keyframes.len()];
        let b = &cycle.keyframes[next % cycle.keyframes.len()];
        let span = if a.phase < b.phase {
            b.phase - a.phase
        } else {
            b.phase + 1. - a.phase
        };
        let t = ((phase - a.phase).rem_euclid(1.) / span) as f32;
        let (a_light, a_angles) = a.resolve(base);
        let (b_light, b_angles) = b.resolve(base);
        // Keep the sun moving through keys; easing angles would stop it at every key.
        let azimuth =
            a_angles[0] + ((b_angles[0] - a_angles[0] + 180.).rem_euclid(360.) - 180.) * t;
        let elevation = lerp(a_angles[1], b_angles[1], t);
        // Ease the lighting channels without overshooting authored values.
        let t = t * t * (3. - 2. * t);
        TimeOfDayLighting {
            sun_direction: sun_direction(azimuth, elevation),
            sun_colour: lerp3(a_light.sun_colour, b_light.sun_colour, t),
            sun_intensity: lerp(a_light.sun_intensity, b_light.sun_intensity, t),
            ambient: lerp3(a_light.ambient, b_light.ambient, t),
            sky: lerp3(a_light.sky, b_light.sky, t),
            sky_texture_intensity: lerp(
                a_light.sky_texture_intensity,
                b_light.sky_texture_intensity,
                t,
            ),
            fog_colour: lerp3(a_light.fog_colour, b_light.fog_colour, t),
            fog_density: lerp(a_light.fog_density, b_light.fog_density, t),
            exposure: lerp(a_light.exposure, b_light.exposure, t),
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a * (1. - t) + b * t
}
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| lerp(a[i], b[i], t))
}
fn sun_direction(azimuth: f32, elevation: f32) -> [f32; 3] {
    let (sa, ca) = azimuth.to_radians().sin_cos();
    let (se, ce) = elevation.to_radians().sin_cos();
    [ca * ce, se, sa * ce]
}

impl TimeOfDayKeyframe {
    fn resolve(&self, base: TimeOfDayLighting) -> (TimeOfDayLighting, [f32; 2]) {
        let direction = glam::Vec3::from_array(base.sun_direction).normalize();
        let angles = [
            self.sun_azimuth_degrees
                .unwrap_or_else(|| direction.z.atan2(direction.x).to_degrees().rem_euclid(360.)),
            self.sun_elevation_degrees
                .unwrap_or_else(|| direction.y.clamp(-1., 1.).asin().to_degrees()),
        ];
        (
            TimeOfDayLighting {
                sun_direction: sun_direction(angles[0], angles[1]),
                sun_colour: self.sun_colour.unwrap_or(base.sun_colour),
                sun_intensity: self.sun_intensity.unwrap_or(base.sun_intensity),
                ambient: self.ambient.unwrap_or(base.ambient),
                sky: self.sky.unwrap_or(base.sky),
                sky_texture_intensity: self
                    .sky_texture_intensity
                    .unwrap_or(base.sky_texture_intensity),
                fog_colour: self.fog_colour.unwrap_or(base.fog_colour),
                fog_density: self.fog_density.unwrap_or(base.fog_density),
                exposure: self.exposure.unwrap_or(base.exposure),
            },
            angles,
        )
    }
}

impl TimeOfDay {
    fn validate(&self, look: &LookSettings) -> Result<()> {
        ensure!(
            self.duration_seconds.is_finite() && self.duration_seconds > 0.,
            "day duration must be finite and positive"
        );
        ensure!(
            self.offset.is_finite() && (0. ..1.).contains(&self.offset),
            "day offset must be in [0, 1)"
        );
        ensure!(
            self.keyframes.len() <= 64,
            "at most 64 time-of-day keys are supported"
        );
        let mut previous = -1.;
        for key in &self.keyframes {
            ensure!(
                key.phase.is_finite() && (0. ..1.).contains(&key.phase) && key.phase > previous,
                "day keys must be strictly increasing phases in [0, 1)"
            );
            previous = key.phase;
            ensure!(
                key.sun_azimuth_degrees
                    .is_none_or(|a| a.is_finite() && (0. ..360.).contains(&a)),
                "sun azimuth must be in [0, 360)"
            );
            ensure!(
                key.sun_elevation_degrees
                    .is_none_or(|a| a.is_finite() && (-90. ..=90.).contains(&a)),
                "sun elevation must be in [-90, 90]"
            );
            for colour in [key.sun_colour, key.ambient, key.sky, key.fog_colour]
                .into_iter()
                .flatten()
            {
                ensure!(
                    colour.into_iter().all(|v| v.is_finite() && v >= 0.),
                    "day colours must be finite and nonnegative"
                );
            }
            for value in [
                key.sun_intensity,
                key.sky_texture_intensity,
                key.fog_density,
            ]
            .into_iter()
            .flatten()
            {
                ensure!(
                    value.is_finite() && value >= 0.,
                    "day intensity/density must be finite and nonnegative"
                );
            }
            ensure!(
                key.exposure.is_none_or(|v| v.is_finite() && v > 0.),
                "day exposure must be finite and positive"
            );
        }
        // The inherited sun must also be normalisable (finite components alone can overflow).
        let length = glam::Vec3::from_array(look.sun_direction).length_squared();
        ensure!(
            length.is_finite() && length > 1e-8,
            "day cycle requires a normalisable base sun"
        );
        Ok(())
    }
}

#[cfg(test)]
mod time_of_day_tests {
    use super::*;
    use serde_json::json;

    fn look(keys: serde_json::Value) -> LookSettings {
        serde_json::from_value(json!({"time_of_day": {
            "duration_seconds": 100., "keyframes": keys
        }}))
        .unwrap()
    }
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }

    #[test]
    fn legacy_and_empty_cycle_preserve_static_channels() {
        let original = LookSettings::default().lighting_at(0.);
        let empty = look(json!([]));
        empty.validate().unwrap();
        for seconds in [-500., 0., 25., 100., 1e12] {
            assert_eq!(empty.lighting_at(seconds), original);
            assert_eq!(LookSettings::default().lighting_at(seconds), original);
        }
        let placeholders =
            look(json!([{"phase": 0.}, {"phase": 0.25}, {"phase": 0.5}, {"phase": 0.75}]));
        placeholders.validate().unwrap();
        let sampled = placeholders.lighting_at(31.);
        near(sampled.sun_direction[1], 1.);
        assert_eq!(sampled.ambient, original.ambient);
        assert_eq!(sampled.sky, original.sky);
        assert_eq!(sampled.sun_colour, original.sun_colour);
        assert_eq!(sampled.exposure, original.exposure);
        assert_eq!(sampled.sky_texture_intensity, 1.);
    }

    #[test]
    fn blends_every_lighting_channel_and_uses_shortest_sun_arc() {
        let s = look(json!([
            {"phase": 0.25, "sun_azimuth_degrees": 350., "sun_elevation_degrees": -30.,
             "sun_colour": [0., 2., 4.], "sun_intensity": 0., "ambient": [0., 0., 0.],
             "sky": [0., 0., 0.], "sky_texture_intensity": 0., "fog_colour": [0., 0., 0.],
             "fog_density": 0., "exposure": 1.},
            {"phase": 0.75, "sun_azimuth_degrees": 10., "sun_elevation_degrees": 30.,
             "sun_colour": [2., 4., 6.], "sun_intensity": 2., "ambient": [2., 4., 6.],
             "sky": [4., 6., 8.], "sky_texture_intensity": 2., "fog_colour": [6., 8., 10.],
             "fog_density": 2., "exposure": 3.}
        ]));
        s.validate().unwrap();
        let middle = s.lighting_at(50.);
        near(middle.sun_direction[0], 1.);
        near(middle.sun_direction[1], 0.);
        near(middle.sun_direction[2], 0.);
        assert_eq!(middle.sun_colour, [1., 3., 5.]);
        assert_eq!(middle.sun_intensity, 1.);
        assert_eq!(middle.ambient, [1., 2., 3.]);
        assert_eq!(middle.sky, [2., 3., 4.]);
        assert_eq!(middle.sky_texture_intensity, 1.);
        assert_eq!(middle.fog_colour, [3., 4., 5.]);
        assert_eq!(middle.fog_density, 1.);
        assert_eq!(middle.exposure, 2.);
        assert_eq!(s.lighting_at(25.).sun_intensity, 0.);
        assert_eq!(s.lighting_at(75.).sun_intensity, 2.);
        let quarter = s.lighting_at(37.5);
        let expected = sun_direction(355., -15.);
        for (actual, expected) in quarter.sun_direction.into_iter().zip(expected) {
            near(actual, expected);
        }
        near(quarter.sun_intensity, 0.3125); // Colours/intensities still ease.
    }

    #[test]
    fn midnight_negative_time_offset_and_large_time_are_periodic() {
        let mut s = look(json!([
            {"phase": 0.25, "sun_intensity": 0.},
            {"phase": 0.75, "sun_intensity": 2.}
        ]));
        s.validate().unwrap();
        assert_eq!(s.lighting_at(0.).sun_intensity, 1.);
        for time in [-1e12, -100., 100., 1e12] {
            assert_eq!(s.lighting_at(time), s.lighting_at(0.));
        }
        near(
            s.lighting_at(-0.00001).sun_intensity,
            s.lighting_at(0.00001).sun_intensity,
        );
        s.time_of_day.as_mut().unwrap().offset = 0.25;
        assert_eq!(s.lighting_at(0.).sun_intensity, 0.);
        assert_eq!(s.lighting_at(f64::NAN), s.lighting_at(0.));
    }

    #[test]
    fn opposite_and_vertical_sun_keys_never_produce_a_zero_direction() {
        let s = look(json!([
            {"phase": 0., "sun_azimuth_degrees": 0., "sun_elevation_degrees": 90.},
            {"phase": 0.5, "sun_azimuth_degrees": 180., "sun_elevation_degrees": -90.}
        ]));
        s.validate().unwrap();
        for step in 0..=1000 {
            let direction =
                glam::Vec3::from_array(s.lighting_at(f64::from(step) / 10.).sun_direction);
            assert!(direction.is_finite());
            near(direction.length(), 1.);
        }
    }

    #[test]
    fn one_key_is_constant_and_unknown_fields_fail() {
        let s = look(json!([{"phase": 0.6, "sun_intensity": 3.}]));
        s.validate().unwrap();
        for time in [-25., 0., 60., 75., 300.] {
            near(s.lighting_at(time).sun_intensity, 3.);
        }
        assert!(
            serde_json::from_value::<TimeOfDay>(
                json!({"duration_seconds": 100., "keyframes": [], "duraton": 1})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<TimeOfDayKeyframe>(json!({"phase": 0., "sun_intensitiy": 1}))
                .is_err()
        );
    }

    #[test]
    fn invalid_cycles_fail_before_rendering() {
        for keys in [
            json!([{"phase": 0.5}, {"phase": 0.5}]),
            json!([{"phase": 0.5}, {"phase": 0.25}]),
            json!([{"phase": 1.}]),
            json!([{"phase": -0.1}]),
            json!([{"phase": 0., "sun_elevation_degrees": 91.}]),
            json!([{"phase": 0., "sun_azimuth_degrees": 360.}]),
            json!([{"phase": 0., "sun_intensity": -1.}]),
            json!([{"phase": 0., "ambient": [-1., 0., 0.]}]),
            json!([{"phase": 0., "sky_texture_intensity": -1.}]),
            json!([{"phase": 0., "fog_density": -1.}]),
            json!([{"phase": 0., "exposure": 0.}]),
        ] {
            assert!(look(keys).validate().is_err());
        }
        for duration in [0., -1., f64::INFINITY, f64::NAN] {
            let mut s = look(json!([]));
            s.time_of_day.as_mut().unwrap().duration_seconds = duration;
            assert!(s.validate().is_err());
        }
        for offset in [-0.1, 1., f64::NAN] {
            let mut s = look(json!([]));
            s.time_of_day.as_mut().unwrap().offset = offset;
            assert!(s.validate().is_err());
        }
        let mut s = look(json!([{"phase": 0.}]));
        s.time_of_day.as_mut().unwrap().keyframes[0].sun_colour = Some([f32::NAN, 0., 0.]);
        assert!(s.validate().is_err());
    }
}
