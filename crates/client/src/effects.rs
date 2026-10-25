//! Combat drawn in the world from `assets/catalog/effects.json`: missiles in flight, the glow
//! in a caster's hands, auras on bodies, warnings and fields on the ground, and the flash where
//! things land. Every look value comes from the table; this module only supplies the shapes,
//! built once and placed by transform.
//!
//! Lights (a bolt's core and halo, a cast's orb, flashes, motes and bubbles) are flat sprites
//! turned to face the camera, with their soft falloff in the vertex opacity, so they read as
//! glows rather than solid balls and cost one blended layer each. Trails are ribbons turned the
//! same way, and ground marks carry their glowing edges in their vertices too. Things that
//! don't glow (an arrow, a thrown axe) are opaque and lit like any other mesh.
use assets::{AuraKind, EffectCatalog, EffectStyle, Glow, ProjectileKind};
use glam::{Mat4, Quat, Vec2, Vec3};
use protocol::{AreaView, CombatView, Event, Shape, UnitState};
use render::{
    character::Draw,
    model::{Material, Model, Node, Primitive, Transform, Vertex, next_model_id},
    visibility::Bounds,
};
use std::{
    collections::HashMap,
    f32::consts::{PI, TAU},
    sync::Arc,
};

/// Unit-sized shapes. Marks and bands are keyed by their proportions so a few meshes serve
/// every size; the set stays small because the content's shapes are few.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Mesh {
    /// A soft round glow of radius 1 in the y = 0 plane, opaque at its centre. Faces the camera.
    Glow,
    /// A bubble seen from outside: faint in the middle and brightest at its rim (radius 1),
    /// as a lit sphere's edge is. Faces the camera.
    Bubble,
    /// A streak from its head at z = 0 back to z = −1, half-width 1 near the head, fading
    /// along its length and towards its sides. Turned about z to face the camera.
    Streak,
    /// A six-sided crystal along z from −0.5 to 0.5, radius 1 at its widest.
    Shard,
    /// A closed cylinder of radius 1 from z = −0.5 to 0.5.
    Shaft,
    /// A sphere of radius 1.
    Ball,
    /// A flat band ending at radius 1, soft on both sides, its width a share of the radius in
    /// thousandths, broken into `dashes` (0 for a whole band).
    Band { width: u16, dashes: u16 },
    /// A flat disc (3600) or a fan about +z of the given angle in tenths of a degree, with its
    /// edge band a share of its radius wide (thousandths).
    Mark {
        angle: u16,
        band: u16,
        profile: Profile,
    },
    /// A flat strip x ∈ ±0.5, z ∈ 0…1, with its edge bands as shares of its width and length.
    Lane {
        across: u16,
        along: u16,
        profile: Profile,
    },
}

/// How a mark's opacity runs from its edge inwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Profile {
    /// A warning's whole shape: a bright edge over a faint body.
    Edge,
    /// The part of a warning that has ripened: denser, with a bright leading edge.
    Fill,
    /// A lasting field: a dense pool, brighter towards its edge.
    Pool,
    /// The edge alone (a field's rim in the colour of who it hurts).
    Rim,
}

/// Where a mark's opacity changes, in edge-band widths in from its edge.
const KNOTS: [f32; 5] = [0., 0.15, 0.5, 1., 2.];

impl Profile {
    /// Opacity `t` edge-band widths in from the edge: nothing at the very edge, brightest a
    /// thin way in, then falling across the band to the body.
    fn alpha(self, t: f32) -> f32 {
        let levels = match self {
            Profile::Edge => [0., 1., 0.45, 0.12, 0.03],
            Profile::Fill => [0., 1., 0.55, 0.3, 0.2],
            Profile::Pool => [0., 1., 0.7, 0.45, 0.35],
            Profile::Rim => [0., 1., 0.3, 0.05, 0.],
        };
        let k = KNOTS
            .iter()
            .rposition(|&k| k <= t)
            .unwrap_or(0)
            .min(KNOTS.len() - 2);
        let f = ((t - KNOTS[k]) / (KNOTS[k + 1] - KNOTS[k])).clamp(0., 1.);
        levels[k] + (levels[k + 1] - levels[k]) * f
    }
}

/// Instance ids for motion vectors: the top bits keep effects apart from units (id << 32),
/// scenery (0x4…) and terrain (0x8…).
const EFFECT: u64 = 0x2000_0000_0000_0000;
fn instance(kind: u64, id: u32, part: u64) -> u64 {
    EFFECT | (kind << 52) | ((id as u64) << 12) | (part & 0xfff)
}

struct Flight {
    visual: String,
    /// Scene time when it was first seen, so a trail never reaches back past the hand.
    born: f32,
}

struct Burst {
    id: u32,
    at: Vec3,
    glow: String,
    radius: f32,
    born: f32,
    /// Flat on the ground (a landed blow) rather than a ball in the air (a missile's flash).
    ground: bool,
}

/// Meshes built so far, by shape, whether it's a hot core, and glow.
#[derive(Default)]
struct Shapes(HashMap<(Mesh, bool), HashMap<String, Arc<Model>>>);

impl Shapes {
    fn get(&mut self, mesh: Mesh, hot: bool, glow: &str, g: &Glow, st: &EffectStyle) -> Arc<Model> {
        let by_glow = self.0.entry((mesh, hot)).or_default();
        if let Some(m) = by_glow.get(glow) {
            return m.clone();
        }
        let m = build(mesh, glow, hot, g, st);
        by_glow.insert(glow.into(), m.clone());
        m
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.values().map(HashMap::len).sum()
    }
}

pub struct Effects {
    table: EffectCatalog,
    shapes: Shapes,
    flights: HashMap<u32, Flight>,
    bursts: Vec<Burst>,
    next_burst: u32,
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
    /// Seconds, steadily increasing: drives pulses, spins and bursts.
    pub time: f32,
    /// A ground skill being aimed: where, and its radius.
    pub aim: Option<(Vec3, f32)>,
    /// The camera, which sprites and streaks turn to face.
    pub eye: Vec3,
}

impl Effects {
    pub fn new(table: EffectCatalog) -> Self {
        Self {
            table,
            shapes: Shapes::default(),
            flights: HashMap::new(),
            bursts: Vec::new(),
            next_burst: 0,
        }
    }

    /// Notes launches and starts the flash for anything that landed.
    pub fn event(&mut self, event: &Event, time: f32) {
        match event {
            Event::ProjectileLaunched { id, visual, .. } => {
                self.flights.entry(*id).or_insert(Flight {
                    visual: visual.clone(),
                    born: time,
                });
            }
            Event::ProjectileEnded { id, at, .. } => {
                if let Some(f) = self.flights.remove(id)
                    && let Some(look) = self.table.projectiles.get(&f.visual)
                {
                    let glow = look.trail_glow.clone().unwrap_or_else(|| look.glow.clone());
                    self.burst(*at, glow, look.burst, time, false);
                }
            }
            Event::AreaImpact {
                at, radius, school, ..
            } => {
                self.burst(*at, school_name(*school), *radius, time, true);
            }
            Event::Combo { at, .. } => self.burst(*at, "combo".into(), 2.5, time, true),
            _ => {}
        }
    }

