//! What the player is holding, turned into the `Move` the server wants, plus the orbit camera.
//! The client maps its window's keys and buttons onto [`Held`]; everything here is plain data.
pub mod bindings;
pub mod camera;
pub mod settings;
pub use bindings::{Bindings, Control, Input};
pub use camera::{Follow, Orbit};
pub use settings::{GroundCast, Settings};

use glam::Vec2;
use protocol::Command;

/// Keyboard turning speed in radians a second: half a turn a second, as in WoW.
pub const TURN_RATE: f32 = std::f32::consts::PI;
/// Classic: seconds of moving without touching the mouse before the camera swings back
/// behind the character.
const SWING_DELAY: f32 = 1.;
/// How quickly it swings back, per second (the gap shrinks by e every 1/rate seconds).
const SWING_RATE: f32 = 3.;

/// The movement keys and mouse buttons down right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub forward: bool,
    pub back: bool,
    /// A and D: strafe in Action or while mouse-looking, turn otherwise.
    pub left: bool,
    pub right: bool,
    /// Q and E: always strafe.
    pub strafe_left: bool,
    pub strafe_right: bool,
    /// Shift: sprint (the sim only allows it out of combat).
    pub sprint: bool,
    /// Right mouse button: the character faces where the camera looks.
    pub mouse_look: bool,
    /// Left mouse button: the camera orbits and the character keeps its heading.
    pub orbiting: bool,
}

/// An angle brought into -π..π.
fn wrap(angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// New World: the mouse always looks and the character always faces the crosshair.
    /// WASD runs and strafes relative to it.
    #[default]
    Action,
    /// WoW and Guild Wars: A/D turn, S backpedals, left-drag looks around, right-drag steers.
    Classic,
}

/// Steers the local character: keeps its heading, which is always sent as `face_yaw`, and
/// turns the camera along with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pilot {
    pub scheme: Scheme,
    /// Where the character faces, in the same unwrapped angle as [`Orbit::yaw`].
    pub heading: f32,
    pub autorun: bool,
    /// Walking instead of running, toggled with its key.
    pub walking: bool,
    /// Classic: swing the camera back behind the character after a second of moving.
    pub swing_back: bool,
    /// Seconds spent moving since the mouse last held the camera.
    moving_for: f32,
}

impl Pilot {
    pub fn new(scheme: Scheme, heading: f32) -> Self {
        Self {
            scheme,
            heading,
            autorun: false,
            walking: false,
            swing_back: true,
            moving_for: 0.,
        }
    }

    pub fn toggle_autorun(&mut self) {
        self.autorun = !self.autorun;
    }

    pub fn toggle_walk(&mut self) {
        self.walking = !self.walking;
    }

    /// Action is Classic with the right button always held. Its left button is for clicking,
    /// not orbiting, so it never adds up to both-buttons run.
    fn effective(&self, held: &Held) -> Held {
        let action = self.scheme == Scheme::Action;
        Held {
            mouse_look: held.mouse_look || action,
            orbiting: held.orbiting && !action,
            ..*held
        }
    }

