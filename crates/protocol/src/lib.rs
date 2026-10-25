//! The contract between the simulation (server) and anything that shows it (the client and
//! renderer). Clients send [`Command`]s; the server sends back a [`Snapshot`] every tick. Both
//! serialise compactly with `postcard`, so the same types work in-process now and over the
//! network later.
//!
//! Conventions: metres, seconds, radians; +y is up, +z is south, and a yaw of 0 faces +z.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Bumped whenever these types change shape.
pub const VERSION: u32 = 8;

/// A unit's ID, stable for its lifetime.
pub type UnitId = u32;

/// What a player asks for. The server checks everything; a command is a request, not a fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Desired movement in world space (length 0…1), and an optional fixed facing for
    /// strafing (mouse-look). `sprint` asks to sprint, which only works out of combat. Sent
    /// whenever it changes.
    Move { direction: Vec2, walk: bool, sprint: bool, face_yaw: Option<f32> },
    Jump,
    /// A quick roll in a world-space direction (zero rolls backwards).
    Dodge { direction: Vec2 },
    SetTarget(Option<UnitId>),
    /// Use the ability in a bar slot (0 is slot 1). `ground` is where the player is aiming:
    /// the spot for ground attacks, and the point aimed shots fly at (lead a moving target).
    UseSlot { slot: u8, ground: Option<Vec3> },
    SetAutoAttack(bool),
    /// Get back up after being defeated.
    Respawn,
    /// Raise (true) or lower (false) your guard. Raised, it blocks blows from the front at a
    /// cost in endurance; raised just before a blow lands, it parries it.
    Guard(bool),
    /// Change to the other weapon set.
    SwapWeapons,
    /// Help a downed ally up (or a defeated one, out of combat). Hold still while it works.
    Revive { target: UnitId },
    /// Give `amount` of your quest XP to a specialty ("warrior", "mage", …).
    AssignXp { specialty: String, amount: u32 },
    /// Pick up what fits from a loot pile within reach.
    Loot { pile: u32 },
    /// Buy `count` of an item from the shop you stand at.
    Buy { shop: String, item: String, count: u16 },
    /// Sell `count` from an inventory slot to the shop you stand at.
    Sell { shop: String, slot: u8, count: u16 },
    /// Start gathering from a node within reach (see `PlayerState::nodes`), swing after swing
    /// until it runs out, your pack fills, or you move.
    Gather { node: u32 },
    /// Start skinning a dead beast within reach.
    Skin { target: UnitId },
    /// Strike in the current swing's sweet spot for a bonus. Only the first strike each swing
    /// counts, so mashing it doesn't help.
    Strike,
    /// Make `count` of a recipe at the station you stand at, one after another.
    Craft { recipe: String, count: u16 },
    /// Stop gathering or crafting.
    StopWork,
}

/// Schools of damage and magic. Armour only stops physical blows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum School {
    #[default]
    Physical,
    Fire,
    Frost,
    Arcane,
    Nature,
    Holy,
    Shadow,
}

/// What a unit's skills spend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    #[default]
    None,
    /// Builds by hitting and being hit; ebbs away out of combat (warriors).
    Rage,
    /// Refills steadily (mages and healers).
    Mana,
    /// Refills steadily (archers).
    Focus,
    /// Refills quickly from a small pool (rogues).
    Energy,
    /// Gathered from the harm you do; fades out of combat (necromancers).
    Essence,
}

/// What a unit's body is doing in an action, coarse enough for any rig to map to a clip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    #[default]
    None,
    Slash,
    Slash2,
    Thrust,
    Overhead,
    Spin,
    Bash,
    Block,
    Parry,
    Shoot,
    CastBolt,
    CastArea,
    Channel,
    CastSelf,
    Kick,
    Leap,
    Charge,
    Bite,
    Claw,
    Slam,
    Roar,
    /// Thrown off balance by a parry.
    Stagger,
    Downed,
    Revive,
}

/// Why a blow did no harm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Avoid {
    /// Rolled through it.
    Evaded,
    Blocked,
    /// Blocked at the last moment: the attacker staggers.
    Parried,
    /// Taken by a shield aura.
    Absorbed,
    Immune,
    Reflected,
    /// An aimed shot that flew past.
    Missed,
}