    fn burst(&mut self, at: Vec3, glow: String, radius: f32, time: f32, ground: bool) {
        self.next_burst = self.next_burst.wrapping_add(1);
        self.bursts.push(Burst {
            id: self.next_burst,
            at,
            glow,
            radius,
            born: time,
            ground,
        });
        // Bounded however busy the fight gets.
        if self.bursts.len() > 128 {
            self.bursts.remove(0);
        }
    }

    pub fn draws(&mut self, s: &Scene) -> Vec<Draw> {
        let mut out = Vec::new();
        let by_id: HashMap<_, _> = s.units.iter().map(|u| (u.id, u)).collect();
        // Missiles that left view without an end event shouldn't be remembered forever.
        let live: std::collections::HashSet<_> = s.fight.projectiles.iter().map(|p| p.id).collect();
        self.flights
            .retain(|id, f| live.contains(id) || s.time - f.born < 1.);
        let seconds = self.table.style.burst_seconds.max(0.01);
        self.bursts.retain(|b| s.time - b.born < seconds);
        let mut p = Painter {
            table: &self.table,
            shapes: &mut self.shapes,
            out: &mut out,
            eye: s.eye,
        };
        for m in &s.fight.projectiles {
            let born = self
                .flights
                .entry(m.id)
                .or_insert(Flight {
                    visual: m.visual.clone(),
                    born: s.time,
                })
                .born;
            p.projectile(m, s.since, s.time, s.time - born);
        }
        for a in &s.fight.areas {
            p.area(a, s);
        }
        if let Some((at, radius)) = s.aim {
            p.aim(at, radius, s);
        }
        for c in &s.fight.units {
            let Some(u) = by_id.get(&c.id) else { continue };
            if u.health <= 0. {
                continue;
            }
            let h = (s.height)(u.id);
            if let Some(cast) = &c.cast
                && let Some(glow) = p.table.casts.get(&cast.ability)
            {
                let progress = if cast.channel {
                    1.
                } else {
                    ((cast.elapsed + s.since) / cast.duration.max(0.001)).clamp(0., 1.)
                };
                p.cast(u, h, glow, progress, s);
            }
            for (i, a) in c.auras.iter().enumerate().take(64) {
                let Some(look) = p.table.auras.get(&a.key) else {
                    continue;
                };
                let f = p.table.style.aura_fade.max(0.001);
                let fade = if a.duration > 0. {
                    ((a.duration - a.remaining) / f).clamp(0., 1.) * (a.remaining / f).clamp(0., 1.)
                } else {
                    1.
                };
                p.aura(u, h, look, fade, i as u64, s);
            }
        }
        for b in &self.bursts {
            p.flash(b, s, seconds);
        }
        out
    }
}

/// Places shapes for one frame.
struct Painter<'a> {
    table: &'a EffectCatalog,
    shapes: &'a mut Shapes,
    out: &'a mut Vec<Draw>,
    eye: Vec3,
}

