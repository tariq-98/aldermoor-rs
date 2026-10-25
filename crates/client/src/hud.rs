//! Stock widgets for authoritative player state; artwork and icons remain supplied assets.
use protocol::{CombatView, Command, Snapshot};
/// What the player's keys are, for the labels.
#[derive(Default)]
pub struct Keys {
    pub hint: String,
    pub slots: [String; 10],
    pub revive: String,
    pub target: String,
    pub swap: String,
    pub respawn: String,
    pub specialties: String,
}
/// White text on a solid colour, readable over any background.
fn badge(ui: &mut egui::Ui, text: &str, fill: egui::Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(4.)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .color(egui::Color32::WHITE)
                    .strong(),
            );
        });
}

/// The sim's messages to the player ("You can't fight while swimming."), newest last, each
/// with the seconds it has left; they fade over their last half second.
pub fn messages(ctx: &egui::Context, messages: &[(String, f32)]) {
    if messages.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("messages"))
        .anchor(egui::Align2::CENTER_TOP, [0., 72.])
        .interactable(false)
        .show(ctx, |ui| {
            for (text, left) in messages {
                let a = (left / 0.5).clamp(0., 1.);
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha((200. * a) as u8))
                    .corner_radius(4.)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(text)
                                .size(16.)
                                .color(egui::Color32::from_white_alpha((255. * a) as u8)),
                        );
                    });
            }
        });
}

pub enum Action {
    Command(Command),
    CycleTarget,
    AimSlot(u8),
    CloseSpecialties,
}

/// "warrior" as "Warrior".
fn title(key: &str) -> String {
    let mut c = key.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// XP into the equipped specialty's level, or that it has reached the cap.
fn xp_bar(ui: &mut egui::Ui, player: &protocol::PlayerState) {
    let (fraction, text) = if player.xp_next == 0 {
        (
            1.,
            format!("{} {} · max level", title(&player.specialty), player.level),
        )
    } else {
        (
            player.xp as f32 / player.xp_next as f32,
            format!(
                "{} {} · XP {}/{}",
                title(&player.specialty),
                player.level,
                player.xp,
                player.xp_next
            ),
        )
    };
    ui.add(
        egui::ProgressBar::new(fraction.clamp(0., 1.))
            .desired_height(10.)
            .fill(egui::Color32::from_rgb(150, 110, 40))
            .text(egui::RichText::new(text).size(11.)),
    );
}

/// The banner for the local player's last level up, with the seconds it has left; it fades
/// over its last second.
pub fn level_up(ctx: &egui::Context, banner: &(String, u8, f32)) {
    let (specialty, level, left) = banner;
    let a = left.clamp(0., 1.);
    egui::Area::new(egui::Id::new("level-up"))
        .anchor(egui::Align2::CENTER_CENTER, [0., -180.])
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha((170. * a) as u8))
                .stroke(egui::Stroke::new(
                    1_f32,
                    egui::Color32::from_rgba_unmultiplied(230, 190, 90, (255. * a) as u8),
                ))
                .corner_radius(6.)
                .inner_margin(egui::Margin::symmetric(24, 10))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Level up").size(15.).color(
                            egui::Color32::from_rgba_unmultiplied(230, 190, 90, (255. * a) as u8),
                        ));
                        ui.label(
                            egui::RichText::new(format!("{} {level}", title(specialty)))
                                .size(28.)
                                .strong()
                                .color(egui::Color32::from_white_alpha((255. * a) as u8)),
                        );
                    });
                });
        });
}

