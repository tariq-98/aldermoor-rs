//! Loot piles, the bag and the shop screen, from the player's own state (docs/ECONOMY.md).
//! Plain widgets until the item icons (#124) exist.
use crate::combat::Projection;
use glam::Vec3;
use protocol::{Command, ItemView, LootView, PlayerState};

/// How close a pile must be to take it (the sim's `Command::Loot` range).
pub const LOOT_RANGE: f32 = 3.;
/// Columns in the bag's grid of 28 slots.
const COLUMNS: usize = 7;
const GOLD: egui::Color32 = egui::Color32::from_rgb(230, 190, 90);

/// The nearest pile you can take from where you stand, if any.
pub fn nearest_pile(player: &PlayerState, at: Vec3) -> Option<&LootView> {
    player
        .loot
        .iter()
        .map(|p| (p.position.distance(at), p))
        .filter(|(d, _)| *d <= LOOT_RANGE)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p)| p)
}

/// What a pile holds, in a line: "12 coins · Wolf pelt ×2 · Fine iron sword".
fn contents(pile: &LootView) -> String {
    let mut parts = Vec::new();
    if pile.coins > 0 {
        parts.push(format!("{} coins", pile.coins));
    }
    parts.extend(pile.items.iter().map(item_name));
    parts.join(" · ")
}

fn item_name(i: &ItemView) -> String {
    let name = if i.fine {
        format!("Fine {}", i.name.to_lowercase())
    } else {
        i.name.clone()
    };
    if i.count > 1 {
        format!("{name} ×{}", i.count)
    } else {
        name
    }
}

/// A gold marker over each pile you may take, and a prompt with its contents when one is in
/// reach.
pub fn loot(
    ctx: &egui::Context,
    projection: &Projection,
    player: &PlayerState,
    at: Vec3,
    key: &str,
) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("loot-markers"),
    ));
    for pile in &player.loot {
        if let Some(p) = projection.point(pile.position + Vec3::Y * 0.6) {
            painter.circle(
                p,
                6.,
                GOLD.gamma_multiply(0.85),
                egui::Stroke::new(1.5_f32, egui::Color32::WHITE),
            );
        }
    }
    if let Some(pile) = nearest_pile(player, at) {
        egui::Area::new(egui::Id::new("loot-prompt"))
            .anchor(egui::Align2::CENTER_CENTER, [0., 90.])
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(190))
                    .corner_radius(4.)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("{key} · Loot: {}", contents(pile)))
                                .size(15.)
                                .color(GOLD),
                        );
                    });
            });
    }
}

pub enum Action {
    Command(Command),
    Close,
}

/// The bag: coins and every slot, with Sell on each slot the shop you stand at will buy. A
/// click sells one, a right-click the whole stack.
pub fn bag(ctx: &egui::Context, player: &PlayerState) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut open = true;
    egui::Window::new("Bag")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, [-24., -150.])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(format!("{} coins", player.coins)).color(GOLD));
            ui.separator();
            egui::Grid::new("bag-slots")
                .spacing([4., 4.])
                .show(ui, |ui| {
                    for (i, slot) in player.inventory.iter().enumerate() {
                        let size = egui::vec2(82., 52.);
                        if slot.item.is_empty() {
                            ui.add_enabled(false, egui::Button::new("").min_size(size));
                        } else {
                            let mut text = egui::RichText::new(item_name(slot)).size(12.);
                            if slot.fine {
                                text = text.color(GOLD);
                            }
                            let shop = player.shop.as_ref().filter(|_| slot.price > 0);
                            let label = match shop {
                                Some(_) => egui::RichText::new(format!(
                                    "{}\nSell {}",
                                    item_name(slot),
                                    slot.price
                                ))
                                .size(12.),
                                None => text,
                            };
                            let response =
                                ui.add(egui::Button::new(label).wrap().min_size(size).stroke(
                                    if slot.fine {
                                        egui::Stroke::new(1.5_f32, GOLD)
                                    } else {
                                        egui::Stroke::NONE
                                    },
                                ));
                            if let Some(shop) = shop {
                                let sell = |count| {
                                    Action::Command(Command::Sell {
                                        shop: shop.shop.clone(),
                                        slot: i as u8,
                                        count,
                                    })
                                };
                                if response.clicked() {
                                    actions.push(sell(1));
                                } else if response.secondary_clicked() {
                                    actions.push(sell(slot.count));
                                }
                                response.on_hover_text(format!(
                                    "{} pays {} each. Click to sell one, right-click to sell {}.",
                                    shop.name, slot.price, slot.count
                                ));
                            }
                        }
                        if (i + 1) % COLUMNS == 0 {
                            ui.end_row();
                        }
                    }
                });
        });
    if !open {
        actions.push(Action::Close);
    }
    actions
}