impl Painter<'_> {
    fn projectile(&mut self, p: &protocol::ProjectileView, since: f32, time: f32, age: f32) {
        let table = self.table;
        let Some(look) = table.projectiles.get(&p.visual) else {
            return;
        };
        let st = &table.style;
        let at = p.position + p.velocity * since.clamp(0., 0.1);
        let speed = p.velocity.length();
        let dir = p.velocity.normalize_or(Vec3::Z);
        let turn = Quat::from_rotation_arc(Vec3::Z, dir);
        let glow = look.glow.as_str();
        let trail_glow = look.trail_glow.as_deref().unwrap_or(glow);
        let place = |scale: Vec3| Mat4::from_scale_rotation_translation(scale, turn, at);
        let id = |part| instance(1, p.id, part);
        let pulse = 1. + 0.1 * (time * 19. + p.id as f32).sin();
        match look.kind {
            ProjectileKind::Bolt => {
                // A white-hot core inside a softer halo of its colour.
                let core = Vec2::splat(look.size * 2.6);
                self.sprite(Mesh::Glow, glow, true, at, core, 0., st.core_alpha, id(0));
                if look.halo > 0. {
                    let halo = Vec2::splat(look.halo * 1.7 * pulse);
                    let a = st.halo_alpha;
                    self.sprite(Mesh::Glow, trail_glow, false, at, halo, 0., a, id(1));
                }
            }
            ProjectileKind::Lance => {
                let shard = place(Vec3::new(look.size, look.size, look.length));
                self.push(Mesh::Shard, glow, true, shard, st.core_alpha, id(0));
                if look.halo > 0. {
                    let halo = Vec2::splat(look.halo * 1.7 * pulse);
                    let a = st.halo_alpha;
                    self.sprite(Mesh::Glow, trail_glow, false, at, halo, 0., a, id(1));
                }
            }
            ProjectileKind::Arrow => {
                // The shaft's tip leads: it sits just behind the arrow's position.
                let shaft = Mat4::from_scale_rotation_translation(
                    Vec3::new(look.size, look.size, look.length),
                    turn,
                    at - dir * look.length * 0.5,
                );
                self.push(Mesh::Shaft, glow, false, shaft, 1., id(0));
            }
            ProjectileKind::Axe => {
                let tumble = turn * Quat::from_rotation_x(time * look.spin * TAU);
                let head = Mat4::from_scale_rotation_translation(
                    Vec3::new(look.size * 0.22, look.size, look.size * 0.7),
                    tumble,
                    at + tumble * Vec3::Y * look.length * 0.35,
                );
                let haft = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.02, 0.02, look.length),
                    tumble * Quat::from_rotation_x(PI * 0.5),
                    at,
                );
                self.push(Mesh::Ball, glow, false, head, 1., id(0));
                self.push(Mesh::Shaft, "wood", false, haft, 1., id(1));
            }
        }
        // Grows from the hand to its full length.
        let trail = look.trail.min(speed * age.max(0.));
        if trail > 0.05 && look.trail_width > 0. {
            let back = if look.kind == ProjectileKind::Arrow {
                at - dir * look.length
            } else {
                at
            };
            let (width, a) = (look.trail_width, st.trail_alpha);
            self.streak(trail_glow, false, back, dir, width, trail, a, id(2));
        }
    }

    fn area(&mut self, a: &AreaView, s: &Scene) {
        let table = self.table;
        let st = &table.style;
        let mark = if a.hostile { "hostile" } else { "friendly" };
        let turn = Quat::from_rotation_y(a.yaw);
        let reach = match a.shape {
            Shape::Circle { radius } => radius,
            Shape::Cone { range, .. } => range,
            Shape::Line { length, .. } => length,
        };
        let y = lift(a, reach, s.ground);
        let at = Vec3::new(a.center.x, y, a.center.z);
        let flat = |scale: Vec3| Mat4::from_scale_rotation_translation(scale, turn, at);
        let id = |part| instance(2, a.id, part);
        if a.warning {
            // The whole shape glows at its edge; the part that has ripened fills from the
            // caster outwards (circles from their centre) with a bright leading edge.
            let progress = ((a.elapsed + s.since) / a.duration.max(0.001)).clamp(0., 1.);
            let (outline, size) = shape_mark(a.shape, st.rim_width, Profile::Edge);
            let edge = st.warning_outline_alpha;
            self.push(outline, mark, false, flat(size), edge, id(0));
            let grown = match a.shape {
                Shape::Line { .. } => size * Vec3::new(1., 1., progress),
                _ => size * Vec3::new(progress, 1., progress),
            };
            let (fill, _) = shape_mark(a.shape, st.rim_width, Profile::Fill);
            let alpha = st.warning_fill_alpha * (0.6 + 0.4 * progress);
            self.push(fill, mark, false, flat(grown), alpha, id(1));
        } else {
            // A field shows what it is, rimmed by who it hurts.
            let field = a
                .field
                .as_ref()
                .map(|f| format!("field.{f}"))
                .filter(|g| table.glows.contains_key(g));
            let fill = field.as_deref().unwrap_or(mark);
            let pulse = 0.85 + 0.15 * (s.time * 2.4 + a.id as f32).sin();
            let (pool, size) = shape_mark(a.shape, st.rim_width * 2., Profile::Pool);
            self.push(pool, fill, false, flat(size), st.field_alpha * pulse, id(0));
            let (rim, size) = shape_mark(a.shape, st.rim_width, Profile::Rim);
            self.push(rim, mark, false, flat(size), st.field_rim_alpha, id(1));
            // Embers rise from all over it and fade, each from a new place every time.
            let count = (reach * 2.5).round().clamp(3., EMBERS as f32) as u32;
            for k in 0..count {
                let life = s.time / EMBER_SECONDS + k as f32 * 0.618;
                let (n, p) = (life.floor() as u32, life.fract());
                let (u, v) = unit(hash(a.id.wrapping_mul(EMBERS) ^ (n << 8) ^ k));
                let drift = Vec3::new((p * 5. + k as f32).sin() * 0.08, p * 1.1, 0.);
                let spot = at + turn * (inside(a.shape, u, v) + drift);
                let alpha = st.core_alpha * (p * PI).sin();
                // A new id each time it starts over, so its jump isn't taken for motion.
                let part = 2 + k as u64 + EMBERS as u64 * (n % 4) as u64;
                let ember = Vec2::splat(0.05);
                self.sprite(Mesh::Glow, fill, true, spot, ember, 0., alpha, id(part));
            }
        }
    }

    /// The circle a ground skill will land in while it's being aimed.
    fn aim(&mut self, at: Vec3, radius: f32, s: &Scene) {
        let st = &self.table.style;
        let shape = Shape::Circle { radius };
        let area = AreaView {
            id: u32::MAX,
            warning: false,
            shape,
            center: at,
            yaw: 0.,
            elapsed: 0.,
            duration: 0.,
            hostile: false,
            school: protocol::School::Physical,
            field: None,
        };
        let at = Vec3::new(at.x, lift(&area, radius, s.ground), at.z);
        let (mesh, size) = shape_mark(shape, st.rim_width, Profile::Edge);
        let place = Mat4::from_translation(at) * Mat4::from_scale(size);
        let (alpha, id) = (st.warning_outline_alpha, instance(2, u32::MAX, 0));
        self.push(mesh, "friendly", false, place, alpha, id);
    }

    fn cast(&mut self, u: &UnitState, h: f32, glow: &str, progress: f32, s: &Scene) {
        let st = &self.table.style;
        let forward = Vec3::new(u.yaw.sin(), 0., u.yaw.cos());
        let flicker = 1. + 0.08 * (s.time * 23. + u.id as f32).sin();
        let r = (st.cast_orb[0] + (st.cast_orb[1] - st.cast_orb[0]) * progress) * flicker;
        let hands = u.position + Vec3::Y * h * 0.62 + forward * 0.42;
        let id = |part| instance(3, u.id, part);
        let (orb, halo) = (Vec2::splat(r * 1.8), Vec2::splat(r * 5.));
        self.sprite(
            Mesh::Glow,
            glow,
            true,
            hands,
            orb,
            0.,
            st.cast_orb_alpha,
            id(0),
        );
        let a = st.halo_alpha * progress;
        self.sprite(Mesh::Glow, glow, false, hands, halo, 0., a, id(1));
        // A broken circle turning slowly at the feet.
        let feet = Vec3::new(
            u.position.x,
            (s.ground)(u.position.x, u.position.z) + 0.03,
            u.position.z,
        );
        let circle = Mat4::from_scale_rotation_translation(
            Vec3::new(st.cast_circle, 1., st.cast_circle),
            Quat::from_rotation_y(s.time * 0.8),
            feet,
        );
        let band = Mesh::Band {
            width: share(st.ring_width),
            dashes: 12,
        };
        let alpha = st.cast_circle_alpha * (0.5 + 0.5 * progress);
        self.push(band, glow, false, circle, alpha, id(2));
    }

    fn aura(
        &mut self,
        u: &UnitState,
        h: f32,
        look: &assets::AuraLook,
        fade: f32,
        index: u64,
        s: &Scene,
    ) {
        let st = &self.table.style;
        let id = |part| instance(4, u.id, index * 8 + part);
        let pulse = 0.85 + 0.15 * (s.time * 3. + index as f32).sin();
        let glow = look.glow.as_str();
        match look.kind {
            AuraKind::Shell => {
                // An upright bubble r tall and three quarters as wide, as seen from the camera's
                // height, drawn just in front of the body so the body shows through it.
                let r = look.size * h;
                let centre = u.position + Vec3::Y * h * 0.5;
                let rise = (self.eye - centre).normalize_or(Vec3::Z).y;
                let wide = r * 0.75;
                let tall = (r * r * (1. - rise * rise) + wide * wide * rise * rise).sqrt();
                let alpha = st.shell_alpha * fade * pulse;
                let (size, nearer) = (Vec2::new(wide, tall), wide * 0.5);
                self.sprite(
                    Mesh::Bubble,
                    glow,
                    false,
                    centre,
                    size,
                    nearer,
                    alpha,
                    id(0),
                );
            }
            AuraKind::Ring => {
                let feet = Vec3::new(
                    u.position.x,
                    (s.ground)(u.position.x, u.position.z) + 0.035,
                    u.position.z,
                );
                let r = look.size * pulse.sqrt();
                let band = Mesh::Band {
                    width: share(st.ring_width),
                    dashes: 0,
                };
                let place = Mat4::from_translation(feet) * Mat4::from_scale(Vec3::new(r, 1., r));
                self.push(band, glow, false, place, st.ring_alpha * fade, id(0));
            }
            AuraKind::Halo => {
                // A faint circle above the head with three motes chasing round it.
                let crown = u.position + Vec3::Y * (h + 0.2);
                let band = Mesh::Band {
                    width: 250,
                    dashes: 0,
                };
                let place = Mat4::from_translation(crown)
                    * Mat4::from_scale(Vec3::new(look.size, 1., look.size));
                let alpha = st.halo_ring_alpha * fade;
                self.push(band, glow, false, place, alpha * 0.35, id(0));
                for k in 0..3 {
                    let a = s.time * 3.5 + k as f32 * TAU / 3.;
                    let p = crown + Vec3::new(a.sin(), 0.06 * (a * 2.).sin(), a.cos()) * look.size;
                    let mote = Vec2::splat(0.1);
                    self.sprite(Mesh::Glow, glow, false, p, mote, 0., alpha, id(1 + k));
                }
            }
        }
    }

    fn flash(&mut self, b: &Burst, s: &Scene, seconds: f32) {
        let st = &self.table.style;
        let t = ((s.time - b.born) / seconds).clamp(0., 1.);
        let grow = 1. - (1. - t) * (1. - t);
        let alpha = st.burst_alpha * (1. - t) * (1. - t);
        let id = |part| instance(5, b.id, part);
        if b.ground {
            // A shock ring running outwards, and a flash where it struck, lifted towards the
            // camera so the ground doesn't cut it off.
            let y = (s.ground)(b.at.x, b.at.z).max(b.at.y) + 0.05;
            let at = Vec3::new(b.at.x, y, b.at.z);
            let r = b.radius * (0.25 + 0.75 * grow);
            let band = Mesh::Band {
                width: 300,
                dashes: 0,
            };
            let place = Mat4::from_translation(at) * Mat4::from_scale(Vec3::new(r, 1., r));
            self.push(band, &b.glow, false, place, alpha, id(0));
            let f = b.radius * 0.5 * (0.6 + 0.4 * grow);
            let (lifted, a) = (at + Vec3::Y * f * 0.3, alpha * (1. - t));
            self.sprite(
                Mesh::Glow,
                &b.glow,
                true,
                lifted,
                Vec2::splat(f),
                f,
                a,
                id(1),
            );
        } else {
            // A hot heart that dies quickly inside a burst of colour that spreads, and sparks
            // flung out of it, falling a little as they go.
            let (heart, a) = (Vec2::splat(b.radius * 0.6), alpha * (1. - t));
            self.sprite(Mesh::Glow, &b.glow, true, b.at, heart, 0., a, id(0));
            let burst = Vec2::splat(b.radius * (0.6 + 0.9 * grow));
            self.sprite(
                Mesh::Glow,
                &b.glow,
                false,
                b.at,
                burst,
                0.,
                alpha * 0.7,
                id(1),
            );
            let age = s.time - b.born;
            for k in 0..SPARKS {
                let dir = scatter(b.id, k);
                let fall = Vec3::Y * 1.5 * age * age;
                let head = b.at + dir * b.radius * (0.15 + 1.2 * grow) - fall;
                let length = b.radius * 0.3 * (1. - t);
                let a = st.core_alpha * (1. - t);
                self.streak(&b.glow, true, head, dir, 0.025, length, a, id(2 + k as u64));
            }
        }
    }

    /// A flat shape turned to face the camera: its x along the camera's right, half-size
    /// `half`, pushed `towards` the camera by that many metres.
    #[allow(clippy::too_many_arguments)]
    fn sprite(
        &mut self,
        mesh: Mesh,
        glow: &str,
        hot: bool,
        at: Vec3,
        half: Vec2,
        towards: f32,
        alpha: f32,
        instance: u64,
    ) {
        // A turn, not a mirror, so the face stays a front face and is lit as one.
        let face = (self.eye - at).normalize_or(Vec3::Y);
        let right = face.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(face);
        let place = Mat4::from_cols(
            (right * half.x).extend(0.),
            face.extend(0.),
            (up * half.y).extend(0.),
            (at + face * towards).extend(1.),
        );
        self.push(mesh, glow, hot, place, alpha, instance);
    }

    /// A streak from `head` back along `-dir`, turned about its length to face the camera.
    #[allow(clippy::too_many_arguments)]
    fn streak(
        &mut self,
        glow: &str,
        hot: bool,
        head: Vec3,
        dir: Vec3,
        width: f32,
        length: f32,
        alpha: f32,
        instance: u64,
    ) {
        let side = (self.eye - head)
            .cross(dir)
            .normalize_or(dir.any_orthonormal_vector());
        let place = Mat4::from_cols(
            (side * width).extend(0.),
            dir.cross(side).extend(0.),
            (dir * length).extend(0.),
            head.extend(1.),
        );
        self.push(Mesh::Streak, glow, hot, place, alpha, instance);
    }

    fn push(
        &mut self,
        mesh: Mesh,
        glow: &str,
        hot: bool,
        transform: Mat4,
        alpha: f32,
        instance: u64,
    ) {
        if alpha <= 0.002 {
            return;
        }
        let Some(g) = self.table.glow(glow) else {
            return;
        };
        let model = self.shapes.get(mesh, hot, glow, g, &self.table.style);
        let c = g.colour.map(|v| v * g.albedo);
        let alpha = if solid(g) { 1. } else { alpha.min(1.) };
        self.out.push(Draw {
            dye: None,
            base_map: None,
            instance,
            model,
            primitive: 0,
            transform,
            palette: Arc::from([]),
            hidden: 0,
            base: [c[0], c[1], c[2], alpha],
            morph: [0.; 2],
        });
    }
}

