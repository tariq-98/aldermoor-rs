//! Combat drawn in the world from `assets/catalog/effects.json`: missiles in flight and the
//! trails they leave, casts, auras, warnings and fields on the ground, and the bursts where
//! things land. The table names an effect for each; an effect is layers of authored art
//! (`assets/effects`, built in Blender) placed and animated by the table's numbers. This module
//! only works out where each effect is, which way it faces and how old it is, and turns layers
//! into the renderer's effect instances (`render::fx`).
//!
//! Particles are worked out afresh every frame from a seed and an age, so a capture at a given
//! frame always shows the same thing and nothing needs simulating between frames.
use anyhow::{Context, Result};
use assets::{AtlasFrame, DecalShape, EffectCatalog, Layer, LayerKind};
use glam::{Mat4, Quat, Vec2, Vec3};
use protocol::{AreaView, CombatView, Event, School, Shape, UnitState};
use render::{
    character::Draw,
    fx,
    model::{Model, ModelCache, Texture},
};
use std::{collections::HashMap, f32::consts::TAU, path::Path, sync::Arc};

/// Where an effect plays and how far along it is.
#[derive(Clone, Copy)]
struct Anchor {
    origin: Vec3,
    /// +z forward, +y up.
    turn: Quat,
    /// Metres a second, for particles left behind and streaks.
    velocity: Vec3,
    /// What `scaled` layers scale by: an area's reach, a unit's height.
    scale: f32,
    age: f32,
    seed: u32,
    /// Fades the whole effect (an aura fading in or out).
    strength: f32,
    /// A cast's or warning's progress (0…1), or negative for none.
    progress: f32,
    shape: Option<Shape>,
    /// The ground under it, for its marks.
    ground: Ground,
}

impl Anchor {
    fn at(origin: Vec3, turn: Quat, age: f32, seed: u32) -> Self {
        Self {
            origin,
            turn,
            velocity: Vec3::ZERO,
            scale: 1.,
            age,
            seed,
            strength: 1.,
            progress: -1.,
            shape: None,
            ground: Ground::flat(origin.y),
        }
    }
}

/// The ground under a mark: the plane that best fits it (its height at the anchor and its rise
/// a metre along x and z) and how far the real ground strays from that plane.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ground {
    y: f32,
    slope: Vec2,
    stray: f32,
}

impl Ground {
    fn flat(y: f32) -> Self {
        Self {
            y,
            slope: Vec2::ZERO,
            stray: 0.,
        }
    }

    /// Fits the ground within `reach` of `centre` from a centre sample and two even rings,
    /// which makes the least-squares plane simple: the mean height, and each slope from its
    /// own axis alone.
    fn fit(centre: Vec3, reach: f32, ground: &dyn Fn(f32, f32) -> f32) -> Self {
        let mut points = vec![(Vec2::ZERO, ground(centre.x, centre.z))];
        for k in 0..12 {
            let t = k as f32 / 12. * TAU;
            for r in [0.5, 1.] {
                let d = Vec2::new(t.sin(), t.cos()) * reach * r;
                points.push((d, ground(centre.x + d.x, centre.z + d.y)));
            }
        }
        let n = points.len() as f32;
        let y = points.iter().map(|p| p.1).sum::<f32>() / n;
        let along = |axis: fn(Vec2) -> f32| {
            let num: f32 = points.iter().map(|(d, h)| axis(*d) * (h - y)).sum();
            let den: f32 = points.iter().map(|(d, _)| axis(*d) * axis(*d)).sum();
            if den > 1e-6 { num / den } else { 0. }
        };
        let slope = Vec2::new(along(|d| d.x), along(|d| d.y));
        let stray = points
            .iter()
            .map(|(d, h)| (h - (y + slope.dot(*d))).abs())
            .fold(0., f32::max);
        Self { y, slope, stray }
    }
}

/// Which art a layer draws with.
#[derive(Clone, Copy)]
enum Art {
    Atlas {
        origin: Vec2,
        cell: Vec2,
        count: u32,
        cols: u32,
    },
    Tile(u32),
    None,
}

/// A layer with its art looked up.
struct Part {
    layer: Layer,
    art: Art,
    ramp: f32,
    mesh: Option<usize>,
}

type Look = Arc<[Part]>;

/// A missile in flight: what it is, when it was first seen, and where it has been.
struct Flight {
    visual: String,
    born: f32,
    seed: u32,
    /// (position, scene time, metres flown).
    path: Vec<(Vec3, f32, f32)>,
    velocity: Vec3,
}

/// A one-shot effect (an impact, a combo) or what's left of a trail after its missile landed.
struct Burst {
    look: Look,
    anchor: Anchor,
    born: f32,
    /// Seconds it lasts.
    span: f32,
    trail: Option<Flight>,
}

pub struct Effects {
    table: EffectCatalog,
    looks: HashMap<String, Look>,
    /// Every name the art is indexed by, and how to draw it.
    atlas: HashMap<String, AtlasFrame>,
    atlas_size: Vec2,
    tiles: HashMap<String, u32>,
    ramps: HashMap<String, u32>,
    meshes: HashMap<String, usize>,
    /// Lit meshes missiles carry, by catalogue mesh name.
    solids: HashMap<String, Arc<Model>>,
    flights: HashMap<u32, Flight>,
    bursts: Vec<Burst>,
    next_seed: u32,
}

/// Where to draw and what's around: everything an effect needs from the rest of the frame.
pub struct Scene<'a> {
    pub fight: &'a CombatView,
    pub units: &'a [UnitState],
    /// A unit's drawn height in metres.
    pub height: &'a dyn Fn(u32) -> f32,
    /// The walkable surface at (x, z).
    pub ground: &'a dyn Fn(f32, f32) -> f32,
    /// Seconds since the combat view was taken, to carry missiles and casts forward.
    pub since: f32,
    /// Seconds, steadily increasing: drives every animation.
    pub time: f32,
    /// A ground skill being aimed: where, and its radius.
    pub aim: Option<(Vec3, f32)>,
}

