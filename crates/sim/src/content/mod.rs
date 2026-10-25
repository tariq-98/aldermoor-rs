//! Combat content: abilities, auras (boons, conditions and crowd control), the weapons and the
//! skills they carry, the six specialties, monsters and combo rules.
//!
//! Plain data, built once (`catalog()`) and checked by `Catalog::problems`. Everything refers to
//! everything else by key. It lives here until the `content` crate takes it (that needs a
//! lockfile change, best made when nobody else has one open).

mod common;
mod mobs;
mod specialties;
mod weapons;

use std::collections::HashMap;
use std::sync::OnceLock;

use protocol::{Action, School};

pub use mobs::{MobDef, MobSkill, Rank};
pub use specialties::{Specialty, SpecialtyDef};
pub use weapons::{Grip, Guard, WeaponDef, UNARMED_SWING, WINDUP};

pub type AbilityId = u16;
pub type AuraId = u16;

/// Who or where an ability is aimed. Shapes in front of the user take the ability's range as
/// their length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Targeting {
    SelfOnly,
    /// An enemy: the user's target. Aimed missiles fly at where it stands when they're loosed
    /// (or where the player aims), so a target that moves can make them miss.
    Enemy,
    /// A friend, or the user when no friend is targeted.
    Ally,
    /// A circle placed on the ground within range.
    Ground { radius: f32 },
    Cone { angle: f32 },
    Line { width: f32 },
    AroundSelf { radius: f32 },
}

impl Targeting {
    pub fn is_area(self) -> bool {
        matches!(self, Targeting::Ground { .. } | Targeting::Cone { .. } | Targeting::Line { .. } | Targeting::AroundSelf { .. })
    }
}

/// Crowd control. Elites and bosses soak it into their defiance bar instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finisher {
    Blast,
    Leap,
    Projectile,
    Whirl,
}

/// One thing an ability (or an aura's tick, or a field's pulse) does.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// `base` grows with the user's level, `coeff` multiplies their power.
    Damage { base: f32, coeff: f32 },
    Heal { base: f32, coeff: f32 },
    /// An aura on each target, or (`on_self`) once on the user.
    Aura { key: &'static str, on_self: bool },
    Knockback { distance: f32 },
    Pull { distance: f32 },
    Interrupt { lockout: f64 },
    Taunt,
    /// Chips an elite's or boss's defiance bar.
    Defiance(f32),
    /// Resource for the user.
    Resource(f32),
    /// The user lunges forward, or (negative) leaps back.
    Dash { distance: f32 },
    /// The user rushes to the target; the rest of the ability lands on arrival.
    Charge,
    /// The user appears behind the target.
    Shadowstep,
    /// The user teleports forward.
    Blink { distance: f32 },
    Cleanse { count: u8 },
    /// Brings a downed (or, out of combat, defeated) ally back at this fraction of health.
    Revive { fraction: f32 },
    SelfHeal { base: f32, coeff: f32 },
    /// A lasting area: maybe a combo field, pulsing on the units inside.
    Field { field: Option<Field>, radius: f32, duration: f64, interval: f64, enemies: Vec<Effect>, allies: Vec<Effect> },
    /// A trap on the ground: the first enemy to step in springs it, and it's gone.
    Trap { radius: f32, lasts: f64, enemies: Vec<Effect> },
    /// Calls monsters to fight beside the user for a while.
    Summon { mob: &'static str, count: u8, lasts: f64 },
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Missile {
    pub speed: f32,
    /// The effect asset to draw ("arrow", "fire_bolt"…).
    pub visual: &'static str,
    /// Homing missiles follow their target; aimed ones fly straight and can miss.
    pub homing: bool,
    pub reflectable: bool,
    /// Hits everything along its flight instead of stopping at the first.
    pub pierce: bool,
}

#[derive(Clone, Debug)]
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
        }
    }

    pub fn is_channel(&self) -> bool {
        self.channel > 0.0
    }

    /// A blow that winds up briefly before it lands (the body is already swinging).
    pub fn is_swing(&self) -> bool {
        self.cast > 0.0 && self.cast <= 0.5 && self.missile.is_none() && !matches!(self.targeting, Targeting::Ground { .. }) && self.school == School::Physical
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
            && self.effects.iter().any(|e| matches!(e, Effect::Damage { .. }))
            && !self.effects.iter().any(|e| matches!(e, Effect::Charge | Effect::Shadowstep | Effect::Pull { .. }))
    }

    pub fn reviving(&self) -> bool {
        self.effects.iter().any(|e| matches!(e, Effect::Revive { .. }))
    }
}