/// Glows with no light of their own are solid things: drawn opaque and lit.
fn solid(g: &Glow) -> bool {
    g.strength <= 0.
}

/// Sparks from a missile's flash; embers at most over a field, and how long one lives.
const SPARKS: u32 = 6;
const EMBERS: u32 = 8;
const EMBER_SECONDS: f32 = 1.6;

/// Scrambles bits: the same number in, the same out, with nearby numbers far apart.
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// Two numbers in 0…1 from a hash.
fn unit(h: u32) -> (f32, f32) {
    ((h & 0xffff) as f32 / 65535., (h >> 16) as f32 / 65535.)
}

/// Which way the `k`th spark of a burst flies: spread evenly round, mostly upwards.
fn scatter(burst: u32, k: u32) -> Vec3 {
    let h = hash(burst.wrapping_mul(SPARKS) ^ k.wrapping_mul(0x9e37_79b9));
    let (u, v) = unit(h);
    let around = (k as f32 + u * 0.8) / SPARKS as f32 * TAU;
    let y = v * 1.2 - 0.4;
    let flat = (1. - y * y).sqrt();
    Vec3::new(around.sin() * flat, y, around.cos() * flat)
}

/// A point within a shape (before it's turned), from two numbers in 0…1, spread evenly and
/// kept a little in from its edge.
fn inside(shape: Shape, u: f32, v: f32) -> Vec3 {
    match shape {
        Shape::Circle { radius } => {
            let (t, r) = (u * TAU, v.sqrt() * radius * 0.85);
            Vec3::new(t.sin() * r, 0., t.cos() * r)
        }
        Shape::Cone { range, angle } => {
            let (t, r) = ((u - 0.5) * angle * 0.85, v.sqrt() * range * 0.9);
            Vec3::new(t.sin() * r, 0., t.cos() * r)
        }
        Shape::Line { length, width } => Vec3::new((u - 0.5) * width * 0.85, 0., v * length),
    }
}

