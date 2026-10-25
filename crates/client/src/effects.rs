//! Combat drawn in the world from `assets/catalog/effects.json`: missiles in flight, the glow
//! in a caster's hands, auras on bodies, warnings and fields on the ground, and the flash where
//! things land. Every look value comes from the table; this module only supplies the shapes
//! (spheres, streaks, discs, rings, fans and strips), built once and placed by transform.
use assets::{AuraKind, EffectCatalog, ProjectileKind};
use glam::{Mat4, Quat, Vec3};
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

/// Unit-sized shapes. Rings, fans and rims are keyed by their proportions so a few meshes
/// serve every size; the set stays small because the content's shapes are few.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Mesh {
    /// Radius 1.
    Sphere,
    /// A cone from radius 1 at z = 0 to a point at z = −1, fading towards the point.
    Trail,
    /// A cylinder of radius 1 from z = −0.5 to 0.5.
    Shaft,
    /// A flat disc of radius 1 on y = 0.
    Disc,
    /// A flat band from 1 − width to 1, width in thousandths.
    Ring(u16),
    /// A flat fan of radius 1 about +z, the full angle in tenths of a degree.
    Fan(u16),
    /// A flat strip x ∈ ±0.5, z ∈ 0…1.
    Strip,
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

pub struct Effects {
    table: EffectCatalog,
    meshes: HashMap<(Mesh, String), Arc<Model>>,
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
}

