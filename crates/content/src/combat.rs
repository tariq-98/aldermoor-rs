//! Combat content: abilities, auras (boons, conditions and crowd control), the weapons and the
//! skills they carry, the six specialties, monsters and combo rules.
//!
//! The tables are RON files in `crates/content/combat/`, built into the binary and loaded once
//! on first use ([`catalog`]). Loading checks every file's syntax (a bad file names its line and
//! column) and then [`Catalog::problems`]: unknown keys, duplicates, impossible numbers.
//! Everything refers to everything else by key; the sim looks things up by index.
//!
//! Strings in the files are borrowed from the file text, so they can't contain backslash
//! escapes.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use protocol::{Action, Look, ResourceKind, School};
use serde::{Deserialize, Serialize};

pub type AbilityId = u16;
pub type AuraId = u16;

/// The combat tables, in load order. Ability and aura ids follow this order.
pub const FILES: [(&str, &str); 4] = [
    ("common.ron", include_str!("../combat/common.ron")),
    ("weapons.ron", include_str!("../combat/weapons.ron")),
    ("specialties.ron", include_str!("../combat/specialties.ron")),
    ("mobs.ron", include_str!("../combat/mobs.ron")),
];

/// The skills a downed player can use: fight on, push enemies away, bandage up.
pub const DOWNED: [&str; 3] = ["downed_strike", "downed_shove", "downed_bandage"];

/// The sweep of a blow with nothing (or no melee weapon) in the main hand.
pub const UNARMED_SWING: f32 = 1.4;

/// Heavy blows wind up this long before they land: long enough to see coming and parry or roll
/// away from, short enough to feel quick. The tables write it as `cast: 0.4`.
pub const WINDUP: f64 = 0.4;

/// How long the client freezes poses when a blow reaches its victim, by weight, in ms. The sim
/// never pauses; this rides on `Event::Hit`. Conditions ticking get none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hitstop {
    pub light: u8,
    pub heavy: u8,
    pub critical: u8,
    pub blocked: u8,
    pub parried: u8,
    /// A monster's big, warned-of blow.
    pub big: u8,
    /// Missiles get this fraction: the archer is far from the arrow.
    pub missile: f32,
    pub cap: u8,
}

pub const HITSTOP: Hitstop = Hitstop {
    light: 40,
    heavy: 80,
    critical: 110,
    blocked: 60,
    parried: 110,
    big: 120,
    missile: 0.5,
    cap: 120,
};

impl Hitstop {
    pub fn ms(&self, weight: protocol::HitWeight, missile: bool) -> u8 {
        use protocol::HitWeight::*;
        let ms = match weight {
            Light => self.light,
            Heavy => self.heavy,
            Critical => self.critical,
            Blocked => self.blocked,
            Parried => self.parried,
            Big => self.big,
        }
        .min(self.cap);
        if missile {
            (ms as f32 * self.missile).round() as u8
        } else {
            ms
        }
    }
}

/// Who or where an ability is aimed. Shapes in front of the user take the ability's range as
/// their length.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Targeting {
    SelfOnly,
    /// An enemy: the user's target. Aimed missiles fly at where it stands when they're loosed
    /// (or where the player aims), so a target that moves can make them miss.
    Enemy,
    /// A friend, or the user when no friend is targeted.
    Ally,
    /// A circle placed on the ground within range.
    Ground {
        radius: f32,
    },
    Cone {
        angle: f32,
    },
    Line {
        width: f32,
    },
    AroundSelf {
        radius: f32,
    },
}

impl Targeting {
    pub fn is_area(self) -> bool {
        matches!(
            self,
            Targeting::Ground { .. }
                | Targeting::Cone { .. }
                | Targeting::Line { .. }
                | Targeting::AroundSelf { .. }
        )
    }
}

/// Crowd control. Elites and bosses soak it into their defiance bar instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cc {
    Stun,
    Daze,
    Root,
    Slow,
    Fear,
    Knockdown,
    Silence,
    Sleep,
}