    /// Advances one frame of `dt` seconds: turns the character (and the camera with it)
    /// and returns the `Move` to send.
    pub fn steer(&mut self, held: &Held, orbit: &mut Orbit, dt: f32) -> Command {
        let held = self.effective(held);
        if held.forward || held.back {
            self.autorun = false;
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
        let direction = self.direction(&held, orbit.yaw);
        if self.swing_back && !held.mouse_look && !held.orbiting && direction != Vec2::ZERO {
            self.moving_for += dt;
            if self.moving_for > SWING_DELAY {
                orbit.yaw += wrap(self.heading - orbit.yaw) * (1. - (-SWING_RATE * dt).exp());
            }
        } else {
            self.moving_for = 0.;
        }
        Command::Move {
            direction,
            walk: self.walking,
            sprint: held.sprint && !self.walking,
            face_yaw: Some(self.heading),
        }
    }

    /// Where the keys point on the ground right now (for a dodge), without turning.
    pub fn direction(&self, held: &Held, camera_yaw: f32) -> Vec2 {
        let held = self.effective(held);
        let heading = if held.mouse_look {
            camera_yaw
        } else {
            self.heading
        };
        let (forward, right) = basis(heading);
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
            ..
        } = c
        else {
            unreachable!()
        };
        (direction, walk, face_yaw)
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

    fn classic() -> (Pilot, Orbit) {
        let orbit = Orbit::default();
        (Pilot::new(Scheme::Classic, orbit.yaw), orbit)
    }

    fn action() -> (Pilot, Orbit) {
        let orbit = Orbit::default();
        (Pilot::new(Scheme::Action, orbit.yaw), orbit)
    }

    #[test]
    fn action_is_the_default() {
        assert_eq!(Scheme::default(), Scheme::Action);
    }

    #[test]
    fn action_faces_wherever_the_mouse_looks() {
        let (mut p, mut o) = action();
        o.look(-200., 0.);
        let (_, _, face) = moving(p.steer(&Held::default(), &mut o, 0.016));
        assert_eq!(p.heading, o.yaw);
        assert_eq!(face, Some(o.yaw));
    }

    #[test]
    fn action_runs_and_strafes_from_the_camera() {
        let (mut p, mut o) = action();
        o.yaw = 0.7;
        let w = held(|h| h.forward = true);
        let (dir, ..) = moving(p.steer(&w, &mut o, 0.016));
        assert!(close(dir, basis(0.7).0));
        // A and D strafe instead of turning, and don't move the camera.
        let (dir, ..) = moving(p.steer(&held(|h| h.left = true), &mut o, 1.));
        assert_eq!(o.yaw, 0.7);
        assert!(close(dir, -basis(0.7).1));
        // Diagonals are not faster, and opposite keys cancel.
        let wd = held(|h| {
            h.forward = true;
            h.right = true
        });
        assert!((p.direction(&wd, o.yaw).length() - 1.).abs() < 1e-5);
        let ws = held(|h| {
            h.forward = true;
            h.back = true
        });
        assert_eq!(p.direction(&ws, o.yaw), Vec2::ZERO);
    }

    #[test]
    fn action_backpedals_facing_the_crosshair() {
        let (mut p, mut o) = action();
        let (dir, _, face) = moving(p.steer(&held(|h| h.back = true), &mut o, 0.016));
        assert_eq!(face, Some(o.yaw));
        assert!(close(dir, -basis(o.yaw).0));
    }

    #[test]
    fn action_autoruns_toward_the_crosshair() {
        let (mut p, mut o) = action();
        p.toggle_autorun();
        o.look(150., 0.);
        let (dir, ..) = moving(p.steer(&Held::default(), &mut o, 0.016));
        assert!(close(dir, basis(o.yaw).0));
    }

    #[test]
    fn action_left_click_does_not_run() {
        let (mut p, mut o) = action();
        let (dir, ..) = moving(p.steer(&held(|h| h.orbiting = true), &mut o, 0.016));
        assert_eq!(dir, Vec2::ZERO);
    }

    #[test]
    fn classic_camera_swings_back_after_a_second() {
        let (mut p, mut o) = classic();
        o.yaw = p.heading + 1.;
        let run = held(|h| h.forward = true);
        for _ in 0..50 {
            p.steer(&run, &mut o, 1. / 60.);
        }
        assert_eq!(o.yaw, p.heading + 1.);
        for _ in 0..180 {
            p.steer(&run, &mut o, 1. / 60.);
        }
        assert!(wrap(o.yaw - p.heading).abs() < 0.01);
    }

    #[test]
    fn swing_back_waits_while_orbiting_or_switched_off() {
        let (mut p, mut o) = classic();
        o.yaw = p.heading + 1.;
        let look = held(|h| {
            h.forward = true;
            h.orbiting = true
        });
        for _ in 0..180 {
            p.steer(&look, &mut o, 1. / 60.);
        }
        assert_eq!(o.yaw, p.heading + 1.);
        p.swing_back = false;
        for _ in 0..180 {
            p.steer(&held(|h| h.forward = true), &mut o, 1. / 60.);
        }
        assert_eq!(o.yaw, p.heading + 1.);
    }

    #[test]
    fn wrap_takes_the_short_way() {
        assert!((wrap(std::f32::consts::TAU + 0.5) - 0.5).abs() < 1e-5);
        assert!((wrap(-std::f32::consts::PI - 0.5) - (std::f32::consts::PI - 0.5)).abs() < 1e-5);
    }

    #[test]
    fn shift_sprints_unless_walking() {
        let (mut p, mut o) = action();
        let sprinting = |c: Command| matches!(c, Command::Move { sprint: true, .. });
        let shift = held(|h| {
            h.forward = true;
            h.sprint = true
        });
        assert!(sprinting(p.steer(&shift, &mut o, 0.016)));
        p.toggle_walk();
        let c = p.steer(&shift, &mut o, 0.016);
        assert!(!sprinting(c.clone()));
        assert!(moving(c).1);
        p.toggle_walk();
        assert!(!moving(p.steer(&shift, &mut o, 0.016)).1);
    }
}
