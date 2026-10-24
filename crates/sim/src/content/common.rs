//! Conditions, boons and crowd control everyone shares; the downed skills; the combo table.

use protocol::{Action, School};

use super::fx::{dmg, heal, on};
use super::{AbilityDef, AuraDef, Cc, ComboRule, Effect, Field, Finisher, Targeting};

pub const DOWNED: [&str; 3] = ["downed_strike", "downed_shove", "downed_bandage"];

pub fn auras(out: &mut Vec<AuraDef>) {
    let b = AuraDef::base;
    out.extend([
        // Crowd control.
        AuraDef { key: "stunned", name: "Stunned", duration: 2.0, debuff: true, cc: Some(Cc::Stun), ..b() },
        AuraDef { key: "knockdown", name: "Knocked Down", duration: 1.0, debuff: true, dispellable: false, cc: Some(Cc::Knockdown), ..b() },
        AuraDef { key: "dazed", name: "Dazed", duration: 1.5, debuff: true, cc: Some(Cc::Daze), ..b() },
        AuraDef { key: "silenced", name: "Silenced", duration: 3.0, debuff: true, cc: Some(Cc::Silence), ..b() },
        AuraDef { key: "rooted", name: "Rooted", duration: 3.0, debuff: true, cc: Some(Cc::Root), ..b() },
        AuraDef { key: "frozen", name: "Frozen", duration: 2.5, debuff: true, cc: Some(Cc::Root), school: School::Frost, ..b() },
        AuraDef { key: "feared", name: "Feared", duration: 2.5, debuff: true, cc: Some(Cc::Fear), school: School::Shadow, ..b() },
        AuraDef { key: "asleep", name: "Asleep", duration: 8.0, debuff: true, cc: Some(Cc::Sleep), breaks_on_damage: true, ..b() },
        AuraDef { key: "crippled", name: "Crippled", duration: 4.0, debuff: true, move_speed: -0.4, cc: Some(Cc::Slow), ..b() },
        AuraDef {
            key: "chilled",
            name: "Chilled",
            duration: 4.0,
            debuff: true,
            move_speed: -0.3,
            haste: -0.2,
            cc: Some(Cc::Slow),
            school: School::Frost,
            ..b()
        },
        // Thrown off balance by a parry.
        AuraDef { key: "staggered", name: "Staggered", duration: 1.2, debuff: true, dispellable: false, cc: Some(Cc::Daze), damage_taken: 0.15, ..b() },
        // Damage over time.
        AuraDef { key: "bleeding", name: "Bleeding", duration: 6.0, debuff: true, max_stacks: 5, tick: Some((1.0, dmg(1.5, 0.06))), ..b() },
        AuraDef {
            key: "burning",
            name: "Burning",
            duration: 4.0,
            debuff: true,
            max_stacks: 5,
            tick: Some((1.0, dmg(1.0, 0.09))),
            school: School::Fire,
            ..b()
        },
        AuraDef {
            key: "poisoned",
            name: "Poisoned",
            duration: 8.0,
            debuff: true,
            max_stacks: 3,
            tick: Some((1.0, dmg(0.5, 0.1))),
            healing_taken: -0.33,
            school: School::Nature,
            ..b()
        },
        // Boons and other states.
        AuraDef { key: "might", name: "Might", duration: 10.0, max_stacks: 10, damage_done: 0.04, ..b() },
        AuraDef { key: "fury", name: "Fury", duration: 6.0, crit: 0.2, ..b() },
        AuraDef { key: "quickness", name: "Quickness", duration: 5.0, haste: 0.3, ..b() },
        AuraDef { key: "swiftness", name: "Swiftness", duration: 6.0, move_speed: 0.33, ..b() },
        AuraDef { key: "protection", name: "Protection", duration: 4.0, damage_taken: -0.25, ..b() },
        AuraDef { key: "regeneration", name: "Regeneration", duration: 6.0, tick: Some((1.0, heal(2.0, 0.08))), ..b() },
        AuraDef { key: "weakness", name: "Weakness", duration: 6.0, debuff: true, damage_done: -0.15, ..b() },
        AuraDef { key: "vulnerable", name: "Vulnerable", duration: 8.0, debuff: true, max_stacks: 5, damage_taken: 0.04, ..b() },
        AuraDef { key: "exposed", name: "Exposed", duration: 6.0, debuff: true, dispellable: false, damage_taken: 0.5, ..b() },
        AuraDef { key: "broken", name: "Defiance Broken", duration: 3.0, debuff: true, dispellable: false, cc: Some(Cc::Stun), ..b() },
        // A monster walking home: it can't be hurt or held.
        AuraDef { key: "evading", name: "Evading", duration: 8.0, dispellable: false, cc_immune: true, invulnerable: true, ..b() },
        AuraDef { key: "taunted", name: "Taunted", duration: 3.0, debuff: true, dispellable: false, ..b() },
        AuraDef { key: "revive_sickness", name: "Weakened", duration: 30.0, debuff: true, dispellable: false, damage_done: -0.1, ..b() },
        // The riposte after a parry: the next blow lands hard.
        AuraDef { key: "riposte", name: "Riposte", duration: 2.0, damage_done: 0.5, dispellable: false, ..b() },
    ]);
}