impl Cc {
    /// Hard control stops everything, casting included.
    pub fn hard(self) -> bool {
        matches!(self, Cc::Stun | Cc::Knockdown | Cc::Fear | Cc::Sleep)
    }

    /// Repeats in the same category within the window land for less, then not at all.
    pub fn category(self) -> Option<usize> {
        match self {
            Cc::Stun | Cc::Knockdown => Some(0),
            Cc::Root => Some(1),
            Cc::Fear | Cc::Sleep => Some(2),
            Cc::Silence | Cc::Daze => Some(3),
            Cc::Slow => None,
        }
    }
}

/// Combo fields (lasting areas) and the finishers that set them off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Field {
    Fire,
    Ice,
    Light,
    Water,
    Poison,
    Lightning,
}

impl Field {
    pub fn name(self) -> &'static str {
        match self {
            Field::Fire => "fire",
            Field::Ice => "ice",
            Field::Light => "light",
            Field::Water => "water",
            Field::Poison => "poison",
            Field::Lightning => "lightning",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Finisher {
    Blast,
    Leap,
    Projectile,
    Whirl,
}

/// One thing an ability (or an aura's tick, or a field's pulse) does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Effect {
    /// `base` grows with the user's level, `coeff` multiplies their power.
    Damage {
        base: f32,
        coeff: f32,
    },
    Heal {
        base: f32,
        coeff: f32,
    },
    /// An aura on each target, or (`on_self`) once on the user.
    Aura {
        key: &'static str,
        #[serde(default)]
        on_self: bool,
    },
    Knockback {
        distance: f32,
    },
    Pull {
        distance: f32,
    },
    Interrupt {
        lockout: f64,
    },
    Taunt,
    /// Chips an elite's or boss's defiance bar.
    Defiance(f32),
    /// Resource for the user.
    Resource(f32),
    /// The user lunges forward, or (negative) leaps back.
    Dash {
        distance: f32,
    },
    /// The user rushes to the target; the rest of the ability lands on arrival.
    Charge,
    /// The user appears behind the target.
    Shadowstep,
    /// The user teleports forward.
    Blink {
        distance: f32,
    },
    Cleanse {
        count: u8,
    },
    /// Brings a downed (or, out of combat, defeated) ally back at this fraction of health.
    Revive {
        fraction: f32,
    },
    SelfHeal {
        base: f32,
        coeff: f32,
    },
    /// A lasting area: maybe a combo field, pulsing on the units inside.
    Field {
        #[serde(default)]
        field: Option<Field>,
        radius: f32,
        duration: f64,
        interval: f64,
        #[serde(default)]
        enemies: Vec<Effect>,
        #[serde(default)]
        allies: Vec<Effect>,
    },
    /// A trap on the ground: the first enemy to step in springs it, and it's gone.
    Trap {
        radius: f32,
        lasts: f64,
        enemies: Vec<Effect>,
    },
    /// Calls monsters to fight beside the user for a while.
    Summon {
        mob: &'static str,
        count: u8,
        lasts: f64,
    },
}

impl Effect {
    /// Effects that act once on the user rather than on each target.
    pub fn on_user(&self) -> bool {
        match self {
            Effect::Aura { on_self, .. } => *on_self,
            Effect::Resource(_)
            | Effect::Dash { .. }
            | Effect::Charge
            | Effect::Shadowstep
            | Effect::Blink { .. }
            | Effect::SelfHeal { .. }
            | Effect::Field { .. }
            | Effect::Trap { .. }
            | Effect::Summon { .. } => true,
            _ => false,
        }
    }
}

/// A projectile: how fast, what it looks like, whether it follows its target.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Missile {
    pub speed: f32,
    /// The effect asset to draw ("arrow", "fire_bolt"…).
    pub visual: &'static str,
    /// Homing missiles follow their target; aimed ones fly straight and can miss.
    #[serde(default)]
    pub homing: bool,
    #[serde(default = "yes")]
    pub reflectable: bool,
    /// Hits everything along its flight instead of stopping at the first.
    #[serde(default)]
    pub pierce: bool,
}

