//! A camera orbiting the player's chest, kept above the ground and out of buildings, and
//! eased so zooming, bumping into walls and stepping up slopes don't jerk the view.
use glam::Vec3;

/// Where the camera looks: this far above the player's feet.
pub const PIVOT_HEIGHT: f32 = 1.25;
pub const FOV_DEGREES: f32 = 52.;
/// How close the eye may come to the ground.
const GROUND_CLEARANCE: f32 = 0.35;
pub const MIN_DISTANCE: f32 = 2.;
pub const MAX_DISTANCE: f32 = 24.;
/// Metres per wheel notch.
const ZOOM_STEP: f32 = 0.6;
/// Radians per pixel of mouse travel.
const YAW_RATE: f32 = 0.004;
const PITCH_RATE: f32 = 0.003;
/// Below the horizon the camera looks up from near the ground; near the top it looks almost
/// straight down.
const MIN_PITCH: f32 = -0.6;
const MAX_PITCH: f32 = 1.4;
/// Easing rates, per second: the gap shrinks by e every 1/rate seconds.
const ZOOM_EASE: f32 = 12.;
const PIVOT_EASE: f32 = 20.;
/// How fast the eye slides back out once nothing is in the way, in metres a second. Pulling
/// in is instant so walls never come between the camera and the player.
const REACH_SPEED: f32 = 4.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    /// The way the camera faces on the ground (0 faces +z); the eye sits behind it.
    pub yaw: f32,
    /// Above the horizon, looking down at the player.
    pub pitch: f32,
    pub distance: f32,
    /// How far right of the player the camera looks, in metres: over the shoulder.
    pub shoulder: f32,
    /// Multiplies mouse look speed.
    pub sensitivity: f32,
    /// Mouse up looks down.
    pub invert_y: bool,
    pub min_distance: f32,
    pub max_distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: std::f32::consts::PI,
            pitch: 0.23,
            distance: 7.,
            shoulder: 0.,
            sensitivity: 1.,
            invert_y: false,
            min_distance: MIN_DISTANCE,
            max_distance: MAX_DISTANCE,
        }
    }
}

