//! Combat effects for the effects pass: what the client hands the renderer each frame, and the
//! art it draws them with.
//!
//! Effects never go through the lit scene shader. Ground marks ([`Decal`]) are projected onto
//! whatever was drawn, inside the scene pass, so they lie on slopes and steps and are smoothed by
//! temporal AA like the ground under them. Everything else ([`Quad`]s and effect meshes) is drawn
//! after tone mapping, straight onto the output, so fast sparks and trails are never smeared or
//! dimmed by temporal AA, and cost nothing when nothing is on screen. All of it is unlit and
//! blended with premultiplied alpha: a colour adds light, an opacity darkens what's behind.
//!
//! A [`Material`] says how a texel becomes colour. Sprites hold three channels: heat (looked up
//! in a colour ramp), cover (opacity) and an erosion order (as `params.x` rises past it, the
//! sprite dissolves). See `art/blender/aldermoor_art/fx.py`.
use crate::model::{Model, Texture};
use std::sync::Arc;

/// Texels per ramp.
pub const RAMP_WIDTH: usize = 128;

/// How a texel is coloured. Shared by quads, decals and meshes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Material {
    /// Linear tint, any brightness (rgb), and opacity (a).
    pub colour: [f32; 4],
    /// Atlas sprites: this flipbook frame's origin (xy) and size (zw) in atlas UV. Tiles: UV
    /// offset (xy) and repeats (zw).
    pub frame: [f32; 4],
    /// Atlas sprites: the next frame's origin (xy) and how far to blend into it (z). w is the
    /// ramp row, or negative for none (heat is then used as a grey).
    pub next: [f32; 4],
    /// Erosion threshold (x) and softness (y, 0 for none); soft-particle depth in metres (z, 0
    /// to clip hard at what's in front); blend (w): 0 adds light, 1 covers like paint.
    pub params: [f32; 4],
    /// Texture (x): 0 for the atlas, n for tile n − 1. Heat multiplier (w), so an ember can
    /// cool as it ages. y and z are unused.
    pub extra: [f32; 4],
}

/// A flat effect: a camera-facing sprite, a beam between two points or a plane in the world.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Quad {
    /// Centre (sprite, plane) or tail (beam); w is the shape: [`Quad::SPRITE`],
    /// [`Quad::BEAM`] or [`Quad::PLANE`].
    pub position: [f32; 4],
    /// Sprite: half width, half height, turn about the view axis (radians). Beam: the head
    /// (xyz) and its half width (w). Plane: its first half axis (xyz).
    pub a: [f32; 4],
    /// Sprite: w moves it this many metres towards the camera. Beam: the tail's half width
    /// (x). Plane: its second half axis (xyz).
    pub b: [f32; 4],
    pub material: Material,
}

impl Quad {
    pub const SPRITE: f32 = 0.;
    pub const BEAM: f32 = 1.;
    pub const PLANE: f32 = 2.;
}

/// A mark projected down onto whatever was drawn: a warning, a field, a scorch, a casting
/// circle. The box `to_world` maps from (−1…1 on every axis) is where it can land; y runs up
/// through it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Decal {
    pub to_decal: [[f32; 4]; 4],
    pub to_world: [[f32; 4]; 4],
    /// x: [`Decal::TEXTURE`], [`Decal::CIRCLE`], [`Decal::CONE`] or [`Decal::LINE`]. y: a cone's
    /// half angle (radians). z: how far the fill has grown (0…1, negative for a full shape
    /// without a leading edge). w: the glowing edge's width in metres.
    pub shape: [f32; 4],
    /// The box's half size in metres (x, y, z), so edges keep their width however it's
    /// stretched. A circle's radius is x. A cone opens along +z from its apex, `w` metres
    /// behind the box's centre, and reaches twice z. A line runs along z from −z to +z and is
    /// x either side.
    pub size: [f32; 4],
    /// Shapes: the fill's colour (rgb) and opacity (a); the edge takes the material's colour.
    /// Textured marks: a pool of light under the mark in its ramp's colour, tinted by rgb and
    /// as strong as a.
    pub fill: [f32; 4],
    /// Textured marks: the sprite, ramp and erosion. Shapes: a tile (`extra.x`) breaks up the
    /// fill, at `frame.zw` repeats a metre, drifting by `frame.xy`.
    pub material: Material,
}

impl Decal {
    pub const TEXTURE: f32 = 0.;
    pub const CIRCLE: f32 = 1.;
    pub const CONE: f32 = 2.;
    pub const LINE: f32 = 3.;
}

/// An effect mesh placed in the world: a ward's shell, a column of light, a crystal.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshInstance {
    pub model: [[f32; 4]; 4],
    /// Rim (Fresnel) power (x) and weight (y); where the vertex colour's red mask starts to
    /// show (z) and its weight (w).
    pub shading: [f32; 4],
    /// Twinkle weight (x) and speed (y), per the vertex colour's green; texture weight (z);
    /// texture scroll up the mesh in UV per second (w).
    pub terms: [f32; 4],
    pub material: Material,
}

/// One frame of effects. Meshes are drawn first, then quads farthest first.
#[derive(Default)]
pub struct Frame {
    pub quads: Vec<Quad>,
    pub decals: Vec<Decal>,
    /// The mesh (an index into [`Library::meshes`]) and where it goes.
    pub meshes: Vec<(usize, MeshInstance)>,
    /// Scene seconds, for the meshes' own animation.
    pub time: f32,
}

impl Frame {
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty() && self.decals.is_empty() && self.meshes.is_empty()
    }
}

/// The art effects are drawn with, uploaded once.
pub struct Library {
    /// Every sprite and flipbook (linear RGBA8: heat, cover, erosion).
    pub atlas: Arc<Texture>,
    /// Tiling strips, all one size.
    pub tiles: Vec<Arc<Texture>>,
    /// Colour ramps, one per row: linear colour (rgb, any brightness), a = 1.
    pub ramps: Vec<[[f32; 4]; RAMP_WIDTH]>,
    /// Effect meshes; each draws its first primitive.
    pub meshes: Vec<Arc<Model>>,
}