fn yes() -> bool {
    true
}

/// Numbers for fights between players (a player or their summon on each side), as multiples of
/// the PvE ones, so duels and the Wilderness are balanced apart from monsters. Left out, they're
/// all 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pvp {
    /// Damage dealt to a player (an aura's: its ticks).
    pub damage: f32,
    /// Healing on a player fighting players.
    pub healing: f32,
    /// How long auras last on a player (an ability's: the auras it puts on them).
    pub duration: f32,
}

impl Default for Pvp {
    fn default() -> Pvp {
        Pvp {
            damage: 1.0,
            healing: 1.0,
            duration: 1.0,
        }
    }
}

impl Pvp {
    fn problem(&self) -> Option<String> {
        let ok = |x: f32| x > 0.0 && x <= 2.0;
        (!ok(self.damage) || !ok(self.healing) || !ok(self.duration))
            .then(|| format!("PvP numbers outside 0 to 2 times PvE: {self:?}"))
    }
}

/// An ability. Fields left out of a table take the values in [`AbilityDef::base`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AbilityDef {
    pub key: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub school: School,
    pub targeting: Targeting,
    pub range: f32,
    pub cast: f64,
    pub channel: f64,
    pub ticks: u8,
    pub gcd: bool,
    pub cooldown: f64,
    pub cost: f32,
    /// Resource the user gains (builders: rage and essence).
    pub generates: f32,
    /// Can be used on the move (casts that aren't stop when the user moves).
    pub movable: bool,
    /// Repeats while there's a target: the basic attack.
    pub auto: bool,
    /// A monster's big blow, warned of on the ground while it winds up.
    pub telegraph: bool,
    /// Can't be blocked or parried; dodge it.
    pub unblockable: bool,
    pub missile: Option<Missile>,
    pub effects: Vec<Effect>,
    pub channel_effects: Vec<Effect>,
    /// Below this fraction of the target's health, damage times this.
    pub execute: Option<(f32, f32)>,
    /// Damage times this when struck from behind.
    pub backstab: f32,
    pub finisher: Option<Finisher>,
    pub max_targets: u8,
    pub action: Action,
    pub breaks_stun: bool,
    pub interruptible: bool,
    /// One of the downed skills.
    pub downed: bool,
    /// Areas that help friends instead of hurting enemies.
    pub allies: bool,
    /// Ground blows land this long after they're loosed, warned of meanwhile.
    pub impact_delay: f64,
    /// Channel ticks loose the missile instead of striking directly.
    pub channel_missile: bool,
    pub pvp: Pvp,
}

impl Default for AbilityDef {
    fn default() -> AbilityDef {
        AbilityDef::base()
    }
}

impl AbilityDef {
    /// The defaults every definition starts from.
    pub fn base() -> AbilityDef {
        AbilityDef {
            key: "",
            name: "",
            summary: "",
            school: School::Physical,
            targeting: Targeting::Enemy,
            range: 3.0,
            cast: 0.0,
            channel: 0.0,
            ticks: 0,
            gcd: true,
            cooldown: 0.0,
            cost: 0.0,
            generates: 0.0,
            movable: true,
            auto: false,
            telegraph: false,
            unblockable: false,
            missile: None,
            effects: Vec::new(),
            channel_effects: Vec::new(),
            execute: None,
            backstab: 1.0,
            finisher: None,
            max_targets: 5,
            action: Action::Slash,
            breaks_stun: false,
            interruptible: true,
            downed: false,
            allies: false,
            impact_delay: 0.0,
            channel_missile: false,
            pvp: Pvp::default(),
        }
    }

    pub fn is_channel(&self) -> bool {
        self.channel > 0.0
    }

