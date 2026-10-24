//! Functional protocol-v4 overlays. Shapes and text are diagnostics, awaiting supplied effect art.
use egui::{Color32, Pos2, Stroke};
use glam::{Mat4, Vec2, Vec3};
use protocol::{AreaView, CombatView, Event, Shape, UnitState};
use std::collections::HashMap;

pub struct Floating {
    position: Vec3,
    text: String,
    colour: Color32,
    crit: bool,
    age: f32,
}
#[derive(Default)]
pub struct Feedback {
    entries: Vec<Floating>,
}
impl Feedback {
    pub fn event(&mut self, event: &Event, units: &HashMap<u32, &UnitState>) {
        let (target, text, colour, crit) = match event {
            Event::Damage {
                target,
                amount,
                crit,
                periodic,
                ..
            } => (
                *target,
                format!(
                    "{}{amount:.0}{}",
                    if *periodic { "tick " } else { "" },
                    if *crit { "!" } else { "" }
                ),
                Color32::LIGHT_RED,
                *crit,
            ),
            Event::Heal {
                target,
                amount,
                crit,
                ..
            } => (
                *target,
                format!("+{amount:.0}{}", if *crit { "!" } else { "" }),
                Color32::LIGHT_GREEN,
                *crit,
            ),
            Event::Avoided { target, how, .. } => (
                *target,
                format!("{how:?}").to_lowercase(),
                Color32::WHITE,
                false,
            ),
            _ => return,
        };
        if let Some(unit) = units.get(&target) {
            // Offset simultaneous ticks so words remain readable; bounded independently of frame rate.
            let offset = self
                .entries
                .iter()
                .filter(|e| e.age < 0.1 && e.position.distance(unit.position + Vec3::Y * 2.) < 2.)
                .count() as f32
                * 0.25;
            self.entries.push(Floating {
                position: unit.position + Vec3::Y * (2. + offset),
                text,
                colour,
                crit,
                age: 0.,
            });
            if self.entries.len() > 256 {
                self.entries.remove(0);
            }
        }
    }
    pub fn update(&mut self, dt: f32) {
        for e in &mut self.entries {
            e.age += dt;
        }
        self.entries.retain(|e| e.age < 1.5);
    }
    pub fn paint(&self, painter: &egui::Painter, projection: &Projection) {
        for e in &self.entries {
            if let Some(p) = projection.point(e.position + Vec3::Y * e.age * 0.7) {
                painter.text(
                    p,
                    egui::Align2::CENTER_CENTER,
                    &e.text,
                    egui::FontId::proportional(if e.crit { 22. } else { 16. }),
                    e.colour.gamma_multiply((1.5 - e.age).min(1.)),
                );
            }
        }
    }
}
pub struct Projection {
    pub matrix: Mat4,
    pub size: Vec2,
}
impl Projection {
    pub fn point(&self, p: Vec3) -> Option<Pos2> {
        let p = self.matrix * p.extend(1.);
        if p.w <= 0. || p.z < 0. {
            return None;
        }
        let ndc = p.truncate() / p.w;
        Some(Pos2::new(
            (ndc.x + 1.) * 0.5 * self.size.x,
            (1. - ndc.y) * 0.5 * self.size.y,
        ))
    }
}
/// Ground ray uses the collision surface, including bridge decks. Bisection refines the first hit.
pub fn ground_point(
    matrix: Mat4,
    cursor: Vec2,
    size: Vec2,
    valley: &world::Valley,
) -> Option<Vec3> {
    let inverse = matrix.inverse();
    let ndc = cursor / size * Vec2::new(2., -2.) + Vec2::new(-1., 1.);
    let near = inverse.project_point3(Vec3::new(ndc.x, ndc.y, 0.));
    let far = inverse.project_point3(Vec3::new(ndc.x, ndc.y, 1.));
    let direction = (far - near).normalize_or_zero();
    let above = |t: f32| {
        let p = near + direction * t;
        p.y - valley.ground(p.x, p.z)
    };
    let mut low = 0.;
    for step in 1..=1600 {
        let high = step as f32 * 0.5;
        if above(high) <= 0. && above(low) > 0. {
            let mut a = low;
            let mut b = high;
            for _ in 0..16 {
                let mid = (a + b) * 0.5;
                if above(mid) > 0. {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            let p = near + direction * ((a + b) * 0.5);
            return (p.x.abs() <= world::BOUNDS && p.z.abs() <= world::BOUNDS).then_some(p);
        }
        low = high;
    }
    None
}
/// Vertices around the authoritative area footprint (+Z forward), before ground projection.
pub fn outline(shape: Shape, center: Vec3, yaw: f32, fraction: f32) -> Vec<Vec3> {
    let fraction = fraction.clamp(0., 1.);
    let rotate = |x: f32, z: f32| {
        center
            + Vec3::new(
                x * yaw.cos() + z * yaw.sin(),
                0.,
                z * yaw.cos() - x * yaw.sin(),
            )
    };
    match shape {
        Shape::Circle { radius } => (0..64)
            .map(|i| {
                let a = i as f32 / 64. * std::f32::consts::TAU;
                rotate(a.sin() * radius * fraction, a.cos() * radius * fraction)
            })
            .collect(),
        Shape::Cone { range, angle } => std::iter::once(center)
            .chain((0..=32).map(|i| {
                let a = -angle * 0.5 + angle * i as f32 / 32.;
                rotate(a.sin() * range * fraction, a.cos() * range * fraction)
            }))
            .collect(),
        Shape::Line { length, width } => vec![
            rotate(-width * 0.5, 0.),
            rotate(width * 0.5, 0.),
            rotate(width * 0.5, length * fraction),
            rotate(-width * 0.5, length * fraction),
        ],
    }
}
fn area(painter: &egui::Painter, projection: &Projection, a: &AreaView, valley: &world::Valley) {
    let colour = if a.hostile {
        Color32::RED
    } else {
        Color32::LIGHT_BLUE
    };
    let points = |fraction| {
        outline(a.shape, a.center, a.yaw, fraction)
            .into_iter()
            .map(|mut p| {
                p.y = valley.ground(p.x, p.z) + 0.04;
                projection.point(p)
            })
            .collect::<Option<Vec<_>>>()
    };
    if let Some(p) = points(1.) {
        painter.add(egui::Shape::closed_line(p, Stroke::new(2_f32, colour)));
    }
    let progress = if a.warning {
        (a.elapsed / a.duration.max(0.001)).clamp(0., 1.)
    } else {
        1.
    };
    if let Some(points) = points(progress) {
        // A fan supports wide cones and terrain-projected polygons without convexity assumptions.
        if points.len() >= 3 {
            let mut mesh = egui::Mesh::default();
            for p in &points {
                mesh.colored_vertex(*p, colour.gamma_multiply(0.2));
            }
            for i in 1..points.len() - 1 {
                mesh.add_triangle(0, i as u32, i as u32 + 1);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    if let Some(p) = projection.point(a.center + Vec3::Y * 0.1) {
        let label = if a.warning {
            format!("{:.1}s", (a.duration - a.elapsed).max(0.))
        } else {
            a.field.clone().unwrap_or_else(|| "field".into())
        };
        painter.text(
            p,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.),
            colour,
        );
    }
}
pub fn paint(
    ctx: &egui::Context,
    projection: &Projection,
    fight: &CombatView,
    units: &[UnitState],
    feedback: &Feedback,
    ground: Option<(Vec3, f32)>,
    valley: &world::Valley,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("combat-world"),
    ));
    for a in &fight.areas {
        area(&painter, projection, a, valley);
    }
    if let Some((center, radius)) = ground {
        area(
            &painter,
            projection,
            &AreaView {
                id: 0,
                warning: true,
                shape: Shape::Circle { radius },
                center,
                yaw: 0.,
                elapsed: 0.,
                duration: 1.,
                hostile: false,
                school: protocol::School::Physical,
                field: None,
            },
            valley,
        );
    }
    for p in &fight.projectiles {
        if let Some(at) = projection.point(p.position) {
            if let Some(tail) = projection.point(p.position - p.velocity.normalize_or_zero() * 0.5)
            {
                painter.line_segment([tail, at], Stroke::new(2_f32, Color32::WHITE));
            }
            painter.circle_filled(at, 3., Color32::WHITE);
            painter.text(
                at + egui::vec2(5., -5.),
                egui::Align2::LEFT_BOTTOM,
                &p.visual,
                egui::FontId::proportional(10.),
                Color32::WHITE,
            );
        }
    }
    let by_id: HashMap<_, _> = units.iter().map(|u| (u.id, u)).collect();
    for c in &fight.units {
        let Some(u) = by_id.get(&c.id) else { continue };
        let Some(p) = projection.point(u.position + Vec3::Y * 2.3) else {
            continue;
        };
        let mut lines = Vec::new();
        if c.downed {
            lines.push("Downed".into());
        }
        if c.guarding {
            lines.push("Guard".into());
        }
        if let Some(cast) = &c.cast {
            lines.push(format!(
                "{} {:.1}/{:.1}",
                cast.name, cast.elapsed, cast.duration
            ));
            bar(
                &painter,
                p + egui::vec2(0., -8.),
                cast.elapsed / cast.duration.max(0.001),
                Color32::GOLD,
            );
        }
        if let Some((now, max)) = c.defiance {
            bar(
                &painter,
                p + egui::vec2(0., -15.),
                now / max.max(1.),
                Color32::LIGHT_BLUE,
            );
        }
        lines.extend(
            c.auras
                .iter()
                .map(|a| format!("{} ×{} {:.0}s", a.name, a.stacks, a.remaining)),
        );
        if !lines.is_empty() {
            painter.text(
                p,
                egui::Align2::CENTER_BOTTOM,
                lines.join("\n"),
                egui::FontId::proportional(11.),
                Color32::WHITE,
            );
        }
    }
    feedback.paint(&painter, projection);
}
fn bar(p: &egui::Painter, at: Pos2, value: f32, colour: Color32) {
    let rect = egui::Rect::from_center_size(at, egui::vec2(90., 4.));
    p.rect_filled(rect, 0., Color32::BLACK);
    p.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(90. * value.clamp(0., 1.), 4.)),
        0.,
        colour,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_distinguishes_all_avoid_results_and_expires() {
        let mut host = server::Host::start(42, "feedback");
        let snap = host.snapshot().unwrap();
        host.stop();
        let units = snap.units.iter().map(|u| (u.id, u)).collect();
        let target = snap.player.as_ref().unwrap().unit;
        let mut feedback = Feedback::default();
        for how in [
            protocol::Avoid::Evaded,
            protocol::Avoid::Blocked,
            protocol::Avoid::Parried,
            protocol::Avoid::Absorbed,
            protocol::Avoid::Immune,
            protocol::Avoid::Reflected,
            protocol::Avoid::Missed,
        ] {
            feedback.event(
                &Event::Avoided {
                    target,
                    source: target,
                    how,
                },
                &units,
            );
        }
        feedback.event(
            &Event::Damage {
                target,
                source: target,
                amount: 42.,
                crit: true,
                periodic: true,
                school: protocol::School::Fire,
            },
            &units,
        );
        feedback.event(
            &Event::Heal {
                target,
                source: target,
                amount: 12.,
                crit: false,
            },
            &units,
        );
        assert_eq!(feedback.entries.len(), 9);
        assert_eq!(feedback.entries[7].text, "tick 42!");
        assert!(feedback.entries[7].crit);
        assert_eq!(feedback.entries[8].text, "+12");
        feedback.update(1.6);
        assert!(feedback.entries.is_empty());
    }
    #[test]
    fn yaw_and_fill_follow_protocol() {
        let p = outline(
            Shape::Line {
                length: 10.,
                width: 2.,
            },
            Vec3::ZERO,
            std::f32::consts::FRAC_PI_2,
            0.5,
        );
        assert!((p[2].x - 5.).abs() < 1e-5);
        assert!((p[2].z + 1.).abs() < 1e-5);
        let p = outline(
            Shape::Cone {
                range: 10.,
                angle: 1.,
            },
            Vec3::ZERO,
            0.,
            1.,
        );
        assert_eq!(p[0], Vec3::ZERO);
        assert!((p[17].z - 10.).abs() < 1e-5);
    }
    #[test]
    fn cursor_ray_hits_collision_ground() {
        let valley = world::Valley::new();
        let camera = render::Camera {
            eye: Vec3::new(0., 20., 20.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        let hit = ground_point(
            camera.matrix(1.),
            Vec2::splat(500.),
            Vec2::splat(1000.),
            &valley,
        )
        .unwrap();
        assert!((hit.y - valley.ground(hit.x, hit.z)).abs() < 0.001);
    }
}
