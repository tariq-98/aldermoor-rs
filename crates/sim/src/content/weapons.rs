//! Weapons and the skills they carry. Skills 1–3 come from the main hand and 4–5 from the off
//! hand, or all five from a two-handed weapon. Weapon skills cost nothing but time: specialties
//! spend their resource on skills 6–0.

use protocol::{Action, School};

use super::fx::{dmg, heal, me, on};
use super::{AbilityDef, AuraDef, Effect, Field, Finisher, Missile, Targeting};

/// How a weapon is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Main,
    Off,
    /// Either hand (daggers): main-hand skills in the main hand, off-hand skills in the off.
    Either,
    Two,
}

#[derive(Clone, Debug)]
pub struct WeaponDef {
    /// The gear catalogue's item id.
    pub item: &'static str,
    pub grip: Grip,
    /// Skills 1–3 (main hand), 1–5 (two-handed) or 4–5 (off hand).
    pub skills: Vec<&'static str>,
    /// For weapons held in either hand: skills 4–5 when in the off hand.
    pub off_skills: Vec<&'static str>,
    /// Shields block wide and cheaply, and stop missiles; other melee weapons parry blows in a
    /// narrower arc and can't stop missiles; ranged and casting weapons can't guard.
    pub guard: Guard,
    /// How wide (the full angle, in radians) its single-target blows sweep in front, out to
    /// each blow's range: spears thrust narrow, greatswords swing wide.
    pub swing: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    None,
    Weapon,
    Shield,
}

/// The sweep of a blow with nothing (or no melee weapon) in the main hand.
pub const UNARMED_SWING: f32 = 1.4;

/// Heavy blows wind up this long before they land: long enough to see coming and parry or roll
/// away from, short enough to feel quick.
pub const WINDUP: f64 = 0.4;

const fn aimed(speed: f32, visual: &'static str) -> Option<Missile> {
    Some(Missile { speed, visual, homing: false, reflectable: true, pierce: false })
}

const fn homing(speed: f32, visual: &'static str) -> Option<Missile> {
    Some(Missile { speed, visual, homing: true, reflectable: true, pierce: false })
}