    /// A blow that winds up briefly before it lands (the body is already swinging).
    pub fn is_swing(&self) -> bool {
        self.cast > 0.0
            && self.cast <= 0.5
            && self.missile.is_none()
            && !matches!(self.targeting, Targeting::Ground { .. })
            && self.school == School::Physical
    }

    /// A single-target melee blow. A player's sweeps the cone in front of them (the weapon's
    /// swing, out to the blow's range) and lands on every enemy in it; a monster's lands on its
    /// target. Gap closers (charges, shadowsteps, hooks) and channels stay single-target.
    pub fn is_strike(&self) -> bool {
        self.targeting == Targeting::Enemy
            && self.missile.is_none()
            && !self.allies
            && !self.is_channel()
            && self.range <= 5.0
            && self
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Damage { .. }))
            && !self
                .effects
                .iter()
                .any(|e| matches!(e, Effect::Charge | Effect::Shadowstep | Effect::Pull { .. }))
    }

    pub fn reviving(&self) -> bool {
        self.effects
            .iter()
            .any(|e| matches!(e, Effect::Revive { .. }))
    }
}

/// A boon, condition, shield or crowd control on a unit. Fields left out of a table take the
/// values in [`AuraDef::base`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuraDef {
    pub key: &'static str,
    pub name: &'static str,
    pub duration: f64,
    pub debuff: bool,
    pub max_stacks: u8,
    pub dispellable: bool,
    /// Every `.0` seconds, `.1` happens to the wearer (times the stacks).
    pub tick: Option<(f64, Effect)>,
    pub school: School,
    /// Fractions, per stack.
    pub damage_done: f32,
    pub damage_taken: f32,
    pub healing_taken: f32,
    pub move_speed: f32,
    pub haste: f32,
    pub crit: f32,
    pub armor: f32,
    /// A shield: absorbs `base + coeff × power` damage.
    pub absorb: Option<(f32, f32)>,
    pub cc: Option<Cc>,
    pub breaks_on_damage: bool,
    pub blocks: bool,
    pub reflects: bool,
    pub cc_immune: bool,
    pub invulnerable: bool,
    /// Threat times this.
    pub threat: f32,
    pub pvp: Pvp,
}

impl Default for AuraDef {
    fn default() -> AuraDef {
        AuraDef::base()
    }
}

impl AuraDef {
    pub fn base() -> AuraDef {
        AuraDef {
            key: "",
            name: "",
            duration: 5.0,
            debuff: false,
            max_stacks: 1,
            dispellable: true,
            tick: None,
            school: School::Physical,
            damage_done: 0.0,
            damage_taken: 0.0,
            healing_taken: 0.0,
            move_speed: 0.0,
            haste: 0.0,
            crit: 0.0,
            armor: 0.0,
            absorb: None,
            cc: None,
            breaks_on_damage: false,
            blocks: false,
            reflects: false,
            cc_immune: false,
            invulnerable: false,
            threat: 1.0,
            pvp: Pvp::default(),
        }
    }
}

/// What a finisher does when it goes off in a field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComboRule {
    pub field: Field,
    pub finisher: Finisher,
    pub name: &'static str,
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub allies: bool,
}

/// How a weapon is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grip {
    Main,
    Off,
    /// Either hand (daggers): main-hand skills in the main hand, off-hand skills in the off.
    Either,
    Two,
}

/// Weapons and the skills they carry. Skills 1–3 come from the main hand and 4–5 from the off
/// hand, or all five from a two-handed weapon. Weapon skills cost nothing but time: specialties
/// spend their resource on skills 6–0.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
    /// The gear catalogue's item id.
    pub item: &'static str,
    pub grip: Grip,
    /// Skills 1–3 (main hand), 1–5 (two-handed) or 4–5 (off hand).
    pub skills: Vec<&'static str>,
    /// For weapons held in either hand: skills 4–5 when in the off hand.
    #[serde(default)]
    pub off_skills: Vec<&'static str>,
    /// Shields block wide and cheaply, and stop missiles; other melee weapons parry blows in a
    /// narrower arc and can't stop missiles; ranged and casting weapons can't guard.
    pub guard: Guard,
    /// How wide (the full angle, in radians) its single-target blows sweep in front, out to
    /// each blow's range: spears thrust narrow, greatswords swing wide.
    #[serde(default = "unarmed_swing")]
    pub swing: f32,
}

