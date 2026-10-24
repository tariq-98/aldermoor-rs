//! The contract between the simulation (server) and anything that shows it (the client and
//! renderer). Clients send [`Command`]s; the server sends back a [`Snapshot`] every tick. Both
//! serialise compactly with `postcard`, so the same types work in-process now and over the
//! network later.
//!
//! Conventions: metres, seconds, radians; +y is up, +z is south, and a yaw of 0 faces +z.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Bumped whenever these types change shape.
pub const VERSION: u32 = 3;

/// A unit's ID, stable for its lifetime.
pub type UnitId = u32;

/// What a player asks for. The server checks everything; a command is a request, not a fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Desired movement in world space (length 0…1), and an optional fixed facing for
    /// strafing (mouse-look). Sent whenever it changes.
    Move { direction: Vec2, walk: bool, face_yaw: Option<f32> },
    Jump,
    /// A quick roll in a world-space direction (zero rolls backwards).
    Dodge { direction: Vec2 },
    SetTarget(Option<UnitId>),
    /// Use the ability in a bar slot (0 is slot 1). `ground` aims area abilities.
    UseSlot { slot: u8, ground: Option<Vec3> },
    SetAutoAttack(bool),
    /// Get back up after being defeated.
    Respawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitKind {
    Player,
    Npc,
    Monster,
}

/// What a unit's body is doing, for animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pose {
    Idle,
    Walk,
    Run,
    Jump,
    Fall,
    Dodge,
    Attack,
    Cast,
    Hit,
    Dead,
}

/// How a unit looks, in the art catalogue's terms (docs/ASSETS.md): the renderer assembles it
/// from `assets/catalog/`. Empty or `None` fields mean the body type's defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Look {
    /// A `body_types` key in `humanoid.json` (e.g. "male", "elf_female", "goblin_male",
    /// "skeleton"), or a creature id in `creatures.json` (e.g. "wolf.grey").
    pub art: String,
    pub tone: Option<String>,
    pub hair: Option<String>,
    pub hair_colour: Option<String>,
    pub brows: Option<String>,
    pub beard: Option<String>,
    /// Clothing and armour piece ids, in any order (their layers decide what covers what).
    pub pieces: Vec<String>,
    /// Gear item ids by slot: "main_hand", "off_hand", "back" or "ammo".
    pub gear: Vec<(String, String)>,
    /// Weapons in hand (true) or put away on their stowed sockets.
    pub drawn: bool,
    /// Dyed pieces and back items: (piece or item id, main cloth dye, trim dye), dye ids from
    /// the catalogue's `dyes`.
    /// `None` keeps the colour that channel was painted (docs/ASSETS.md, "Tints").
    #[serde(default)]
    pub dyes: Vec<(String, Option<String>, Option<String>)>,
    /// Pieces in another colourway: (piece id, texture name from `texture_variants`).
    #[serde(default)]
    pub variants: Vec<(String, String)>,
}

/// One unit as the client should draw it this tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitState {
    pub id: UnitId,
    pub kind: UnitKind,
    /// Which model and animation set to use, e.g. "player", "wolf".
    pub body: String,
    pub name: String,
    pub level: u8,
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pose: Pose,
    /// Seconds since the pose began, for animation timing.
    pub pose_time: f32,
    pub health: f32,
    pub max_health: f32,
    pub hostile: bool,
    pub target: Option<UnitId>,
    /// What to draw: body, colours, clothing, armour and gear.
    pub look: Look,
}

/// The local player's own numbers (only sent to them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub unit: UnitId,
    pub endurance: f32,
    pub max_endurance: f32,
    /// Seconds left on each bar slot's cooldown.
    pub cooldowns: Vec<f32>,
    pub auto_attack: bool,
    pub in_combat: bool,
    pub can_respawn: bool,
}

/// Things that happened this tick, for effects, sounds and floating numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Damage { target: UnitId, source: UnitId, amount: f32, crit: bool },
    Died { unit: UnitId, killer: Option<UnitId> },
    Respawned { unit: UnitId },
    Jumped { unit: UnitId },
    Dodged { unit: UnitId },
    Message { text: String },
}

/// Everything a client needs for one tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u64,
    /// Simulation time in seconds.
    pub time: f64,
    pub units: Vec<UnitState>,
    pub player: Option<PlayerState>,
    pub events: Vec<Event>,
}

impl Snapshot {
    pub fn unit(&self, id: UnitId) -> Option<&UnitState> {
        self.units.iter().find(|u| u.id == id)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("snapshots always serialise")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Snapshot, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

impl Command {
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("commands always serialise")
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Command, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_snapshots_round_trip() {
        let c = Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, face_yaw: Some(1.5) };
        assert_eq!(Command::from_bytes(&c.to_bytes()).unwrap(), c);
        let s = Snapshot {
            tick: 42,
            time: 0.7,
            units: vec![UnitState {
                id: 1,
                kind: UnitKind::Player,
                body: "player".into(),
                name: "Hero".into(),
                level: 1,
                position: Vec3::new(6.0, 9.5, 28.0),
                velocity: Vec3::ZERO,
                yaw: 3.14,
                pose: Pose::Idle,
                pose_time: 0.0,
                health: 100.0,
                max_health: 100.0,
                hostile: false,
                target: None,
                look: Look {
                    art: "male".into(),
                    pieces: vec!["peasant.top".into()],
                    gear: vec![("main_hand".into(), "sword.arming".into())],
                    drawn: true,
                    dyes: vec![("peasant.top".into(), Some("woad".into()), None)],
                    ..Default::default()
                },
            }],
            player: Some(PlayerState { unit: 1, endurance: 100.0, max_endurance: 100.0, ..Default::default() }),
            events: vec![Event::Jumped { unit: 1 }],
        };
        assert_eq!(Snapshot::from_bytes(&s.to_bytes()).unwrap(), s);
    }
}