impl Orbit {
    /// Turns by a mouse movement in pixels.
    pub fn look(&mut self, dx: f64, dy: f64) {
        let dy = if self.invert_y { -dy } else { dy };
        self.yaw -= dx as f32 * YAW_RATE * self.sensitivity;
        self.pitch =
            (self.pitch + dy as f32 * PITCH_RATE * self.sensitivity).clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Zooms by wheel notches; positive comes closer.
    pub fn zoom(&mut self, notches: f32) {
        self.distance =
            (self.distance - notches * ZOOM_STEP).clamp(self.min_distance, self.max_distance);
    }

    /// The point the camera looks at for a player standing at `feet`: chest height, moved
    /// right by [`Orbit::shoulder`] so the player stands left of the crosshair.
    pub fn target(&self, feet: Vec3) -> Vec3 {
        let right = Vec3::new(-self.yaw.cos(), 0., self.yaw.sin());
        feet + Vec3::Y * PIVOT_HEIGHT + right * self.shoulder
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

/// Where the camera is this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub target: Vec3,
    pub eye: Vec3,
}

/// What the camera shows, trailing the [`Orbit`] a little: the zoom eases, the pivot eases
/// up and down with the ground, and after a wall pulls the eye in it slides back out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Follow {
    /// None until the first frame, which snaps straight to the orbit.
    eased: Option<Eased>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Eased {
    distance: f32,
    feet_y: f32,
    /// How far out the eye may go this frame.
    reach: f32,
}

fn ease(from: f32, to: f32, rate: f32, dt: f32) -> f32 {
    from + (to - from) * (1. - (-rate * dt).exp())
}

impl Follow {
    /// Advances by `dt` seconds towards `orbit` around a player at `feet`. `blocked` is as
    /// for [`Orbit::eye`].
    pub fn update(
        &mut self,
        orbit: &Orbit,
        feet: Vec3,
        dt: f32,
        blocked: impl Fn(Vec3, Vec3) -> Vec3,
    ) -> View {
        let e = match self.eased {
            None => Eased {
                distance: orbit.distance,
                feet_y: feet.y,
                reach: f32::INFINITY,
            },
            Some(e) => Eased {
                distance: ease(e.distance, orbit.distance, ZOOM_EASE, dt),
                feet_y: ease(e.feet_y, feet.y, PIVOT_EASE, dt),
                ..e
            },
        };
        let shown = Orbit {
            distance: e.distance,
            ..*orbit
        };
        let target = shown.target(Vec3::new(feet.x, e.feet_y, feet.z));
        let clear = shown.eye(target, blocked);
        let room = clear.distance(target);
        let reach = if room <= e.reach {
            room
        } else {
            (e.reach + REACH_SPEED * dt).min(room)
        };
        let mut eye = if room > 1e-4 {
            target + (clear - target) * (reach / room)
        } else {
            clear
        };
        eye.y = eye.y.max(world::height(eye.x, eye.z) + GROUND_CLEARANCE);
        self.eased = Some(Eased { reach, ..e });
        View { target, eye }
    }
}

/// How hard blows shake the camera.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenShake {
    Off,
    Low,
    #[default]
    Full,
}

impl ScreenShake {
    fn scale(self) -> f32 {
        match self {
            Self::Off => 0.,
            Self::Low => 0.4,
            Self::Full => 1.,
        }
    }
}

/// The furthest kicks can add up to, in metres.
const KICK_CAP: f32 = 0.09;
/// Kicks die away by e every this many seconds: about 5% is left after 150 ms.
const KICK_DECAY: f32 = 0.05;
/// The look point moves this share of the eye's kick, which tips the view by a degree or so.
const KICK_LOOK: f32 = 0.4;

/// A short jolt of the camera when a blow lands, along the way the blow travelled. Kicks add
/// up to a cap and die away quickly, so a pack of wolves can't keep the screen shaking.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Kick {
    push: Vec3,
}

impl Kick {
    /// Adds a jolt of `metres` (before the setting) along `dir` on the ground.
    pub fn add(&mut self, dir: glam::Vec2, metres: f32, shake: ScreenShake) {
        let push =
            self.push + Vec3::new(dir.x, 0., dir.y).normalize_or_zero() * metres * shake.scale();
        self.push = push.clamp_length_max(KICK_CAP);
    }

    /// Dies away by `dt` seconds and moves `view` by what is left.
    pub fn apply(&mut self, view: View, dt: f32) -> View {
        self.push *= (-dt / KICK_DECAY).exp();
        View {
            eye: view.eye + self.push,
            target: view.target + self.push * KICK_LOOK,
        }
    }