fn unarmed_swing() -> f32 {
    UNARMED_SWING
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Guard {
    None,
    Weapon,
    Shield,
}

/// The six specialties. Each brings its own resource and skills 6–0 (a heal, three utilities
/// and an elite skill), its default weapons, and how hardy, strong and armoured it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Specialty {
    Warrior,
    Archer,
    Mage,
    Necromancer,
    Rogue,
    Healer,
}

impl Specialty {
    pub const ALL: [Specialty; 6] = [
        Specialty::Warrior,
        Specialty::Archer,
        Specialty::Mage,
        Specialty::Necromancer,
        Specialty::Rogue,
        Specialty::Healer,
    ];

    /// Its place in [`Specialty::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its name on the wire, e.g. "warrior".
    pub fn key(self) -> &'static str {
        match self {
            Specialty::Warrior => "warrior",
            Specialty::Archer => "archer",
            Specialty::Mage => "mage",
            Specialty::Necromancer => "necromancer",
            Specialty::Rogue => "rogue",
            Specialty::Healer => "healer",
        }
    }

    pub fn from_key(key: &str) -> Option<Specialty> {
        Specialty::ALL.into_iter().find(|s| s.key() == key)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecialtyDef {
    pub id: Specialty,
    pub name: &'static str,
    pub resource: ResourceKind,
    pub max_resource: f32,
    /// Per second, in and out of combat (negative ebbs away).
    pub regen_in: f32,
    pub regen_out: f32,
    /// Resource on a fresh start (builders start empty).
    pub start: f32,
    /// Multipliers on the level's health and power, and armour as a share of the level's.
    pub health: f32,
    pub power: f32,
    pub armor: f32,
    pub heals: Vec<&'static str>,
    pub utilities: Vec<&'static str>,
    pub elites: Vec<&'static str>,
    /// Default weapon sets: (main or two-handed item, off-hand item).
    pub sets: Vec<(Option<&'static str>, Option<&'static str>)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rank {
    #[default]
    Normal,
    /// Tougher, with a defiance bar.
    Elite,
    Boss,
}

/// When a monster uses a skill: below a health fraction, with a chance each time it thinks.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MobSkill {
    pub ability: &'static str,
    pub below: f32,
    pub chance: f32,
}

/// Monsters: how tough and strong they are for their level, their basic attack, and the skills
/// they reach for and when. Their big blows wind up with a warning on the ground: dodge them.
/// Fields left out of a table take the values in [`MobDef::base`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MobDef {
    pub key: &'static str,
    pub name: &'static str,
    /// The simulation's body and animation set ("wolf", "goblin", "humanoid").
    pub body: &'static str,
    pub look: Look,
    pub level: u8,
    pub rank: Rank,
    /// Multipliers on the level's health and power; armour as a share of the level's.
    pub health: f32,
    pub power: f32,
    pub armor: f32,
    pub auto: &'static str,
    pub skills: Vec<MobSkill>,
    pub aggro: f32,
    /// Pack animals: the whole pack joins a fight.
    pub social: bool,
    /// Where it likes to fight from (0: in melee).
    pub range: f32,
    pub speed: f32,
    pub radius: f32,
    pub height: f32,
    /// The defiance bar (0: none).
    pub defiance: f32,
    pub respawn: f64,
    pub wander: f32,
}

impl Default for MobDef {
    fn default() -> MobDef {
        MobDef::base()
    }
}

