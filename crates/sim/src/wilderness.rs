//! The Wilderness (#76): banded ground where players may fight each other as well as monsters,
//! with real stakes. Each band has a level ceiling (the deepest none): a player above it fights
//! at the ceiling's strength while inside. A player who dies in the Wilderness drops everything
//! they carry, and monsters there drop more the deeper they fall.
//!
//! A map lists its bands in the [`Wilderness`] resource. Brambleford has none yet: the region and
//! its art come later (#138), so for now bands are set up by tests and tools.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use std::collections::BTreeMap;

use crate::combat::{entity, tell};
use crate::loot::{Inventory, Loot, Pile, Stack};
use crate::progress::Progress;
use crate::{Clock, Fighter, PlayerControl, Transform, Unit, Vitals};
use protocol::UnitId;

/// One band: the ground it covers (an axis-aligned rectangle), the highest level it lets anyone
/// fight at, and how rich its monsters' drops are (1 for the shallowest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub min: Vec2,
    pub max: Vec2,
    pub ceiling: Option<u8>,
    pub tier: u8,
}

impl Band {
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
}

/// The map's Wilderness bands, shallowest first. Where bands overlap, the first listed wins.
#[derive(Resource, Clone, Debug, Default)]
pub struct Wilderness {
    pub bands: Vec<Band>,
}

impl Wilderness {
    /// Which band `p` is in, by its place in the list.
    pub fn band_at(&self, p: Vec3) -> Option<usize> {
        let p = Vec2::new(p.x, p.z);
        self.bands.iter().position(|b| b.contains(p))
    }
}

/// The band each player was in at the last step, for the warning on crossing.
#[derive(Resource, Default)]
pub(crate) struct Crossings(BTreeMap<UnitId, Option<usize>>);

/// The band unit `e` stands in, if it's a player in the Wilderness.
pub(crate) fn band_of(world: &World, e: Entity) -> Option<usize> {
    world.get::<PlayerControl>(e)?;
    let p = world.get::<Transform>(e)?.pos;
    world.get_resource::<Wilderness>()?.band_at(p)
}

/// Two players may fight when both stand in the Wilderness.
pub(crate) fn may_duel(world: &World, a: Entity, b: Entity) -> bool {
    a != b && band_of(world, a).is_some() && band_of(world, b).is_some()
}

/// The same, for a blow from unit `attacker` (it may be gone, for a missile still flying).
pub(crate) fn may_hit(world: &World, attacker: UnitId, target: Entity) -> bool {
    entity(world, attacker).is_some_and(|a| may_duel(world, a, target))
}

/// Each step: warns players crossing into or out of a band, and fights them at the band's
/// ceiling while they're inside it (at their own level again once they leave).
pub(crate) fn update(world: &mut World) {
    if world.get_resource::<Wilderness>().is_none_or(|w| w.bands.is_empty()) {
        return;
    }
    let players: Vec<(Entity, UnitId)> = {
        let mut q = world.query_filtered::<(Entity, &Unit), With<PlayerControl>>();
        q.iter(world).map(|(e, u)| (e, u.id)).collect()
    };
    for (e, id) in players {
        let band = band_of(world, e);
        let before = world.get_resource_or_insert_with(Crossings::default).0.insert(id, band).flatten();
        let bands = world.resource::<Wilderness>().bands.clone();
        if band != before {
            let text = match (before, band) {
                (_, Some(b)) => {
                    let limit = match bands[b].ceiling {
                        Some(c) => format!("levels up to {c}"),
                        None => "no level limit".to_string(),
                    };
                    let into = match before {
                        None => "You enter the Wilderness",
                        Some(a) if b > a => "Deeper into the Wilderness",
                        Some(_) => "Back toward the edge of the Wilderness",
                    };
                    format!("{into}: band {} ({limit}). Players can attack you here, and you drop everything if you die.", b + 1)
                }
                (Some(_), None) => "You leave the Wilderness.".to_string(),
                (None, None) => unreachable!(),
            };
            tell(world, e, &text);
        }
        // The level to fight at: the equipped specialty's, held to the band's ceiling.
        let Some(spec) = world.get::<Fighter>(e).and_then(|f| f.specialty) else { continue };
        let own = world.get::<Progress>(e).map_or(1, |p| p.level(spec));
        let at = match band.and_then(|b| bands[b].ceiling) {
            Some(c) => own.min(c),
            None => own,
        };
        if world.get::<Unit>(e).is_some_and(|u| u.level != at) && world.get::<Vitals>(e).is_some_and(|v| !v.dead) {
            crate::progress::set_level(world, e, spec, at);
        }
    }
}

/// A player who dies in the Wilderness drops everything they carry, coins and all, in a pile
/// where they fell. Whoever felled them may loot it (anyone, when no player did).
pub(crate) fn drop_everything(world: &mut World, dead: Entity, killer: Option<UnitId>) {
    if band_of(world, dead).is_none() {
        return;
    }
    let Some(at) = world.get::<Transform>(dead).map(|t| t.pos) else { return };
    let (items, coins): (Vec<Stack>, u32) = {
        let Some(mut inv) = world.get_mut::<Inventory>(dead) else { return };
        let items = inv.slots.iter_mut().filter_map(Option::take).collect();
        (items, std::mem::take(&mut inv.coins))
    };
    if items.is_empty() && coins == 0 {
        return;
    }
    let owner = killer.filter(|&k| entity(world, k).is_some_and(|e| world.get::<PlayerControl>(e).is_some()));
    let t = world.resource::<Clock>().time;
    world.resource_mut::<Loot>().push(Pile { id: 0, at, owner, items, coins, dropped: t });
}

/// How many times a monster dying at `p` rolls its loot table: once at the edge, once more for
/// each band deeper.
pub(crate) fn loot_rolls(world: &World, p: Vec3) -> u8 {
    world.get_resource::<Wilderness>().and_then(|w| w.band_at(p).map(|b| w.bands[b].tier.max(1))).unwrap_or(1)
}
