//! What the player is holding, turned into the `Move` the server wants, plus the orbit camera.
//! The client maps its window's keys and buttons onto [`Held`]; everything here is plain data.
pub mod camera;
pub use camera::Orbit;

use glam::Vec2;
use protocol::Command;

/// The movement keys and mouse buttons down right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub walk: bool,
    /// Right mouse button: the character faces where the camera looks.
    pub mouse_look: bool,
}

impl Held {
    /// A world-space direction on the ground, relative to the camera's yaw (0 faces +z).
    pub fn direction(&self, yaw: f32) -> Vec2 {
        let axis = |positive: bool, negative: bool| (positive as i32 - negative as i32) as f32;
        let forward = Vec2::new(yaw.sin(), yaw.cos());
        let right = Vec2::new(-yaw.cos(), yaw.sin());
        (forward * axis(self.forward, self.back) + right * axis(self.right, self.left))
            .normalize_or_zero()
    }

    /// The `Move` to send for these keys with the camera at `yaw`.
    pub fn movement(&self, yaw: f32) -> Command {
        Command::Move {
            direction: self.direction(yaw),
            walk: self.walk,
            face_yaw: self.mouse_look.then_some(yaw),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-5
    }

    #[test]
    fn forward_follows_the_camera() {
        let w = Held {
            forward: true,
            ..Default::default()
        };
        assert!(close(w.direction(0.), Vec2::Y));
        assert!(close(w.direction(PI), -Vec2::Y));
        assert!(close(w.direction(PI / 2.), Vec2::X));
    }

    #[test]
    fn strafing_is_square_to_forward() {
        let d = Held {
            right: true,
            ..Default::default()
        };
        let a = Held {
            left: true,
            ..Default::default()
        };
        for yaw in [0., 1., PI, -2.5] {
            let forward = Held {
                forward: true,
                ..Default::default()
            }
            .direction(yaw);
            assert!(d.direction(yaw).dot(forward).abs() < 1e-5);
            assert!(close(a.direction(yaw), -d.direction(yaw)));
        }
    }

    #[test]
    fn diagonals_are_not_faster() {
        let wd = Held {
            forward: true,
            right: true,
            ..Default::default()
        };
        assert!((wd.direction(0.7).length() - 1.).abs() < 1e-5);
    }

    #[test]
    fn opposite_keys_cancel() {
        let ws = Held {
            forward: true,
            back: true,
            ..Default::default()
        };
        assert_eq!(ws.direction(1.), Vec2::ZERO);
    }

    #[test]
    fn faces_the_camera_only_with_mouse_look() {
        let mut held = Held {
            forward: true,
            walk: true,
            ..Default::default()
        };
        let Command::Move { walk, face_yaw, .. } = held.movement(0.5) else {
            unreachable!()
        };
        assert!(walk);
        assert_eq!(face_yaw, None);
        held.mouse_look = true;
        let Command::Move { face_yaw, .. } = held.movement(0.5) else {
            unreachable!()
        };
        assert_eq!(face_yaw, Some(0.5));
    }
}