impl MobDef {
    pub fn base() -> MobDef {
        MobDef {
            key: "",
            name: "",
            body: "humanoid",
            look: Look::default(),
            level: 1,
            rank: Rank::Normal,
            health: 0.5,
            power: 0.5,
            armor: 0.2,
            auto: "punch",
            skills: Vec::new(),
            aggro: 10.0,
            social: false,
            range: 0.0,
            speed: 5.4,
            radius: 0.4,
            height: 1.8,
            defiance: 0.0,
            respawn: 20.0,
            wander: 6.0,
        }
    }
}

/// One table file. Every list is optional, so a file holds whichever kinds it needs.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Table {
    pub auras: Vec<AuraDef>,
    pub abilities: Vec<AbilityDef>,
    pub weapons: Vec<WeaponDef>,
    pub specialties: Vec<SpecialtyDef>,
    pub mobs: Vec<MobDef>,
    pub combos: Vec<ComboRule>,
}

/// Why the tables didn't load.
#[derive(Debug)]
pub enum LoadError {
    /// A file isn't valid RON, or doesn't match the table's shape.
    Syntax {
        file: String,
        error: Box<ron::error::SpannedError>,
    },
    /// The files parse, but don't add up.
    Problems(Vec<String>),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Syntax { file, error } => write!(
                f,
                "crates/content/combat/{file}:{}: {}",
                error.span.start, error.code
            ),
            LoadError::Problems(p) => {
                write!(f, "the combat tables don't add up:\n  {}", p.join("\n  "))
            }
        }
    }
}

impl std::error::Error for LoadError {}

pub struct Catalog {
    pub abilities: Vec<AbilityDef>,
    pub auras: Vec<AuraDef>,
    pub weapons: Vec<WeaponDef>,
    pub specialties: Vec<SpecialtyDef>,
    pub mobs: Vec<MobDef>,
    pub combos: Vec<ComboRule>,
    ability_ids: HashMap<&'static str, AbilityId>,
    aura_ids: HashMap<&'static str, AuraId>,
    /// Monsters by key: every monster looks its own up every step.
    mob_ids: HashMap<&'static str, usize>,
}

impl Catalog {
    /// Loads and checks the built-in tables.
    pub fn load() -> Result<Catalog, LoadError> {
        Catalog::from_files(&FILES)
    }

