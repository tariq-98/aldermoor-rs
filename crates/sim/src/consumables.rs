//! Eating food and drinking potions from the pack (#205, docs/ECONOMY.md).
//!
//! A consumable's `effect` is an aura from the combat tables, put on whoever uses it: food's buff,
//! or a potion's heal over time. Food is only eaten out of combat. Potions share one cooldown,
//! [`POTION_COOLDOWN`], kept in a [`PotionCooldown`] that only someone who has drunk one carries.

use bevy_ecs::prelude::*;
use content::items::{economy, POTION_COOLDOWN};

use crate::combat::{auras, now, source, tell};
use crate::loot::{Inventory, Stack};
use crate::vitals::COMBAT_TIMEOUT;
use crate::Vitals;

/// When this unit may drink its next potion.
#[derive(Component, Clone, Copy, Debug)]
pub struct PotionCooldown(pub f64);

/// Uses one of the consumable in `slot`. Nothing is used up if it can't take effect.
pub(crate) fn use_item(world: &mut World, e: Entity, slot: u8) {
    let t = now(world);
    let Some(v) = world.get::<Vitals>(e).copied() else { return };
    if v.dead || v.downed {
        return;
    }
    let Some(Some(stack)) = world.get::<Inventory>(e).and_then(|i| i.slots.get(slot as usize).cloned()) else { return };
    let Some(def) = economy().item(stack.item) else { return };
    let Some(effect) = def.effect else {
        tell(world, e, "You can't use that.");
        return;
    };
    if def.is_food() && t - v.last_combat < COMBAT_TIMEOUT {
        tell(world, e, "You can't eat while fighting.");
        return;
    }
    if def.is_potion() && potion_ready(world, e, t) > 0.0 {
        tell(world, e, "You can't drink another potion yet.");
        return;
    }
    let Some(src) = source(world, e, None) else { return };
    if !auras::apply_key(world, effect, e, &src) {
        return;
    }
    if def.is_potion() {
        world.entity_mut(e).insert(PotionCooldown(t + POTION_COOLDOWN));
    }
    let mut inv = world.get_mut::<Inventory>(e).unwrap();
    let left = stack.count - 1;
    inv.slots[slot as usize] = (left > 0).then_some(Stack { count: left, ..stack });
}

/// Seconds until `e` can drink another potion.
pub(crate) fn potion_ready(world: &World, e: Entity, t: f64) -> f32 {
    world.get::<PotionCooldown>(e).map_or(0.0, |c| (c.0 - t).max(0.0) as f32)
}
