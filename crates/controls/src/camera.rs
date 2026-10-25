//! A camera orbiting the player's chest, kept above the ground and out of buildings.
use glam::Vec3;

/// Where the camera looks: this far above the player's feet.
pub const PIVOT_HEIGHT: f32 = 1.25;
pub const FOV_DEGREES: f32 = 52.;
/// How close the eye may come to the ground.
const GROUND_CLEARANCE: f32 = 0.35;
const MIN_DISTANCE: f32 = 2.;
const MAX_DISTANCE: f32 = 24.;
/// Metres per wheel notch.
const ZOOM_STEP: f32 = 0.6;
/// Radians per pixel of mouse travel.
const YAW_RATE: f32 = 0.004;
const PITCH_RATE: f32 = 0.003;
const MIN_PITCH: f32 = -0.1;
const MAX_PITCH: f32 = 1.1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    /// The way the camera faces on the ground (0 faces +z); the eye sits behind it.
    pub yaw: f32,
    /// Above the horizon, looking down at the player.
    pub pitch: f32,
    pub distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: std::f32::consts::PI,
            pitch: 0.23,
            distance: 7.,
        }
    }
}

impl Orbit {
    /// Turns by a mouse movement in pixels.
    pub fn look(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx as f32 * YAW_RATE;
        self.pitch = (self.pitch + dy as f32 * PITCH_RATE).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Zooms by wheel notches; positive comes closer.
    pub fn zoom(&mut self, notches: f32) {
        self.distance = (self.distance - notches * ZOOM_STEP).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// The point the camera looks at for a player standing at `feet`.
    pub fn target(feet: Vec3) -> Vec3 {
        feet + Vec3::Y * PIVOT_HEIGHT
    }

    /// From the target out to where the eye would be with nothing in the way.
    pub fn offset(&self) -> Vec3 {
        Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        ) * self.distance
    }

    /// The eye looking at `target`, pulled in front of the terrain and then of whatever
    /// `blocked(target, eye)` says is in the way (the scenery's collision boxes).
    pub fn eye(&self, target: Vec3, blocked: impl Fn(Vec3, Vec3) -> Vec3) -> Vec3 {
        let offset = self.offset();
        let mut eye = target + offset;
        for i in 1..=40 {
            let at = target + offset * (i as f32 / 40.);
            if at.y < world::height(at.x, at.z) + GROUND_CLEARANCE {
                eye = target + offset * ((i - 1) as f32 / 40.);
                break;
            }
        }
        eye = blocked(target, eye);
        eye.y = eye.y.max(world::height(eye.x, eye.z) + GROUND_CLEARANCE);
        eye
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standing_at(x: f32, z: f32) -> Vec3 {
        Orbit::target(Vec3::new(x, world::height(x, z), z))
    }

    #[test]
    fn pitch_and_zoom_stay_in_range() {
        let mut o = Orbit::default();
        o.look(0., 1e6);
        assert_eq!(o.pitch, MAX_PITCH);
        o.look(0., -1e6);
        assert_eq!(o.pitch, MIN_PITCH);
        o.zoom(100.);
        assert_eq!(o.distance, MIN_DISTANCE);
        o.zoom(-100.);
        assert_eq!(o.distance, MAX_DISTANCE);
    }

    #[test]
    fn looks_from_behind() {
        // Facing -z (the default), the eye sits on the +z side.
        let o = Orbit::default();
        assert!(o.offset().z > 0.);
        assert!((o.offset().length() - o.distance).abs() < 1e-4);
    }

    #[test]
    fn never_below_the_ground_or_further_than_asked() {
        let mut o = Orbit {
            distance: 24.,
            ..Default::default()
        };
        for (x, z) in [(5., 10.), (6., 28.), (-40., 60.), (80., -30.)] {
            let target = standing_at(x, z);
            for step in 0..24 {
                o.yaw = step as f32 * std::f32::consts::TAU / 24.;
                for pitch in [MIN_PITCH, 0., 0.5, MAX_PITCH] {
                    o.pitch = pitch;
                    let eye = o.eye(target, |_, eye| eye);
                    assert!(eye.is_finite());
                    assert!(eye.y >= world::height(eye.x, eye.z) + GROUND_CLEARANCE - 1e-4);
                    assert!(eye.distance(target) <= o.distance + 1e-3);
                }
            }
        }
    }

    #[test]
    fn scenery_can_pull_the_eye_in() {
        let o = Orbit {
            pitch: MAX_PITCH,
            ..Default::default()
        };
        let target = standing_at(5., 10.);
        let eye = o.eye(target, |target, eye| target.lerp(eye, 0.5));
        assert!(eye.distance(target) < o.distance * 0.6);
    }
}