/// How hard a blow that reached its victim was, for the client's hitstop, camera kick and
/// flinch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitWeight {
    Light,
    /// A blow that winds up first.
    Heavy,
    Critical,
    /// Stopped by a guard.
    Blocked,
    /// Stopped by a guard raised just in time.
    Parried,
    /// A monster's big blow, warned of on the ground.
    Big,
}

/// The ground an area covers. Cones and lines point along the area's yaw.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Circle { radius: f32 },
    Cone { range: f32, angle: f32 },
    Line { length: f32, width: f32 },
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
#[serde(default, deny_unknown_fields)]
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
    /// What to draw: body, colours, clothing, armour and gear. Shared, not copied, by every
    /// snapshot that shows the unit (#189); on the wire it's the look itself.
    pub look: Arc<Look>,
}

/// The local player's own numbers (only sent to them).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub unit: UnitId,
    pub endurance: f32,
    pub max_endurance: f32,
    /// Seconds left on each bar slot's cooldown (ten slots: keys 1–9 and 0).
    pub cooldowns: Vec<f32>,
    pub auto_attack: bool,
    pub in_combat: bool,
    pub can_respawn: bool,
    pub resource_kind: ResourceKind,
    pub resource: f32,
    pub max_resource: f32,
    /// The skill bar: 1–5 from the weapons in hand, 6 a heal, 7–9 utilities, 0 an elite skill
    /// (while downed, the downed skills instead).
    pub bar: Vec<Slot>,
    /// Seconds left on the global cooldown.
    pub gcd: f32,
    pub cast: Option<CastBar>,
    pub auras: Vec<AuraView>,
    pub guarding: bool,
    pub downed: bool,
    /// 0 or 1: which weapon set is in hand, and seconds until you can swap again.
    pub weapon_set: u8,
    pub swap_ready: f32,
    /// In deep water: no skills, guard or rolls.
    pub swimming: bool,
    pub sprinting: bool,
    /// The equipped specialty ("warrior", …), its level (the one that fights), XP into that
    /// level and what the next takes (0 at the cap), and quest XP not yet given to a specialty.
    pub specialty: String,
    pub level: u8,
    pub xp: u32,
    pub xp_next: u32,
    pub unassigned_xp: u32,
    pub coins: u32,
    /// Every inventory slot in order; an empty one has an empty `item`. Each `price` is what
    /// the shop you stand at pays for it (0 if none, or it doesn't buy that).
    pub inventory: Vec<ItemView>,
    /// Loot piles nearby that you may take.
    pub loot: Vec<LootView>,
    /// The shop whose counter you stand at.
    pub shop: Option<ShopView>,
    /// Every gathering and crafting skill, in `content::crafts::Skill` order (mining,
    /// woodcutting, skinning, herbalism, fishing, smithing, tailoring, leatherworking,
    /// enchanting, cooking, alchemy).
    pub skills: Vec<SkillView>,
    /// The gathering or crafting under way.
    pub work: Option<WorkView>,
    /// Gathering nodes nearby, depleted or not.
    pub nodes: Vec<NodeView>,
    /// The station you stand at, with what you could make there.
    pub station: Option<StationView>,
}

/// A gathering or crafting skill's level, XP into it and what the next level takes (0 at the cap).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillView {
    pub level: u8,
    pub xp: u32,
    pub xp_next: u32,
}

/// A swing at a node, or one item being made.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkView {
    /// The skill at work ("mining", "smithing", …).
    pub skill: String,
    /// What's being worked: the node's or the recipe's output's name.
    pub name: String,
    /// Seconds into this swing or item, and how long it takes.
    pub elapsed: f32,
    pub duration: f32,
    /// The sweet spot, in seconds into the swing (both 0 when crafting, which has none).
    pub sweet_from: f32,
    pub sweet_to: f32,
    /// This swing's strike landed in the sweet spot, or missed it.
    pub struck: Option<bool>,
    /// Items left to make after this one (crafting).
    pub left: u16,
}

/// A gathering node: an ore vein, a tree, a herb patch, a fishing spot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeView {
    pub id: u32,
    /// Its key in the craft tables (what to draw).
    pub node: String,
    pub name: String,
    pub position: Vec3,
    /// The skill level it takes.
    pub level: u8,
    /// False while it's run out and growing back.
    pub ready: bool,
}

