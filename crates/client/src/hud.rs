//! Functional HUD using stock widgets. No game HUD artwork, icons or fonts are authored here.
use protocol::{Command, Snapshot};
pub enum Action {
    Command(Command),
    CycleTarget,
}
/// Reserved consumer input. Protocol v3 supplies no casts; v4 will populate this.
pub struct CastBar<'a> {
    pub label: &'a str,
    pub elapsed: f32,
    pub duration: f32,
}
pub fn draw(ctx: &egui::Context, snapshot: &Snapshot, cast: Option<CastBar<'_>>) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(player) = snapshot.player.as_ref() else {
        return actions;
    };
    let Some(me) = snapshot.unit(player.unit) else {
        return actions;
    };
    egui::Area::new(egui::Id::new("player-frame"))
        .anchor(egui::Align2::LEFT_TOP, [12., 12.])
        .show(ctx, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(240.);
                ui.label(format!("{} · Level {}", me.name, me.level));
                ui.add(
                    egui::ProgressBar::new((me.health / me.max_health.max(1.)).clamp(0., 1.))
                        .text(format!("Health {:.0} / {:.0}", me.health, me.max_health)),
                );
                ui.add(
                    egui::ProgressBar::new(
                        (player.endurance / player.max_endurance.max(1.)).clamp(0., 1.),
                    )
                    .text(format!(
                        "Endurance {:.0} / {:.0}",
                        player.endurance, player.max_endurance
                    )),
                );
                if player.in_combat {
                    ui.label("In combat");
                }
            });
        });
    if let Some(target) = me.target.and_then(|id| snapshot.unit(id)) {
        egui::Area::new(egui::Id::new("target-frame"))
            .anchor(egui::Align2::CENTER_TOP, [0., 12.])
            .show(ctx, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(250.);
                    ui.label(format!("{} · Level {}", target.name, target.level));
                    ui.add(
                        egui::ProgressBar::new(
                            (target.health / target.max_health.max(1.)).clamp(0., 1.),
                        )
                        .text(format!("{:.0} / {:.0}", target.health, target.max_health)),
                    );
                });
            });
    }
    egui::Area::new(egui::Id::new("ability-bar"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0., -16.])
        .show(ctx, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                if let Some(cast) = cast {
                    ui.add(
                        egui::ProgressBar::new(
                            (cast.elapsed / cast.duration.max(0.001)).clamp(0., 1.),
                        )
                        .text(cast.label),
                    );
                }
                ui.horizontal(|ui| {
                    for (slot, name) in ["Strike", "Sweep"].into_iter().enumerate() {
                        let cd = player.cooldowns.get(slot).copied().unwrap_or(0.);
                        let label = if cd > 0. {
                            format!("{}  {name}\n{cd:.1}s", slot + 1)
                        } else {
                            format!("{}  {name}", slot + 1)
                        };
                        if ui
                            .add_enabled(
                                cd <= 0. && me.health > 0.,
                                egui::Button::new(label).min_size(egui::vec2(100., 40.)),
                            )
                            .clicked()
                        {
                            actions.push(Action::Command(Command::UseSlot {
                                slot: slot as u8,
                                ground: None,
                            }));
                        }
                    }
                    if ui.button("Tab · Target").clicked() {
                        actions.push(Action::CycleTarget)
                    }
                    if ui
                        .button(if player.auto_attack {
                            "Stop attacking"
                        } else {
                            "Auto attack"
                        })
                        .clicked()
                    {
                        actions.push(Action::Command(Command::SetAutoAttack(!player.auto_attack)))
                    }
                    if ui
                        .add_enabled(
                            player.endurance >= 50. && me.health > 0.,
                            egui::Button::new("V · Dodge"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Command(Command::Dodge {
                            direction: glam::Vec2::ZERO,
                        }))
                    }
                });
                ui.label("WASD move · Shift walk · Space jump · Right-drag camera · Scroll zoom");
                if player.can_respawn && ui.button("R · Respawn").clicked() {
                    actions.push(Action::Command(Command::Respawn));
                }
            });
        });
    // Game Tab/Space bindings retain priority over focus on these mouse-operated HUD buttons.
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Look, PlayerState, Pose, UnitKind, UnitState};

    fn frame(ctx: &egui::Context, snapshot: &Snapshot, events: Vec<egui::Event>) -> Vec<Action> {
        let mut actions = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ctx| actions = draw(ctx, snapshot, None),
        );
        actions
    }

    #[test]
    fn clicking_an_ability_sends_a_command_and_cooldown_blocks_it() {
        let ctx = egui::Context::default();
        let mut snapshot = Snapshot {
            tick: 1,
            time: 0.,
            units: vec![UnitState {
                id: 1,
                kind: UnitKind::Player,
                body: "player".into(),
                name: "Hero".into(),
                level: 1,
                position: glam::Vec3::ZERO,
                velocity: glam::Vec3::ZERO,
                yaw: 0.,
                pose: Pose::Idle,
                pose_time: 0.,
                health: 100.,
                max_health: 100.,
                hostile: false,
                target: None,
                look: Look::default(),
            }],
            player: Some(PlayerState {
                unit: 1,
                endurance: 100.,
                max_endurance: 100.,
                cooldowns: vec![0., 0.],
                ..Default::default()
            }),
            events: Vec::new(),
        };
        // egui sizes an anchored area in its first pass; let its geometry settle.
        frame(&ctx, &snapshot, Vec::new());
        frame(&ctx, &snapshot, Vec::new());
        let area = ctx.memory(|m| m.area_rect(egui::Id::new("ability-bar")).unwrap());
        let pos = area.min + egui::vec2(50., 25.);
        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            &snapshot,
            vec![egui::Event::PointerMoved(pos), click(true)],
        );
        let actions = frame(&ctx, &snapshot, vec![click(false)]);
        assert!(matches!(
            actions.as_slice(),
            [Action::Command(Command::UseSlot {
                slot: 0,
                ground: None
            })]
        ));
        snapshot.player.as_mut().unwrap().cooldowns[0] = 2.;
        frame(&ctx, &snapshot, vec![click(true)]);
        assert!(frame(&ctx, &snapshot, vec![click(false)]).is_empty());
        assert!(ctx.memory(|m| m.focused()).is_none());
    }
}