    /// Loads and checks tables given as (file name, RON text), in order. Text that isn't
    /// `'static` (read from disk, say) can be leaked first: the catalogue lives for the program.
    pub fn from_files(files: &[(&str, &'static str)]) -> Result<Catalog, LoadError> {
        let mut all = Table::default();
        for (file, text) in files {
            let t: Table = ron::from_str(text).map_err(|error| LoadError::Syntax {
                file: file.to_string(),
                error: Box::new(error),
            })?;
            all.auras.extend(t.auras);
            all.abilities.extend(t.abilities);
            all.weapons.extend(t.weapons);
            all.specialties.extend(t.specialties);
            all.mobs.extend(t.mobs);
            all.combos.extend(t.combos);
        }
        let cat = Catalog::new(all);
        let problems = cat.problems();
        if problems.is_empty() {
            Ok(cat)
        } else {
            Err(LoadError::Problems(problems))
        }
    }

    fn new(t: Table) -> Catalog {
        let ability_ids = t
            .abilities
            .iter()
            .enumerate()
            .map(|(i, a)| (a.key, i as AbilityId))
            .collect();
        let aura_ids = t
            .auras
            .iter()
            .enumerate()
            .map(|(i, a)| (a.key, i as AuraId))
            .collect();
        // The first of a duplicated key wins, as a search would find it (`problems` reports it).
        let mut mob_ids = HashMap::new();
        for (i, m) in t.mobs.iter().enumerate() {
            mob_ids.entry(m.key).or_insert(i);
        }
        Catalog {
            abilities: t.abilities,
            auras: t.auras,
            weapons: t.weapons,
            specialties: t.specialties,
            mobs: t.mobs,
            combos: t.combos,
            ability_ids,
            aura_ids,
            mob_ids,
        }
    }

    pub fn ability(&self, id: AbilityId) -> &AbilityDef {
        &self.abilities[id as usize]
    }

    pub fn ability_id(&self, key: &str) -> Option<AbilityId> {
        self.ability_ids.get(key).copied()
    }

    pub fn aura(&self, id: AuraId) -> &AuraDef {
        &self.auras[id as usize]
    }

    pub fn aura_id(&self, key: &str) -> Option<AuraId> {
        self.aura_ids.get(key).copied()
    }

    pub fn weapon(&self, item: &str) -> Option<&WeaponDef> {
        self.weapons.iter().find(|w| w.item == item)
    }

    pub fn specialty(&self, s: Specialty) -> &SpecialtyDef {
        self.specialties
            .iter()
            .find(|d| d.id == s)
            .expect("every specialty is defined")
    }

    pub fn mob(&self, key: &str) -> Option<&MobDef> {
        self.mob_ids.get(key).map(|&i| &self.mobs[i])
    }

    /// Everything that doesn't add up: unknown keys, duplicates, impossible numbers.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.abilities.len() > AbilityId::MAX as usize || self.auras.len() > AuraId::MAX as usize
        {
            out.push("more abilities or auras than their ids can count".into());
        }
        let mut seen = std::collections::HashSet::new();
        for a in &self.abilities {
            if !seen.insert(a.key) {
                out.push(format!("ability {} is defined twice", a.key));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for a in &self.auras {
            if !seen.insert(a.key) {
                out.push(format!("aura {} is defined twice", a.key));
            }
            if a.name.is_empty() {
                out.push(format!("aura {}: no name", a.key));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for m in &self.mobs {
            if !seen.insert(m.key) {
                out.push(format!("mob {} is defined twice", m.key));
            }
            if m.key.is_empty() || m.name.is_empty() {
                out.push(format!("mob {:?}: needs a key and a name", m.key));
            }
        }
        for s in Specialty::ALL {
            match self.specialties.iter().filter(|d| d.id == s).count() {
                1 => {}
                0 => out.push(format!("specialty {s:?} isn't defined")),
                _ => out.push(format!("specialty {s:?} is defined twice")),
            }
        }
        let check_effects = |owner: &str, effects: &[Effect], out: &mut Vec<String>| {
            fn walk(owner: &str, effects: &[Effect], cat: &Catalog, out: &mut Vec<String>) {
                for e in effects {
                    match e {
                        Effect::Aura { key, .. } if cat.aura_id(key).is_none() => {
                            out.push(format!("{owner}: unknown aura {key}"))
                        }
                        Effect::Summon { mob, .. } if cat.mob(mob).is_none() => {
                            out.push(format!("{owner}: unknown mob {mob}"))
                        }
                        Effect::Field {
                            enemies, allies, ..
                        } => {
                            walk(owner, enemies, cat, out);
                            walk(owner, allies, cat, out);
                        }
                        Effect::Trap { enemies, .. } => walk(owner, enemies, cat, out),
                        _ => {}
                    }
                }
            }
            walk(owner, effects, self, out);
        };
        for a in &self.abilities {
            check_effects(a.key, &a.effects, &mut out);
            check_effects(a.key, &a.channel_effects, &mut out);
            if a.is_channel() && a.ticks == 0 {
                out.push(format!("{}: a channel with no ticks", a.key));
            }
            if a.auto && a.cooldown <= 0.0 {
                out.push(format!(
                    "{}: a basic attack needs a swing time (cooldown)",
                    a.key
                ));
            }
            if a.auto && a.gcd {
                out.push(format!(
                    "{}: a basic attack mustn't hold up the other skills (global cooldown)",
                    a.key
                ));
            }
            if a.name.is_empty() {
                out.push(format!("{}: no name", a.key));
            }
            if let Some(p) = a.pvp.problem() {
                out.push(format!("{}: {p}", a.key));
            }
        }
        for a in &self.auras {
            if let Some(p) = a.pvp.problem() {
                out.push(format!("{}: {p}", a.key));
            }
            if let Some((every, e)) = &a.tick {
                check_effects(a.key, std::slice::from_ref(e), &mut out);
                if *every < 0.1 {
                    out.push(format!("{}: ticks too often ({every} s)", a.key));
                }
            }
        }
        let known = |key: &str, what: &str, out: &mut Vec<String>| {
            if self.ability_id(key).is_none() {
                out.push(format!("{what}: unknown ability {key}"));
            }
        };
        for w in &self.weapons {
            for k in w.skills.iter().chain(&w.off_skills) {
                known(k, w.item, &mut out);
            }
        }
        for s in &self.specialties {
            for k in s.heals.iter().chain(&s.utilities).chain(&s.elites) {
                known(k, s.name, &mut out);
            }
            for (main, off) in &s.sets {
                for item in main.iter().chain(off.iter()) {
                    if self.weapon(item).is_none() {
                        out.push(format!("{}: unknown weapon {item}", s.name));
                    }
                }
            }
        }
        for m in &self.mobs {
            known(m.auto, m.key, &mut out);
            for s in &m.skills {
                known(s.ability, m.key, &mut out);
            }
        }
        for c in &self.combos {
            check_effects(c.name, &c.effects, &mut out);
        }
        for k in DOWNED {
            known(k, "downed skills", &mut out);
        }
        out
    }
}

/// The built-in tables, loaded and checked on first use. A table that doesn't load is a bug in
/// the data, caught by this crate's tests, so this panics with the reason.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog::load().unwrap_or_else(|e| panic!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_load_and_add_up() {
        let cat = Catalog::load().unwrap_or_else(|e| panic!("{e}"));
        assert!(cat.abilities.len() > 100 && cat.auras.len() > 40);
        assert_eq!(cat.specialties.len(), Specialty::ALL.len());
        assert!(cat.mob("barrow_king").is_some_and(|m| m.rank == Rank::Boss));
    }

    #[test]
    fn left_out_fields_take_the_defaults() {
        let cat = catalog();
        let punch = cat.ability(cat.ability_id("punch").unwrap());
        assert_eq!(
            (punch.max_targets, punch.backstab, punch.interruptible),
            (5, 1.0, true)
        );
        let stunned = cat.aura(cat.aura_id("stunned").unwrap());
        assert_eq!(
            (stunned.max_stacks, stunned.dispellable, stunned.threat),
            (1, true, 1.0)
        );
    }

    #[test]
    fn a_malformed_file_names_where() {
        let bad = "(\n    auras: [\n        (key: \"x\", name: \"X\", duraton: 2.0),\n    ],\n)\n";
        let e = Catalog::from_files(&[("bad.ron", bad)])
            .err()
            .expect("a typo in a field name fails")
            .to_string();
        assert!(e.starts_with("crates/content/combat/bad.ron:3:"), "{e}");
        assert!(e.contains("duraton"), "{e}");
        let e = Catalog::from_files(&[("bad.ron", "( auras: [ (key: \"x\" ")])
            .err()
            .unwrap()
            .to_string();
        assert!(e.starts_with("crates/content/combat/bad.ron:1:"), "{e}");
    }

    #[test]
    fn tables_that_dont_add_up_say_why() {
        let mut files = FILES.to_vec();
        files.push((
            "extra.ron",
            "(abilities: [(key: \"oops\", name: \"Oops\", effects: [Aura(key: \"nonesuch\")])])",
        ));
        let e = Catalog::from_files(&files)
            .err()
            .expect("an unknown aura fails")
            .to_string();
        assert!(e.contains("oops: unknown aura nonesuch"), "{e}");
        let e = Catalog::from_files(&[(
            "bad.ron",
            "(abilities: [(key: \"x\", effects: [Damage(base: 1.0, coef: 0.1)])])",
        )])
        .err()
        .unwrap()
        .to_string();
        assert!(e.contains("coef"), "{e}");
    }
}