/// A crafting station and its recipes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StationView {
    pub station: String,
    pub name: String,
    pub recipes: Vec<RecipeView>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecipeView {
    pub recipe: String,
    pub skill: String,
    pub level: u8,
    pub inputs: Vec<ItemView>,
    pub output: ItemView,
    /// How many you could make now from what you carry (0 below the level).
    pub can_make: u16,
}

/// Some of one item: in a slot, a pile or a shop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemView {
    /// The item's key in the content tables (its icon name too).
    pub item: String,
    pub name: String,
    pub count: u16,
    /// The rare version with a small fixed bonus.
    pub fine: bool,
    /// Coins, each: what a shop sells it for, or pays for it.
    pub price: u32,
}

/// What a dead monster left on the ground.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LootView {
    pub id: u32,
    pub position: Vec3,
    pub coins: u32,
    pub items: Vec<ItemView>,
}

/// A shop's wares, with their prices.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShopView {
    pub shop: String,
    pub name: String,
    pub sells: Vec<ItemView>,
}

/// One skill on the bar.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// The ability's key in the content tables (its icon name too); empty for an empty slot.
    pub ability: String,
    pub name: String,
    /// Seconds left before it's ready, and its full recharge.
    pub cooldown: f32,
    pub recharge: f32,
    pub cost: f32,
    pub range: f32,
    /// True when it's aimed at the ground (send `UseSlot::ground`).
    pub ground: bool,
    /// For a ground skill, the radius of the circle it lands in (for the aim preview); 0 for
    /// everything else.
    pub radius: f32,
    /// True when it repeats on its own while you have a target (the basic attack).
    pub auto: bool,
}

/// A cast or channel in progress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CastBar {
    pub ability: String,
    pub name: String,
    pub elapsed: f32,
    pub duration: f32,
    pub channel: bool,
    pub interruptible: bool,
}

/// A buff, debuff or condition on a unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuraView {
    /// Its key in the content tables (and its icon name).
    pub key: String,
    pub name: String,
    pub stacks: u8,
    pub remaining: f32,
    pub duration: f32,
    pub debuff: bool,
}

/// Combat detail for everyone in view: actions, casts, auras, projectiles and areas. The host
/// publishes one beside each snapshot (`server::Host::combat`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CombatView {
    pub tick: u64,
    pub units: Vec<UnitCombat>,
    pub projectiles: Vec<ProjectileView>,
    pub areas: Vec<AreaView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitCombat {
    pub id: UnitId,
    /// The action to animate and how long ago it began.
    pub action: Action,
    pub action_time: f32,
    pub cast: Option<CastBar>,
    pub auras: Vec<AuraView>,
    pub resource_kind: ResourceKind,
    pub resource: f32,
    pub max_resource: f32,
    pub guarding: bool,
    /// Down but not out: crawling, fighting on, waiting for help.
    pub downed: bool,
    /// Elites and bosses shrug off crowd control until their defiance bar breaks.
    pub defiance: Option<(f32, f32)>,
}

/// An arrow or bolt in flight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectileView {
    pub id: u32,
    pub position: Vec3,
    pub velocity: Vec3,
    /// What it looks like (e.g. "arrow", "fire_bolt"): an effect asset's name.
    pub visual: String,
    pub school: School,
}

/// Something on the ground: a warning of a blow about to land, or a lasting field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaView {
    pub id: u32,
    /// True for a warning (it lands when `elapsed` reaches `duration`); false for a field.
    pub warning: bool,
    pub shape: Shape,
    pub center: Vec3,
    pub yaw: f32,
    pub elapsed: f32,
    pub duration: f32,
    /// True when it threatens the viewer.
    pub hostile: bool,
    pub school: School,
    /// The combo field it is, if any ("fire", "ice", "light", "water", "poison", "lightning").
    pub field: Option<String>,
}

