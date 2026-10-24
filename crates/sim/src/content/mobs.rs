//! Monsters: how tough and strong they are for their level, their basic attack, and the skills
//! they reach for and when. Their big blows wind up with a warning on the ground: dodge them.

use protocol::{Action, Look, School};

use super::fx::{dmg, on};
use super::{AbilityDef, AuraDef, Effect, Missile, Targeting};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    Normal,
    /// Tougher, with a defiance bar.
    Elite,
    Boss,
}

/// When a monster uses a skill: below a health fraction, with a chance each time it thinks.
#[derive(Clone, Copy, Debug)]
pub struct MobSkill {
    pub ability: &'static str,
    pub below: f32,
    pub chance: f32,
}

#[derive(Clone, Debug)]
pub struct MobDef {
    pub key: &'static str,
    pub name: &'static str,
    /// The simulation's body and animation set ("wolf", "goblin", "humanoid").
    pub body: &'static str,
    pub look: fn() -> Look,
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

fn skill(ability: &'static str, below: f32, chance: f32) -> MobSkill {
    MobSkill { ability, below, chance }
}

fn wolf_look() -> Look {
    Look { art: "wolf.grey".into(), ..Default::default() }
}

fn dire_wolf_look() -> Look {
    Look { art: "wolf.dire".into(), ..Default::default() }
}

fn goblin_look() -> Look {
    Look {
        art: "goblin_male".into(),
        tone: Some("goblin green".into()),
        pieces: ["ranger.top", "peasant.trousers", "peasant.shoes"].map(String::from).to_vec(),
        gear: vec![("main_hand".into(), "dagger.hunting".into()), ("off_hand".into(), "shield.round".into())],
        // Scavenged clothes, dyed dull: not mistaken for a ranger at a distance.
        dyes: vec![
            ("ranger.top".into(), Some("rust".into()), Some("walnut".into())),
            ("peasant.trousers".into(), Some("slate".into()), Some("charcoal".into())),
        ],
        ..Default::default()
    }
}

fn goblin_archer_look() -> Look {
    let mut l = goblin_look();
    l.gear = vec![("main_hand".into(), "bow.hunting".into()), ("ammo".into(), "quiver.leather".into())];
    l
}

fn skeleton_look() -> Look {
    Look {
        art: "skeleton".into(),
        gear: vec![("main_hand".into(), "sword.arming".into()), ("off_hand".into(), "shield.round".into())],
        ..Default::default()
    }
}

fn golem_look() -> Look {
    Look { art: "golem".into(), ..Default::default() }
}

fn barrow_king_look() -> Look {
    Look { art: "barrow_king".into(), gear: vec![("main_hand".into(), "sword.great".into())], ..Default::default() }
}

pub fn mobs(abilities: &mut Vec<AbilityDef>, _auras: &mut Vec<AuraDef>) -> Vec<MobDef> {
    let b = AbilityDef::base;
    abilities.extend([
        AbilityDef { key: "wolf_bite", name: "Bite", range: 1.9, gcd: false, cooldown: 1.8, auto: true, effects: vec![dmg(3.0, 0.4)], action: Action::Bite, ..b() },
        AbilityDef {
            key: "wolf_lunge",
            name: "Lunge",
            summary: "The wolf crouches, then lunges along a line.",
            targeting: Targeting::Line { width: 1.6 },
            range: 7.0,
            cast: 0.6,
            cooldown: 8.0,
            telegraph: true,
            effects: vec![Effect::Dash { distance: 5.0 }, dmg(5.0, 0.8)],
            action: Action::Leap,
            ..b()
        },
        AbilityDef {
            key: "wolf_howl",
            name: "Howl",
            summary: "The pack howls, and fights harder.",
            targeting: Targeting::AroundSelf { radius: 12.0 },
            range: 0.0,
            cooldown: 30.0,
            allies: true,
            effects: vec![on("might"), on("might")],
            action: Action::Roar,
            ..b()
        },
        AbilityDef {
            key: "dire_maul",
            name: "Maul",
            summary: "A savage maul in front of the dire wolf. Too heavy to block: dodge it.",
            targeting: Targeting::Cone { angle: 2.0 },
            range: 3.5,
            cast: 0.8,
            cooldown: 10.0,
            telegraph: true,
            unblockable: true,
            effects: vec![dmg(8.0, 1.2), on("knockdown")],
            action: Action::Claw,
            ..b()
        },
        AbilityDef { key: "goblin_stab", name: "Stab", range: 2.0, gcd: false, cooldown: 2.0, auto: true, effects: vec![dmg(3.0, 0.45)], action: Action::Thrust, ..b() },
        AbilityDef {
            key: "goblin_chop",
            name: "Wild Chop",
            summary: "The goblin winds up a wild chop in front of it.",
            targeting: Targeting::Cone { angle: 1.4 },
            range: 3.0,
            cast: 0.7,
            cooldown: 9.0,
            telegraph: true,
            effects: vec![dmg(6.0, 0.9)],
            action: Action::Overhead,
            ..b()
        },
        AbilityDef {
            key: "goblin_trick",
            name: "Dirty Trick",
            summary: "A handful of grit in the eyes.",
            range: 2.5,
            cooldown: 15.0,
            effects: vec![on("dazed")],
            action: Action::Kick,
            ..b()
        },
        AbilityDef {
            key: "goblin_shot",
            name: "Shoot",
            range: 16.0,
            gcd: false,
            cooldown: 2.2,
            auto: true,
            missile: Some(Missile { speed: 34.0, visual: "arrow", homing: false, reflectable: true, pierce: false }),
            effects: vec![dmg(3.0, 0.45)],
            action: Action::Shoot,
            ..b()
        },
        AbilityDef {
            key: "goblin_volley",
            name: "Volley",
            summary: "Arrows fall where you stand a moment later.",
            targeting: Targeting::Ground { radius: 3.0 },
            range: 16.0,
            cooldown: 12.0,
            impact_delay: 1.0,
            effects: vec![dmg(4.0, 0.7)],
            action: Action::Shoot,
            ..b()
        },
        AbilityDef { key: "skeleton_slash", name: "Slash", range: 2.6, gcd: false, cooldown: 1.8, auto: true, effects: vec![dmg(4.0, 0.45)], ..b() },
        AbilityDef { key: "golem_pound", name: "Pound", range: 2.8, gcd: false, cooldown: 2.4, auto: true, effects: vec![dmg(6.0, 0.6)], action: Action::Slam, ..b() },
        AbilityDef {
            key: "golem_quake",
            name: "Quake",
            summary: "The golem raises its fists to shake the ground around it. Get clear or roll.",
            targeting: Targeting::AroundSelf { radius: 5.0 },
            range: 0.0,
            cast: 1.1,
            cooldown: 12.0,
            telegraph: true,
            unblockable: true,
            effects: vec![dmg(6.0, 0.8), on("knockdown")],
            action: Action::Slam,
            ..b()
        },
        AbilityDef {
            key: "golem_hurl",
            name: "Hurl Boulder",
            summary: "A boulder lands where you stood.",
            targeting: Targeting::Ground { radius: 3.0 },
            range: 18.0,
            cooldown: 9.0,
            impact_delay: 1.0,
            effects: vec![dmg(5.0, 0.7)],
            action: Action::Slam,
            ..b()
        },
        AbilityDef {
            key: "king_cleave",
            name: "Grave Cleave",
            targeting: Targeting::Cone { angle: 2.0 },
            range: 4.0,
            gcd: false,
            cooldown: 2.2,
            auto: true,
            max_targets: 3,
            effects: vec![dmg(7.0, 0.6)],
            ..b()
        },
        AbilityDef {
            key: "king_slam",
            name: "Barrow Slam",
            summary: "The king lifts his blade to smash the ground around him. Get clear or roll.",
            targeting: Targeting::AroundSelf { radius: 6.0 },
            range: 0.0,
            cast: 1.3,
            cooldown: 12.0,
            telegraph: true,
            unblockable: true,
            effects: vec![dmg(8.0, 1.0), on("knockdown")],
            action: Action::Overhead,
            ..b()
        },
        AbilityDef {
            key: "king_frost",
            name: "Grave Frost",
            summary: "Frost from the grave lands where you stand.",
            school: School::Frost,
            targeting: Targeting::Ground { radius: 4.0 },
            range: 20.0,
            cast: 0.8,
            cooldown: 14.0,
            impact_delay: 1.2,
            effects: vec![dmg(6.0, 0.7), on("chilled")],
            action: Action::CastArea,
            ..b()
        },
        AbilityDef {
            key: "king_summon",
            name: "Call the Dead",
            summary: "The king calls his dead to fight beside him.",
            school: School::Shadow,
            targeting: Targeting::SelfOnly,
            range: 0.0,
            cast: 1.5,
            cooldown: 30.0,
            effects: vec![Effect::Summon { mob: "skeleton_minion", count: 2, lasts: 40.0 }],
            action: Action::Roar,
            ..b()
        },
    ]);
    let m = |key, name, body, look, level, rank| MobDef {
        key,
        name,
        body,
        look,
        level,
        rank,
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
    };
    vec![
        MobDef {
            health: 0.45,
            power: 0.45,
            armor: 0.15,
            auto: "wolf_bite",
            skills: vec![skill("wolf_lunge", 1.01, 0.5), skill("wolf_howl", 0.7, 1.0)],
            aggro: 12.0,
            social: true,
            speed: 6.4,
            radius: 0.5,
            height: 0.9,
            wander: 7.0,
            ..m("wolf", "Grey wolf", "wolf", wolf_look, 2, Rank::Normal)
        },
        MobDef {
            health: 2.4,
            power: 0.75,
            armor: 0.3,
            auto: "wolf_bite",
            skills: vec![skill("dire_maul", 1.01, 0.6), skill("wolf_howl", 0.7, 1.0)],
            aggro: 14.0,
            social: true,
            speed: 6.8,
            radius: 0.65,
            height: 1.2,
            defiance: 250.0,
            respawn: 60.0,
            wander: 8.0,
            ..m("dire_wolf", "Dire wolf", "wolf", dire_wolf_look, 5, Rank::Elite)
        },
        MobDef {
            auto: "goblin_stab",
            skills: vec![skill("goblin_chop", 1.01, 0.5), skill("goblin_trick", 1.01, 0.3)],
            social: true,
            height: 1.3,
            ..m("goblin", "Goblin", "goblin", goblin_look, 3, Rank::Normal)
        },
        MobDef {
            health: 0.4,
            auto: "goblin_shot",
            skills: vec![skill("goblin_volley", 1.01, 0.4)],
            social: true,
            range: 11.0,
            height: 1.3,
            ..m("goblin_archer", "Goblin archer", "goblin", goblin_archer_look, 3, Rank::Normal)
        },
        MobDef {
            health: 0.6,
            power: 0.6,
            armor: 0.4,
            auto: "skeleton_slash",
            skills: vec![skill("shield_bash", 1.01, 0.3)],
            ..m("skeleton", "Skeleton warrior", "humanoid", skeleton_look, 5, Rank::Normal)
        },
        MobDef {
            health: 0.35,
            power: 0.4,
            armor: 0.3,
            auto: "skeleton_slash",
            aggro: 14.0,
            respawn: 0.0,
            wander: 0.0,
            ..m("skeleton_minion", "Risen skeleton", "humanoid", skeleton_look, 1, Rank::Normal)
        },
        MobDef {
            health: 3.0,
            power: 0.9,
            armor: 0.8,
            auto: "golem_pound",
            skills: vec![skill("golem_quake", 1.01, 0.5), skill("golem_hurl", 1.01, 0.4)],
            speed: 4.6,
            radius: 0.8,
            height: 2.4,
            defiance: 400.0,
            respawn: 90.0,
            wander: 3.0,
            ..m("golem", "Stone golem", "humanoid", golem_look, 6, Rank::Elite)
        },
        MobDef {
            health: 10.0,
            power: 1.2,
            armor: 0.5,
            auto: "king_cleave",
            skills: vec![skill("king_summon", 0.6, 1.0), skill("king_slam", 1.01, 0.5), skill("king_frost", 1.01, 0.4)],
            aggro: 16.0,
            speed: 5.0,
            radius: 0.6,
            height: 2.4,
            defiance: 600.0,
            respawn: 300.0,
            wander: 0.0,
            ..m("barrow_king", "The Barrow King", "humanoid", barrow_king_look, 8, Rank::Boss)
        },
    ]
}