/// The Specialties screen: every specialty's level, and quest XP to give one of them.
/// `amount` is how much the next Give sends; it stays within the pool.
pub fn specialties(ctx: &egui::Context, snapshot: &Snapshot, amount: &mut u32) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(player) = snapshot.player.as_ref() else {
        return actions;
    };
    let pool = player.unassigned_xp;
    *amount = (*amount).min(pool);
    let mut open = true;
    egui::Window::new("Specialties")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_CENTER, [-24., 0.])
        .default_width(360.)
        .show(ctx, |ui| {
            ui.label("Each specialty levels on its own. The equipped one fights at its level.");
            ui.separator();
            egui::Grid::new("specialty-levels")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for s in &player.specialties {
                        let equipped = s.specialty == player.specialty;
                        let name = egui::RichText::new(title(&s.specialty));
                        ui.label(if equipped { name.strong() } else { name });
                        ui.label(if equipped {
                            format!("Level {} · equipped", s.level)
                        } else {
                            format!("Level {}", s.level)
                        });
                        if ui
                            .add_enabled(*amount > 0, egui::Button::new(format!("Give {amount}")))
                            .clicked()
                        {
                            actions.push(Action::Command(Command::AssignXp {
                                specialty: s.specialty.clone(),
                                amount: *amount,
                            }));
                        }
                        ui.end_row();
                    }
                });
            ui.separator();
            if pool == 0 {
                ui.label("No quest XP to give. Quests reward XP you can give to any specialty.");
            } else {
                ui.horizontal(|ui| {
                    ui.label(format!("Quest XP: {pool} · give"));
                    ui.add(egui::Slider::new(amount, 0..=pool));
                    if ui.button("All").clicked() {
                        *amount = pool;
                    }
                });
            }
        });
    if !open {
        actions.push(Action::CloseSpecialties);
    }
    actions
}
fn cast(ui: &mut egui::Ui, c: &protocol::CastBar) {
    ui.add(
        egui::ProgressBar::new((c.elapsed / c.duration.max(0.001)).clamp(0., 1.)).text(format!(
            "{}{} {:.1}/{:.1}s{}",
            if c.channel { "Channel · " } else { "" },
            c.name,
            c.elapsed,
            c.duration,
            if c.interruptible {
                ""
            } else {
                " · uninterruptible"
            }
        )),
    );
}
fn auras(ui: &mut egui::Ui, auras: &[protocol::AuraView]) {
    ui.horizontal_wrapped(|ui| {
        for a in auras {
            ui.colored_label(
                if a.debuff {
                    egui::Color32::LIGHT_RED
                } else {
                    egui::Color32::LIGHT_BLUE
                },
                format!("{} ×{} {:.0}s", a.name, a.stacks, a.remaining),
            )
            .on_hover_text(format!("Duration {:.1}s", a.duration));
        }
    });
}
// Laid out by hand: rustfmt would reflow the whole ability bar.
#[rustfmt::skip]
pub fn draw(
    ctx: &egui::Context,
    snapshot: &Snapshot,
    fight: &CombatView,
    ground: Option<glam::Vec3>,
    keys: &Keys,
) -> Vec<Action> {
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
                if !player.specialty.is_empty() {
                    xp_bar(ui, player);
                }
                if player.unassigned_xp > 0 {
                    badge(
                        ui,
                        &format!(
                            "{} quest XP to give · {} Specialties",
                            player.unassigned_xp, keys.specialties
                        ),
                        egui::Color32::from_rgb(140, 100, 30),
                    );
                }
                ui.add(
                    egui::ProgressBar::new(me.health / me.max_health.max(1.)).text(format!(
                        "{} {:.0}/{:.0}",
                        if player.downed { "Downed" } else { "Health" },
                        me.health,
                        me.max_health
                    )),
                );
                ui.add(
                    egui::ProgressBar::new(player.endurance / player.max_endurance.max(1.)).text(
                        format!(
                            "Endurance {:.0}/{:.0}",
                            player.endurance, player.max_endurance
                        ),
                    ),
                );
                ui.add(
                    egui::ProgressBar::new(player.resource / player.max_resource.max(1.)).text(
                        format!(
                            "{:?} {:.0}/{:.0}",
                            player.resource_kind, player.resource, player.max_resource
                        ),
                    ),
                );
                if player.guarding {
                    ui.label("Guarding");
                }
                if player.in_combat {
                    ui.label("In combat");
                }
                if player.swimming {
                    badge(ui, "Swimming · no skills", egui::Color32::from_rgb(40, 110, 190));
                } else if player.sprinting {
                    badge(ui, "Sprinting", egui::Color32::from_rgb(60, 130, 70));
                }
                auras(ui, &player.auras);
            });
        });
    if let Some(target) = me.target.and_then(|id| snapshot.unit(id)) {
        egui::Area::new(egui::Id::new("target-frame"))
            .anchor(egui::Align2::CENTER_TOP, [0., 12.])
            .show(ctx, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(280.);
                    ui.label(format!("{} · Level {}", target.name, target.level));
                    ui.add(
                        egui::ProgressBar::new(target.health / target.max_health.max(1.))
                            .text(format!("{:.0}/{:.0}", target.health, target.max_health)),
                    );
                    if let Some(c) = fight.units.iter().find(|c| c.id == target.id) {
                        if let Some(c) = &c.cast {
                            cast(ui, c);
                        }
                        if let Some((now, max)) = c.defiance {
                            ui.add(
                                egui::ProgressBar::new(now / max.max(1.))
                                    .text(format!("Defiance {now:.0}/{max:.0}")),
                            );
                        }
                        if c.downed {
                            ui.label(format!("Downed · {} to revive", keys.revive));
                        }
                        if c.guarding {
                            ui.label("Guarding");
                        }
                        auras(ui, &c.auras);
                    }
                    if target.health <= 0. && !target.hostile {
                        ui.label(format!("Defeated · {} to revive", keys.revive));
                    }
                });
            });
    }
    egui::Area::new(egui::Id::new("ability-bar")).anchor(egui::Align2::CENTER_BOTTOM,[0.,-16.]).show(ctx,|ui| {
        egui::Frame::group(ui.style()).show(ui,|ui| {
            ui.set_width((ctx.content_rect().width()-40.).clamp(200.,1100.));
            if let Some(c)=&player.cast {cast(ui,c);}
            egui::ScrollArea::horizontal().max_width((ctx.content_rect().width()-40.).max(200.)).show(ui,|ui| {
                ui.horizontal(|ui| {for slot in 0..10 {
                    let data=player.bar.get(slot);
                    let key=&keys.slots[slot];
                    let label=data.map_or_else(||format!("{key}\n—"),|s|format!("{key} · {}\nCost {:.0}{}",s.name,s.cost,if s.ground {" · ground"} else {""}));
                    let ready=data.is_some_and(|s|!s.ability.is_empty() && s.cooldown<=0. && s.cost<=player.resource) && me.health>0.;
                    // GCD queueing belongs to the sim; don't disable a ready button during GCD.
                    let response=ui.add_enabled(ready,egui::Button::new(label).min_size(egui::vec2(95.,48.)));
                    ctx.data_mut(|d|d.insert_temp(egui::Id::new(("slot-rect",slot)),response.rect));
                    if let Some(s)=data {
                        let fraction=if s.cooldown>0. {s.cooldown/s.recharge.max(0.001)} else if !s.auto {player.gcd} else {0.};
                        if fraction>0. {
                            let centre=response.rect.center();let radius=20.;let mut points=vec![centre];
                            points.extend((0..=32).map(|i|{let a=-std::f32::consts::FRAC_PI_2+std::f32::consts::TAU*fraction.clamp(0.,1.)*i as f32/32.;centre+egui::vec2(a.cos(),a.sin())*radius}));
                            let mut mesh=egui::Mesh::default();for p in &points{mesh.colored_vertex(*p,egui::Color32::from_black_alpha(95));}for i in 1..points.len()-1{mesh.add_triangle(0,i as u32,i as u32+1);}ui.painter().add(egui::Shape::mesh(mesh));
                            if s.cooldown>0. {ui.painter().text(response.rect.center(),egui::Align2::CENTER_CENTER,format!("{:.1}s",s.cooldown),egui::FontId::proportional(16.),egui::Color32::WHITE);}
                        }
                        if response.hovered() && s.ground {actions.push(Action::AimSlot(slot as u8));}
                        if response.clicked() {actions.push(Action::Command(Command::UseSlot{slot:slot as u8,ground}));}
                        response.on_hover_text(format!("{} · range {:.0}m · recharge {:.1}s",s.name,s.range,s.recharge));
                    }
                }});
            });
            ui.horizontal(|ui| {
                if ui.button(format!("{} · Target",keys.target)).clicked(){actions.push(Action::CycleTarget);}
                if ui.button(if player.auto_attack {"Stop attacking"} else {"Auto attack"}).clicked(){actions.push(Action::Command(Command::SetAutoAttack(!player.auto_attack)));}
                if ui.add_enabled(player.swap_ready<=0.,egui::Button::new(format!("{} · Set {} · {:.1}s",keys.swap,player.weapon_set+1,player.swap_ready))).clicked(){actions.push(Action::Command(Command::SwapWeapons));}
                if player.can_respawn && ui.button(format!("{} · Respawn",keys.respawn)).clicked(){actions.push(Action::Command(Command::Respawn));}
            });
            ui.label(&keys.hint);
        });
    });
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
    #[test]
    fn ten_slots_follow_state_and_click_sends_ground() {
        let mut host = server::Host::start(42, "HUD test");
        let mut snapshot = (*host.snapshot().unwrap()).clone();
        host.stop();
        let ctx = egui::Context::default();
        let ground = Some(glam::Vec3::new(4., 2., 3.));
        let run = |snapshot: &Snapshot, events| {
            let mut actions = Vec::new();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1800., 1000.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    actions = draw(
                        ctx,
                        snapshot,
                        &CombatView::default(),
                        ground,
                        &Keys::default(),
                    )
                },
            );
            actions
        };
        run(&snapshot, vec![]);
        run(&snapshot, vec![]);
        for slot in 0..10 {
            assert!(
                ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("slot-rect", slot))))
                    .is_some()
            );
        }
        let pos = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new(("slot-rect", 0))))
            .unwrap()
            .center();
        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        run(&snapshot, vec![egui::Event::PointerMoved(pos), click(true)]);
        assert!(
            run(&snapshot, vec![click(false)]).iter().any(
                |a| matches!(a,Action::Command(Command::UseSlot{slot:0,ground:g}) if *g==ground)
            )
        );
        snapshot.player.as_mut().unwrap().bar[0].cooldown = 2.;
        run(&snapshot, vec![click(true)]);
        assert!(run(&snapshot, vec![click(false)]).is_empty());
    }
    #[test]
    fn xp_bar_and_specialties_screen_follow_state() {
        let mut host = server::Host::start(42, "XP test");
        let mut snapshot = (*host.snapshot().unwrap()).clone();
        host.stop();
        let player = snapshot.player.as_mut().unwrap();
        assert_eq!(player.specialties.len(), 6);
        player.unassigned_xp = 300;
        let ctx = egui::Context::default();
        let mut amount = 500;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            draw(
                ctx,
                &snapshot,
                &CombatView::default(),
                None,
                &Keys::default(),
            );
            level_up(ctx, &("warrior".into(), 2, 3.));
            assert!(specialties(ctx, &snapshot, &mut amount).is_empty());
        });
        // Give never offers more than the pool holds.
        assert_eq!(amount, 300);
        snapshot.player.as_mut().unwrap().unassigned_xp = 0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            specialties(ctx, &snapshot, &mut amount);
        });
        assert_eq!(amount, 0);
        assert_eq!(title("necromancer"), "Necromancer");
    }
}
