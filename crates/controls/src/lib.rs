//! What the player is holding, turned into the `Move` the server wants, plus the orbit camera.
//! The client maps its window's keys and buttons onto [`Held`]; everything here is plain data.
pub mod camera;
pub use camera::Orbit;

use glam::Vec2;
use protocol::Command;

/// Keyboard turning speed in radians a second: half a turn a second, as in WoW.
pub const TURN_RATE: f32 = std::f32::consts::PI;

/// The movement keys and mouse buttons down right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub forward: bool,
    pub back: bool,
    /// A and D: turn in Classic, strafe in Action or while mouse-looking.
    pub left: bool,
    pub right: bool,
    /// Q and E: always strafe.
    pub strafe_left: bool,
    pub strafe_right: bool,
    pub walk: bool,
    /// Right mouse button: the character faces where the camera looks.
    pub mouse_look: bool,
    /// Left mouse button: the camera orbits and the character keeps its heading.
    pub orbiting: bool,
}

fn axis(positive: bool, negative: bool) -> f32 {
    (positive as i32 - negative as i32) as f32
}

/// Forward and right on the ground for a yaw (0 faces +z).
fn basis(yaw: f32) -> (Vec2, Vec2) {
    (
        Vec2::new(yaw.sin(), yaw.cos()),
        Vec2::new(-yaw.cos(), yaw.sin()),
    )
}

impl Held {
    /// A world-space direction on the ground, relative to the camera's yaw: A/D and Q/E
    /// all strafe. This is the Action scheme.
    pub fn direction(&self, yaw: f32) -> Vec2 {
        let (forward, right) = basis(yaw);
        let strafe = axis(
            self.right || self.strafe_right,
            self.left || self.strafe_left,
        );
        (forward * axis(self.forward, self.back) + right * strafe).normalize_or_zero()
    }

    /// The `Move` to send for these keys with the camera at `yaw`, in the Action scheme.
    pub fn movement(&self, yaw: f32) -> Command {
        Command::Move {
            direction: self.direction(yaw),
            walk: self.walk,
            face_yaw: self.mouse_look.then_some(yaw),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    /// WoW and Guild Wars: A/D turn, S backpedals, left-drag looks around, right-drag steers.
    #[default]
    Classic,
    /// Camera-relative: the character runs whichever way the keys point from the camera.
    Action,
}

/// Steers the local character: keeps its heading, which the Classic scheme always sends as
/// `face_yaw`, and turns the camera along with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pilot {
    pub scheme: Scheme,
    /// Where the character faces, in the same unwrapped angle as [`Orbit::yaw`].
    pub heading: f32,
    pub autorun: bool,
}

impl Pilot {
    pub fn new(scheme: Scheme, heading: f32) -> Self {
        Self {
            scheme,
            heading,
            autorun: false,
        }
    }

    pub fn toggle_autorun(&mut self) {
        self.autorun = !self.autorun;
    }

    /// Advances one frame of `dt` seconds: turns the character (and the camera with it)
    /// and returns the `Move` to send.
    pub fn steer(&mut self, held: &Held, orbit: &mut Orbit, dt: f32) -> Command {
        if held.forward || held.back {
            self.autorun = false;
        }
        if self.scheme == Scheme::Action {
            return held.movement(orbit.yaw);
        }
        if held.mouse_look {
            self.heading = orbit.yaw;
        } else {
            let turn = axis(held.left, held.right) * TURN_RATE * dt;
            self.heading += turn;
            // The camera stays behind, unless the player is holding it somewhere else.
            if !held.orbiting {
                orbit.yaw += turn;
            }
        }
        Command::Move {
            direction: self.direction(held, orbit.yaw),
            walk: held.walk,
            face_yaw: Some(self.heading),
        }
    }

    /// Where the keys point on the ground right now (for a dodge), without turning.
    pub fn direction(&self, held: &Held, camera_yaw: f32) -> Vec2 {
        if self.scheme == Scheme::Action {
            return held.direction(camera_yaw);
        }
        let (forward, right) = basis(self.heading);
        let run = held.forward || self.autorun || (held.mouse_look && held.orbiting);
        let mut strafe = axis(held.strafe_right, held.strafe_left);
        if held.mouse_look {
            strafe += axis(held.right, held.left);
        }
        (forward * axis(run, held.back) + right * strafe.clamp(-1., 1.)).normalize_or_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-5
    }

    fn held(f: impl FnOnce(&mut Held)) -> Held {
        let mut h = Held::default();
        f(&mut h);
        h
    }

    fn moving(c: Command) -> (Vec2, bool, Option<f32>) {
        let Command::Move {
            direction,
            walk,
            face_yaw,
        } = c
        else {
            unreachable!()
        };
        (direction, walk, face_yaw)
    }

    #[test]
    fn forward_follows_the_camera() {
        let w = held(|h| h.forward = true);
        assert!(close(w.direction(0.), Vec2::Y));
        assert!(close(w.direction(PI), -Vec2::Y));
        assert!(close(w.direction(PI / 2.), Vec2::X));
    }

    #[test]
    fn strafing_is_square_to_forward() {
        let d = held(|h| h.right = true);
        let a = held(|h| h.left = true);
        for yaw in [0., 1., PI, -2.5] {
            let forward = held(|h| h.forward = true).direction(yaw);
            assert!(d.direction(yaw).dot(forward).abs() < 1e-5);
            assert!(close(a.direction(yaw), -d.direction(yaw)));
        }
    }

