//! Gathering and crafting from the player's own state (docs/CRAFTING.md): node markers, the
//! swing bar with its sweet spot, the crafting window at a station and the skills panel.
//! Placeholder markers and plain widgets until the node, tool and station art lands.
use crate::combat::Projection;
use glam::Vec3;
use protocol::{Command, NodeView, PlayerState, Snapshot, UnitId, WorkView};

/// How close a dead beast must be to skin it (the carcasses' reach in the craft tables).
pub const SKIN_REACH: f32 = 3.;
/// The skills in `PlayerState::skills` order (`content::crafts::Skill::ALL`).
pub const SKILLS: [&str; 11] = [
    "Mining",
    "Woodcutting",
    "Skinning",
    "Herbalism",
    "Fishing",
    "Smithing",
    "Tailoring",
    "Leatherworking",
    "Enchanting",
    "Cooking",
    "Alchemy",
];
const READY: egui::Color32 = egui::Color32::from_rgb(120, 200, 110);
const SPENT: egui::Color32 = egui::Color32::from_rgb(120, 120, 120);
const SWEET: egui::Color32 = egui::Color32::from_rgb(230, 190, 90);

/// What the interact key would do from here.
#[derive(Clone, Debug, PartialEq)]
pub enum Interact {
    Gather { node: u32, name: String },
    Skin { target: UnitId, name: String },
}

impl Interact {
    pub fn command(&self) -> Command {
        match self {
            Interact::Gather { node, .. } => Command::Gather { node: *node },
            Interact::Skin { target, .. } => Command::Skin { target: *target },
        }
    }
    fn label(&self) -> String {
        match self {
            Interact::Gather { name, .. } => format!("Gather · {name}"),
            Interact::Skin { name, .. } => format!("Skin · {name}"),
        }
    }
}

/// The nearest dead beast in reach to skin, else the nearest ready node in its reach.
pub fn interact(snapshot: &Snapshot, player: &PlayerState, at: Vec3) -> Option<Interact> {
    let dead = snapshot
        .units
        .iter()
        .filter(|u| u.id != player.unit && u.hostile && u.health <= 0.)
        .map(|u| (u.position.distance(at), u))
        .filter(|(d, _)| *d <= SKIN_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, u)) = dead {
        return Some(Interact::Skin {
            target: u.id,
            name: u.name.clone(),
        });
    }
    player
        .nodes
        .iter()
        .filter(|n| n.ready)
        .map(|n| (n.position.distance(at), n))
        .filter(|(d, n)| *d <= n.reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, n)| Interact::Gather {
            node: n.id,
            name: n.name.clone(),
        })
}

/// A marker over each node, green when ready and grey while it grows back, with its name
/// when close; and a prompt for what the interact key would do.
pub fn world(
    ctx: &egui::Context,
    projection: &Projection,
    nodes: &[NodeView],
    at: Vec3,
    interact: Option<&Interact>,
    key: &str,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("node-markers"),
    ));
    for n in nodes {
        let Some(p) = projection.point(n.position + Vec3::Y * 1.2) else {
            continue;
        };
        let colour = if n.ready { READY } else { SPENT };
        painter.add(egui::Shape::convex_polygon(
            vec![
                p + egui::vec2(0., -6.),
                p + egui::vec2(5., 0.),
                p + egui::vec2(0., 6.),
                p + egui::vec2(-5., 0.),
            ],
            colour,
            egui::Stroke::new(1_f32, egui::Color32::from_black_alpha(160)),
        ));
        if n.position.distance(at) < 15. {
            painter.text(
                p + egui::vec2(0., -10.),
                egui::Align2::CENTER_BOTTOM,
                if n.ready {
                    n.name.clone()
                } else {
                    format!("{} (growing back)", n.name)
                },
                egui::FontId::proportional(13.),
                colour,
            );
        }
    }
    if let Some(i) = interact {
        egui::Area::new(egui::Id::new("interact-prompt"))
            .anchor(egui::Align2::CENTER_CENTER, [0., 120.])
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(190))
                    .corner_radius(4.)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("{key} · {}", i.label()))
                                .size(15.)
                                .color(READY),
                        );
                    });
            });
    }
}

