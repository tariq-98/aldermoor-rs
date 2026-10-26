//! The controls menu on Esc: pick the scheme, rebind keys, reset them, or quit. Every change
//! is saved to the settings file straight away.
use controls::camera::ScreenShake;
use controls::{Control, Input, Scheme, Settings};
use std::path::Path;

#[derive(Default)]
pub struct Menu {
    /// The control waiting for its next key or mouse button.
    pub capture: Option<Control>,
    /// What the last change did, or why saving failed.
    pub notice: Option<String>,
}

pub enum Change {
    Capture(Control),
    Unbind(Control, Input),
    Reset,
    Scheme(Scheme),
    ScreenShake(ScreenShake),
    Close,
    Quit,
}

pub fn draw(ctx: &egui::Context, menu: &Menu, settings: &Settings, path: &Path) -> Vec<Change> {
    let mut changes = Vec::new();
    egui::Window::new("Controls")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0., 0.])
        .default_width(560.)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Scheme:");
                for (scheme, name) in [
                    (Scheme::Action, "Action (mouse always aims)"),
                    (Scheme::Classic, "Classic (WoW)"),
                ] {
                    if ui.radio(settings.scheme == scheme, name).clicked()
                        && settings.scheme != scheme
                    {
                        changes.push(Change::Scheme(scheme));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Screen shake:");
                for (shake, name) in [
                    (ScreenShake::Full, "Full"),
                    (ScreenShake::Low, "Low"),
                    (ScreenShake::Off, "Off"),
                ] {
                    if ui.radio(settings.screen_shake == shake, name).clicked()
                        && settings.screen_shake != shake
                    {
                        changes.push(Change::ScreenShake(shake));
                    }
                }
            });
            if let Some(notice) = &menu.notice {
                ui.colored_label(egui::Color32::YELLOW, notice);
            }
            for (input, controls) in settings.bindings.conflicts() {
                let names: Vec<_> = controls.iter().map(|c| c.name()).collect();
                ui.colored_label(
                    egui::Color32::LIGHT_RED,
                    format!("{} is bound to {}", input.label(), names.join(" and ")),
                );
            }
            ui.label("Click a key to unbind it, or + then press a key or mouse button 3-5.");
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() * 0.6).max(200.))
                .show(ui, |ui| {
                    egui::Grid::new("bindings").striped(true).show(ui, |ui| {
                        for control in Control::ALL {
                            ui.label(control.name());
                            ui.horizontal(|ui| {
                                for input in settings.bindings.inputs(control) {
                                    if ui.button(format!("{}  x", input.label())).clicked() {
                                        changes.push(Change::Unbind(control, input.clone()));
                                    }
                                }
                                let waiting = menu.capture == Some(control);
                                let add = if waiting { "Press a key..." } else { "+" };
                                if ui.selectable_label(waiting, add).clicked() {
                                    changes.push(Change::Capture(control));
                                }
                            });
                            ui.end_row();
                        }
                    });
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Reset keys to defaults").clicked() {
                    changes.push(Change::Reset);
                }
                if ui.button("Close (Esc)").clicked() {
                    changes.push(Change::Close);
                }
                if ui.button("Quit").clicked() {
                    changes.push(Change::Quit);
                }
            });
            ui.small(format!("Saved to {}", path.display()));
        });
    changes
}