    #[test]
    fn diagonals_are_not_faster() {
        let wd = held(|h| {
            h.forward = true;
            h.right = true
        });
        assert!((wd.direction(0.7).length() - 1.).abs() < 1e-5);
    }

    #[test]
    fn opposite_keys_cancel() {
        let ws = held(|h| {
            h.forward = true;
            h.back = true
        });
        assert_eq!(ws.direction(1.), Vec2::ZERO);
    }

    #[test]
    fn action_faces_the_camera_only_with_mouse_look() {
        let mut h = held(|h| {
            h.forward = true;
            h.walk = true
        });
        let (_, walk, face) = moving(h.movement(0.5));
        assert!(walk);
        assert_eq!(face, None);
        h.mouse_look = true;
        assert_eq!(moving(h.movement(0.5)).2, Some(0.5));
    }

    fn classic() -> (Pilot, Orbit) {
        let orbit = Orbit::default();
        (Pilot::new(Scheme::Classic, orbit.yaw), orbit)
    }

    #[test]
    fn classic_always_sends_the_heading() {
        let (mut p, mut o) = classic();
        let (dir, _, face) = moving(p.steer(&Held::default(), &mut o, 0.016));
        assert_eq!(dir, Vec2::ZERO);
        assert_eq!(face, Some(p.heading));
    }

    #[test]
    fn s_backpedals_instead_of_turning_round() {
        let (mut p, mut o) = classic();
        let before = p.heading;
        let (dir, _, face) = moving(p.steer(&held(|h| h.back = true), &mut o, 0.016));
        assert_eq!(face, Some(before));
        assert!(close(dir, -basis(before).0));
    }

    #[test]
    fn a_and_d_turn_character_and_camera() {
        let (mut p, mut o) = classic();
        let start = p.heading;
        for _ in 0..30 {
            p.steer(&held(|h| h.left = true), &mut o, 1. / 60.);
        }
        assert!((p.heading - start - TURN_RATE * 0.5).abs() < 1e-4);
        assert!((o.yaw - p.heading).abs() < 1e-5);
        // Turning left goes the way strafing left would.
        let left = -basis(start).1;
        assert!(basis(p.heading).0.dot(left) > 0.99);
    }

    #[test]
    fn a_and_d_strafe_while_mouse_looking() {
        let (mut p, mut o) = classic();
        o.yaw = 1.;
        let (dir, _, face) = moving(p.steer(
            &held(|h| {
                h.mouse_look = true;
                h.right = true
            }),
            &mut o,
            0.016,
        ));
        assert_eq!(p.heading, 1.);
        assert_eq!(face, Some(1.));
        assert!(close(dir, basis(1.).1));
    }

    #[test]
    fn q_and_e_strafe_without_turning() {
        let (mut p, mut o) = classic();
        let start = p.heading;
        let (dir, ..) = moving(p.steer(&held(|h| h.strafe_left = true), &mut o, 0.5));
        assert_eq!(p.heading, start);
        assert!(close(dir, -basis(start).1));
        // With the right button too, D and E don't add up to a faster strafe.
        let both = held(|h| {
            h.mouse_look = true;
            h.right = true;
            h.strafe_right = true
        });
        assert!((p.direction(&both, o.yaw).length() - 1.).abs() < 1e-5);
    }

    #[test]
    fn left_drag_leaves_the_heading_alone() {
        let (mut p, mut o) = classic();
        let start = p.heading;
        o.look(300., 0.);
        p.steer(&held(|h| h.orbiting = true), &mut o, 0.016);
        assert_eq!(p.heading, start);
        assert_ne!(o.yaw, start);
        // Turning by key while orbiting moves only the character.
        let camera = o.yaw;
        p.steer(
            &held(|h| {
                h.orbiting = true;
                h.right = true
            }),
            &mut o,
            0.1,
        );
        assert_eq!(o.yaw, camera);
        assert!(p.heading < start);
    }

    #[test]
    fn both_buttons_run_forward() {
        let (mut p, mut o) = classic();
        let (dir, ..) = moving(p.steer(
            &held(|h| {
                h.mouse_look = true;
                h.orbiting = true
            }),
            &mut o,
            0.016,
        ));
        assert!(close(dir, basis(o.yaw).0));
    }

    #[test]
    fn autorun_runs_until_w_or_s() {
        let (mut p, mut o) = classic();
        p.toggle_autorun();
        let (dir, ..) = moving(p.steer(&Held::default(), &mut o, 0.016));
        assert!(close(dir, basis(p.heading).0));
        p.steer(&held(|h| h.back = true), &mut o, 0.016);
        assert!(!p.autorun);
        let (dir, ..) = moving(p.steer(&Held::default(), &mut o, 0.016));
        assert_eq!(dir, Vec2::ZERO);
    }

    #[test]
    fn action_ignores_the_heading() {
        let mut p = Pilot::new(Scheme::Action, 0.);
        let mut o = Orbit::default();
        let (dir, _, face) = moving(p.steer(&held(|h| h.left = true), &mut o, 1.));
        assert_eq!(p.heading, 0.);
        assert_eq!(face, None);
        assert!(close(dir, -basis(o.yaw).1));
    }
}
