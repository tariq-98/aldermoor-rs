//! Character animation that needs no renderer, so it builds and tests on every platform (the
//! `render` crate needs Metal). `render::character` builds each frame's pose from clips and hands
//! it to these for blending. How the pieces fit together: docs/ANIMATION.md.
pub mod inertial;

pub use inertial::{Blender, Bone, Method, Switch};
