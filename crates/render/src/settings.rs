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

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Returns a new scale at most once per second. Frame IDs reject stale or repeated
    /// completion records; elapsed seconds make the response independent of frame rate.
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
        let filtered = match (self.filtered_ms, self.last_seconds) {
            (Some(previous), Some(last)) => {
                // A quarter-second time constant, regardless of submission/frame rate.
                let alpha = 1. - (-(seconds - last) / 0.25).exp();
                previous + alpha * (gpu_ms - previous)
            }
            _ => {
                self.next_change_seconds = seconds + 1.;
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
            controller.observe(frame, frame as f64 / 60., 20.);
        }
        assert_eq!(controller.scale(), DynamicResolution::MIN_SCALE);
        for frame in 600..1200 {
            assert!(controller.observe(frame, frame as f64 / 60., 11.).is_none());
        }
        // A short quiet patch cannot immediately raise resolution.
        for frame in 1200..1320 {
            assert!(controller.observe(frame, frame as f64 / 60., 8.).is_none());
        }
        for frame in 1320..3000 {
            controller.observe(frame, frame as f64 / 60., 8.);
        }
        assert_eq!(controller.scale(), DynamicResolution::MAX_SCALE);
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
}