/// Where a strike in this swing would land: in the sweet spot or not.
pub fn in_sweet_spot(w: &WorkView) -> bool {
    w.sweet_to > w.sweet_from && (w.sweet_from..=w.sweet_to).contains(&w.elapsed)
}

pub enum Action {
    Command(Command),
    CloseSkills,
}

/// The swing (or item) under way: a bar with the sweet spot marked, which flashes gold on a
/// strike that landed and red on one that missed; and Stop.
pub fn work(ctx: &egui::Context, w: &WorkView, key: &str) -> Vec<Action> {
    let mut actions = Vec::new();
    egui::Area::new(egui::Id::new("work-bar"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0., -170.])
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(190))
                .corner_radius(4.)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let left = if w.left > 0 {
                            format!(" · {} more", w.left)
                        } else {
                            String::new()
                        };
                        ui.label(egui::RichText::new(format!("{}{left}", w.name)).strong());
                        if ui.small_button("Stop").clicked() {
                            actions.push(Action::Command(Command::StopWork));
                        }
                    });
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(320., 14.), egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    let x = |t: f32| rect.left() + rect.width() * (t / w.duration.max(0.01));
                    painter.rect_filled(rect, 3., egui::Color32::from_gray(40));
                    let sweet = w.sweet_to > w.sweet_from;
                    if sweet {
                        painter.rect_filled(
                            egui::Rect::from_x_y_ranges(
                                x(w.sweet_from)..=x(w.sweet_to),
                                rect.y_range(),
                            ),
                            0.,
                            // Brighter while the swing is inside it, when a strike would land.
                            SWEET.gamma_multiply(if w.struck.is_none() && in_sweet_spot(w) {
                                1.
                            } else {
                                0.5
                            }),
                        );
                    }
                    let fill = match w.struck {
                        Some(true) => SWEET,
                        Some(false) => egui::Color32::from_rgb(210, 70, 60),
                        None => egui::Color32::from_rgb(90, 150, 210),
                    };
                    painter.rect_filled(
                        egui::Rect::from_x_y_ranges(
                            rect.left()..=x(w.elapsed.min(w.duration)),
                            rect.y_range(),
                        ),
                        3.,
                        fill.gamma_multiply(0.85),
                    );
                    painter.line_segment(
                        [
                            egui::pos2(x(w.elapsed.min(w.duration)), rect.top()),
                            egui::pos2(x(w.elapsed.min(w.duration)), rect.bottom()),
                        ],
                        egui::Stroke::new(2_f32, egui::Color32::WHITE),
                    );
                    if sweet {
                        ui.small(match w.struck {
                            Some(true) => "Struck the sweet spot!".to_string(),
                            Some(false) => "Missed the sweet spot.".to_string(),
                            None => format!("{key} in the gold for a bonus"),
                        });
                    }
                });
        });
    actions
}

/// The station you stand at: each recipe with what it takes, and Make one or Make all.
pub fn station(ctx: &egui::Context, player: &PlayerState) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(station) = &player.station else {
        return actions;
    };
    egui::Window::new(&station.name)
        .id(egui::Id::new("station"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::LEFT_CENTER, [24., 0.])
        .show(ctx, |ui| {
            egui::Grid::new("recipes")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for r in &station.recipes {
                        let inputs: Vec<_> = r
                            .inputs
                            .iter()
                            .map(|i| format!("{} {}", i.count, i.name))
                            .collect();
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&r.output.name).strong());
                            ui.small(format!(
                                "{} · {} {}",
                                inputs.join(", "),
                                SKILLS
                                    .iter()
                                    .find(|s| s.eq_ignore_ascii_case(&r.skill))
                                    .unwrap_or(&r.skill.as_str()),
                                r.level
                            ));
                        });
                        ui.label(format!("Can make {}", r.can_make));
                        ui.horizontal(|ui| {
                            let craft = |count| {
                                Action::Command(Command::Craft {
                                    recipe: r.recipe.clone(),
                                    count,
                                })
                            };
                            if ui
                                .add_enabled(r.can_make > 0, egui::Button::new("Make one"))
                                .clicked()
                            {
                                actions.push(craft(1));
                            }
                            if ui
                                .add_enabled(r.can_make > 1, egui::Button::new("Make all"))
                                .clicked()
                            {
                                actions.push(craft(r.can_make));
                            }
                        });
                        ui.end_row();
                    }
                });
        });
    actions
}