impl Effects {
    pub fn new(table: EffectCatalog) -> Self {
        Self {
            table,
            meshes: HashMap::new(),
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
        for p in &s.fight.projectiles {
            let born = self
                .flights
                .entry(p.id)
                .or_insert(Flight {
                    visual: p.visual.clone(),
                    born: s.time,
                })
                .born;
            self.projectile(&mut out, p, s.since, s.time, s.time - born);
        }
        for a in &s.fight.areas {
            self.area(&mut out, a, s);
        }
        if let Some((at, radius)) = s.aim {
            let aim = AreaView {
                id: u32::MAX,
                warning: false,
                shape: Shape::Circle { radius },
                center: at,
                yaw: 0.,
                elapsed: 0.,
                duration: 0.,
                hostile: false,
                school: protocol::School::Physical,
                field: None,
            };
            self.area(&mut out, &aim, s);
        }
        for c in &s.fight.units {
            let Some(u) = by_id.get(&c.id) else { continue };
            if u.health <= 0. {
                continue;
            }
            let h = (s.height)(u.id);
            if let Some(cast) = &c.cast
                && let Some(glow) = self.table.casts.get(&cast.ability).cloned()
            {
                let progress = if cast.channel {
                    1.
                } else {
                    ((cast.elapsed + s.since) / cast.duration.max(0.001)).clamp(0., 1.)
                };
                self.cast(&mut out, u, h, &glow, progress, s);
            }
            for (i, a) in c.auras.iter().enumerate().take(64) {
                let Some(look) = self.table.auras.get(&a.key).cloned() else {
                    continue;
                };
                let f = self.table.style.aura_fade.max(0.001);
                let fade = if a.duration > 0. {
                    ((a.duration - a.remaining) / f).clamp(0., 1.) * (a.remaining / f).clamp(0., 1.)
                } else {
                    1.
                };
                self.aura(&mut out, u, h, &look, fade, i as u64, s);
            }
        }
        let seconds = self.table.style.burst_seconds.max(0.01);
        self.bursts.retain(|b| s.time - b.born < seconds);
        let bursts = std::mem::take(&mut self.bursts);
        for b in &bursts {
            self.flash(&mut out, b, s, seconds);
        }
        self.bursts = bursts;
        out
    }

    fn projectile(
        &mut self,
        out: &mut Vec<Draw>,
        p: &protocol::ProjectileView,
        since: f32,
        time: f32,
        age: f32,
    ) {
        let Some(look) = self.table.projectiles.get(&p.visual).cloned() else {
            return;
        };
        let st = self.table.style.clone();
        let at = p.position + p.velocity * since.clamp(0., 0.1);
        let speed = p.velocity.length();
        let dir = p.velocity.normalize_or(Vec3::Z);
        let turn = Quat::from_rotation_arc(Vec3::Z, dir);
        let glow = look.glow.as_str();
        let trail_glow = look.trail_glow.as_deref().unwrap_or(glow);
        let place = |scale: Vec3| Mat4::from_scale_rotation_translation(scale, turn, at);
        let id = |part| instance(1, p.id, part);
        match look.kind {
            ProjectileKind::Bolt => {
                self.push(
                    out,
                    Mesh::Sphere,
                    glow,
                    place(Vec3::splat(look.size)),
                    st.core_alpha,
                    id(0),
                );
                if look.halo > 0. {
                    let pulse = 1. + 0.1 * (time * 19. + p.id as f32).sin();
                    let halo = place(Vec3::splat(look.halo * pulse));
                    self.push(out, Mesh::Sphere, trail_glow, halo, st.halo_alpha, id(1));
                }
            }
            ProjectileKind::Lance => {
                let shard = place(Vec3::new(look.size, look.size, look.length * 0.5));
                self.push(out, Mesh::Sphere, glow, shard, st.core_alpha, id(0));
                if look.halo > 0. {
                    let halo = place(Vec3::new(look.halo, look.halo, look.length * 0.6));
                    self.push(out, Mesh::Sphere, trail_glow, halo, st.halo_alpha, id(1));
                }
            }
            ProjectileKind::Arrow => {
                // The shaft's tip leads: it sits just behind the arrow's position.
                let shaft = Mat4::from_scale_rotation_translation(
                    Vec3::new(look.size, look.size, look.length),
                    turn,
                    at - dir * look.length * 0.5,
                );
                self.push(out, Mesh::Shaft, glow, shaft, 1., id(0));
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
                self.push(out, Mesh::Sphere, glow, head, 1., id(0));
                self.push(out, Mesh::Shaft, "wood", haft, 1., id(1));
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
            let streak = Mat4::from_scale_rotation_translation(
                Vec3::new(look.trail_width, look.trail_width, trail),
                turn,
                back,
            );
            self.push(out, Mesh::Trail, trail_glow, streak, st.trail_alpha, id(2));
        }
    }

    fn area(&mut self, out: &mut Vec<Draw>, a: &AreaView, s: &Scene) {
        let st = self.table.style.clone();
        let mark = if a.hostile { "hostile" } else { "friendly" };
        let field = a
            .field
            .as_ref()
            .map(|f| format!("field.{f}"))
            .filter(|g| self.table.glows.contains_key(g));
        // Warnings say who they threaten; fields show what they are, rimmed by who they hurt.
        let fill = if a.warning {
            mark.to_string()
        } else {
            field.unwrap_or_else(|| mark.into())
        };
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
        let (mesh, size) = match a.shape {
            Shape::Circle { radius } => (Mesh::Disc, Vec3::new(radius, 1., radius)),
            Shape::Cone { range, angle } => (fan(angle), Vec3::new(range, 1., range)),
            Shape::Line { length, width } => (Mesh::Strip, Vec3::new(width, 1., length)),
        };
        if a.warning {
            let progress = ((a.elapsed + s.since) / a.duration.max(0.001)).clamp(0., 1.);
            self.push(
                out,
                mesh,
                &fill,
                flat(size),
                st.warning_outline_alpha,
                id(0),
            );
            // Circles fill outwards; cones and lines from their origin to their far edge.
            let grown = match a.shape {
                Shape::Line { .. } => size * Vec3::new(1., 1., progress),
                _ => size * Vec3::new(progress, 1., progress),
            };
            self.push(out, mesh, &fill, flat(grown), st.warning_fill_alpha, id(1));
        } else {
            let pulse = 0.85 + 0.15 * (s.time * 2.4 + a.id as f32).sin();
            self.push(out, mesh, &fill, flat(size), st.field_alpha * pulse, id(0));
        }
        if let Shape::Circle { radius } = a.shape {
            let rim = ring(st.rim_width / radius.max(0.1));
            let edge = if a.warning {
                st.warning_fill_alpha
            } else {
                st.field_rim_alpha
            };
            self.push(
                out,
                rim,
                mark,
                flat(Vec3::new(radius, 1., radius)),
                edge,
                id(2),
            );
        }
    }

    fn cast(
        &mut self,
        out: &mut Vec<Draw>,
        u: &UnitState,
        h: f32,
        glow: &str,
        progress: f32,
        s: &Scene,
    ) {
        let st = self.table.style.clone();
        let forward = Vec3::new(u.yaw.sin(), 0., u.yaw.cos());
        let flicker = 1. + 0.08 * (s.time * 23. + u.id as f32).sin();
        let r = (st.cast_orb[0] + (st.cast_orb[1] - st.cast_orb[0]) * progress) * flicker;
        let hands = u.position + Vec3::Y * h * 0.62 + forward * 0.42;
        let id = |part| instance(3, u.id, part);
        self.push(
            out,
            Mesh::Sphere,
            glow,
            Mat4::from_scale_rotation_translation(Vec3::splat(r), Quat::IDENTITY, hands),
            st.cast_orb_alpha,
            id(0),
        );
        self.push(
            out,
            Mesh::Sphere,
            glow,
            Mat4::from_scale_rotation_translation(Vec3::splat(r * 2.4), Quat::IDENTITY, hands),
            st.halo_alpha * progress,
            id(1),
        );
        let feet = Vec3::new(
            u.position.x,
            (s.ground)(u.position.x, u.position.z) + 0.03,
            u.position.z,
        );
        let circle = Mat4::from_scale_rotation_translation(
            Vec3::new(st.cast_circle, 1., st.cast_circle),
            Quat::IDENTITY,
            feet,
        );
        self.push(
            out,
            ring(st.ring_width),
            glow,
            circle,
            st.cast_circle_alpha * (0.5 + 0.5 * progress),
            id(2),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn aura(
        &mut self,
        out: &mut Vec<Draw>,
        u: &UnitState,
        h: f32,
        look: &assets::AuraLook,
        fade: f32,
        index: u64,
        s: &Scene,
    ) {
        let st = self.table.style.clone();
        let id = |part| instance(4, u.id, index * 8 + part);
        let pulse = 0.85 + 0.15 * (s.time * 3. + index as f32).sin();
        match look.kind {
            AuraKind::Shell => {
                let r = look.size * h;
                let shell = Mat4::from_scale_rotation_translation(
                    Vec3::new(r * 0.75, r, r * 0.75),
                    Quat::IDENTITY,
                    u.position + Vec3::Y * h * 0.5,
                );
                self.push(
                    out,
                    Mesh::Sphere,
                    &look.glow,
                    shell,
                    st.shell_alpha * fade * pulse,
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
                let band = Mat4::from_scale_rotation_translation(
                    Vec3::new(r, 1., r),
                    Quat::IDENTITY,
                    feet,
                );
                self.push(
                    out,
                    ring(st.ring_width),
                    &look.glow,
                    band,
                    st.ring_alpha * fade,
                    id(0),
                );
            }
            AuraKind::Halo => {
                let crown = u.position + Vec3::Y * (h + 0.2);
                let band = Mat4::from_scale_rotation_translation(
                    Vec3::new(look.size, 1., look.size),
                    Quat::IDENTITY,
                    crown,
                );
                self.push(
                    out,
                    ring(st.ring_width),
                    &look.glow,
                    band,
                    st.halo_ring_alpha * fade * 0.4,
                    id(0),
                );
                // Three motes circling the head.
                for k in 0..3 {
                    let a = s.time * 3.5 + k as f32 * TAU / 3.;
                    let p = crown + Vec3::new(a.sin(), 0., a.cos()) * look.size;
                    let mote = Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.045),
                        Quat::IDENTITY,
                        p,
                    );
                    self.push(
                        out,
                        Mesh::Sphere,
                        &look.glow,
                        mote,
                        st.halo_ring_alpha * fade,
                        id(1 + k),
                    );
                }
            }
        }
    }

    fn flash(&mut self, out: &mut Vec<Draw>, b: &Burst, s: &Scene, seconds: f32) {
        let st = self.table.style.clone();
        let t = ((s.time - b.born) / seconds).clamp(0., 1.);
        let grow = 1. - (1. - t) * (1. - t);
        let alpha = st.burst_alpha * (1. - t) * (1. - t);
        let id = |part| instance(5, b.id, part);
        if b.ground {
            let y = (s.ground)(b.at.x, b.at.z).max(b.at.y) + 0.05;
            let at = Vec3::new(b.at.x, y, b.at.z);
            let r = b.radius * (0.3 + 0.7 * grow);
            let flat =
                Mat4::from_scale_rotation_translation(Vec3::new(r, 1., r), Quat::IDENTITY, at);
            self.push(out, Mesh::Disc, &b.glow, flat, alpha * 0.6, id(0));
            self.push(out, ring(st.ring_width), &b.glow, flat, alpha, id(1));
            let dome = Mat4::from_scale_rotation_translation(
                Vec3::new(r * 0.6, r * 0.35, r * 0.6),
                Quat::IDENTITY,
                at,
            );
            self.push(out, Mesh::Sphere, &b.glow, dome, alpha * 0.5, id(2));
        } else {
            let r = b.radius * (0.35 + 0.65 * grow);
            let ball = Mat4::from_scale_rotation_translation(Vec3::splat(r), Quat::IDENTITY, b.at);
            self.push(out, Mesh::Sphere, &b.glow, ball, alpha, id(0));
        }
    }

    fn push(
        &mut self,
        out: &mut Vec<Draw>,
        mesh: Mesh,
        glow: &str,
        transform: Mat4,
        alpha: f32,
        instance: u64,
    ) {
        if alpha <= 0.002 {
            return;
        }
        let Some(g) = self.table.glow(glow).copied() else {
            return;
        };
        let model = self
            .meshes
            .entry((mesh, glow.to_string()))
            .or_insert_with(|| build(mesh, glow, &g))
            .clone();
        let c = g.colour.map(|v| v * g.albedo);
        out.push(Draw {
            dye: None,
            base_map: None,
            instance,
            model,
            primitive: 0,
            transform,
            palette: Arc::from([]),
            hidden: 0,
            base: [c[0], c[1], c[2], alpha.min(1.)],
            morph: [0.; 2],
        });
    }
}

fn school_name(school: protocol::School) -> String {
    format!("{school:?}").to_lowercase()
}

fn ring(width: f32) -> Mesh {
    Mesh::Ring((width.clamp(0.01, 0.95) * 1000.).round() as u16)
}

fn fan(angle: f32) -> Mesh {
    Mesh::Fan((angle.to_degrees().clamp(1., 360.) * 10.).round() as u16)
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

fn build(mesh: Mesh, glow: &str, g: &assets::Glow) -> Arc<Model> {
    let (vertices, indices) = shape(mesh);
    let bounds = Bounds::from_points(vertices.iter().map(|v| Vec3::from_slice(&v.position[..3])));
    Arc::new(Model {
        id: next_model_id(),
        key: format!("effect:{mesh:?}:{glow}"),
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
                roughness: 1.,
                emissive: g.colour.map(|c| c * g.strength),
                double_sided: true,
                alpha_mode: 2,
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
    let solid = [1.; 4];
    match mesh {
        Mesh::Sphere => {
            let (rings, segs) = (8, 14);
            for r in 0..=rings {
                let phi = PI * r as f32 / rings as f32;
                for k in 0..=segs {
                    let theta = TAU * k as f32 / segs as f32;
                    let n = Vec3::new(phi.sin() * theta.sin(), phi.cos(), phi.sin() * theta.cos());
                    v.push(Vertex::plain(n, n, solid));
                }
            }
            grid(&mut i, rings, segs);
        }
        Mesh::Trail | Mesh::Shaft => {
            let segs = 10;
            for (z, r, a) in if mesh == Mesh::Trail {
                [(0., 1., 1.), (-1., 0.05, 0.)]
            } else {
                [(0.5, 1., 1.), (-0.5, 1., 1.)]
            } {
                for k in 0..=segs {
                    let t = TAU * k as f32 / segs as f32;
                    let n = Vec3::new(t.sin(), t.cos(), 0.);
                    v.push(Vertex::plain(
                        Vec3::new(n.x * r, n.y * r, z),
                        n,
                        [1., 1., 1., a],
                    ));
                }
            }
            grid(&mut i, 1, segs);
        }
        Mesh::Disc | Mesh::Fan(_) => {
            let (from, to, segs) = match mesh {
                Mesh::Fan(tenths) => {
                    let angle = (tenths as f32 / 10.).to_radians();
                    (
                        -angle * 0.5,
                        angle * 0.5,
                        ((angle / TAU) * 40.).ceil().max(4.) as u32,
                    )
                }
                _ => (0., TAU, 40),
            };
            v.push(Vertex::plain(Vec3::ZERO, Vec3::Y, solid));
            for k in 0..=segs {
                let t = from + (to - from) * k as f32 / segs as f32;
                v.push(Vertex::plain(
                    Vec3::new(t.sin(), 0., t.cos()),
                    Vec3::Y,
                    solid,
                ));
            }
            for k in 1..=segs {
                i.extend([0, k, k + 1]);
            }
        }
        Mesh::Ring(thousandths) => {
            let inner = 1. - thousandths as f32 / 1000.;
            let segs = 48;
            for k in 0..=segs {
                let t = TAU * k as f32 / segs as f32;
                let d = Vec3::new(t.sin(), 0., t.cos());
                v.push(Vertex::plain(d * inner, Vec3::Y, solid));
                v.push(Vertex::plain(d, Vec3::Y, solid));
            }
            for k in 0..segs {
                let a = k * 2;
                i.extend([a, a + 1, a + 2, a + 2, a + 1, a + 3]);
            }
        }
        Mesh::Strip => {
            for (x, z) in [(-0.5, 0.), (0.5, 0.), (-0.5, 1.), (0.5, 1.)] {
                v.push(Vertex::plain(Vec3::new(x, 0., z), Vec3::Y, solid));
            }
            i.extend([0, 2, 1, 1, 2, 3]);
        }
    }
    (v, i)
}

/// Quads joining `rows + 1` rings of `cols + 1` vertices.
fn grid(i: &mut Vec<u32>, rows: u32, cols: u32) {
    for r in 0..rows {
        for k in 0..cols {
            let a = r * (cols + 1) + k;
            let b = a + cols + 1;
            i.extend([a, b, a + 1, a + 1, b, b + 1]);
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

    fn draw(fx: &mut Effects, fight: &CombatView, units: &[UnitState], time: f32) -> Vec<Draw> {
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
        me.auras.push(AuraView {
            key: "shield_wall".into(),
            name: "Shield Wall".into(),
            stacks: 1,
            remaining: 2.,
            duration: 2.5,
            debuff: false,
        });
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
        // Core, halo and trail; orb, halo and circle; the shell.
        assert_eq!(first.len(), 7, "{}", first.len());
        let meshes = fx.meshes.len();
        let again = draw(&mut fx, &fight, &units, 0.6);
        assert_eq!(fx.meshes.len(), meshes, "shapes are reused, not rebuilt");
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
        assert_eq!(draw(&mut fx, &fight, &[], 0.1).len(), 4);
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
    fn ring_and_fan_keys_are_bounded() {
        assert_eq!(ring(0.0), Mesh::Ring(10));
        assert_eq!(ring(4.), Mesh::Ring(950));
        assert_eq!(fan(TAU * 2.), Mesh::Fan(3600));
        let (v, i) = shape(Mesh::Fan(900));
        assert!(i.iter().all(|&k| (k as usize) < v.len()));
    }
}
