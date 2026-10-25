//! How combat looks (`assets/catalog/effects.json`): named effects built from layers of authored
//! art (`assets/effects/`: a sprite atlas, tiling strips and meshes made by
//! `art/blender/build_effects.py`), and which effect plays for each missile, cast, aura, field,
//! warning and impact.
//!
//! An effect is a list of [`Layer`]s. Each layer is one kind of thing drawn at the effect's
//! anchor: camera-facing sprites (often many particles), beams, trails of sprites left along a
//! missile's path, marks projected on the ground, or effect meshes. All look values (colours,
//! ramps, sizes, timings) live here; the client only places and animates layers by these numbers.

use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectCatalog {
    pub version: u32,
    /// The sprite atlas and its index (`atlas.json`), relative to the asset root.
    pub atlas: String,
    pub frames: String,
    /// Tiling strips by name; their order (by name) is their layer in the tile array.
    pub tiles: BTreeMap<String, String>,
    /// Effect meshes by name.
    pub meshes: BTreeMap<String, String>,
    /// Colour ramps: stops of `[position 0…1, r, g, b]`, linear and any brightness. A sprite's
    /// heat picks its colour from its layer's ramp.
    pub ramps: BTreeMap<String, Vec<[f32; 4]>>,
    pub effects: BTreeMap<String, Effect>,
    /// By `ProjectileView::visual`.
    pub projectiles: BTreeMap<String, ProjectileLook>,
    /// Abilities whose cast or channel shows on the caster, and the effect it plays. Wind-ups
    /// for heavy blows aren't listed: the swing shows them.
    pub casts: BTreeMap<String, String>,
    /// By aura key. Auras that aren't listed show only on the HUD.
    pub auras: BTreeMap<String, String>,
    /// Lasting combo fields by name (`AreaView::field`); a field with no entry uses its school.
    /// `<name>.hostile`, if listed, is shown instead when the field threatens the viewer.
    pub fields: BTreeMap<String, String>,
    /// Per school (lowercase): the field a school's area leaves when it has no named field, and
    /// the burst where a warned-of blow lands.
    pub schools: BTreeMap<String, SchoolLook>,
    /// Warnings of a blow about to land, by who they threaten.
    pub warnings: Warnings,
    /// A combo's burst, and the circle a ground skill is aimed with.
    pub combo: String,
    pub aim: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Warnings {
    pub hostile: String,
    pub friendly: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchoolLook {
    pub field: String,
    pub impact: String,
}

/// An effect: its layers, or another effect's layers with some ramps swapped (a frost cast is
/// a fire cast in the frost ramps).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Effect {
    Layers(Vec<Layer>),
    Like {
        like: String,
        #[serde(default)]
        ramps: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileLook {
    /// Plays on the missile while it flies: +z along its flight.
    pub flight: String,
    /// Left along its path: the layers' particles stay where they were dropped.
    #[serde(default)]
    pub trail: Option<String>,
    /// Plays once where it stops.
    pub impact: String,
    /// A solid, lit mesh carried by the missile (an arrow), tip at the missile's position.
    #[serde(default)]
    pub mesh: Option<String>,
    /// Turns a second end over end (a thrown axe's mesh), about `pivot` in the mesh's frame.
    #[serde(default)]
    pub spin: f32,
    #[serde(default)]
    pub pivot: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerKind {
    /// Camera-facing sprites (`count` of them), or streaks when `stretch` is set.
    Sprite,
    /// One beam behind the anchor, from its position back along its flight (`size` is its
    /// half width at the head and tail, `length` its length).
    Beam,
    /// Sprites dropped along a missile's path, `rate` a metre, each then living on its own.
    Trail,
    /// A mark projected down onto the ground.
    Decal,
    /// An effect mesh.
    Mesh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecalShape {
    /// The layer's sprite or tile, `size` its half width.
    Texture,
    /// The area's own shape (circle, cone or line) with a glowing edge and a fill that grows
    /// as a warning ripens.
    Area,
}

/// One part of an effect. Every field has a default, so a layer names only what it changes.
///
/// Particles: a layer with `life` cycles `count` particles through lives of that many seconds
/// (staggered), each born somewhere in `spread` around `offset` and flying off; with `life` 0
/// it is one steady thing. In a one-shot effect (an impact) particles live once, starting after
/// a `delay`. Over a particle's life `t` runs 0…1; sizes, erosion and heat follow it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Layer {
    pub kind: LayerKind,
    /// A sprite or flipbook in the atlas, or a tiling strip (`tile`).
    pub frame: Option<String>,
    pub tile: Option<String>,
    pub ramp: Option<String>,
    /// Linear tint, any brightness, and opacity.
    pub colour: [f32; 3],
    pub alpha: f32,
    /// 0 adds light; 1 covers what's behind like paint (smoke, frost on the ground).
    pub blend: f32,
    pub count: u32,
    pub life: f32,
    /// Trails: particles per metre of flight.
    pub rate: f32,
    /// Half size in metres at birth and at death. Decals: half width. Meshes: radius. A steady
    /// layer (no `life`) on a cast or warning goes from the first to the second as it ripens,
    /// and so does its erosion.
    pub size: [f32; 2],
    /// Each particle's size differs from the layer's by up to this share either way.
    pub vary: f32,
    /// Height over width (sprites, meshes).
    pub aspect: f32,
    /// Beams: length in metres. Meshes: length along z, as a share of their size.
    pub length: f32,
    /// Sprites become streaks this many seconds of their flight long.
    pub stretch: f32,
    /// Turn speed range (radians a second, either way round); and a fixed turn.
    pub spin: [f32; 2],
    pub angle: f32,
    /// Where particles are born, in the anchor's frame (x right, y up, z forward), and how
    /// far either side of it (an ellipsoid). `ring` puts them on its rim instead, and `orbit`
    /// (radians a second) sets them evenly round it, circling.
    pub offset: [f32; 3],
    pub spread: [f32; 3],
    pub ring: bool,
    pub orbit: f32,
    /// Outward speed range from where they're born, a steady velocity in the anchor's frame,
    /// gravity (m/s², down) and drag (per second).
    pub speed: [f32; 2],
    pub velocity: [f32; 3],
    pub gravity: f32,
    pub drag: f32,
    /// Shares of a life spent fading in and out.
    pub fade: [f32; 2],
    /// Erosion threshold at birth and at death (sprites dissolve as it rises), and how soft
    /// the dissolving edge is: small values break shapes up crisply, as painted effects do.
    pub erode: [f32; 2],
    pub erode_soft: f32,
    /// Soft-particle depth in metres (0: clip hard against what's in front).
    pub soft: f32,
    /// Share of its heat a particle loses over its life (fire cooling to embers).
    pub cool: f32,
    /// One-shot effects: start delay range in seconds.
    pub delay: [f32; 2],
    /// Flipbooks: 0 plays once over a life; otherwise frames a second, looping.
    pub fps: f32,
    /// Brightness wobble: amount and speed (Hz).
    pub pulse: [f32; 2],
    /// Sprites are moved this many metres towards the camera.
    pub towards: f32,
    /// Sizes, offsets, spread and speeds scale with the anchor (a field's radius, a unit's
    /// height), and so does `count` for decals' particles over an area.
    pub scaled: bool,
    /// Particles stay where they were born instead of moving with the anchor.
    pub world: bool,
    /// Decals.
    pub shape: DecalShape,
    /// Decals: the glowing edge's width (metres) and the fill's colour and opacity. A textured
    /// mark's fill is a pool of light under it in its ramp's colour (rgb tints it, a is its
    /// strength).
    pub edge: f32,
    pub fill: [f32; 4],
    /// Decals and beams: tile repeats a metre, and drift (UV a second).
    pub repeat: f32,
    pub scroll: [f32; 2],
    /// Meshes. `upright` stands one on end (its +z up) with a random heading, for crystals
    /// growing out of the ground.
    pub mesh: Option<String>,
    pub upright: bool,
    /// Meshes: rim (Fresnel) power and weight; the vertex mask's start and weight; twinkle
    /// weight and speed; texture weight.
    pub rim: [f32; 2],
    pub mask: [f32; 2],
    pub twinkle: [f32; 2],
    pub texture: f32,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            kind: LayerKind::Sprite,
            frame: None,
            tile: None,
            ramp: None,
            colour: [1.; 3],
            alpha: 1.,
            blend: 0.,
            count: 1,
            life: 0.,
            rate: 0.,
            size: [0.5; 2],
            vary: 0.,
            aspect: 1.,
            length: 1.,
            stretch: 0.,
            spin: [0.; 2],
            angle: 0.,
            offset: [0.; 3],
            spread: [0.; 3],
            ring: false,
            orbit: 0.,
            speed: [0.; 2],
            velocity: [0.; 3],
            gravity: 0.,
            drag: 0.,
            fade: [0.1, 0.3],
            erode: [0.; 2],
            erode_soft: 0.18,
            soft: 0.25,
            cool: 0.,
            delay: [0.; 2],
            fps: 0.,
            pulse: [0.; 2],
            towards: 0.,
            scaled: false,
            world: false,
            shape: DecalShape::Texture,
            edge: 0.15,
            fill: [0.; 4],
            repeat: 0.5,
            scroll: [0.; 2],
            mesh: None,
            upright: false,
            rim: [2., 1.],
            mask: [0., 0.],
            twinkle: [0., 1.],
            texture: 1.,
        }
    }
}

impl EffectCatalog {
    /// An effect's layers, following `like` (to any depth) and swapping its ramps.
    pub fn layers(&self, name: &str) -> Option<Vec<Layer>> {
        let mut swaps: Vec<&BTreeMap<String, String>> = Vec::new();
        let mut at = name;
        for _ in 0..8 {
            match self.effects.get(at)? {
                Effect::Layers(layers) => {
                    let mut out = layers.clone();
                    for layer in &mut out {
                        // The nearest `like` wins: apply the outermost swaps last.
                        for swap in swaps.iter().rev() {
                            if let Some(r) = layer.ramp.as_ref().and_then(|r| swap.get(r)) {
                                layer.ramp = Some(r.clone());
                            }
                        }
                    }
                    return Some(out);
                }
                Effect::Like { like, ramps } => {
                    swaps.push(ramps);
                    at = like;
                }
            }
        }
        None
    }

    /// Every effect a missile, cast, aura, field, school or warning names, with what names it.
    fn named(&self) -> Vec<(String, &str)> {
        let mut out = Vec::new();
        for (id, p) in &self.projectiles {
            out.push((format!("projectile {id}"), p.flight.as_str()));
            out.push((format!("projectile {id} impact"), p.impact.as_str()));
            if let Some(t) = &p.trail {
                out.push((format!("projectile {id} trail"), t.as_str()));
            }
        }
        for (k, e) in &self.casts {
            out.push((format!("cast {k}"), e.as_str()));
        }
        for (k, e) in &self.auras {
            out.push((format!("aura {k}"), e.as_str()));
        }
        for (k, e) in &self.fields {
            out.push((format!("field {k}"), e.as_str()));
        }
        for (k, s) in &self.schools {
            out.push((format!("school {k} field"), s.field.as_str()));
            out.push((format!("school {k} impact"), s.impact.as_str()));
        }
        out.push(("hostile warnings".into(), self.warnings.hostile.as_str()));
        out.push(("friendly warnings".into(), self.warnings.friendly.as_str()));
        out.push(("combos".into(), self.combo.as_str()));
        out.push(("aiming".into(), self.aim.as_str()));
        out
    }

    /// Everything wrong with the effects table that can be seen without its art files.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (what, name) in self.named() {
            if !self.effects.contains_key(name) {
                out.push(format!("effects: {what} plays {name}, which isn't defined"));
            }
        }
        for school in ["physical", "fire", "frost", "arcane", "nature", "holy", "shadow"] {
            if !self.schools.contains_key(school) {
                out.push(format!("effects: school {school} has no look"));
            }
        }
        for (name, ramp) in &self.ramps {
            if ramp.len() < 2 || ramp.iter().flatten().any(|v| !v.is_finite() || *v < 0.) {
                out.push(format!("effects: ramp {name} needs two or more stops of finite, non-negative values"));
            }
        }
        for (name, effect) in &self.effects {
            let Some(layers) = self.layers(name) else {
                out.push(format!("effects: {name} is like an effect that isn't defined (or loops)"));
                continue;
            };
            if let Effect::Like { ramps, .. } = effect {
                for to in ramps.values() {
                    if !self.ramps.contains_key(to) {
                        out.push(format!("effects: {name} swaps in ramp {to}, which isn't defined"));
                    }
                }
            }
            for (i, l) in layers.iter().enumerate() {
                let what = format!("effects: {name} layer {i}");
                if let Some(r) = &l.ramp
                    && !self.ramps.contains_key(r)
                {
                    out.push(format!("{what} uses ramp {r}, which isn't defined"));
                }
                if let Some(t) = &l.tile
                    && !self.tiles.contains_key(t)
                {
                    out.push(format!("{what} uses tile {t}, which isn't defined"));
                }
                match l.kind {
                    LayerKind::Mesh => match &l.mesh {
                        Some(m) if self.meshes.contains_key(m) => {}
                        _ => out.push(format!("{what} draws no mesh the catalogue has")),
                    },
                    LayerKind::Decal if l.shape == DecalShape::Area => {}
                    _ if l.frame.is_none() && l.tile.is_none() => {
                        out.push(format!("{what} has neither a frame nor a tile"));
                    }
                    _ => {}
                }
                if l.kind == LayerKind::Trail && l.rate <= 0. {
                    out.push(format!("{what} is a trail dropping nothing (rate 0)"));
                }
                let numbers = l
                    .colour
                    .iter()
                    .chain(&l.size)
                    .chain(&l.fill)
                    .chain([&l.alpha, &l.life, &l.rate, &l.aspect, &l.length, &l.soft, &l.edge]);
                if numbers.clone().any(|v| !v.is_finite() || *v < 0.) {
                    out.push(format!("{what} has a negative or non-finite size, colour or time"));
                }
                if !(0.0..=1.0).contains(&l.blend) || !(0.0..=1.0).contains(&l.cool) {
                    out.push(format!("{what}: blend and cool must be 0…1"));
                }
                if l.fade.iter().any(|f| !(0.0..=1.0).contains(f)) {
                    out.push(format!("{what}: fade shares must be 0…1"));
                }
            }
        }
        for (id, p) in &self.projectiles {
            if let Some(m) = &p.mesh
                && !self.meshes.contains_key(m)
            {
                out.push(format!("effects: projectile {id} carries mesh {m}, which isn't defined"));
            }
        }
        out
    }
}