/// Instance ids for motion vectors: the top bits keep effects apart from units (id << 32),
/// scenery (0x4…) and terrain (0x8…).
const EFFECT: u64 = 0x2000_0000_0000_0000;

/// Scrambles bits: the same number in, the same out, with nearby numbers far apart.
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A stream of numbers in 0…1 from a seed.
struct Dice(u32);

impl Dice {
    fn roll(&mut self) -> f32 {
        self.0 = hash(self.0.wrapping_add(0x9e37_79b9));
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    fn between(&mut self, range: [f32; 2]) -> f32 {
        range[0] + (range[1] - range[0]) * self.roll()
    }

    /// A direction spread evenly over the sphere.
    fn direction(&mut self) -> Vec3 {
        let y = self.roll() * 2. - 1.;
        let a = self.roll() * TAU;
        let r = (1. - y * y).max(0.).sqrt();
        Vec3::new(a.cos() * r, y, a.sin() * r)
    }

    /// A point in the unit ball.
    fn ball(&mut self) -> Vec3 {
        self.direction() * self.roll().cbrt()
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Fading in over the first `fade[0]` of a life and out over the last `fade[1]`.
fn envelope(t: f32, fade: [f32; 2]) -> f32 {
    let rise = if fade[0] > 0. {
        smoothstep(0., fade[0], t)
    } else {
        1.
    };
    let fall = if fade[1] > 0. {
        1. - smoothstep(1. - fade[1], 1., t)
    } else {
        1.
    };
    rise * fall
}

/// A ramp's stops baked into one row.
fn bake(stops: &[[f32; 4]]) -> [[f32; 4]; fx::RAMP_WIDTH] {
    let mut sorted = stops.to_vec();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
    std::array::from_fn(|i| {
        let x = i as f32 / (fx::RAMP_WIDTH - 1) as f32;
        let hi = sorted.partition_point(|s| s[0] <= x).min(sorted.len() - 1);
        let lo = hi.saturating_sub(1);
        let (a, b) = (sorted[lo], sorted[hi]);
        let t = if b[0] > a[0] {
            ((x - a[0]) / (b[0] - a[0])).clamp(0., 1.)
        } else {
            1.
        };
        [
            lerp(a[1], b[1], t),
            lerp(a[2], b[2], t),
            lerp(a[3], b[3], t),
            1.,
        ]
    })
}

fn school_name(school: School) -> String {
    format!("{school:?}").to_lowercase()
}

impl Effects {
    /// Reads the effects' art from `root` and returns the table ready to play, with the art the
    /// renderer needs uploaded once (`Renderer::load_effects`).
    pub fn load(
        table: EffectCatalog,
        root: &Path,
        cache: &mut ModelCache,
    ) -> Result<(Self, fx::Library)> {
        let index = table
            .atlas_index(root)
            .map_err(|e| anyhow::anyhow!("effects atlas index: {e}"))?;
        let atlas = Texture::load(&root.join(&table.atlas)).context("effects atlas")?;
        let mut tiles = Vec::new();
        let mut tile_index = HashMap::new();
        for (i, (name, path)) in table.tiles.iter().enumerate() {
            tiles.push(
                Texture::load(&root.join(path)).with_context(|| format!("effect tile {name}"))?,
            );
            tile_index.insert(name.clone(), i as u32);
        }
        let ramps: Vec<_> = table.ramps.values().map(|stops| bake(stops)).collect();
        let ramp_index = table
            .ramps
            .keys()
            .enumerate()
            .map(|(i, k)| (k.clone(), i as u32))
            .collect();
        let mut meshes = Vec::new();
        let mut mesh_index = HashMap::new();
        for (i, (name, path)) in table.meshes.iter().enumerate() {
            meshes.push(
                cache
                    .get(&root.join(path))
                    .with_context(|| format!("effect mesh {name}"))?,
            );
            mesh_index.insert(name.clone(), i);
        }
        let mut solids = HashMap::new();
        for p in table.projectiles.values() {
            if let Some(m) = &p.mesh
                && let Some(&i) = mesh_index.get(m)
            {
                solids.insert(m.clone(), meshes[i].clone());
            }
        }
        let fx = Self {
            atlas_size: Vec2::new(index.size[0] as f32, index.size[1] as f32),
            atlas: index.frames.into_iter().collect(),
            tiles: tile_index,
            ramps: ramp_index,
            meshes: mesh_index,
            table,
            looks: HashMap::new(),
            solids,
            flights: HashMap::new(),
            bursts: Vec::new(),
            next_seed: 1,
        };
        let library = fx::Library {
            atlas,
            tiles,
            ramps,
            meshes,
        };
        Ok((fx, library))
    }

    /// An effect's layers with their art looked up, built once.
    fn look(&mut self, name: &str) -> Option<Look> {
        if let Some(l) = self.looks.get(name) {
            return Some(l.clone());
        }
        let layers = self.table.layers(name)?;
        let parts: Vec<Part> = layers
            .into_iter()
            .map(|layer| {
                let art = if let Some(f) = layer.frame.as_ref().and_then(|f| self.atlas.get(f)) {
                    let rows = f.count.div_ceil(f.cols.max(1)).max(1);
                    let [x, y, w, h] = f.rect.map(|v| v as f32);
                    Art::Atlas {
                        origin: Vec2::new(x, y) / self.atlas_size,
                        cell: Vec2::new(w / f.cols.max(1) as f32, h / rows as f32)
                            / self.atlas_size,
                        count: f.count.max(1),
                        cols: f.cols.max(1),
                    }
                } else if let Some(&t) = layer.tile.as_ref().and_then(|t| self.tiles.get(t)) {
                    Art::Tile(t)
                } else {
                    Art::None
                };
                let ramp = layer
                    .ramp
                    .as_ref()
                    .and_then(|r| self.ramps.get(r))
                    .map_or(-1., |&r| r as f32);
                let mesh = layer
                    .mesh
                    .as_ref()
                    .and_then(|m| self.meshes.get(m))
                    .copied();
                Part {
                    layer,
                    art,
                    ramp,
                    mesh,
                }
            })
            .collect();
        let look: Look = parts.into();
        self.looks.insert(name.into(), look.clone());
        Some(look)
    }

    fn seed(&mut self) -> u32 {
        self.next_seed = self.next_seed.wrapping_add(1);
        hash(self.next_seed)
    }

    fn burst(&mut self, effect: &str, at: Vec3, turn: Quat, scale: f32, time: f32) {
        let Some(look) = self.look(effect) else {
            return;
        };
        let span = look
            .iter()
            .map(|p| p.layer.life.max(0.3) + p.layer.delay[1])
            .fold(0.3, f32::max);
        let mut anchor = Anchor::at(at, turn, 0., self.seed());
        anchor.scale = scale;
        self.bursts.push(Burst {
            look,
            anchor,
            born: time,
            span,
            trail: None,
        });
        // Bounded however busy the fight gets.
        if self.bursts.len() > 96 {
            self.bursts.remove(0);
        }
    }

    /// Notes launches and starts whatever plays when something lands.
    pub fn event(&mut self, event: &Event, time: f32) {
        match event {
            Event::ProjectileLaunched { id, visual, .. } => {
                let seed = self.seed();
                self.flights.entry(*id).or_insert(Flight {
                    visual: visual.clone(),
                    born: time,
                    seed,
                    path: Vec::new(),
                    velocity: Vec3::ZERO,
                });
            }
            Event::ProjectileEnded { id, at, .. } => {
                let Some(mut flight) = self.flights.remove(id) else {
                    return;
                };
                let Some(look) = self.table.projectiles.get(&flight.visual).cloned() else {
                    return;
                };
                let turn = Quat::from_rotation_arc(Vec3::Z, flight.velocity.normalize_or(Vec3::Z));
                self.burst(&look.impact, *at, turn, 1., time);
                // What it left behind lives on after it lands.
                if let Some(trail) = look.trail.as_deref().and_then(|t| self.look(t)) {
                    let last = flight.path.last().map_or(0., |p| p.2);
                    flight.path.push((
                        *at,
                        time,
                        last + flight.path.last().map_or(0., |p| p.0.distance(*at)),
                    ));
                    let span = trail.iter().map(|p| p.layer.life).fold(0., f32::max);
                    let anchor = Anchor::at(*at, turn, 0., flight.seed);
                    self.bursts.push(Burst {
                        look: trail,
                        anchor,
                        born: time,
                        span,
                        trail: Some(flight),
                    });
                }
            }
            Event::AreaImpact {
                at, radius, school, ..
            } => {
                if let Some(s) = self.table.schools.get(&school_name(*school)) {
                    let effect = s.impact.clone();
                    self.burst(&effect, *at, Quat::IDENTITY, *radius, time);
                }
            }
            Event::Combo { at, .. } => {
                let effect = self.table.combo.clone();
                self.burst(&effect, *at, Quat::IDENTITY, 2.5, time);
            }
            _ => {}
        }
    }

    /// This frame's effects: lit meshes carried by missiles (drawn with the scene), and
    /// everything else for the effects pass.
    pub fn draws(&mut self, s: &Scene) -> (Vec<Draw>, fx::Frame) {
        let mut out = fx::Frame {
            time: s.time,
            ..Default::default()
        };
        let mut solids = Vec::new();
        let by_id: HashMap<_, _> = s.units.iter().map(|u| (u.id, u)).collect();
        // Missiles that left view without an end event aren't remembered for long.
        let live: std::collections::HashSet<_> = s.fight.projectiles.iter().map(|p| p.id).collect();
        self.flights
            .retain(|id, f| live.contains(id) || s.time - f.born < 1.);
        self.bursts.retain(|b| s.time - b.born < b.span);
        for p in &s.fight.projectiles {
            let Some(look) = self.table.projectiles.get(&p.visual).cloned() else {
                continue;
            };
            let at = p.position + p.velocity * s.since.clamp(0., 0.1);
            let seed = self.seed();
            let flight = self.flights.entry(p.id).or_insert(Flight {
                visual: p.visual.clone(),
                born: s.time,
                seed,
                path: Vec::new(),
                velocity: p.velocity,
            });
            flight.velocity = p.velocity;
            match flight.path.last() {
                Some(&(last, _, flown)) if last.distance(at) > 0.02 => {
                    flight.path.push((at, s.time, flown + last.distance(at)));
                }
                None => flight.path.push((at, s.time, 0.)),
                _ => {}
            }
            // Only the part of the path a trail can still show is kept.
            let keep = s.time - 3.;
            let old = flight
                .path
                .partition_point(|q| q.1 < keep)
                .saturating_sub(1);
            flight.path.drain(..old);
            let (born, fseed) = (flight.born, flight.seed);
            let dir = p.velocity.normalize_or(Vec3::Z);
            let turn = Quat::from_rotation_arc(Vec3::Z, dir);
            let mut anchor = Anchor::at(at, turn, s.time - born, fseed);
            anchor.velocity = p.velocity;
            // A missile's marks are the light it throws on the ground under it.
            anchor.ground = Ground::flat((s.ground)(at.x, at.z));
            if let Some(f) = self.look(&look.flight) {
                for part in f.iter() {
                    emit(part, &anchor, false, &mut out, s);
                }
            }
            if let Some(t) = look.trail.as_deref().and_then(|t| self.look(t))
                && let Some(flight) = self.flights.get(&p.id)
            {
                for part in t.iter() {
                    trail(part, flight, &anchor, s.time, &mut out);
                }
            }
            if let Some(model) = look.mesh.as_ref().and_then(|m| self.solids.get(m)) {
                let spin = Quat::from_rotation_x((s.time - born) * look.spin * TAU);
                let transform = Mat4::from_rotation_translation(turn * spin, at)
                    * Mat4::from_translation(-Vec3::from(look.pivot));
                for (i, prim) in model.primitives.iter().enumerate() {
                    solids.push(Draw {
                        dye: None,
                        base_map: None,
                        instance: EFFECT | ((p.id as u64) << 8) | i as u64,
                        model: model.clone(),
                        primitive: i,
                        transform: transform * model.rest_world[prim.node],
                        palette: Arc::from([]),
                        hidden: 0,
                        base: [1.; 4],
                        morph: [0.; 2],
                    });
                }
            }
        }
        for a in &s.fight.areas {
            let effect = if a.warning {
                Some(if a.hostile {
                    self.table.warnings.hostile.clone()
                } else {
                    self.table.warnings.friendly.clone()
                })
            } else {
                a.field
                    .as_ref()
                    .and_then(|f| {
                        let hostile = a
                            .hostile
                            .then(|| self.table.fields.get(&format!("{f}.hostile")));
                        hostile.flatten().or_else(|| self.table.fields.get(f))
                    })
                    .or_else(|| {
                        self.table
                            .schools
                            .get(&school_name(a.school))
                            .map(|s| &s.field)
                    })
                    .cloned()
            };
            let Some(look) = effect.and_then(|e| self.look(&e)) else {
                continue;
            };
            let anchor = area_anchor(a, s);
            for part in look.iter() {
                emit(part, &anchor, false, &mut out, s);
            }
        }
        if let Some((at, radius)) = s.aim
            && let Some(look) = self.look(&self.table.aim.clone())
        {
            let area = AreaView {
                id: u32::MAX,
                warning: false,
                shape: Shape::Circle { radius },
                center: at,
                yaw: 0.,
                elapsed: s.time,
                duration: 0.,
                hostile: false,
                school: School::Physical,
                field: None,
            };
            let anchor = area_anchor(&area, s);
            for part in look.iter() {
                emit(part, &anchor, false, &mut out, s);
            }
        }
        for c in &s.fight.units {
            let Some(u) = by_id.get(&c.id) else { continue };
            if u.health <= 0. {
                continue;
            }
            let h = (s.height)(u.id);
            let feet = Vec3::new(
                u.position.x,
                (s.ground)(u.position.x, u.position.z),
                u.position.z,
            );
            let turn = Quat::from_rotation_y(u.yaw);
            let under = Ground::fit(feet, h * 0.6, s.ground);
            if let Some(cast) = &c.cast
                && let Some(effect) = self.table.casts.get(&cast.ability).cloned()
                && let Some(look) = self.look(&effect)
            {
                let elapsed = cast.elapsed + s.since;
                let mut anchor = Anchor::at(feet, turn, elapsed, hash(u.id ^ 0x5eed));
                anchor.scale = h;
                anchor.ground = under;
                anchor.progress = if cast.channel {
                    1.
                } else {
                    (elapsed / cast.duration.max(0.001)).clamp(0., 1.)
                };
                for part in look.iter() {
                    emit(part, &anchor, false, &mut out, s);
                }
            }
            for (i, aura) in c.auras.iter().enumerate().take(16) {
                let Some(effect) = self.table.auras.get(&aura.key).cloned() else {
                    continue;
                };
                let Some(look) = self.look(&effect) else {
                    continue;
                };
                const FADE: f32 = 0.35;
                let shown = aura.duration - aura.remaining;
                let fade = if aura.duration > 0. {
                    (shown / FADE).clamp(0., 1.) * (aura.remaining / FADE).clamp(0., 1.)
                } else {
                    1.
                };
                let mut anchor =
                    Anchor::at(feet, turn, s.time, hash(u.id.wrapping_mul(31) ^ i as u32));
                anchor.scale = h;
                anchor.ground = under;
                anchor.strength = fade;
                for part in look.iter() {
                    emit(part, &anchor, false, &mut out, s);
                }
            }
        }
        for b in &self.bursts {
            let mut anchor = b.anchor;
            anchor.age = s.time - b.born;
            match &b.trail {
                Some(flight) => {
                    for part in b.look.iter() {
                        trail(part, flight, &anchor, s.time, &mut out);
                    }
                }
                None => {
                    for part in b.look.iter() {
                        emit(part, &anchor, true, &mut out, s);
                    }
                }
            }
        }
        (solids, out)
    }
}

/// An area's anchor: on the ground at its centre, facing its way, sized by its reach, with
/// the ground under it for its marks.
fn area_anchor(a: &AreaView, s: &Scene) -> Anchor {
    let reach = match a.shape {
        Shape::Circle { radius } => radius,
        Shape::Cone { range, .. } => range,
        Shape::Line { length, .. } => length,
    };
    let turn = Quat::from_rotation_y(a.yaw);
    let y = (s.ground)(a.center.x, a.center.z).max(a.center.y);
    let mut anchor = Anchor::at(
        Vec3::new(a.center.x, y, a.center.z),
        turn,
        a.elapsed + s.since,
        hash(a.id ^ 0xa4ea),
    );
    anchor.scale = reach;
    anchor.shape = Some(a.shape);
    anchor.ground = Ground::fit(a.center, reach, s.ground);
    if a.warning {
        anchor.progress = ((a.elapsed + s.since) / a.duration.max(0.001)).clamp(0., 1.);
    }
    anchor
}

/// One particle's life: how far through it is (0…1), seconds into it, and its own seed.
struct Life {
    t: f32,
    seconds: f32,
    seed: u32,
    steady: bool,
    /// Which of the layer's particles it is.
    index: u32,
}

/// The lives of a layer's particles at the anchor's age. Ongoing effects cycle them, staggered;
/// one-shot effects play each once, after its delay.
fn lives(l: &Layer, a: &Anchor, once: bool) -> Vec<Life> {
    (0..l.count.max(1))
        .filter_map(|i| {
            let base = hash(a.seed ^ hash(i.wrapping_add(1).wrapping_mul(0x632b_e5ab)));
            let mut dice = Dice(base);
            if l.life <= 0. {
                // Steady: a cast's or warning's progress carries it from birth to death.
                return Some(Life {
                    t: a.progress.max(0.),
                    seconds: a.age,
                    seed: base,
                    steady: true,
                    index: i,
                });
            }
            if once {
                let delay = dice.between(l.delay);
                let seconds = a.age - delay;
                return (seconds >= 0. && seconds < l.life).then_some(Life {
                    t: seconds / l.life,
                    seconds,
                    seed: base,
                    steady: false,
                    index: i,
                });
            }
            let x = a.age / l.life + dice.roll();
            let cycle = x.floor();
            Some(Life {
                t: x - cycle,
                seconds: (x - cycle) * l.life,
                seed: hash(base ^ (cycle as i64 as u32).wrapping_mul(0x9e37_79b9)),
                steady: false,
                index: i,
            })
        })
        .collect()
}

/// A particle: where it is, which way and how fast it's going, its size, turn and opacity.
struct Particle {
    at: Vec3,
    velocity: Vec3,
    size: f32,
    angle: f32,
    alpha: f32,
}

fn particle(l: &Layer, a: &Anchor, life: &Life) -> Particle {
    let s = if l.scaled { a.scale } else { 1. };
    let mut dice = Dice(life.seed ^ 0x51ab);
    let spread = Vec3::from(l.spread) * s;
    let (born, out) = if l.ring || l.orbit != 0. {
        let t = if l.orbit != 0. {
            (life.index as f32 / l.count.max(1) as f32) * TAU + l.orbit * a.age
        } else {
            dice.roll() * TAU
        };
        let rim = Vec3::new(t.cos(), 0., t.sin());
        (
            rim * spread + Vec3::Y * (dice.roll() * 2. - 1.) * spread.y,
            rim,
        )
    } else {
        let p = dice.ball();
        (p * spread, dice.direction())
    };
    let born = Vec3::from(l.offset) * s + born;
    let start = Vec3::from(l.velocity) * s + out * dice.between(l.speed) * s;
    let t = life.seconds;
    let drift = if l.drag > 0. {
        (1. - (-l.drag * t).exp()) / l.drag
    } else {
        t
    };
    let mut at = a.origin + a.turn * (born + start * drift) - Vec3::Y * 0.5 * l.gravity * t * t;
    let mut velocity = a.turn * (start * (-l.drag * t).exp()) - Vec3::Y * l.gravity * t;
    if l.world {
        // Left where it was born while the anchor moved on.
        at -= a.velocity * t;
    } else {
        velocity += a.velocity;
    }
    // Particles that spin start at any angle; the rest stand upright (flames rise).
    let turning = dice.between(l.spin) * if dice.roll() < 0.5 { -1. } else { 1. };
    let start = if life.steady || l.spin == [0., 0.] {
        0.
    } else {
        dice.roll() * TAU
    };
    let angle = l.angle + start + turning * if life.steady { a.age } else { t };
    let pulse = 1. + l.pulse[0] * (TAU * l.pulse[1] * a.age + dice.roll() * TAU).sin();
    let fade = if life.steady {
        1.
    } else {
        envelope(life.t, l.fade)
    };
    let vary = 1. + (dice.roll() * 2. - 1.) * l.vary;
    Particle {
        at,
        velocity,
        size: lerp(l.size[0], l.size[1], life.t) * s * vary,
        angle,
        alpha: l.alpha * fade * a.strength * pulse.max(0.),
    }
}

impl Part {
    /// The material for a particle `t` through a life `seconds` long so far.
    fn material(&self, t: f32, seconds: f32, alpha: f32, tile: [f32; 4]) -> fx::Material {
        let l = &self.layer;
        let (frame, next, source) = match self.art {
            Art::Atlas {
                origin,
                cell,
                count,
                cols,
            } => {
                let at = |i: u32| origin + Vec2::new((i % cols) as f32, (i / cols) as f32) * cell;
                let (i, j, blend) = if count <= 1 {
                    (0, 0, 0.)
                } else if l.fps > 0. {
                    let x = (seconds * l.fps).rem_euclid(count as f32);
                    let i = (x as u32).min(count - 1);
                    (i, (i + 1) % count, x - i as f32)
                } else {
                    let x = (t * count as f32).min(count as f32 - 1.001);
                    let i = x as u32;
                    (i, (i + 1).min(count - 1), x - i as f32)
                };
                let (a, b) = (at(i), at(j));
                ([a.x, a.y, cell.x, cell.y], [b.x, b.y, blend, self.ramp], 0.)
            }
            Art::Tile(layer) => (tile, [0., 0., 0., self.ramp], layer as f32 + 1.),
            Art::None => ([0.; 4], [0., 0., 0., self.ramp], 0.),
        };
        let eroding = l.erode != [0., 0.];
        fx::Material {
            colour: [l.colour[0], l.colour[1], l.colour[2], alpha],
            frame,
            next,
            params: [
                lerp(l.erode[0], l.erode[1], t),
                if eroding { l.erode_soft.max(0.01) } else { 0. },
                l.soft,
                l.blend,
            ],
            extra: [source, 0., 0., 1. - l.cool * t],
        }
    }
}

/// Puts one layer of an effect into the frame.
fn emit(part: &Part, a: &Anchor, once: bool, out: &mut fx::Frame, s: &Scene) {
    match part.layer.kind {
        LayerKind::Sprite => {
            for life in lives(&part.layer, a, once) {
                let p = particle(&part.layer, a, &life);
                if p.alpha > 0.002 {
                    sprite(part, &p, &life, out);
                }
            }
        }
        LayerKind::Beam => beam(part, a, out),
        LayerKind::Decal => {
            for life in lives(&part.layer, a, once) {
                decal(part, a, &life, out, s);
            }
        }
        LayerKind::Mesh => {
            for life in lives(&part.layer, a, once) {
                mesh(part, a, &life, out);
            }
        }
        // Trails need a path: see `trail`.
        LayerKind::Trail => {}
    }
}

fn sprite(part: &Part, p: &Particle, life: &Life, out: &mut fx::Frame) {
    let l = &part.layer;
    let material = part.material(life.t, life.seconds, p.alpha, [0., 0., 1., 1.]);
    let speed = p.velocity.length();
    out.quads.push(if l.stretch > 0. && speed > 0.05 {
        let tail = p.at - p.velocity * l.stretch;
        fx::Quad {
            position: [tail.x, tail.y, tail.z, fx::Quad::BEAM],
            a: [p.at.x, p.at.y, p.at.z, p.size],
            b: [p.size * 0.35, 0., 0., 0.],
            material,
        }
    } else {
        fx::Quad {
            position: [p.at.x, p.at.y, p.at.z, fx::Quad::SPRITE],
            a: [p.size, p.size * l.aspect, p.angle, 0.],
            b: [0., 0., 0., l.towards],
            material,
        }
    });
}

/// A beam from the anchor back along its flight, no longer than the flight so far.
fn beam(part: &Part, a: &Anchor, out: &mut fx::Frame) {
    let l = &part.layer;
    let s = if l.scaled { a.scale } else { 1. };
    let forward = a.turn * Vec3::Z;
    let head = a.origin + a.turn * (Vec3::from(l.offset) * s);
    let flown = a.velocity.length() * a.age;
    let length = (l.length * s).min(flown.max(0.05));
    let tail = head - forward * length;
    let tile = [
        -a.age * l.scroll[0],
        a.age * l.scroll[1],
        length * l.repeat,
        1.,
    ];
    let pulse = 1. + l.pulse[0] * (TAU * l.pulse[1] * a.age).sin();
    let material = part.material(0., a.age, l.alpha * a.strength * pulse, tile);
    out.quads.push(fx::Quad {
        position: [tail.x, tail.y, tail.z, fx::Quad::BEAM],
        a: [head.x, head.y, head.z, l.size[0] * s],
        b: [l.size[1] * s, 0., 0., 0.],
        material,
    });
}

/// Sprites dropped along a missile's path every `1 / rate` metres, counted from where it
/// started so each stays put from frame to frame.
fn trail(part: &Part, flight: &Flight, a: &Anchor, now: f32, out: &mut fx::Frame) {
    let l = &part.layer;
    if l.kind != LayerKind::Trail || l.rate <= 0. || flight.path.len() < 2 || l.life <= 0. {
        return;
    }
    let spacing = 1. / l.rate;
    let path = &flight.path;
    let oldest = now - l.life;
    let first = path.partition_point(|p| p.1 < oldest).saturating_sub(1);
    let from = (path[first].2 / spacing).ceil() as i64;
    let to = (path[path.len() - 1].2 / spacing).floor() as i64;
    let mut k = first;
    for n in from..=to {
        let d = n as f32 * spacing;
        while k + 1 < path.len() && path[k + 1].2 < d {
            k += 1;
        }
        let (p0, t0, d0) = path[k];
        let (p1, t1, d1) = path[(k + 1).min(path.len() - 1)];
        let f = if d1 > d0 {
            ((d - d0) / (d1 - d0)).clamp(0., 1.)
        } else {
            0.
        };
        let at = p0.lerp(p1, f);
        let born = lerp(t0, t1, f);
        let seconds = now - born;
        if !(0. ..l.life).contains(&seconds) {
            continue;
        }
        let life = Life {
            t: seconds / l.life,
            seconds,
            seed: hash(flight.seed ^ hash(n as u32).wrapping_add(0x7a11)),
            steady: false,
            index: n as u32,
        };
        let mut dropped = Anchor::at(at, a.turn, seconds, life.seed);
        dropped.scale = a.scale;
        let p = particle(l, &dropped, &life);
        if p.alpha > 0.002 {
            sprite(part, &p, &life, out);
        }
    }
}

fn decal(part: &Part, a: &Anchor, life: &Life, out: &mut fx::Frame, _s: &Scene) {
    let l = &part.layer;
    let s = if l.scaled { a.scale } else { 1. };
    let fade = if life.steady {
        1.
    } else {
        envelope(life.t, l.fade)
    };
    let pulse = 1. + l.pulse[0] * (TAU * l.pulse[1] * a.age).sin();
    // A mark cast from above (a missile's light) weakens the higher its source is.
    let g = a.ground;
    let lift = (a.origin.y - g.y).max(0.);
    let alpha = l.alpha * fade * a.strength * pulse / (1. + 0.5 * lift * lift);
    if alpha <= 0.002 {
        return;
    }
    let spin = Quat::from_rotation_y(l.angle + l.spin[0] * a.age);
    // The box lies on the ground's plane, deep enough for the ground straying from it, and
    // no deeper: anyone standing in the mark is left out of it.
    let tilt = Quat::from_rotation_arc(Vec3::Y, Vec3::new(-g.slope.x, 1., -g.slope.y).normalize());
    let half_y = g.stray + 0.35;
    let size = lerp(l.size[0], l.size[1], life.t) * s;
    let (kind, half, centre, cone) = match (l.shape, a.shape) {
        (DecalShape::Area, Some(Shape::Circle { radius })) => (
            fx::Decal::CIRCLE,
            Vec3::new(radius, half_y, radius),
            Vec3::ZERO,
            0.,
        ),
        // The box hugs the cone (it opens along +z from the area's centre), not its circle.
        (DecalShape::Area, Some(Shape::Cone { range, angle })) => (
            fx::Decal::CONE,
            Vec3::new(
                range * (angle * 0.5).sin().clamp(0.05, 1.),
                half_y,
                range * 0.5,
            ),
            Vec3::Z * range * 0.5,
            angle * 0.5,
        ),
        (DecalShape::Area, Some(Shape::Line { length, width })) => (
            fx::Decal::LINE,
            Vec3::new(width * 0.5, half_y, length * 0.5),
            Vec3::Z * length * 0.5,
            0.,
        ),
        _ => (
            fx::Decal::TEXTURE,
            Vec3::new(size, half_y, size),
            Vec3::ZERO,
            0.,
        ),
    };
    let turn = tilt
        * if kind == fx::Decal::TEXTURE {
            a.turn * spin
        } else {
            a.turn
        };
    let origin = Vec3::new(a.origin.x, g.y, a.origin.z) + tilt * (a.turn * centre);
    let to_world = Mat4::from_scale_rotation_translation(half, turn, origin);
    let tile = [a.age * l.scroll[0], a.age * l.scroll[1], l.repeat, l.repeat];
    let material = part.material(life.t, life.seconds, alpha, tile);
    let fill = [l.fill[0], l.fill[1], l.fill[2], l.fill[3] * alpha];
    out.decals.push(fx::Decal {
        to_decal: to_world.inverse().to_cols_array_2d(),
        to_world: to_world.to_cols_array_2d(),
        shape: [kind, cone, a.progress, l.edge],
        // A cone's apex is half its box behind the box's centre.
        size: [
            half.x,
            half.y,
            half.z,
            if kind == fx::Decal::CONE { half.z } else { 0. },
        ],
        fill,
        material,
    });
}

fn mesh(part: &Part, a: &Anchor, life: &Life, out: &mut fx::Frame) {
    let (Some(index), l) = (part.mesh, &part.layer) else {
        return;
    };
    // Placed like any particle (so a layer can scatter crystals over a field), then turned
    // with the anchor, or stood on end with a random heading.
    let p = particle(l, a, life);
    if p.alpha <= 0.002 {
        return;
    }
    let turn = if l.upright {
        let heading = (hash(life.seed ^ 0x4ead) >> 8) as f32 / (1u32 << 24) as f32 * TAU;
        Quat::from_rotation_y(heading + p.angle)
            * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
    } else {
        a.turn * Quat::from_rotation_y(l.angle + l.spin[0] * a.age)
    };
    let scale = Vec3::new(p.size, p.size * l.aspect, p.size * l.length);
    let model = Mat4::from_scale_rotation_translation(scale, turn, p.at);
    let tile = [0., 0., l.repeat, l.repeat];
    out.meshes.push((
        index,
        fx::MeshInstance {
            model: model.to_cols_array_2d(),
            shading: [l.rim[0], l.rim[1], l.mask[0], l.mask[1]],
            terms: [l.twinkle[0], l.twinkle[1], l.texture, l.scroll[1]],
            material: part.material(life.t, life.seconds, p.alpha, tile),
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{AuraView, CastBar, ProjectileView, UnitCombat};

    fn effects() -> Effects {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
        let table = assets::Catalog::load(root)
            .unwrap()
            .effects
            .expect("effects.json");
        let mut cache = ModelCache::default();
        Effects::load(table, Path::new(root), &mut cache).unwrap().0
    }

    fn unit(id: u32) -> UnitState {
        UnitState {
            id,
            kind: protocol::UnitKind::Player,
            body: "player".into(),
            name: "Test".into(),
            level: 1,
            position: Vec3::new(id as f32, 0., 0.),
            velocity: Vec3::ZERO,
            yaw: 0.,
            pose: protocol::Pose::Idle,
            pose_time: 0.,
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look: Default::default(),
        }
    }

    fn combat(id: u32, auras: &[&str], cast: Option<&str>) -> UnitCombat {
        UnitCombat {
            id,
            action: Default::default(),
            action_time: 0.,
            cast: cast.map(|a| CastBar {
                ability: a.into(),
                name: a.into(),
                elapsed: 0.5,
                duration: 1.5,
                channel: false,
                interruptible: true,
            }),
            auras: auras
                .iter()
                .map(|k| AuraView {
                    key: (*k).into(),
                    name: (*k).into(),
                    stacks: 1,
                    remaining: 0.,
                    duration: 0.,
                    debuff: false,
                })
                .collect(),
            resource_kind: Default::default(),
            resource: 0.,
            max_resource: 0.,
            guarding: false,
            downed: false,
            defiance: None,
        }
    }

    fn frame(
        fx: &mut Effects,
        fight: &CombatView,
        units: &[UnitState],
        time: f32,
    ) -> (Vec<Draw>, fx::Frame) {
        fx.draws(&Scene {
            fight,
            units,
            height: &|_| 1.8,
            ground: &|_, _| 0.,
            since: 0.,
            time,
            aim: None,
        })
    }

    #[test]
    fn every_effect_the_table_names_is_drawn_with_art_it_has() {
        let mut fx = effects();
        let names: Vec<String> = fx.table.effects.keys().cloned().collect();
        for name in names {
            let look = fx.look(&name).unwrap();
            for (i, part) in look.iter().enumerate() {
                let l = &part.layer;
                match l.kind {
                    LayerKind::Mesh => assert!(part.mesh.is_some(), "{name} layer {i} has no mesh"),
                    LayerKind::Decal if l.shape == DecalShape::Area => {}
                    _ => assert!(
                        !matches!(part.art, Art::None),
                        "{name} layer {i} has no art"
                    ),
                }
                if l.ramp.is_some() {
                    assert!(part.ramp >= 0., "{name} layer {i}'s ramp isn't loaded");
                }
            }
        }
    }

    #[test]
    fn missiles_casts_auras_and_fields_all_draw_something() {
        let mut fx = effects();
        let units: Vec<_> = (1..=3).map(unit).collect();
        let fight = CombatView {
            tick: 1,
            units: vec![
                combat(1, &["shield_wall"], Some("staff_firebolt")),
                combat(2, &["stunned"], None),
                combat(3, &["burning"], None),
            ],
            projectiles: vec![ProjectileView {
                id: 7,
                position: Vec3::new(0., 1.5, 5.),
                velocity: Vec3::new(20., 0., 0.),
                visual: "fire_bolt".into(),
                school: School::Fire,
            }],
            areas: vec![AreaView {
                id: 3,
                warning: true,
                shape: Shape::Circle { radius: 2. },
                center: Vec3::new(0., 0., 4.),
                yaw: 0.,
                elapsed: 0.5,
                duration: 1.,
                hostile: true,
                school: School::Physical,
                field: None,
            }],
        };
        let mut last = fx::Frame::default();
        for f in 0..30 {
            last = frame(&mut fx, &fight, &units, f as f32 / 60.).1;
        }
        assert!(!last.quads.is_empty(), "sprites");
        assert!(!last.decals.is_empty(), "the warning and the cast's circle");
        assert!(!last.meshes.is_empty(), "the shield's shell");
        for q in &last.quads {
            assert!(
                q.position
                    .iter()
                    .chain(&q.a)
                    .chain(&q.material.colour)
                    .all(|v| v.is_finite())
            );
        }
    }

    #[test]
    fn a_landed_missile_bursts_then_its_burst_and_trail_end() {
        let mut fx = effects();
        let units = vec![unit(1)];
        let flying = CombatView {
            projectiles: vec![ProjectileView {
                id: 9,
                position: Vec3::new(0., 1.5, 0.),
                velocity: Vec3::new(25., 0., 0.),
                visual: "fire_bolt".into(),
                school: School::Fire,
            }],
            ..Default::default()
        };
        fx.event(
            &Event::ProjectileLaunched {
                id: 9,
                from: 1,
                visual: "fire_bolt".into(),
            },
            0.,
        );
        for f in 0..20 {
            let mut view = flying.clone();
            view.projectiles[0].position.x = f as f32 / 60. * 25.;
            frame(&mut fx, &view, &units, f as f32 / 60.);
        }
        fx.event(
            &Event::ProjectileEnded {
                id: 9,
                at: Vec3::new(8., 1.5, 0.),
                hit: None,
            },
            0.34,
        );
        let empty = CombatView::default();
        let burst = frame(&mut fx, &empty, &units, 0.4).1;
        assert!(!burst.quads.is_empty(), "the impact shows");
        let later = frame(&mut fx, &empty, &units, 10.).1;
        assert!(later.is_empty(), "and it's all gone ten seconds later");
    }

    #[test]
    fn the_same_moment_draws_the_same_effects() {
        let units = vec![unit(1)];
        let fight = CombatView {
            units: vec![combat(
                1,
                &["burning", "regeneration"],
                Some("healing_light"),
            )],
            ..Default::default()
        };
        let (mut a, mut b) = (effects(), effects());
        let fa = frame(&mut a, &fight, &units, 2.5).1;
        let fb = frame(&mut b, &fight, &units, 2.5).1;
        assert_eq!(fa.quads, fb.quads);
        assert_eq!(fa.decals, fb.decals);
    }

    #[test]
    fn ground_fits_a_slope_and_says_how_far_it_strays() {
        let slope = |x: f32, z: f32| 2. + 0.5 * x - 0.25 * z;
        let g = Ground::fit(Vec3::new(3., 0., -1.), 2., &slope);
        assert!((g.y - slope(3., -1.)).abs() < 1e-4, "{g:?}");
        assert!((g.slope - Vec2::new(0.5, -0.25)).length() < 1e-4, "{g:?}");
        assert!(g.stray < 1e-4, "{g:?}");
        let bumpy = |x: f32, _: f32| if x > 1. { 1. } else { 0. };
        assert!(Ground::fit(Vec3::ZERO, 2., &bumpy).stray > 0.3);
    }

    #[test]
    fn ramps_are_baked_between_their_stops() {
        let row = bake(&[[0., 0., 0., 0.], [1., 2., 4., 8.]]);
        assert_eq!(row[0], [0., 0., 0., 1.]);
        assert_eq!(row[fx::RAMP_WIDTH - 1], [2., 4., 8., 1.]);
        let mid = row[fx::RAMP_WIDTH / 2];
        assert!((mid[1] - 2.).abs() < 0.1, "{mid:?}");
    }

    #[test]
    fn particles_cycle_without_jumping_back_in_time() {
        let l = Layer {
            count: 4,
            life: 1.,
            ..Default::default()
        };
        let a = Anchor::at(Vec3::ZERO, Quat::IDENTITY, 3.25, 11);
        let lives = lives(&l, &a, false);
        assert_eq!(lives.len(), 4);
        for life in &lives {
            assert!((0. ..1.).contains(&life.t));
            assert!((life.seconds - life.t * l.life).abs() < 1e-5);
        }
        let once = super::lives(&l, &Anchor::at(Vec3::ZERO, Quat::IDENTITY, 2., 11), true);
        assert!(
            once.is_empty(),
            "a one-shot layer's particles are long gone"
        );
    }
}