fn school_name(school: protocol::School) -> String {
    format!("{school:?}").to_lowercase()
}

/// A share of a radius, in thousandths, as the shapes are keyed.
fn share(v: f32) -> u16 {
    (v.clamp(0.01, 0.95) * 1000.).round() as u16
}

/// The mark for a shape, with its edge band `edge` metres wide, and the scale that sizes it.
fn shape_mark(shape: Shape, edge: f32, profile: Profile) -> (Mesh, Vec3) {
    let band = |size: f32| share((edge / size.max(0.05)).min(0.45));
    match shape {
        Shape::Circle { radius } => (
            Mesh::Mark {
                angle: 3600,
                band: band(radius),
                profile,
            },
            Vec3::new(radius, 1., radius),
        ),
        Shape::Cone { range, angle } => (
            Mesh::Mark {
                angle: (angle.to_degrees().clamp(1., 360.) * 10.).round() as u16,
                band: band(range),
                profile,
            },
            Vec3::new(range, 1., range),
        ),
        Shape::Line { length, width } => (
            Mesh::Lane {
                across: band(width),
                along: band(length),
                profile,
            },
            Vec3::new(width, 1., length),
        ),
    }
}

/// The height to lay a ground mark at: just above the highest ground it covers, so slopes
/// never swallow it (a flat mark floats a little over hollows instead).
fn lift(a: &AreaView, reach: f32, ground: &dyn Fn(f32, f32) -> f32) -> f32 {
    let turn = Quat::from_rotation_y(a.yaw);
    let mut points = vec![Vec3::ZERO];
    match a.shape {
        Shape::Circle { .. } => {
            for k in 0..8 {
                let t = k as f32 * TAU / 8.;
                points.push(Vec3::new(t.sin(), 0., t.cos()) * reach);
                points.push(Vec3::new(t.sin(), 0., t.cos()) * reach * 0.5);
            }
        }
        Shape::Cone { angle, .. } => {
            for k in 0..=4 {
                let t = -angle * 0.5 + angle * k as f32 / 4.;
                points.push(Vec3::new(t.sin(), 0., t.cos()) * reach);
                points.push(Vec3::new(t.sin(), 0., t.cos()) * reach * 0.5);
            }
        }
        Shape::Line { width, length } => {
            for k in 0..=4 {
                let z = length * k as f32 / 4.;
                points.extend([
                    Vec3::new(-width * 0.5, 0., z),
                    Vec3::new(width * 0.5, 0., z),
                ]);
            }
        }
    }
    points
        .into_iter()
        .map(|p| {
            let w = a.center + turn * p;
            ground(w.x, w.z)
        })
        .fold(a.center.y, f32::max)
        + 0.04
}

