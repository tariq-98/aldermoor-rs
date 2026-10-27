//! A player's character as it is kept between sessions (#88): who it is, its specialties' levels
//! and XP, what it carries and where it stood. The server writes these to its database and puts
//! them back when the player logs in again.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::content::Specialty;
use crate::loot::{Inventory, Stack};
use crate::progress::Progress;
use crate::{Fighter, Sim, Transform};
use protocol::UnitId;

/// Everything about a character that outlasts a session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterRecord {
    pub name: String,
    /// The equipped specialty's key (`Specialty::key`).
    pub specialty: String,
    /// Each specialty's level and XP into it, in `Specialty::ALL` order.
    pub levels: [u8; 6],
    pub xp: [u32; 6],
    /// Quest XP not yet given to a specialty.
    pub unassigned: u32,
    /// The pack, slot by slot: item key, count and whether it's fine.
    pub slots: Vec<Option<(String, u16, bool)>>,
    pub coins: u32,
    /// Where it stood on the ground, and which way it faced.
    pub at: [f32; 2],
    pub yaw: f32,
}

impl Sim {
    /// Player `id`'s character as it stands, to keep.
    pub fn record(&self, id: UnitId) -> Option<CharacterRecord> {
        let unit = self.get::<crate::Unit>(id)?;
        let p = self.get::<Progress>(id)?;
        let t = self.get::<Transform>(id)?;
        let inv = self.get::<Inventory>(id)?;
        let spec = self.get::<Fighter>(id).and_then(|f| f.specialty).unwrap_or(Specialty::Warrior);
        Some(CharacterRecord {
            name: unit.name.clone(),
            specialty: spec.key().to_string(),
            levels: p.levels,
            xp: p.xp,
            unassigned: p.unassigned,
            slots: inv.slots.iter().map(|s| s.as_ref().map(|s| (s.item.to_string(), s.count, s.fine))).collect(),
            coins: inv.coins,
            at: [t.pos.x, t.pos.z],
            yaw: t.yaw,
        })
    }

    /// Adds a player as `record` left them: where they stood, at their specialties' levels and
    /// XP, carrying what they carried. Items the game no longer knows are left out.
    pub fn spawn_record(&mut self, record: &CharacterRecord) -> UnitId {
        let id = self.spawn_player(&record.name, Vec2::from(record.at), record.yaw);
        self.restore(id, record);
        id
    }

    /// Puts `record`'s levels, XP, pack and specialty on player `id` (already in the world).
    pub fn restore(&mut self, id: UnitId, record: &CharacterRecord) {
        if let Some(mut p) = self.get_mut::<Progress>(id) {
            p.levels = record.levels.map(|l| l.max(1));
            p.xp = record.xp;
            p.unassigned = record.unassigned;
        }
        let eco = content::items::economy();
        if let Some(mut inv) = self.get_mut::<Inventory>(id) {
            inv.coins = record.coins;
            for (slot, kept) in inv.slots.iter_mut().zip(&record.slots) {
                *slot = kept.as_ref().and_then(|(key, count, fine)| eco.item(key).map(|def| Stack { item: def.key, count: *count, fine: *fine }));
            }
        }
        let spec = Specialty::from_key(&record.specialty).unwrap_or(Specialty::Warrior);
        // `switch_specialty` refuses the one already equipped; then only the level needs setting.
        if !self.switch_specialty(id, spec)
            && let Some(e) = self.entity(id)
        {
            let level = record.levels[spec.index()].max(1);
            crate::progress::set_level(&mut self.world, e, spec, level);
        }
        if let Some(e) = self.entity(id)
            && let Some(mut v) = self.world.get_mut::<crate::Vitals>(e)
        {
            v.health = v.max_health;
        }
    }
}