/// Things that happened this tick, for effects, sounds and floating numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// `periodic` damage comes from a condition (bleeding, burning…), not a blow.
    Damage { target: UnitId, source: UnitId, amount: f32, crit: bool, school: School, periodic: bool },
    Heal { target: UnitId, source: UnitId, amount: f32, crit: bool },
    /// A blow reached `victim`: it landed, or a guard stopped it. `dir` is the way it travelled
    /// on the ground, as a yaw in 256ths of a turn (0 is +z, 64 is +x). `hitstop_ms` is how long
    /// the client may freeze both bodies' poses; the sim itself never pauses.
    Hit { attacker: UnitId, victim: UnitId, dir: u8, weight: HitWeight, hitstop_ms: u8 },
    /// A blow or effect that did no harm, and why.
    Avoided { target: UnitId, source: UnitId, how: Avoid },
    Died { unit: UnitId, killer: Option<UnitId> },
    /// A player's specialty reached a new level.
    LevelUp { unit: UnitId, specialty: String, level: u8 },
    /// A player's gathering or crafting skill reached a new level.
    SkillUp { unit: UnitId, skill: String, level: u8 },
    Respawned { unit: UnitId },
    Jumped { unit: UnitId },
    Dodged { unit: UnitId },
    Message { text: String },
    AbilityUsed { unit: UnitId, ability: String, target: Option<UnitId>, ground: Option<Vec3> },
    CastStarted { unit: UnitId, ability: String, duration: f32, channel: bool },
    CastInterrupted { unit: UnitId },
    AuraApplied { unit: UnitId, aura: String },
    AuraRemoved { unit: UnitId, aura: String },
    ProjectileLaunched { id: u32, from: UnitId, visual: String },
    /// A projectile's flight ended: on `hit`, or against the world.
    ProjectileEnded { id: u32, at: Vec3, hit: Option<UnitId> },
    /// A warned-of blow landed.
    AreaImpact { id: u32, at: Vec3, radius: f32, school: School },
    /// Knocked off balance by a parry.
    Staggered { unit: UnitId, by: UnitId },
    Downed { unit: UnitId },
    /// Back up from downed (by help, a bandage, or felling an attacker).
    Rallied { unit: UnitId },
    /// Back from defeat, by another's hand.
    Revived { unit: UnitId },
    DefianceBroken { unit: UnitId },
    /// A finisher went through a combo field.
    Combo { unit: UnitId, name: String, at: Vec3 },
    WeaponsSwapped { unit: UnitId },
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
        let c = Command::Move { direction: Vec2::new(0.0, -1.0), walk: false, sprint: false, face_yaw: Some(1.5) };
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
                yaw: 3.0,
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
                }
                .into(),
            }],
            player: Some(PlayerState {
                unit: 1,
                endurance: 100.0,
                max_endurance: 100.0,
                resource_kind: ResourceKind::Rage,
                bar: vec![Slot { ability: "sword_chain".into(), name: "Sword Chain".into(), auto: true, ..Default::default() }],
                cast: Some(CastBar { ability: "fire_bolt".into(), name: "Fire Bolt".into(), elapsed: 0.2, duration: 1.0, channel: false, interruptible: true }),
                ..Default::default()
            }),
            events: vec![
                Event::Jumped { unit: 1 },
                Event::Damage { target: 2, source: 1, amount: 12.0, crit: false, school: School::Fire, periodic: true },
                Event::Avoided { target: 1, source: 2, how: Avoid::Parried },
            ],
        };
        assert_eq!(Snapshot::from_bytes(&s.to_bytes()).unwrap(), s);
        let v = CombatView {
            tick: 42,
            units: vec![UnitCombat {
                id: 2,
                action: Action::Bite,
                action_time: 0.1,
                cast: None,
                auras: vec![AuraView { key: "bleeding".into(), name: "Bleeding".into(), stacks: 2, remaining: 3.0, duration: 6.0, debuff: true }],
                resource_kind: ResourceKind::None,
                resource: 0.0,
                max_resource: 0.0,
                guarding: false,
                downed: false,
                defiance: Some((300.0, 400.0)),
            }],
            projectiles: vec![ProjectileView { id: 7, position: Vec3::ONE, velocity: Vec3::X, visual: "arrow".into(), school: School::Physical }],
            areas: vec![AreaView {
                id: 9,
                warning: true,
                shape: Shape::Cone { range: 4.0, angle: 1.5 },
                center: Vec3::ZERO,
                yaw: 0.5,
                elapsed: 0.2,
                duration: 0.8,
                hostile: true,
                school: School::Physical,
                field: None,
            }],
        };
        let bytes = postcard::to_stdvec(&v).unwrap();
        assert_eq!(postcard::from_bytes::<CombatView>(&bytes).unwrap(), v);
    }
}
