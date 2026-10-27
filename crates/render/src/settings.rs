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
    /// Screen-space obscurance; radius and bias are world metres. Disabled by default.
    pub ao_enabled: bool,
    pub ao_radius: f32,
    pub ao_bias: f32,
    pub ao_strength: f32,
    /// Linear-HDR bloom, disabled by default.
    pub bloom_enabled: bool,
    pub bloom_threshold: f32,
    /// Soft-knee width as a fraction of threshold; zero gives a hard threshold.
    pub bloom_knee: f32,
    /// Wider-level weight, 0..1: zero keeps the tight half-resolution core; one weights all levels equally.
    pub bloom_radius: f32,
    pub bloom_strength: f32,
    /// Optional art-authored cycle, evaluated against simulation seconds. None preserves
    /// the static look; omitted keyframe channels inherit that look without new art values.
    pub time_of_day: Option<TimeOfDay>,
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
            ao_enabled: false,
            ao_radius: 0.,
            ao_bias: 0.,
            ao_strength: 0.,
            bloom_enabled: false,
            bloom_threshold: 0.,
            bloom_knee: 0.5,
            bloom_radius: 0.,
            bloom_strength: 0.,
            time_of_day: None,
        }
    }
}
impl LookSettings {
    pub fn ao_active(&self) -> bool {
        self.ao_enabled && self.ao_strength > 0. && self.ao_radius > 0.
    }
    pub fn bloom_active(&self) -> bool {
        self.bloom_enabled && self.bloom_strength > 0.
    }
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
                && (0. ..=1.).contains(&self.bloom_radius)
                && self.bloom_strength >= 0.,
            "invalid AO/bloom settings"
        );
        ensure!(
            !self.ao_enabled || self.ao_strength == 0. || self.ao_radius > 0.,
            "enabled AO with nonzero strength needs a positive radius"
        );
        if let Some(cycle) = &self.time_of_day {
            cycle.validate(self)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod height_fog_tests {
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
        look.validate().unwrap();
        assert!(look.bloom_active(), "zero radius retains the tight core");
        for radius in [0., 0.5, 1.] {
            look.bloom_radius = radius;
            look.validate().unwrap();
        }
        for radius in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            look.bloom_radius = radius;
            assert!(look.validate().is_err());
        }
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

/// Backend-neutral dynamic resolution, driven only by completed GPU measurements.
/// The client bypasses this controller when an explicit render scale is supplied.
#[derive(Debug)]
pub struct DynamicResolution {
    scale: f32,
    filtered_ms: Option<f64>,
    last_frame: Option<usize>,
    last_seconds: Option<f64>,
    next_change_seconds: f64,
    headroom_since: Option<f64>,
}
impl Default for DynamicResolution {
    fn default() -> Self {
        Self {
            scale: Self::MAX_SCALE,
            filtered_ms: None,
            last_frame: None,
            last_seconds: None,
            next_change_seconds: 0.,
            headroom_since: None,
        }
    }
}
impl DynamicResolution {
    pub const MIN_SCALE: f32 = 0.55;
    pub const MAX_SCALE: f32 = 0.75;
    /// Frames ignored after a resize: the first frames at a new size can be slow once.
    const SETTLE_FRAMES: usize = 2;
    /// Twice the 12 ms budget. A single hitch is capped so it cannot step the scale down
    /// a second later; sustained overload still reads well above the budget.
    const SPIKE_MS: f64 = 24.;

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Returns a new scale at most once per second. Frame IDs reject stale or repeated
    /// completion records; current wall-clock seconds (not a submission timestamp) make
    /// the response independent of frame rate. Call `resize_completed` after applying a change.
    /// Invalid timings do not consume a frame ID or advance the controller's clock.
    pub fn observe(&mut self, frame: usize, seconds: f64, gpu_ms: f64) -> Option<f32> {
        if !seconds.is_finite()
            || seconds < 0.
            || !gpu_ms.is_finite()
            || gpu_ms <= 0.
            || self.last_frame.is_some_and(|last| frame <= last)
            || self.last_seconds.is_some_and(|last| seconds <= last)
        {
            return None;
        }
        let gpu_ms = gpu_ms.min(Self::SPIKE_MS);
        let filtered = match (self.filtered_ms, self.last_seconds) {
            (Some(previous), Some(last)) => {
                // A quarter-second time constant, regardless of submission/frame rate.
                let alpha = 1. - (-(seconds - last) / 0.25).exp();
                previous + alpha * (gpu_ms - previous)
            }
            _ => {
                if self.last_seconds.is_none() {
                    self.next_change_seconds = seconds + 1.;
                }
                gpu_ms
            }
        };
        self.filtered_ms = Some(filtered);
        self.last_frame = Some(frame);
        self.last_seconds = Some(seconds);
        if filtered < 10. {
            self.headroom_since.get_or_insert(seconds);
        } else {
            self.headroom_since = None;
        }
        if seconds < self.next_change_seconds {
            return None;
        }
        self.next_change_seconds = seconds + 1.;
        let next = if filtered > 12. {
            (((self.scale * 40.).round() - 1.) / 40.).max(Self::MIN_SCALE)
        } else if self
            .headroom_since
            .is_some_and(|start| seconds - start >= 3.)
        {
            (((self.scale * 40.).round() + 1.) / 40.).min(Self::MAX_SCALE)
        } else {
            self.scale
        };
        if (next - self.scale).abs() < 1e-6 {
            return None;
        }
        self.scale = next;
        Some(next)
    }

    /// Start the cooldown after allocation/draining finishes, and discard measurements
    /// from submissions made at the old scale and the first frames at the new one.
    /// A backend only supplies the last frame it submitted at the old scale and a clock.
    pub fn resize_completed(&mut self, last_completed_frame: usize, seconds: f64) {
        if !seconds.is_finite()
            || seconds < 0.
            || self.last_seconds.is_some_and(|last| seconds < last)
        {
            return;
        }
        let settled = last_completed_frame + Self::SETTLE_FRAMES;
        self.last_frame = Some(self.last_frame.map_or(settled, |last| last.max(settled)));
        self.last_seconds = Some(seconds);
        self.filtered_ms = None;
        self.headroom_since = None;
        self.next_change_seconds = seconds + 1.;
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::DynamicResolution;

    #[test]
    fn sustained_overload_is_bounded_and_changes_at_most_once_a_second() {
        for fps in [20, 60, 240] {
            let mut controller = DynamicResolution::default();
            let mut last_change = 0.;
            let mut changes = 0;
            for frame in 0..fps * 12 {
                let seconds = frame as f64 / fps as f64;
                if let Some(scale) = controller.observe(frame, seconds, 20.) {
                    assert!(seconds - last_change >= 1. - 1e-9);
                    assert!((0.55..=0.75).contains(&scale));
                    last_change = seconds;
                    changes += 1;
                    controller.resize_completed(frame, seconds);
                }
            }
            assert_eq!(changes, 8);
            assert_eq!(controller.scale(), DynamicResolution::MIN_SCALE);
        }
    }

    #[test]
    fn recovery_requires_sustained_headroom_and_does_not_hunt_in_the_dead_band() {
        let mut controller = DynamicResolution::default();
        for frame in 0..600 {
            if controller.observe(frame, frame as f64 / 60., 20.).is_some() {
                controller.resize_completed(frame, frame as f64 / 60.);
            }
        }
        assert_eq!(controller.scale(), DynamicResolution::MIN_SCALE);
        for frame in 600..1200 {
            assert!(controller.observe(frame, frame as f64 / 60., 11.).is_none());
        }
        // A short quiet patch cannot immediately raise resolution.
        for frame in 1200..1320 {
            assert!(controller.observe(frame, frame as f64 / 60., 8.).is_none());
        }
        // Allow fresh headroom after each of the eight recovery steps.
        for frame in 1320..3600 {
            if controller.observe(frame, frame as f64 / 60., 8.).is_some() {
                controller.resize_completed(frame, frame as f64 / 60.);
            }
        }
        assert_eq!(controller.scale(), DynamicResolution::MAX_SCALE);
    }

    #[test]
    fn slow_resize_restarts_cooldown_and_discards_old_scale_completions() {
        let mut controller = DynamicResolution::default();
        assert_eq!(controller.observe(1, 0., 20.), None);
        assert!(controller.observe(2, 1., 20.).is_some());
        // Draining/allocation took five seconds and completed three older submissions.
        controller.resize_completed(5, 6.);
        assert_eq!(controller.observe(3, 6.1, 100.), None);
        assert_eq!(controller.observe(5, 6.2, 100.), None);
        assert_eq!(controller.observe(6, 6.3, 20.), None);
        assert_eq!(controller.observe(7, 6.99, 20.), None);
        assert!((controller.observe(8, 7., 20.).unwrap() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn invalid_duplicate_and_out_of_order_measurements_do_not_steer() {
        let mut controller = DynamicResolution::default();
        assert_eq!(controller.observe(1, 1., 20.), None);
        for (frame, seconds, gpu) in [
            (1, 3., 200.),
            (0, 3., 200.),
            (2, 0.5, 200.),
            (2, 1., 200.),
            (2, f64::NAN, 200.),
            (2, 2., f64::INFINITY),
            (2, 2., 0.),
            (2, 2., -1.),
        ] {
            assert_eq!(controller.observe(frame, seconds, gpu), None);
        }
        assert_eq!(controller.scale(), DynamicResolution::MAX_SCALE);
        assert!((controller.observe(2, 2., 20.).unwrap() - 0.725).abs() < 1e-6);
        assert_eq!(controller.observe(3, 2.5, 20.), None);
    }

    #[test]
    fn a_single_hitch_or_slow_first_frames_do_not_step_the_scale() {
        let mut controller = DynamicResolution::default();
        let mut scales = Vec::new();
        for frame in 0..600 {
            let seconds = frame as f64 / 60.;
            // A 400 ms hitch at 1.5 s, and a step to 0.725 at 4 s whose first frames are slow.
            let gpu = if matches!(frame, 90 | 241 | 242) {
                400.
            } else {
                11.
            };
            if frame == 240 {
                controller.scale = 0.725;
                controller.resize_completed(240, seconds);
                continue;
            }
            if let Some(scale) = controller.observe(frame, seconds, gpu) {
                scales.push(scale);
            }
        }
        assert!(scales.is_empty(), "hitches stepped the scale: {scales:?}");
        assert_eq!(controller.scale(), 0.725);
        // Sustained overload still steps down.
        for frame in 600..700 {
            if let Some(scale) = controller.observe(frame, frame as f64 / 60., 30.) {
                scales.push(scale);
            }
        }
        assert_eq!(scales, [0.7]);
    }
}