impl EffectCatalog {
    /// The atlas index (`frames`), from the asset root.
    pub fn atlas_index(&self, root: &Path) -> Result<AtlasIndex, crate::Error> {
        crate::read_json(&root.join(&self.frames))
    }

    /// Problems only the art files show: a file that isn't there, or a frame the atlas lacks.
    pub fn art_problems(&self, root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let files = [&self.atlas, &self.frames]
            .into_iter()
            .chain(self.tiles.values())
            .chain(self.meshes.values());
        for f in files {
            if !root.join(f).exists() {
                out.push(format!("effects: {f} is missing"));
            }
        }
        let index = match self.atlas_index(root) {
            Ok(i) => i,
            Err(e) => {
                out.push(format!("effects: the atlas index won't load: {e}"));
                return out;
            }
        };
        for (name, f) in &index.frames {
            let [x, y, w, h] = f.rect;
            if x + w > index.size[0] || y + h > index.size[1] || f.count == 0 || f.cols == 0 {
                out.push(format!("effects: atlas frame {name} doesn't fit the atlas"));
            }
        }
        for name in self.effects.keys() {
            for (i, l) in self.layers(name).unwrap_or_default().iter().enumerate() {
                if let Some(f) = &l.frame
                    && !index.frames.contains_key(f)
                {
                    out.push(format!("effects: {name} layer {i} uses frame {f}, which the atlas lacks"));
                }
            }
        }
        out
    }
}

/// The atlas index written beside the atlas by the Blender build.
#[derive(Debug, Clone, Deserialize)]
pub struct AtlasIndex {
    pub size: [u32; 2],
    pub frames: BTreeMap<String, AtlasFrame>,
}

/// A sprite, or a flipbook whose `count` frames fill `rect` in `cols` columns, left to right
/// and top to bottom.
#[derive(Debug, Clone, Deserialize)]
pub struct AtlasFrame {
    pub rect: [u32; 4],
    pub count: u32,
    pub cols: u32,
}
