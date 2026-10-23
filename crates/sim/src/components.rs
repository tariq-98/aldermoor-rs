use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::{Pose, UnitId, UnitKind};

/// Who a unit is.
#[derive(Component, Clone, Debug)]
pub struct Unit {
    pub id: UnitId,
    pub kind: UnitKind,
    /// Which model and animation set the client uses.
    pub body: &'static str,
    pub name: String,
    pub level: u8,
    pub faction: Faction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faction {
    Player,
    Hostile,
}

impl Faction {
    pub fn is_enemy(self, other: Faction) -> bool {
        self != other
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Transform {
    pub pos: Vec3,
    /// 0 faces +z.
    pub yaw: f32,
}

impl Transform {
    pub fn forward(&self) -> Vec2 {
        Vec2::new(self.yaw.sin(), self.yaw.cos())
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Body {
    pub radius: f32,
    pub height: f32,
}

/// What a unit wants to do with its legs this step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveIntent {
    /// World-space direction, length 0…1.
    pub direction: Vec2,
    pub walk: bool,
    /// Locked facing while strafing; `None` turns toward travel.
    pub face_yaw: Option<f32>,
}

/// A move the body is carried through (a dodge roll), ignoring the controls until it ends.
#[derive(Clone, Copy, Debug)]
pub struct Forced {
    pub from: Vec3,
    pub to: Vec3,
    pub start: f64,
    pub duration: f64,
    pub arc: f32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Motion {
    /// Physics velocity: intended ground speed, and falling or jumping.
    pub vel: Vec3,
    /// How far the body actually moved last step, per second (after walls and slopes).
    pub observed: Vec3,
    pub grounded: bool,
    pub run_speed: f32,
    pub intent: MoveIntent,
    pub jump_queued: bool,
    pub forced: Option<Forced>,
}

impl Motion {
    pub fn new(run_speed: f32) -> Motion {
        Motion {
            vel: Vec3::ZERO,
            observed: Vec3::ZERO,
            grounded: true,
            run_speed,
            intent: MoveIntent::default(),
            jump_queued: false,
            forced: None,
        }
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Vitals {
    pub health: f32,
    pub max_health: f32,
    pub endurance: f32,
    pub dead: bool,
    pub died_at: f64,
    pub last_combat: f64,
    pub invulnerable_until: f64,
    /// Monsters come back this long after dying; `None` for players (they choose when).
    pub respawn_at: Option<f64>,
}

impl Vitals {
    pub fn new(max_health: f32) -> Vitals {
        Vitals {
            health: max_health,
            max_health,
            endurance: crate::vitals::ENDURANCE_MAX,
            dead: false,
            died_at: 0.0,
            last_combat: -100.0,
            invulnerable_until: 0.0,
            respawn_at: None,
        }
    }
}

/// A basic attack: how far it reaches, how often it swings and how hard it hits.
#[derive(Clone, Copy, Debug)]
pub struct Attack {
    pub range: f32,
    pub interval: f64,
    pub damage: f32,
}

#[derive(Component, Clone, Debug)]
pub struct Fighter {
    pub target: Option<UnitId>,
    pub auto_attack: bool,
    pub attack: Attack,
    pub next_swing: f64,
    /// When each bar slot is ready again (simulation time).
    pub cooldowns: [f64; 2],
}

impl Fighter {
    pub fn new(attack: Attack) -> Fighter {
        Fighter { target: None, auto_attack: false, attack, next_swing: 0.0, cooldowns: [0.0; 2] }
    }
}

/// The current animation pose and when it began; action poses hold until `hold_until`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Anim {
    pub pose: Pose,
    pub since: f64,
    pub hold_until: f64,
}

impl Default for Anim {
    fn default() -> Self {
        Anim { pose: Pose::Idle, since: 0.0, hold_until: 0.0 }
    }
}

/// Where a unit started, for respawning.
#[derive(Component, Clone, Copy, Debug)]
pub struct Home {
    pub pos: Vec3,
    pub yaw: f32,
}

/// Marks units steered by a player's commands.
#[derive(Component, Clone, Copy, Debug)]
pub struct PlayerControl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Chase,
    /// Walking back home after giving up a chase; ignores everyone until it arrives.
    Return,
}

/// A monster's mind.
#[derive(Component, Clone, Copy, Debug)]
pub struct Brain {
    pub mode: Mode,
    pub home: Vec2,
    pub aggro_radius: f32,
    pub wander: f32,
    pub wander_target: Option<Vec2>,
    pub next_think: f64,
    pub next_wander: f64,
    /// When the current fight began (a monster that's never been hit still keeps chasing).
    pub engaged_at: f64,
}

impl Brain {
    pub fn new(home: Vec2, aggro_radius: f32, wander: f32) -> Brain {
        Brain {
            mode: Mode::Idle,
            home,
            aggro_radius,
            wander,
            wander_target: None,
            next_think: 0.0,
            next_wander: 0.0,
            engaged_at: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonsterKind {
    Wolf,
    Goblin,
}

pub struct MonsterDef {
    pub name: &'static str,
    pub body: &'static str,
    pub level: u8,
    pub health: f32,
    pub attack: Attack,
    pub run_speed: f32,
    pub radius: f32,
    pub height: f32,
    pub aggro_radius: f32,
    pub wander: f32,
}

impl MonsterKind {
    pub fn def(self) -> MonsterDef {
        match self {
            MonsterKind::Wolf => MonsterDef {
                name: "Grey wolf",
                body: "wolf",
                level: 2,
                health: 70.0,
                attack: Attack { range: 1.9, interval: 1.8, damage: 7.0 },
                run_speed: 6.4,
                radius: 0.5,
                height: 0.9,
                aggro_radius: 12.0,
                wander: 7.0,
            },
            MonsterKind::Goblin => MonsterDef {
                name: "Goblin",
                body: "goblin",
                level: 3,
                health: 90.0,
                attack: Attack { range: 2.0, interval: 2.2, damage: 9.0 },
                run_speed: 5.4,
                radius: 0.4,
                height: 1.3,
                aggro_radius: 10.0,
                wander: 6.0,
            },
        }
    }
}