pub fn abilities(out: &mut Vec<AbilityDef>) {
    let b = AbilityDef::base;
    out.extend([
        AbilityDef {
            key: "downed_strike",
            name: "Desperate Strike",
            summary: "Lash out at an enemy within reach.",
            range: 4.0,
            gcd: false,
            cooldown: 1.6,
            auto: true,
            effects: vec![dmg(3.0, 0.25)],
            downed: true,
            ..b()
        },
        AbilityDef {
            key: "downed_shove",
            name: "Shove",
            summary: "Knock nearby enemies back and interrupt them.",
            targeting: Targeting::AroundSelf { radius: 4.0 },
            range: 0.0,
            gcd: false,
            cooldown: 10.0,
            effects: vec![Effect::Knockback { distance: 5.0 }, Effect::Interrupt { lockout: 1.5 }],
            action: Action::Kick,
            downed: true,
            ..b()
        },
        AbilityDef {
            key: "downed_bandage",
            name: "Bandage",
            summary: "Bandage your wounds. Hold still to keep it going.",
            targeting: Targeting::SelfOnly,
            range: 0.0,
            channel: 4.0,
            ticks: 4,
            gcd: false,
            cooldown: 12.0,
            movable: false,
            channel_effects: vec![heal(6.0, 0.3)],
            action: Action::Channel,
            downed: true,
            ..b()
        },
        // Bare hands, for anyone holding nothing to fight with.
        AbilityDef { key: "punch", name: "Punch", summary: "Fists.", range: 2.4, gcd: false, cooldown: 1.2, auto: true, effects: vec![dmg(2.0, 0.15)], action: Action::Bash, ..b() },
    ]);
}

pub fn combos() -> Vec<ComboRule> {
    use Field::*;
    use Finisher::*;
    let rule = |field, finisher, name, effects: Vec<Effect>, allies| ComboRule { field, finisher, name, effects, allies };
    vec![
        rule(Fire, Blast, "Area Might", vec![on("might"), on("might")], true),
        rule(Fire, Projectile, "Burning Bolt", vec![on("burning")], false),
        rule(Fire, Whirl, "Burning Whirl", vec![on("burning")], false),
        rule(Fire, Leap, "Fiery Leap", vec![on("might")], true),
        rule(Ice, Projectile, "Chilling Bolt", vec![on("chilled")], false),
        rule(Ice, Blast, "Frost Blast", vec![on("chilled")], false),
        rule(Light, Blast, "Area Cleanse", vec![Effect::Cleanse { count: 1 }], true),
        rule(Light, Whirl, "Purifying Whirl", vec![Effect::Cleanse { count: 1 }], true),
        rule(Water, Blast, "Healing Burst", vec![heal(10.0, 0.35)], true),
        rule(Water, Leap, "Healing Leap", vec![heal(6.0, 0.2)], true),
        rule(Poison, Projectile, "Poison Bolt", vec![on("poisoned")], false),
        rule(Poison, Blast, "Weakening Cloud", vec![on("weakness")], false),
        rule(Lightning, Projectile, "Static Bolt", vec![on("vulnerable")], false),
    ]
}