/// The shop you stand at: its wares and prices, and Buy. A click buys one, a right-click ten.
pub fn shop(ctx: &egui::Context, player: &PlayerState) -> Vec<Action> {
    let mut actions = Vec::new();
    let Some(shop) = &player.shop else {
        return actions;
    };
    egui::Window::new(&shop.name)
        .id(egui::Id::new("shop"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::LEFT_CENTER, [24., 0.])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(format!("You have {} coins", player.coins)).color(GOLD));
            ui.separator();
            egui::Grid::new("shop-wares")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    for ware in &shop.sells {
                        ui.label(item_name(ware));
                        ui.label(format!("{} coins", ware.price));
                        let response =
                            ui.add_enabled(ware.price <= player.coins, egui::Button::new("Buy"));
                        let buy = |count| {
                            Action::Command(Command::Buy {
                                shop: shop.shop.clone(),
                                item: ware.item.clone(),
                                count,
                            })
                        };
                        if response.clicked() {
                            actions.push(buy(1));
                        } else if response.secondary_clicked() {
                            actions.push(buy(10));
                        }
                        response.on_hover_text("Click to buy one, right-click to buy ten.");
                        ui.end_row();
                    }
                });
            ui.small("Sell from your bag while you stand here.");
        });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, count: u16, fine: bool) -> ItemView {
        ItemView {
            item: name.to_lowercase(),
            name: name.into(),
            count,
            fine,
            price: 0,
        }
    }

    #[test]
    fn piles_are_taken_only_in_reach_and_nearest_first() {
        let pile = |id, x| LootView {
            id,
            position: Vec3::new(x, 0., 0.),
            coins: 5,
            items: vec![item("Wolf pelt", 2, false), item("Iron sword", 1, true)],
        };
        let player = PlayerState {
            loot: vec![pile(1, 2.5), pile(2, 1.), pile(3, 9.)],
            ..Default::default()
        };
        assert_eq!(nearest_pile(&player, Vec3::ZERO).map(|p| p.id), Some(2));
        assert!(nearest_pile(&player, Vec3::new(20., 0., 0.)).is_none());
        assert_eq!(
            contents(&player.loot[0]),
            "5 coins · Wolf pelt ×2 · Fine iron sword"
        );
    }

    #[test]
    fn bag_and_shop_draw_every_slot_and_ware() {
        let mut player = PlayerState {
            coins: 30,
            inventory: vec![ItemView::default(); 28],
            shop: Some(protocol::ShopView {
                shop: "general".into(),
                name: "General store".into(),
                sells: vec![ItemView {
                    price: 12,
                    ..item("Bread", 1, false)
                }],
            }),
            ..Default::default()
        };
        player.inventory[0] = ItemView {
            price: 4,
            ..item("Wolf pelt", 3, false)
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            assert!(bag(ctx, &player).is_empty());
            assert!(shop(ctx, &player).is_empty());
        });
        player.shop = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            assert!(shop(ctx, &player).is_empty());
        });
    }
}
