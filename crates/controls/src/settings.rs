//! The player's control settings, read from a JSON file. Every field is optional; a missing
//! file means the defaults.
use crate::{Bindings, Orbit, Pilot, Scheme, camera};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// How a ground-targeted skill is cast from its key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroundCast {
    /// Lands at the crosshair (or pointer) the moment the key goes down.
    #[default]
    Instant,
    /// Holding the key shows where it will land; letting go casts it, and Esc or the right
    /// mouse button cancels.
    HoldToAim,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub scheme: Scheme,
    /// Multiplies mouse look speed, 0.1..5.
    pub sensitivity: f32,
    pub invert_y: bool,
    /// Vertical field of view, 30..100 degrees.
    pub fov_degrees: f32,
    /// How close and how far the wheel zooms, in metres.
    pub min_distance: f32,
    pub max_distance: f32,
    /// Over-the-shoulder offset in metres; unset uses the scheme's own (0.6 m in Action,
    /// centred in Classic).
    pub shoulder: Option<f32>,
    /// Classic: the camera swings back behind the character after a second of moving.
    pub swing_back: bool,
    /// How hard landed blows kick the camera.
    pub screen_shake: camera::ScreenShake,
    /// What pressing a ground skill's key does.
    pub ground_cast: GroundCast,
    /// Keys and mouse buttons; controls left out keep their defaults.
    pub bindings: Bindings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scheme: Scheme::default(),
            sensitivity: 1.,
            invert_y: false,
            fov_degrees: camera::FOV_DEGREES,
            min_distance: camera::MIN_DISTANCE,
            max_distance: camera::MAX_DISTANCE,
            shoulder: None,
            swing_back: true,
            screen_shake: camera::ScreenShake::Full,
            ground_cast: GroundCast::Instant,
            bindings: Bindings::default(),
        }
    }
}

impl Settings {
    /// Reads `path`, or the defaults when it doesn't exist. Out-of-range values are clamped.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let s: Self =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(s.clamped())
    }

    /// Writes the settings to `path`, making its folder if needed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }

    fn clamped(mut self) -> Self {
        self.sensitivity = self.sensitivity.clamp(0.1, 5.);
        self.fov_degrees = self.fov_degrees.clamp(30., 100.);
        self.min_distance = self.min_distance.clamp(1., 40.);
        self.max_distance = self.max_distance.clamp(self.min_distance, 40.);
        self.shoulder = self.shoulder.map(|s| s.clamp(-1.5, 1.5));
        self
    }

    pub fn shoulder(&self) -> f32 {
        self.shoulder.unwrap_or(match self.scheme {
            Scheme::Action => 0.6,
            Scheme::Classic => 0.,
        })
    }

    pub fn orbit(&self) -> Orbit {
        let o = Orbit::default();
        Orbit {
            shoulder: self.shoulder(),
            sensitivity: self.sensitivity,
            invert_y: self.invert_y,
            min_distance: self.min_distance,
            max_distance: self.max_distance,
            distance: o.distance.clamp(self.min_distance, self.max_distance),
            ..o
        }
    }

    pub fn pilot(&self, heading: f32) -> Pilot {
        Pilot {
            swing_back: self.swing_back,
            ..Pilot::new(self.scheme, heading)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_the_other_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{ "scheme": "classic", "invert_y": true }"#).unwrap();
        assert_eq!(s.scheme, Scheme::Classic);
        assert!(s.invert_y);
        assert_eq!(s.fov_degrees, camera::FOV_DEGREES);
        assert_eq!(s.shoulder(), 0.);
        assert_eq!(Settings::default().shoulder(), 0.6);
        assert_eq!(s.screen_shake, camera::ScreenShake::Full);
        let s: Settings = serde_json::from_str(r#"{ "screen_shake": "low" }"#).unwrap();
        assert_eq!(s.screen_shake, camera::ScreenShake::Low);
    }

    #[test]
    fn typos_are_errors_and_ranges_clamp() {
        assert!(serde_json::from_str::<Settings>(r#"{ "sensitivty": 2 }"#).is_err());
        let s = Settings {
            sensitivity: 50.,
            min_distance: 10.,
            max_distance: 3.,
            ..Default::default()
        }
        .clamped();
        assert_eq!(s.sensitivity, 5.);
        assert_eq!(s.max_distance, 10.);
        assert_eq!(s.orbit().distance, 10.);
    }

    #[test]
    fn saves_and_loads_back() {
        let dir = std::env::temp_dir().join(format!("aldermoor-settings-{}", std::process::id()));
        let path = dir.join("nested/controls.json");
        let mut s = Settings {
            scheme: Scheme::Classic,
            invert_y: true,
            ..Default::default()
        };
        s.bindings
            .bind(crate::Control::Jump, crate::Input::new("KeyE"));
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_the_defaults() {
        let s = Settings::load(Path::new("/nonexistent/aldermoor-controls.json")).unwrap();
        assert_eq!(s, Settings::default());
    }
}
