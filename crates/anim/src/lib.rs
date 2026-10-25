//! Character animation that needs no renderer, so it builds and tests on every platform (the
//! `render` crate needs Metal). `render::character` builds each frame's pose from clips and hands
//! it to these for blending. How the pieces fit together: docs/ANIMATION.md.
pub mod feet;
pub mod ik;
pub mod inertial;
pub mod locomotion;
pub mod lod;
pub mod look;
pub mod turn;

pub use feet::{Contact, Feet, Leg, Placement};
pub use inertial::{Blender, Bone, Method, Switch};
pub use locomotion::{Gait, Step, Stride};
pub use lod::{Crowd, Importance, Tier};
pub use look::{Aim, Gaze, Turns};
pub use turn::{Plant, Stance, Steps, Turn};