fn build(mesh: Mesh, glow: &str, hot: bool, g: &Glow, st: &EffectStyle) -> Arc<Model> {
    let (vertices, indices) = shape(mesh);
    let bounds = Bounds::from_points(vertices.iter().map(|v| Vec3::from_slice(&v.position[..3])));
    // A hot core burns brighter and runs towards white, its weaker channels rising most so a
    // fire's heart goes gold rather than pink.
    let colour = Vec3::from(g.colour);
    let emissive = if hot {
        colour.powf(1. - st.core_white.clamp(0., 1.)) * g.strength * st.core_boost
    } else {
        colour * g.strength
    };
    let closed = matches!(mesh, Mesh::Shard | Mesh::Shaft | Mesh::Ball);
    Arc::new(Model {
        id: next_model_id(),
        key: format!("effect:{mesh:?}:{glow}:{hot}"),
        nodes: vec![Node {
            name: "effect".into(),
            parent: None,
            rest: Transform {
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        }],
        rest_world: vec![Mat4::IDENTITY],
        skins: Vec::new(),
        primitives: vec![Primitive {
            vertices,
            indices,
            bounds,
            material: Material {
                name: format!("__effect_{glow}"),
                base: [1.; 4],
                roughness: if solid(g) { 0.7 } else { 1. },
                emissive: emissive.to_array(),
                // Closed shapes show only their near side, so they cost one blended layer.
                double_sided: !closed,
                alpha_mode: if solid(g) { 0 } else { 2 },
                ..Default::default()
            },
            node: 0,
            skin: None,
            morphs: Vec::new(),
            morph_names: Vec::new(),
        }],
        clips: HashMap::new(),
    })
}

fn shape(mesh: Mesh) -> (Vec<Vertex>, Vec<u32>) {
    let mut v = Vec::new();
    let mut i = Vec::new();
    let flat = |p: Vec3, alpha: f32| Vertex::plain(p, Vec3::Y, [1., 1., 1., alpha]);
    match mesh {
        Mesh::Glow | Mesh::Bubble => {
            // Rings of radius and opacity: for a glow, a light's falloff (a gaussian, bright
            // at the heart with a long faint tail, so its size seems to follow its strength);
            // for a bubble, a faint middle rising to its rim as (1 − cos θ)² does on a sphere,
            // then a quick fade just outside so the rim has no hard edge.
            let rings: &[(f32, f32)] = if mesh == Mesh::Glow {
                &[
                    (0., 1.),
                    (0.15, 0.91),
                    (0.3, 0.69),
                    (0.45, 0.43),
                    (0.6, 0.22),
                    (0.8, 0.057),
                    (1., 0.),
                ]
            } else {
                &[
                    (0., 0.06),
                    (0.5, 0.08),
                    (0.75, 0.14),
                    (0.88, 0.26),
                    (0.95, 0.45),
                    (1., 1.),
                    (1.07, 0.),
                ]
            };
            let segs = 32;
            for &(r, a) in rings {
                for k in 0..=segs {
                    let t = TAU * k as f32 / segs as f32;
                    v.push(flat(Vec3::new(t.sin(), 0., t.cos()) * r, a));
                }
            }
            grid(&mut i, rings.len() as u32 - 1, segs);
        }
        Mesh::Streak => {
            // Rows along the streak: where, half-width, opacity. Columns fade to its sides.
            let rows = [
                (0., 0.7, 0.85),
                (-0.06, 1., 1.),
                (-0.2, 0.85, 0.7),
                (-0.45, 0.6, 0.4),
                (-0.75, 0.35, 0.15),
                (-1., 0.12, 0.),
            ];
            let across = [(-1., 0.), (-0.45, 0.6), (0., 1.), (0.45, 0.6), (1., 0.)];
            for (z, w, a) in rows {
                for (x, fade) in across {
                    v.push(flat(Vec3::new(x * w, 0., z), a * fade));
                }
            }
            grid(&mut i, rows.len() as u32 - 1, across.len() as u32 - 1);
        }
        Mesh::Shard => {
            // Tips at either end and a ring of six, widest a little behind the middle.
            v.push(Vertex::plain(Vec3::Z * 0.5, Vec3::Z, [1.; 4]));
            v.push(Vertex::plain(Vec3::Z * -0.5, -Vec3::Z, [1.; 4]));
            for k in 0..6 {
                let t = TAU * k as f32 / 6.;
                let d = Vec3::new(t.cos(), t.sin(), 0.);
                v.push(Vertex::plain(d - Vec3::Z * 0.1, d, [1.; 4]));
            }
            for k in 0..6 {
                let (a, b) = (2 + k, 2 + (k + 1) % 6);
                i.extend([0, a, b, 1, b, a]);
            }
        }
        Mesh::Shaft => {
            let segs = 8;
            for z in [0.5, -0.5] {
                for k in 0..segs {
                    let t = TAU * k as f32 / segs as f32;
                    let n = Vec3::new(t.cos(), t.sin(), 0.);
                    v.push(Vertex::plain(n + Vec3::Z * z, n, [1.; 4]));
                }
            }
            v.push(Vertex::plain(Vec3::Z * 0.5, Vec3::Z, [1.; 4]));
            v.push(Vertex::plain(Vec3::Z * -0.5, -Vec3::Z, [1.; 4]));
            for k in 0..segs {
                let (a, b) = (k, (k + 1) % segs);
                i.extend([a, b, a + segs, b, b + segs, a + segs]);
                i.extend([2 * segs, a, b, 2 * segs + 1, b + segs, a + segs]);
            }
        }
        Mesh::Ball => {
            let (rings, segs) = (8, 12);
            for r in 0..=rings {
                let phi = PI * r as f32 / rings as f32;
                for k in 0..=segs {
                    let theta = TAU * k as f32 / segs as f32;
                    let n = Vec3::new(phi.sin() * theta.sin(), phi.cos(), phi.sin() * theta.cos());
                    v.push(Vertex::plain(n, n, [1.; 4]));
                }
            }
            grid(&mut i, rings, segs);
        }
        Mesh::Band { width, dashes } => {
            let w = width as f32 / 1000.;
            let rows = [(0., 0.), (0.25, 0.6), (0.5, 1.), (0.75, 0.6), (1., 0.)];
            let segs = if dashes > 0 { dashes as u32 * 8 } else { 64 };
            for (t, a) in rows {
                let r = 1. - w + w * t;
                for k in 0..=segs {
                    let around = k as f32 / segs as f32;
                    // Each dash rises and falls smoothly; the gaps keep a trace of light.
                    let dash = if dashes > 0 {
                        ((around * dashes as f32 * TAU).sin() * 2.5 + 0.4).clamp(0.12, 1.)
                    } else {
                        1.
                    };
                    let d = Vec3::new((around * TAU).sin(), 0., (around * TAU).cos());
                    v.push(flat(d * r, a * dash));
                }
            }
            grid(&mut i, rows.len() as u32 - 1, segs);
        }
        Mesh::Mark {
            angle,
            band,
            profile,
        } => {
            let b = band as f32 / 1000.;
            let fan = angle < 3600;
            let half = (angle as f32 / 10.).to_radians() * 0.5;
            // Columns round the shape: even steps, plus extra just inside a fan's sides so
            // their edges glow like the arc's.
            let steps = ((64. * half * 2. / TAU).ceil() as u32).max(8);
            let mut columns: Vec<f32> = (0..=steps)
                .map(|k| -half + 2. * half * k as f32 / steps as f32)
                .collect();
            if fan {
                for t in &KNOTS[1..] {
                    let d = (t * b).min(half);
                    columns.extend([-half + d, half - d]);
                }
                columns.sort_by(f32::total_cmp);
                columns.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
            }
            // Rings in from the edge through the band, then across the body to the centre.
            let mut rings: Vec<f32> = KNOTS
                .iter()
                .map(|t| 1. - t * b)
                .filter(|r| *r > 0.05)
                .collect();
            let inner = *rings.last().unwrap();
            rings.extend([inner * 0.6, inner * 0.3, 0.]);
            for &r in &rings {
                for &c in &columns {
                    let side = if fan {
                        r * (c + half).min(half - c).max(0.).sin()
                    } else {
                        f32::INFINITY
                    };
                    let edge = (1. - r).min(side);
                    v.push(flat(
                        Vec3::new(c.sin(), 0., c.cos()) * r,
                        profile.alpha(edge / b),
                    ));
                }
            }
            grid(&mut i, rings.len() as u32 - 1, columns.len() as u32 - 1);
        }
        Mesh::Lane {
            across,
            along,
            profile,
        } => {
            let (bx, bz) = (across as f32 / 1000., along as f32 / 1000.);
            let lines = |b: f32, half: f32| {
                let mut at: Vec<f32> = KNOTS
                    .iter()
                    .map(|t| t * b)
                    .filter(|d| *d < half)
                    .flat_map(|d| [d, 2. * half - d])
                    .chain([half])
                    .collect();
                at.sort_by(f32::total_cmp);
                at.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
                at
            };
            let (xs, zs) = (lines(bx, 0.5), lines(bz, 0.5));
            for &z in &zs {
                for &x in &xs {
                    let edge = (x.min(1. - x) / bx).min(z.min(1. - z) / bz);
                    v.push(flat(Vec3::new(x - 0.5, 0., z), profile.alpha(edge)));
                }
            }
            grid(&mut i, zs.len() as u32 - 1, xs.len() as u32 - 1);
        }
    }
    // Fronts face outwards on closed shapes, and up on flat ones, where their normals point.
    let closed = matches!(mesh, Mesh::Shard | Mesh::Shaft | Mesh::Ball);
    wind(&v, &mut i, |centre| if closed { centre } else { Vec3::Y });
    (v, i)
}

/// Quads joining `rows + 1` rows of `cols + 1` vertices.
fn grid(i: &mut Vec<u32>, rows: u32, cols: u32) {
    for r in 0..rows {
        for k in 0..cols {
            let a = r * (cols + 1) + k;
            let b = a + cols + 1;
            i.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
}

/// Winds every triangle counter-clockwise seen from the side `out` gives for it (from the
/// sum of its corners), so that side is its front: culled shapes keep it, and double-sided
/// ones are lit on it.
fn wind(v: &[Vertex], i: &mut [u32], out: impl Fn(Vec3) -> Vec3) {
    let at = |k: u32| Vec3::from_slice(&v[k as usize].position[..3]);
    for t in i.chunks_mut(3) {
        let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
        let n = (b - a).cross(c - a);
        if n.dot(out(a + b + c)) < 0. {
            t.swap(1, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{AuraView, CastBar, ProjectileView, School, UnitCombat};

    fn table() -> EffectCatalog {
        assets::Catalog::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .unwrap()
            .effects
            .expect("effects.json")
    }

    fn unit(id: u32) -> UnitState {
        UnitState {
            id,
            kind: protocol::UnitKind::Player,
            body: "player".into(),
            name: "Hero".into(),
            level: 1,
            position: Vec3::new(0., 0., 0.),
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

    fn combat(id: u32) -> UnitCombat {
        UnitCombat {
            id,
            action: Default::default(),
            action_time: 0.,
            cast: None,
            auras: Vec::new(),
            resource_kind: Default::default(),
            resource: 0.,
            max_resource: 0.,
            guarding: false,
            downed: false,
            defiance: None,
        }
    }

    const EYE: Vec3 = Vec3::new(3., 4., 9.);

    fn draw(fx: &mut Effects, fight: &CombatView, units: &[UnitState], time: f32) -> Vec<Draw> {
        fx.draws(&Scene {
            fight,
            units,
            height: &|_| 1.8,
            ground: &|_, _| 0.,
            since: 0.,
            time,
            aim: None,
            eye: EYE,
        })
    }

    fn shield_wall(remaining: f32) -> AuraView {
        AuraView {
            key: "shield_wall".into(),
            name: "Shield Wall".into(),
            stacks: 1,
            remaining,
            duration: 2.5,
            debuff: false,
        }
    }

    #[test]
    fn missiles_casts_and_auras_draw_and_shapes_are_built_once() {
        let mut fx = Effects::new(table());
        let mut fight = CombatView::default();
        fight.projectiles.push(ProjectileView {
            id: 7,
            position: Vec3::new(0., 1.5, 3.),
            velocity: Vec3::new(0., 0., 26.),
            visual: "fire_bolt".into(),
            school: School::Fire,
        });
        let mut me = combat(1);
        me.cast = Some(CastBar {
            ability: "staff_firebolt".into(),
            name: "Firebolt".into(),
            elapsed: 0.5,
            duration: 1.,
            channel: false,
            interruptible: true,
        });
        me.auras.push(shield_wall(2.));
        fight.units.push(me);
        let units = [unit(1)];
        fx.event(
            &Event::ProjectileLaunched {
                id: 7,
                from: 1,
                visual: "fire_bolt".into(),
            },
            0.,
        );
        let first = draw(&mut fx, &fight, &units, 0.5);
        // Core, halo and trail; orb, halo and circle; the bubble.
        assert_eq!(first.len(), 7, "{}", first.len());
        let meshes = fx.shapes.len();
        let again = draw(&mut fx, &fight, &units, 0.6);
        assert_eq!(fx.shapes.len(), meshes, "shapes are reused, not rebuilt");
        // Ids are stable between frames, so motion vectors line up.
        let ids = |d: &[Draw]| d.iter().map(|d| d.instance).collect::<Vec<_>>();
        assert_eq!(ids(&first), ids(&again));
        let unique: std::collections::HashSet<_> = ids(&first).into_iter().collect();
        assert_eq!(unique.len(), first.len());
        assert!(
            first
                .iter()
                .all(|d| d.model.primitives[0].material.alpha_mode == 2)
        );
    }

    #[test]
    fn lights_face_the_camera_and_streaks_lie_along_their_flight() {
        let mut fx = Effects::new(table());
        let mut fight = CombatView::default();
        let (at, velocity) = (Vec3::new(1., 1.5, -2.), Vec3::new(20., 0., 10.));
        fight.projectiles.push(ProjectileView {
            id: 2,
            position: at,
            velocity,
            visual: "holy_bolt".into(),
            school: School::Holy,
        });
        fx.event(
            &Event::ProjectileLaunched {
                id: 2,
                from: 1,
                visual: "holy_bolt".into(),
            },
            0.,
        );
        let d = draw(&mut fx, &fight, &[], 1.);
        let (core, streak) = (&d[0], &d[2]);
        // A sprite's face (its local y) points at the eye from where it is.
        let facing = core.transform.transform_vector3(Vec3::Y).normalize();
        assert!(facing.dot((EYE - at).normalize()) > 0.999);
        // Turned, never mirrored, so their fronts stay fronts.
        assert!(core.transform.determinant() > 0. && streak.transform.determinant() > 0.);
        // A streak runs back along the flight from the missile, its face turned to the eye.
        let back = streak.transform.transform_vector3(-Vec3::Z);
        assert!(back.normalize().dot(-velocity.normalize()) > 0.999);
        let face = streak.transform.transform_vector3(Vec3::Y);
        assert!(face.dot(EYE - at) > 0. && face.dot(velocity).abs() < 1e-3);
    }

    #[test]
    fn arrows_and_axes_are_solid() {
        let mut fx = Effects::new(table());
        let mut fight = CombatView::default();
        for (id, visual) in [(1, "arrow"), (2, "axe")] {
            fight.projectiles.push(ProjectileView {
                id,
                position: Vec3::new(id as f32, 1.5, 0.),
                velocity: Vec3::new(0., 0., 30.),
                visual: visual.into(),
                school: School::Physical,
            });
        }
        let d = draw(&mut fx, &fight, &[], 0.);
        let solid: Vec<_> = d
            .iter()
            .filter(|d| d.model.primitives[0].material.alpha_mode == 0)
            .collect();
        // The shaft, and the axe's head and haft; a new missile has no streak yet.
        assert_eq!((solid.len(), d.len()), (3, 3));
        assert!(solid.iter().all(|d| d.base[3] == 1.));
    }

    #[test]
    fn a_landed_missile_flashes_then_fades() {
        let mut fx = Effects::new(table());
        let fight = CombatView::default();
        fx.event(
            &Event::ProjectileLaunched {
                id: 3,
                from: 1,
                visual: "arrow".into(),
            },
            0.,
        );
        fx.event(
            &Event::ProjectileEnded {
                id: 3,
                at: Vec3::new(1., 1., 1.),
                hit: None,
            },
            0.,
        );
        fx.event(
            &Event::AreaImpact {
                id: 9,
                at: Vec3::ZERO,
                radius: 3.,
                school: School::Frost,
            },
            0.,
        );
        // The arrow's flash (heart, burst and sparks); the impact's shock ring and flash.
        let d = draw(&mut fx, &fight, &[], 0.1);
        assert_eq!(d.len(), 2 + SPARKS as usize + 2);
        let unique: std::collections::HashSet<_> = d.iter().map(|d| d.instance).collect();
        assert_eq!(unique.len(), d.len());
        assert!(draw(&mut fx, &fight, &[], 1.).is_empty());
        assert!(fx.bursts.is_empty() && fx.flights.is_empty());
    }

    #[test]
    fn warnings_fill_as_they_ripen() {
        let mut fx = Effects::new(table());
        let mut fight = CombatView::default();
        fight.areas.push(AreaView {
            id: 4,
            warning: true,
            shape: Shape::Cone {
                range: 6.,
                angle: 1.2,
            },
            center: Vec3::ZERO,
            yaw: 0.5,
            elapsed: 0.25,
            duration: 1.,
            hostile: true,
            school: School::Physical,
            field: None,
        });
        let d = draw(&mut fx, &fight, &[], 0.);
        assert_eq!(d.len(), 2);
        let reach = |d: &Draw| d.transform.transform_vector3(Vec3::Z).length();
        assert!((reach(&d[0]) - 6.).abs() < 1e-4 && (reach(&d[1]) - 1.5).abs() < 1e-4);
        // The mark threatens the viewer, so it takes the hostile glow.
        let red = fx.table.glows["hostile"]
            .colour
            .map(|c| c * fx.table.glows["hostile"].strength);
        assert_eq!(d[0].model.primitives[0].material.emissive, red);
    }

    #[test]
    fn embers_rise_from_within_a_field_and_start_over_elsewhere() {
        let mut fx = Effects::new(table());
        let mut fight = CombatView::default();
        fight.areas.push(AreaView {
            id: 6,
            warning: false,
            shape: Shape::Circle { radius: 2. },
            center: Vec3::new(10., 0., 5.),
            yaw: 0.,
            elapsed: 1.,
            duration: 8.,
            hostile: true,
            school: School::Fire,
            field: Some("fire".into()),
        });
        let embers = |fx: &mut Effects, time| {
            let d = draw(fx, &fight, &[], time);
            assert_eq!(d.len(), 2 + 5, "pool, rim and five embers");
            d[2..]
                .iter()
                .map(|d| (d.instance, d.transform.w_axis.truncate()))
                .collect::<Vec<_>>()
        };
        for (_, at) in embers(&mut fx, 3.) {
            let off = at - Vec3::new(10., 0., 5.);
            assert!(
                Vec2::new(off.x, off.z).length() < 2. && (0.04..1.3).contains(&off.y),
                "{off}"
            );
        }
        // One life later each ember rises from somewhere new, under a new id.
        let (a, b) = (embers(&mut fx, 3.), embers(&mut fx, 3. + EMBER_SECONDS));
        for ((id_a, at_a), (id_b, at_b)) in a.iter().zip(&b) {
            assert!(id_a != id_b && at_a.distance(*at_b) > 1e-3);
        }
    }

    #[test]
    fn marks_glow_at_their_edges_and_fade_out_beyond_them() {
        for profile in [Profile::Edge, Profile::Fill, Profile::Pool, Profile::Rim] {
            assert_eq!(profile.alpha(0.), 0., "no hard outer edge");
            assert_eq!(profile.alpha(KNOTS[1]), 1., "brightest just inside");
            assert!(profile.alpha(5.) < profile.alpha(0.6));
        }
        for mesh in [
            Mesh::Mark {
                angle: 3600,
                band: 80,
                profile: Profile::Edge,
            },
            Mesh::Mark {
                angle: 700,
                band: 60,
                profile: Profile::Fill,
            },
            Mesh::Lane {
                across: 200,
                along: 40,
                profile: Profile::Pool,
            },
        ] {
            let (v, i) = shape(mesh);
            assert!(i.iter().all(|&k| (k as usize) < v.len()), "{mesh:?}");
            assert!(i.len().is_multiple_of(3));
            // Every vertex lies within the unit shape, and its brightest part is the edge.
            let peak = v.iter().map(|v| v.color[3]).fold(0., f32::max);
            assert!((peak - 1.).abs() < 1e-3, "{mesh:?}: {peak}");
        }
    }

    #[test]
    fn mark_keys_are_bounded_whatever_the_size() {
        let (tiny, _) = shape_mark(Shape::Circle { radius: 0.01 }, 0.2, Profile::Edge);
        let (huge, _) = shape_mark(Shape::Circle { radius: 1e4 }, 0.2, Profile::Edge);
        assert_eq!(
            (tiny, huge),
            (
                Mesh::Mark {
                    angle: 3600,
                    band: 450,
                    profile: Profile::Edge
                },
                Mesh::Mark {
                    angle: 3600,
                    band: 10,
                    profile: Profile::Edge
                }
            )
        );
        let (cone, _) = shape_mark(
            Shape::Cone {
                range: 3.,
                angle: TAU * 2.,
            },
            0.2,
            Profile::Fill,
        );
        assert!(matches!(cone, Mesh::Mark { angle: 3600, .. }));
    }

    #[test]
    fn closed_shapes_face_outwards_and_flat_ones_up() {
        let flat = [
            Mesh::Glow,
            Mesh::Bubble,
            Mesh::Streak,
            Mesh::Band {
                width: 140,
                dashes: 12,
            },
            Mesh::Mark {
                angle: 900,
                band: 50,
                profile: Profile::Edge,
            },
            Mesh::Lane {
                across: 100,
                along: 30,
                profile: Profile::Rim,
            },
        ];
        for mesh in [Mesh::Shard, Mesh::Shaft, Mesh::Ball]
            .into_iter()
            .chain(flat)
        {
            let (v, i) = shape(mesh);
            let at = |k: u32| Vec3::from_slice(&v[k as usize].position[..3]);
            for t in i.chunks(3) {
                let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
                let n = (b - a).cross(c - a);
                let out = if flat.contains(&mesh) {
                    Vec3::Y
                } else {
                    a + b + c
                };
                assert!(n.length() < 1e-6 || n.dot(out) > 0., "{mesh:?}");
            }
        }
    }
}