    pub fn size(&self) -> f32 {
        self.push.length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kicks_add_up_to_a_cap_and_die_away_in_a_moment() {
        let mut k = Kick::default();
        let east = glam::Vec2::X;
        k.add(east, 0.04, ScreenShake::Full);
        assert!((k.size() - 0.04).abs() < 1e-6);
        let view = View {
            eye: Vec3::ZERO,
            target: Vec3::Z,
        };
        let moved = k.apply(view, 0.);
        assert!(moved.eye.x > 0.039 && moved.target.x > 0.01 && moved.target.x < moved.eye.x);
        for _ in 0..20 {
            k.add(east, 0.04, ScreenShake::Full);
        }
        assert!(
            (k.size() - KICK_CAP).abs() < 1e-6,
            "a crowd's blows went past the cap"
        );
        for _ in 0..9 {
            k.apply(view, 1. / 60.);
        }
        assert!(
            k.size() < KICK_CAP * 0.06,
            "still {} m after 150 ms",
            k.size()
        );
    }

    #[test]
    fn the_setting_scales_kicks() {
        let size = |shake| {
            let mut k = Kick::default();
            k.add(glam::Vec2::Y, 0.05, shake);
            k.size()
        };
        assert_eq!(size(ScreenShake::Off), 0.);
        assert!((size(ScreenShake::Low) - 0.02).abs() < 1e-6);
        assert!((size(ScreenShake::Full) - 0.05).abs() < 1e-6);
    }

    fn standing_at(x: f32, z: f32) -> Vec3 {
        Orbit::default().target(Vec3::new(x, world::height(x, z), z))
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
        o.max_distance = 10.;
        o.zoom(-100.);
        assert_eq!(o.distance, 10.);
    }

    #[test]
    fn sensitivity_and_invert() {
        let mut o = Orbit::default();
        let mut fast = Orbit {
            sensitivity: 2.,
            invert_y: true,
            ..o
        };
        o.look(10., 10.);
        fast.look(10., 10.);
        let start = Orbit::default();
        assert!(((fast.yaw - start.yaw) - 2. * (o.yaw - start.yaw)).abs() < 1e-6);
        assert!(((fast.pitch - start.pitch) + 2. * (o.pitch - start.pitch)).abs() < 1e-6);
    }

    #[test]
    fn zoom_eases_in() {
        let mut o = Orbit::default();
        let feet = Vec3::new(5., world::height(5., 10.), 10.);
        let mut f = Follow::default();
        let first = f.update(&o, feet, 0.016, |_, eye| eye);
        assert!((first.eye.distance(first.target) - 7.).abs() < 1e-3);
        o.distance = 3.;
        let next = f.update(&o, feet, 0.016, |_, eye| eye);
        let d = next.eye.distance(next.target);
        assert!(d < 7. && d > 6.);
        for _ in 0..120 {
            f.update(&o, feet, 0.016, |_, eye| eye);
        }
        let settled = f.update(&o, feet, 0.016, |_, eye| eye);
        assert!((settled.eye.distance(settled.target) - 3.).abs() < 1e-2);
    }

    #[test]
    fn walls_pull_in_at_once_and_let_go_slowly() {
        let o = Orbit::default();
        let feet = Vec3::new(5., world::height(5., 10.), 10.);
        let mut f = Follow::default();
        f.update(&o, feet, 0.016, |_, eye| eye);
        let wall = |target: Vec3, eye: Vec3| target.lerp(eye, 0.25);
        let hit = f.update(&o, feet, 0.016, wall);
        assert!((hit.eye.distance(hit.target) - 7. * 0.25).abs() < 1e-2);
        let free = f.update(&o, feet, 0.1, |_, eye| eye);
        let d = free.eye.distance(free.target);
        assert!((d - (7. * 0.25 + REACH_SPEED * 0.1)).abs() < 1e-2);
        for _ in 0..200 {
            f.update(&o, feet, 0.016, |_, eye| eye);
        }
        let back = f.update(&o, feet, 0.016, |_, eye| eye);
        assert!((back.eye.distance(back.target) - 7.).abs() < 1e-2);
    }

    #[test]
    fn pivot_eases_up_steps() {
        let o = Orbit::default();
        let mut f = Follow::default();
        let low = Vec3::new(5., world::height(5., 10.), 10.);
        let a = f.update(&o, low, 0.016, |_, eye| eye);
        let b = f.update(&o, low + Vec3::Y * 0.5, 0.016, |_, eye| eye);
        let rise = b.target.y - a.target.y;
        assert!(rise > 0. && rise < 0.5);
    }

    #[test]
    fn shoulder_moves_the_view_right() {
        let mut o = Orbit {
            shoulder: 0.6,
            ..Default::default()
        };
        for yaw in [0., 1., std::f32::consts::PI, -2.] {
            o.yaw = yaw;
            let d = o.target(Vec3::ZERO) - Vec3::Y * PIVOT_HEIGHT;
            // Right of where the camera faces: forward (sin, cos) turned a quarter clockwise.
            let forward = Vec3::new(yaw.sin(), 0., yaw.cos());
            assert!((d.length() - 0.6).abs() < 1e-5);
            assert!(d.dot(forward).abs() < 1e-5);
            assert!(forward.cross(d).y < 0.);
        }
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