/// A boon, condition, shield or crowd control on a unit.
#[derive(Clone, Debug)]
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
        }
    }
}

/// What a finisher does when it goes off in a field.
#[derive(Clone, Debug)]
pub struct ComboRule {
    pub field: Field,
    pub finisher: Finisher,
    pub name: &'static str,
    pub effects: Vec<Effect>,
    pub allies: bool,
}

pub struct Catalog {
    pub abilities: Vec<AbilityDef>,
    pub auras: Vec<AuraDef>,
    pub weapons: Vec<WeaponDef>,
    pub specialties: Vec<SpecialtyDef>,
    pub mobs: Vec<MobDef>,
    pub combos: Vec<ComboRule>,
    ability_ids: HashMap<&'static str, AbilityId>,
    aura_ids: HashMap<&'static str, AuraId>,
}

impl Catalog {
    fn build() -> Catalog {
        let mut abilities = Vec::new();
        let mut auras = Vec::new();
        common::auras(&mut auras);
        common::abilities(&mut abilities);
        let weapons = weapons::weapons(&mut abilities, &mut auras);
        let specialties = specialties::specialties(&mut abilities, &mut auras);
        let mobs = mobs::mobs(&mut abilities, &mut auras);
        let combos = common::combos();
        let ability_ids = abilities.iter().enumerate().map(|(i, a)| (a.key, i as AbilityId)).collect();
        let aura_ids = auras.iter().enumerate().map(|(i, a)| (a.key, i as AuraId)).collect();
        Catalog { abilities, auras, weapons, specialties, mobs, combos, ability_ids, aura_ids }
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
        self.specialties.iter().find(|d| d.id == s).expect("every specialty is defined")
    }

    pub fn mob(&self, key: &str) -> Option<&MobDef> {
        self.mobs.iter().find(|m| m.key == key)
    }

    /// Everything that doesn't add up: unknown keys, duplicates, impossible numbers.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
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
        }
        let check_effects = |owner: &str, effects: &[Effect], out: &mut Vec<String>| {
            fn walk(owner: &str, effects: &[Effect], cat: &Catalog, out: &mut Vec<String>) {
                for e in effects {
                    match e {
                        Effect::Aura { key, .. } if cat.aura_id(key).is_none() => out.push(format!("{owner}: unknown aura {key}")),
                        Effect::Summon { mob, .. } if cat.mob(mob).is_none() => out.push(format!("{owner}: unknown mob {mob}")),
                        Effect::Field { enemies, allies, .. } => {
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
                out.push(format!("{}: a basic attack needs a swing time (cooldown)", a.key));
            }
            if a.auto && a.gcd {
                out.push(format!("{}: a basic attack mustn't hold up the other skills (global cooldown)", a.key));
            }
            if a.name.is_empty() {
                out.push(format!("{}: no name", a.key));
            }
        }
        for a in &self.auras {
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
            for k in &w.skills {
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
        for k in common::DOWNED {
            known(k, "downed skills", &mut out);
        }
        out
    }
}

/// The content, built on first use.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(Catalog::build)
}

/// The skills a downed player can use: fight on, push enemies away, bandage up.
pub fn downed_skills() -> [&'static str; 3] {
    common::DOWNED
}

/// Shorthands for writing content.
pub(crate) mod fx {
    use super::Effect;

    pub fn dmg(base: f32, coeff: f32) -> Effect {
        Effect::Damage { base, coeff }
    }

    pub fn heal(base: f32, coeff: f32) -> Effect {
        Effect::Heal { base, coeff }
    }

    /// An aura on the target.
    pub fn on(key: &'static str) -> Effect {
        Effect::Aura { key, on_self: false }
    }

    /// An aura on the user.
    pub fn me(key: &'static str) -> Effect {
        Effect::Aura { key, on_self: true }
    }
}