/// Every gathering and crafting skill's level and XP toward the next.
pub fn skills(ctx: &egui::Context, player: &PlayerState) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut open = true;
    egui::Window::new("Skills")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, [-24., 120.])
        .show(ctx, |ui| {
            egui::Grid::new("skill-levels")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for (i, s) in player.skills.iter().enumerate() {
                        if i == 5 {
                            ui.separator();
                            ui.separator();
                            ui.separator();
                            ui.end_row();
                        }
                        ui.label(SKILLS.get(i).copied().unwrap_or("?"));
                        ui.label(format!("Level {}", s.level));
                        let (fraction, text) = if s.xp_next == 0 {
                            (1., "max".to_string())
                        } else {
                            (
                                s.xp as f32 / s.xp_next as f32,
                                format!("{}/{}", s.xp, s.xp_next),
                            )
                        };
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(140.)
                                .text(text),
                        );
                        ui.end_row();
                    }
                });
        });
    if !open {
        actions.push(Action::CloseSkills);
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, x: f32, ready: bool, reach: f32) -> NodeView {
        NodeView {
            id,
            node: "copper_vein".into(),
            name: "Copper Vein".into(),
            position: Vec3::new(x, 0., 0.),
            level: 1,
            ready,
            reach,
        }
    }

    #[test]
    fn interact_skins_a_dead_beast_first_then_gathers_the_nearest_ready_node() {
        let mut host = server::Host::start(42, "Craft test");
        let mut snapshot = (*host.snapshot().unwrap()).clone();
        host.stop();
        let player = PlayerState {
            unit: snapshot.player.as_ref().unwrap().unit,
            nodes: vec![
                node(1, 1., false, 3.),
                node(2, 2.5, true, 3.),
                node(3, 1.5, true, 1.),
                node(4, 8., true, 9.),
            ],
            ..Default::default()
        };
        // Far from every unit: node 1 is spent and node 3 is out of its own reach.
        let at = Vec3::new(5000., 0., 0.);
        for n in &mut snapshot.units {
            n.position += at;
        }
        let shifted = |p: &PlayerState| {
            let mut p = p.clone();
            for n in &mut p.nodes {
                n.position += at;
            }
            p
        };
        let player = shifted(&player);
        assert_eq!(
            interact(&snapshot, &player, at).map(|i| i.command()),
            Some(Command::Gather { node: 2 })
        );
        // A fishing spot works from further off.
        assert_eq!(
            interact(&snapshot, &player, at + Vec3::new(10., 0., 0.)).map(|i| i.command()),
            Some(Command::Gather { node: 4 })
        );
        let wolf = snapshot
            .units
            .iter_mut()
            .find(|u| u.hostile)
            .expect("a monster in the valley");
        wolf.health = 0.;
        wolf.position = at + Vec3::new(0., 0., 1.);
        let id = wolf.id;
        assert_eq!(
            interact(&snapshot, &player, at).map(|i| i.command()),
            Some(Command::Skin { target: id })
        );
    }

    #[test]
    fn sweet_spot_and_screens() {
        let mut w = WorkView {
            name: "Copper Vein".into(),
            elapsed: 1.7,
            duration: 2.4,
            sweet_from: 1.56,
            sweet_to: 2.04,
            ..Default::default()
        };
        assert!(in_sweet_spot(&w));
        w.elapsed = 0.5;
        assert!(!in_sweet_spot(&w));
        // Crafting has no sweet spot.
        w.sweet_from = 0.;
        w.sweet_to = 0.;
        assert!(!in_sweet_spot(&w));
        let player = PlayerState {
            skills: vec![
                protocol::SkillView {
                    level: 3,
                    xp: 40,
                    xp_next: 250
                };
                11
            ],
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            assert!(work(ctx, &w, "X").is_empty());
            assert!(station(ctx, &player).is_empty());
            assert!(skills(ctx, &player).is_empty());
        });
    }
}