pub fn weapons(abilities: &mut Vec<AbilityDef>, auras: &mut Vec<AuraDef>) -> Vec<WeaponDef> {
    let a = AuraDef::base;
    auras.extend([
        AuraDef { key: "shield_wall", name: "Shield Wall", duration: 2.5, dispellable: false, blocks: true, reflects: true, ..a() },
        // A sidestep or vault: nothing lands for a moment.
        AuraDef { key: "evasion", name: "Evasion", duration: 0.75, dispellable: false, invulnerable: true, ..a() },
        AuraDef { key: "arcane_ward", name: "Arcane Ward", duration: 8.0, absorb: Some((10.0, 0.8)), school: School::Arcane, ..a() },
        AuraDef { key: "barrier", name: "Barrier", duration: 8.0, absorb: Some((8.0, 0.7)), school: School::Arcane, ..a() },
    ]);
    let b = AbilityDef::base;
    abilities.extend([
        // Sword (main hand): a steady chain, a bleeding cut, a lunge.
        AbilityDef {
            key: "sword_chain",
            name: "Sword Chain",
            summary: "A steady chain of cuts.",
            gcd: false,
            cooldown: 1.2,
            auto: true,
            generates: 5.0,
            effects: vec![dmg(4.0, 0.3)],
            ..b()
        },
        AbilityDef {
            key: "sword_rend",
            name: "Rending Blade",
            summary: "Cut deep, leaving two stacks of bleeding.",
            cast: WINDUP,
            cooldown: 6.0,
            effects: vec![dmg(3.0, 0.28), on("bleeding"), on("bleeding")],
            action: Action::Slash2,
            ..b()
        },
        AbilityDef {
            key: "sword_lunge",
            name: "Lunge",
            summary: "Close in on your target and cripple it.",
            range: 10.0,
            cooldown: 12.0,
            effects: vec![Effect::Charge, dmg(4.0, 0.35), on("crippled")],
            finisher: Some(Finisher::Leap),
            action: Action::Thrust,
            ..b()
        },
        // Axe (main hand).
        AbilityDef { key: "axe_chop", name: "Chop", summary: "Heavy chops.", gcd: false, cooldown: 1.4, auto: true, generates: 5.0, effects: vec![dmg(5.0, 0.34)], ..b() },
        AbilityDef {
            key: "axe_cleave",
            name: "Cleave",
            summary: "Cleave up to three foes in front of you, leaving them vulnerable.",
            targeting: Targeting::Cone { angle: 2.1 },
            range: 3.4,
            cast: WINDUP,
            cooldown: 6.0,
            max_targets: 3,
            effects: vec![dmg(4.0, 0.3), on("vulnerable")],
            action: Action::Slash2,
            ..b()
        },
        AbilityDef {
            key: "axe_throw",
            name: "Throwing Axe",
            summary: "Hurl an axe that cripples. Aim it: it flies straight.",
            range: 15.0,
            cooldown: 10.0,
            missile: aimed(22.0, "axe"),
            effects: vec![dmg(4.0, 0.32), on("crippled")],
            action: Action::Shoot,
            ..b()
        },
        // Mace (main hand).
        AbilityDef { key: "mace_smash", name: "Smash", summary: "Crushing blows.", gcd: false, cooldown: 1.5, auto: true, generates: 5.0, effects: vec![dmg(5.0, 0.36)], action: Action::Bash, ..b() },
        AbilityDef {
            key: "mace_crush",
            name: "Crushing Blow",
            summary: "Daze your target. Breaks defiance.",
            cast: WINDUP,
            cooldown: 12.0,
            effects: vec![dmg(5.0, 0.4), on("dazed"), Effect::Defiance(120.0)],
            action: Action::Overhead,
            ..b()
        },
        AbilityDef {
            key: "mace_tremor",
            name: "Tremor",
            summary: "Strike the ground, knocking down everyone around you.",
            targeting: Targeting::AroundSelf { radius: 4.0 },
            range: 0.0,
            cast: WINDUP,
            cooldown: 15.0,
            effects: vec![dmg(3.0, 0.22), on("knockdown"), Effect::Defiance(80.0)],
            finisher: Some(Finisher::Blast),
            action: Action::Slam,
            ..b()
        },
        // Dagger (either hand).
        AbilityDef { key: "dagger_stab", name: "Quick Stab", summary: "Fast, light stabs.", range: 2.6, gcd: false, cooldown: 0.9, auto: true, generates: 4.0, effects: vec![dmg(3.0, 0.2)], action: Action::Thrust, ..b() },
        AbilityDef {
            key: "dagger_backstab",
            name: "Backstab",
            summary: "A vicious stab: twice as hard from behind.",
            range: 2.6,
            cast: WINDUP,
            cooldown: 6.0,
            backstab: 2.0,
            effects: vec![dmg(5.0, 0.42)],
            action: Action::Thrust,
            ..b()
        },
        AbilityDef {
            key: "dagger_shadowstep",
            name: "Shadowstep",
            summary: "Step through the shadows to behind your target, crippling it.",
            range: 12.0,
            cooldown: 15.0,
            effects: vec![Effect::Shadowstep, dmg(2.0, 0.15), on("crippled")],
            action: Action::Leap,
            ..b()
        },
        AbilityDef {
            key: "dagger_fan",
            name: "Fan of Knives",
            summary: "Slash everything around you, leaving it bleeding.",
            targeting: Targeting::AroundSelf { radius: 3.5 },
            range: 0.0,
            cooldown: 10.0,
            effects: vec![dmg(3.0, 0.22), on("bleeding")],
            finisher: Some(Finisher::Whirl),
            action: Action::Spin,
            ..b()
        },
        AbilityDef {
            key: "dagger_sidestep",
            name: "Sidestep",
            summary: "Hop back out of harm's way.",
            targeting: Targeting::SelfOnly,
            range: 0.0,
            gcd: false,
            cooldown: 12.0,
            effects: vec![Effect::Dash { distance: -4.0 }, me("evasion")],
            action: Action::Leap,
            ..b()
        },
        // Spear (two hands): reach.
        AbilityDef { key: "spear_thrust", name: "Thrust", summary: "Long-reaching thrusts.", range: 4.5, gcd: false, cooldown: 1.3, auto: true, generates: 5.0, effects: vec![dmg(4.0, 0.32)], action: Action::Thrust, ..b() },
        AbilityDef {
            key: "spear_sweep",
            name: "Sweep",
            summary: "Sweep the spear round, knocking foes back.",
            targeting: Targeting::Cone { angle: 2.6 },
            range: 4.5,
            cast: WINDUP,
            cooldown: 10.0,
            effects: vec![dmg(3.0, 0.26), Effect::Knockback { distance: 3.0 }, Effect::Defiance(60.0)],
            action: Action::Slash2,
            ..b()
        },
        AbilityDef {
            key: "spear_impale",
            name: "Impale",
            summary: "Drive the spear through everything in a line.",
            targeting: Targeting::Line { width: 1.4 },
            range: 6.0,
            cast: WINDUP,
            cooldown: 8.0,
            effects: vec![dmg(6.0, 0.45), on("bleeding"), on("bleeding")],
            action: Action::Thrust,
            ..b()
        },
        AbilityDef {
            key: "spear_vault",
            name: "Vault",
            summary: "Vault back on the spear's haft.",
            targeting: Targeting::SelfOnly,
            range: 0.0,
            gcd: false,
            cooldown: 12.0,
            effects: vec![Effect::Dash { distance: -6.0 }, me("evasion")],
            action: Action::Leap,
            ..b()
        },
        AbilityDef {
            key: "spear_skewer",
            name: "Skewer",
            summary: "Hook a foe and drag it in.",
            range: 7.0,
            cooldown: 15.0,
            effects: vec![dmg(3.0, 0.3), Effect::Pull { distance: 4.0 }, on("crippled")],
            action: Action::Thrust,
            ..b()
        },
        // Greatsword (two hands).
        AbilityDef {
            key: "gs_swing",
            name: "Greatsword Swing",
            summary: "Wide swings that hit up to three foes.",
            targeting: Targeting::Cone { angle: 2.1 },
            range: 3.6,
            gcd: false,
            cooldown: 1.8,
            auto: true,
            generates: 6.0,
            max_targets: 3,
            effects: vec![dmg(6.0, 0.45)],
            ..b()
        },
        AbilityDef {
            key: "gs_whirlwind",
            name: "Whirlwind",
            summary: "Spin through your enemies. You can move while spinning.",
            targeting: Targeting::AroundSelf { radius: 3.5 },
            range: 0.0,
            channel: 1.5,
            ticks: 3,
            cooldown: 10.0,
            channel_effects: vec![dmg(2.5, 0.22)],
            finisher: Some(Finisher::Whirl),
            action: Action::Spin,
            ..b()
        },
        AbilityDef {
            key: "gs_earthsplitter",
            name: "Earthsplitter",
            summary: "Split the ground in a line, knocking enemies down.",
            targeting: Targeting::Line { width: 2.5 },
            range: 9.0,
            cast: WINDUP,
            cooldown: 15.0,
            effects: vec![dmg(5.0, 0.5), on("knockdown"), Effect::Defiance(100.0)],
            finisher: Some(Finisher::Blast),
            action: Action::Overhead,
            ..b()
        },
        AbilityDef {
            key: "gs_rush",
            name: "Rush",
            summary: "Charge a distant enemy.",
            range: 15.0,
            cooldown: 15.0,
            effects: vec![Effect::Charge, dmg(3.0, 0.28)],
            finisher: Some(Finisher::Leap),
            action: Action::Charge,
            ..b()
        },
        AbilityDef {
            key: "gs_execution",
            name: "Execution",
            summary: "A heavy blow, far harder below a quarter health.",
            range: 3.6,
            cast: WINDUP,
            cooldown: 8.0,
            execute: Some((0.25, 2.5)),
            effects: vec![dmg(8.0, 0.7)],
            action: Action::Overhead,
            ..b()
        },
        // Shield (off hand).
        AbilityDef {
            key: "shield_bash",
            name: "Shield Bash",
            summary: "Interrupt and daze. Breaks defiance.",
            cooldown: 15.0,
            effects: vec![dmg(2.0, 0.18), Effect::Interrupt { lockout: 3.0 }, on("dazed"), Effect::Defiance(150.0)],
            finisher: Some(Finisher::Blast),
            action: Action::Bash,
            ..b()
        },
        AbilityDef {
            key: "shield_wall",
            name: "Shield Wall",
            summary: "Block everything from the front and turn missiles back.",
            targeting: Targeting::SelfOnly,
            range: 0.0,
            gcd: false,
            cooldown: 20.0,
            effects: vec![me("shield_wall")],
            action: Action::Block,
            ..b()
        },
        // Hunting bow (two hands): quick shots.
        AbilityDef {
            key: "bow_quick",
            name: "Quick Shot",
            summary: "Loose arrows quickly. They fly straight: lead a moving target.",
            range: 18.0,
            gcd: false,
            cooldown: 1.0,
            auto: true,
            missile: aimed(40.0, "arrow"),
            effects: vec![dmg(3.0, 0.25)],
            finisher: Some(Finisher::Projectile),
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "bow_cripple",
            name: "Crippling Shot",
            summary: "An arrow that cripples.",
            range: 18.0,
            cooldown: 8.0,
            missile: aimed(40.0, "arrow"),
            effects: vec![dmg(3.0, 0.3), on("crippled")],
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "bow_poison",
            name: "Poison Volley",
            summary: "A fan of poisoned arrows.",
            targeting: Targeting::Cone { angle: 0.8 },
            range: 14.0,
            cooldown: 10.0,
            max_targets: 3,
            school: School::Nature,
            effects: vec![dmg(2.0, 0.18), on("poisoned")],
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "bow_disengage",
            name: "Disengage",
            summary: "Leap back and run.",
            targeting: Targeting::SelfOnly,
            range: 0.0,
            gcd: false,
            cooldown: 15.0,
            effects: vec![Effect::Dash { distance: -7.0 }, me("swiftness")],
            action: Action::Leap,
            ..b()
        },
        AbilityDef {
            key: "bow_rain",
            name: "Rain of Arrows",
            summary: "Arrows fall on the spot you choose a moment later.",
            targeting: Targeting::Ground { radius: 4.0 },
            range: 18.0,
            cooldown: 20.0,
            impact_delay: 0.6,
            effects: vec![dmg(5.0, 0.5), on("crippled")],
            action: Action::Shoot,
            ..b()
        },
        // Longbow (two hands): reach and power.
        AbilityDef {
            key: "longbow_shot",
            name: "Long Shot",
            summary: "Long, heavy arrows. They fly straight.",
            range: 25.0,
            gcd: false,
            cooldown: 1.7,
            auto: true,
            missile: aimed(55.0, "arrow"),
            effects: vec![dmg(5.0, 0.42)],
            finisher: Some(Finisher::Projectile),
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "longbow_pierce",
            name: "Piercing Arrow",
            summary: "An arrow that passes through everything in its path.",
            range: 25.0,
            cooldown: 8.0,
            missile: Some(Missile { speed: 60.0, visual: "arrow", homing: false, reflectable: true, pierce: true }),
            effects: vec![dmg(5.0, 0.42), on("vulnerable"), on("vulnerable")],
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "longbow_barrage",
            name: "Barrage",
            summary: "Rain arrows on an area while you stand and shoot.",
            targeting: Targeting::Ground { radius: 5.0 },
            range: 22.0,
            channel: 2.5,
            ticks: 5,
            cooldown: 20.0,
            movable: false,
            channel_effects: vec![dmg(2.0, 0.16), on("crippled")],
            action: Action::Channel,
            ..b()
        },
        AbilityDef {
            key: "longbow_pointblank",
            name: "Point Blank Shot",
            summary: "Knock a close foe away.",
            range: 6.0,
            cooldown: 15.0,
            missile: homing(50.0, "arrow"),
            effects: vec![dmg(3.0, 0.25), Effect::Knockback { distance: 5.0 }],
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "longbow_mark",
            name: "Hunter's Mark",
            summary: "Mark your quarry: it takes 20% more damage.",
            range: 25.0,
            cooldown: 25.0,
            effects: vec![on("vulnerable"), on("vulnerable"), on("vulnerable"), on("vulnerable"), on("vulnerable")],
            action: Action::Shoot,
            ..b()
        },
        // Oak staff (two hands): the mage's fire and frost.
        AbilityDef {
            key: "staff_firebolt",
            name: "Fire Bolt",
            summary: "Hurl fire. It flies straight; you can cast on the move.",
            school: School::Fire,
            range: 20.0,
            cast: 1.0,
            gcd: false,
            cooldown: 0.1,
            auto: true,
            missile: aimed(26.0, "fire_bolt"),
            effects: vec![dmg(5.0, 0.42)],
            finisher: Some(Finisher::Projectile),
            action: Action::CastBolt,
            ..b()
        },
        AbilityDef {
            key: "staff_frostlance",
            name: "Frost Lance",
            summary: "A homing lance of ice that chills. Stand still to cast.",
            school: School::Frost,
            range: 20.0,
            cast: 1.4,
            movable: false,
            cooldown: 6.0,
            missile: homing(30.0, "frost_lance"),
            effects: vec![dmg(5.0, 0.45), on("chilled")],
            action: Action::CastBolt,
            ..b()
        },
        AbilityDef {
            key: "staff_meteor",
            name: "Meteor",
            summary: "Call down a meteor: it lands a moment later, burning and knocking down.",
            school: School::Fire,
            targeting: Targeting::Ground { radius: 3.5 },
            range: 20.0,
            cast: 1.8,
            movable: false,
            cooldown: 15.0,
            impact_delay: 0.9,
            effects: vec![dmg(8.0, 0.85), on("burning"), on("knockdown"), Effect::Defiance(100.0)],
            finisher: Some(Finisher::Blast),
            action: Action::CastArea,
            ..b()
        },
        AbilityDef {
            key: "staff_nova",
            name: "Frost Nova",
            summary: "Freeze everything around you in place.",
            school: School::Frost,
            targeting: Targeting::AroundSelf { radius: 6.0 },
            range: 0.0,
            cooldown: 18.0,
            effects: vec![dmg(2.0, 0.15), on("frozen")],
            action: Action::CastArea,
            ..b()
        },
        AbilityDef {
            key: "staff_ring",
            name: "Ring of Fire",
            summary: "A ring of flame around you: a fire field that burns whoever stands in it.",
            school: School::Fire,
            targeting: Targeting::SelfOnly,
            range: 0.0,
            cooldown: 20.0,
            effects: vec![Effect::Field { field: Some(Field::Fire), radius: 4.0, duration: 6.0, interval: 1.0, enemies: vec![on("burning")], allies: vec![] }],
            action: Action::CastArea,
            ..b()
        },
        // Gravecaller's staff (two hands): the necromancer's shadow.
        AbilityDef {
            key: "necro_bolt",
            name: "Shadow Bolt",
            summary: "A bolt of shadow that feeds your essence. It flies straight.",
            school: School::Shadow,
            range: 20.0,
            cast: 1.0,
            gcd: false,
            cooldown: 0.1,
            auto: true,
            generates: 4.0,
            missile: aimed(24.0, "shadow_bolt"),
            effects: vec![dmg(5.0, 0.4)],
            finisher: Some(Finisher::Projectile),
            action: Action::CastBolt,
            ..b()
        },
        AbilityDef {
            key: "necro_drain",
            name: "Life Drain",
            summary: "Drain life from your target while you hold still.",
            school: School::Shadow,
            range: 15.0,
            channel: 2.0,
            ticks: 4,
            cooldown: 10.0,
            movable: false,
            generates: 8.0,
            channel_effects: vec![dmg(2.0, 0.18), Effect::SelfHeal { base: 2.0, coeff: 0.18 }],
            action: Action::Channel,
            ..b()
        },
        AbilityDef {
            key: "necro_plague",
            name: "Plague",
            summary: "A poison field: it poisons whoever stands in it.",
            school: School::Nature,
            targeting: Targeting::Ground { radius: 4.0 },
            range: 18.0,
            cast: 1.0,
            cooldown: 18.0,
            effects: vec![Effect::Field { field: Some(Field::Poison), radius: 4.0, duration: 6.0, interval: 1.0, enemies: vec![on("poisoned")], allies: vec![] }],
            action: Action::CastArea,
            ..b()
        },
        AbilityDef {
            key: "necro_terror",
            name: "Terror",
            summary: "Send nearby enemies fleeing in fear.",
            school: School::Shadow,
            targeting: Targeting::AroundSelf { radius: 5.0 },
            range: 0.0,
            cooldown: 25.0,
            effects: vec![on("feared")],
            action: Action::Roar,
            ..b()
        },
        AbilityDef {
            key: "necro_raise",
            name: "Raise Skeleton",
            summary: "Raise a skeleton to fight for you for half a minute.",
            school: School::Shadow,
            targeting: Targeting::SelfOnly,
            range: 0.0,
            cast: 1.5,
            cooldown: 30.0,
            effects: vec![Effect::Summon { mob: "skeleton_minion", count: 1, lasts: 30.0 }],
            action: Action::CastSelf,
            ..b()
        },
        // Sunward staff (two hands): the healer's light.
        AbilityDef {
            key: "holy_bolt",
            name: "Holy Bolt",
            summary: "A homing bolt of light.",
            school: School::Holy,
            range: 18.0,
            cast: 0.8,
            gcd: false,
            cooldown: 0.1,
            auto: true,
            missile: homing(28.0, "holy_bolt"),
            effects: vec![dmg(4.0, 0.35)],
            action: Action::CastBolt,
            ..b()
        },
        AbilityDef {
            key: "healing_light",
            name: "Healing Light",
            summary: "Heal a friend, or yourself. Stand still to cast.",
            school: School::Holy,
            targeting: Targeting::Ally,
            range: 25.0,
            cast: 1.5,
            movable: false,
            effects: vec![heal(12.0, 0.85)],
            action: Action::CastSelf,
            ..b()
        },
        AbilityDef {
            key: "renew",
            name: "Renew",
            summary: "Heal a friend over time.",
            school: School::Holy,
            targeting: Targeting::Ally,
            range: 25.0,
            cooldown: 6.0,
            effects: vec![on("regeneration")],
            action: Action::CastSelf,
            ..b()
        },
        AbilityDef {
            key: "sanctuary",
            name: "Sanctuary",
            summary: "A light field that heals friends standing in it.",
            school: School::Holy,
            targeting: Targeting::Ground { radius: 5.0 },
            range: 20.0,
            cooldown: 25.0,
            allies: true,
            effects: vec![Effect::Field { field: Some(Field::Light), radius: 5.0, duration: 8.0, interval: 1.0, enemies: vec![], allies: vec![heal(3.0, 0.15)] }],
            action: Action::CastArea,
            ..b()
        },
        AbilityDef {
            key: "smite",
            name: "Smite",
            summary: "Light strikes the ground a moment later, dazing.",
            school: School::Holy,
            targeting: Targeting::Ground { radius: 3.0 },
            range: 20.0,
            cast: 1.0,
            cooldown: 10.0,
            impact_delay: 0.5,
            effects: vec![dmg(5.0, 0.5), on("dazed")],
            finisher: Some(Finisher::Blast),
            action: Action::CastArea,
            ..b()
        },
        // Wand (main hand).
        AbilityDef {
            key: "wand_missile",
            name: "Arcane Missile",
            summary: "Homing bolts of force.",
            school: School::Arcane,
            range: 18.0,
            gcd: false,
            cooldown: 1.1,
            auto: true,
            missile: homing(32.0, "arcane_missile"),
            effects: vec![dmg(3.0, 0.26)],
            action: Action::CastBolt,
            ..b()
        },
        AbilityDef {
            key: "wand_spark",
            name: "Arcane Spark",
            summary: "A straight-flying spark that leaves its target vulnerable.",
            school: School::Arcane,
            range: 18.0,
            cooldown: 6.0,
            missile: aimed(34.0, "arcane_spark"),
            effects: vec![dmg(4.0, 0.36), on("vulnerable"), on("vulnerable")],
            action: Action::CastBolt,
            ..b()
        },
        AbilityDef {
            key: "wand_hex",
            name: "Slowing Hex",
            summary: "Chill and weaken your target.",
            school: School::Arcane,
            range: 18.0,
            cooldown: 12.0,
            effects: vec![on("chilled"), on("weakness")],
            action: Action::CastBolt,
            ..b()
        },
        // Spellbook (off hand).
        AbilityDef {
            key: "book_ward",
            name: "Arcane Ward",
            summary: "Ward yourself against harm.",
            school: School::Arcane,
            targeting: Targeting::SelfOnly,
            range: 0.0,
            gcd: false,
            cooldown: 20.0,
            effects: vec![me("arcane_ward")],
            action: Action::CastSelf,
            ..b()
        },
        AbilityDef {
            key: "book_silence",
            name: "Silencing Word",
            summary: "Interrupt and silence a caster.",
            school: School::Arcane,
            range: 18.0,
            cooldown: 20.0,
            effects: vec![Effect::Interrupt { lockout: 3.0 }, on("silenced")],
            action: Action::CastBolt,
            ..b()
        },
        // Focus orb (off hand).
        AbilityDef {
            key: "focus_barrier",
            name: "Barrier",
            summary: "Shield a friend, or yourself.",
            school: School::Arcane,
            targeting: Targeting::Ally,
            range: 20.0,
            cooldown: 18.0,
            effects: vec![on("barrier")],
            action: Action::CastSelf,
            ..b()
        },
        AbilityDef {
            key: "focus_pulse",
            name: "Arcane Pulse",
            summary: "A burst of force on the ground a moment later, dazing.",
            school: School::Arcane,
            targeting: Targeting::Ground { radius: 4.0 },
            range: 18.0,
            cooldown: 25.0,
            impact_delay: 0.6,
            effects: vec![dmg(3.0, 0.25), on("dazed")],
            finisher: Some(Finisher::Blast),
            action: Action::CastArea,
            ..b()
        },
    ]);
    let w = |item, grip, skills: &[&'static str], off_skills: &[&'static str], guard, swing| WeaponDef {
        item,
        grip,
        skills: skills.to_vec(),
        off_skills: off_skills.to_vec(),
        guard,
        swing,
    };
    vec![
        w("sword.arming", Grip::Main, &["sword_chain", "sword_rend", "sword_lunge"], &[], Guard::Weapon, 1.6),
        w("axe.bearded", Grip::Main, &["axe_chop", "axe_cleave", "axe_throw"], &[], Guard::Weapon, 1.6),
        w("mace.flanged", Grip::Main, &["mace_smash", "mace_crush", "mace_tremor"], &[], Guard::Weapon, 1.5),
        w("dagger.hunting", Grip::Either, &["dagger_stab", "dagger_backstab", "dagger_shadowstep"], &["dagger_fan", "dagger_sidestep"], Guard::Weapon, 1.4),
        w("spear.ash", Grip::Two, &["spear_thrust", "spear_sweep", "spear_impale", "spear_vault", "spear_skewer"], &[], Guard::Weapon, 0.9),
        w("sword.great", Grip::Two, &["gs_swing", "gs_whirlwind", "gs_earthsplitter", "gs_rush", "gs_execution"], &[], Guard::Weapon, 1.8),
        w("shield.round", Grip::Off, &["shield_bash", "shield_wall"], &[], Guard::Shield, 1.6),
        w("shield.kite", Grip::Off, &["shield_bash", "shield_wall"], &[], Guard::Shield, 1.6),
        w("bow.hunting", Grip::Two, &["bow_quick", "bow_cripple", "bow_poison", "bow_disengage", "bow_rain"], &[], Guard::None, UNARMED_SWING),
        w("bow.long", Grip::Two, &["longbow_shot", "longbow_pierce", "longbow_barrage", "longbow_pointblank", "longbow_mark"], &[], Guard::None, UNARMED_SWING),
        w("staff.oak", Grip::Two, &["staff_firebolt", "staff_frostlance", "staff_meteor", "staff_nova", "staff_ring"], &[], Guard::None, UNARMED_SWING),
        w("staff.necro", Grip::Two, &["necro_bolt", "necro_drain", "necro_plague", "necro_terror", "necro_raise"], &[], Guard::None, UNARMED_SWING),
        w("staff.healer", Grip::Two, &["holy_bolt", "healing_light", "renew", "sanctuary", "smite"], &[], Guard::None, UNARMED_SWING),
        w("wand.yew", Grip::Main, &["wand_missile", "wand_spark", "wand_hex"], &[], Guard::None, UNARMED_SWING),
        w("book.spells", Grip::Off, &["book_ward", "book_silence"], &[], Guard::None, UNARMED_SWING),
        w("orb.focus", Grip::Off, &["focus_barrier", "focus_pulse"], &[], Guard::None, UNARMED_SWING),
    ]
}
