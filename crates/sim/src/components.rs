use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use std::sync::Arc;
use protocol::{Action, Pose, ResourceKind, UnitId, UnitKind};

use crate::content::{AbilityId, AuraId, Specialty};

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

    pub fn xz(&self) -> Vec2 {
        Vec2::new(self.pos.x, self.pos.z)
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
    /// Asking to sprint: granted out of combat, on land.
    pub sprint: bool,
    /// Locked facing while strafing; `None` turns toward travel.
    pub face_yaw: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ForcedKind {
    Dodge,
    /// Thrown: can't act until it lands.
    Knockback,
    /// A lunge, vault or leap from a skill.
    Leap,
    /// Rushing at a target; the rest of the ability lands on arrival.
    Charge { target: UnitId, ability: AbilityId },
    /// Pulling up over a ledge: up first, then over the edge.
    Climb,
    /// Pulling up onto a ledge caught in a jump: as a climb, from higher up the face.
    Mantle,
    /// Hopping a low wall in stride, at the speed it was run at (`speed`, m/s), which it keeps
    /// on landing. `Forced::arc` is how high above the start the hop clears.
    Vault { speed: f32 },
    /// Onto a hold to hang from it: caught in the air, lowered from the edge above, or climbing
    /// to the next hold (docs/MOVEMENT.md, "Climbing").
    ToHold { hold: u32, at: f32 },
    /// Round a corner onto the hold that meets this one there, to hang from it, swinging about
    /// the `corner`.
    Corner { hold: u32, at: f32, corner: Vec2 },
    /// Running up a wall too tall to climb (Shift into it), catching a hold on the way; `away` is
    /// out of the wall.
    WallRun { away: Vec2 },
}

/// Hanging from a hold (`world::Hold`, by its index in the valley's holds), with the hands `at`
/// metres along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hang {
    pub hold: u32,
    pub at: f32,
}

/// A move the body is carried through (a roll, a leap, being thrown), ignoring the controls.
#[derive(Clone, Copy, Debug)]
pub struct Forced {
    pub kind: ForcedKind,
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
    /// A jump was pressed: it goes off if the feet are on the ground (or just off an edge)
    /// before this time.
    pub jump_until: f64,
    /// Just walked off an edge: a jump still works until this time.
    pub coyote_until: f64,
    /// A roll pressed near the end of another forced move: which way, and until when it waits
    /// for that move to end.
    pub dodge_queued: Option<(Vec2, f64)>,
    pub forced: Option<Forced>,
    /// In deep water: floating at the surface, slow, and unable to fight.
    pub swimming: bool,
    /// On its feet in water deeper than `movement::WADE_DEPTH` after the last step, for the
    /// snapshot (found once a step, not once for each viewer).
    pub wading: bool,
    /// Sprinting this step (asked to, and allowed).
    pub sprinting: bool,
    /// Where a player aims up or down (`Command::Aim`), as `protocol::aim_pitch_byte`.
    pub aim_pitch: i8,
    /// The highest the feet have been since they last touched the ground or the water, for fall
    /// damage.
    pub fall_from: f32,
    /// Hanging from a hold, between moves.
    pub hang: Option<Hang>,
    /// The hold just let go of, which can't be caught again until the time given.
    pub released: Option<(u32, f64)>,
    /// Holds are caught in the air without Shift until this time: just after a leap, an eject or
    /// a wall run.
    pub catch_until: f64,
    /// Roll was pressed in the air: landing before this time rolls the fall off.
    pub landing_roll_until: f64,
}

impl Motion {
    pub fn new(run_speed: f32) -> Motion {
        Motion {
            vel: Vec3::ZERO,
            observed: Vec3::ZERO,
            grounded: true,
            run_speed,
            intent: MoveIntent::default(),
            jump_until: f64::NEG_INFINITY,
            coyote_until: f64::NEG_INFINITY,
            dodge_queued: None,
            forced: None,
            swimming: false,
            wading: false,
            sprinting: false,
            aim_pitch: 0,
            fall_from: f32::NEG_INFINITY,
            hang: None,
            released: None,
            catch_until: f64::NEG_INFINITY,
            landing_roll_until: f64::NEG_INFINITY,
        }
    }

    /// Trying to move this step (a cast that needs stillness stops).
    pub fn moving(&self) -> bool {
        self.intent.direction.length_squared() > 0.01 || !self.grounded || self.forced.is_some()
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct Vitals {
    pub health: f32,
    pub max_health: f32,
    pub endurance: f32,
    pub dead: bool,
    pub died_at: f64,
    /// Down but not out: health ran out and the downed pool is bleeding away.
    pub downed: bool,
    pub downed_health: f32,
    pub downed_max: f32,
    pub last_combat: f64,
    /// Rolling: blows and missiles pass through until then.
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
            downed: false,
            downed_health: 0.0,
            downed_max: 0.0,
            last_combat: -100.0,
            invulnerable_until: 0.0,
            respawn_at: None,
        }
    }

    pub fn alive(&self) -> bool {
        !self.dead && !self.downed
    }

    pub fn fraction(&self) -> f32 {
        if self.max_health > 0.0 { self.health / self.max_health } else { 0.0 }
    }
}

/// A unit's numbers, from its level, specialty or monster type (and later its gear).
#[derive(Component, Clone, Copy, Debug)]
pub struct Stats {
    pub power: f32,
    pub armor: f32,
    pub crit: f32,
    pub haste: f32,
}

/// A cast or channel in progress. Area abilities keep the aim they started with, so a warned
/// blow lands where it was warned of.
#[derive(Clone, Copy, Debug)]
pub struct Cast {
    pub ability: AbilityId,
    pub target: Option<UnitId>,
    pub ground: Option<Vec3>,
    pub yaw: f32,
    pub start: f64,
    pub duration: f64,
    pub channel: bool,
    pub next_tick: f64,
    pub ticks_done: u8,
}

/// A skill pressed just before the global cooldown ended: it goes off the moment it does.
#[derive(Clone, Copy, Debug)]
pub struct Queued {
    pub ability: AbilityId,
    pub ground: Option<Vec3>,
    pub until: f64,
}

/// Everything about how a unit fights.
#[derive(Component, Clone, Debug)]
pub struct Fighter {
    pub target: Option<UnitId>,
    pub auto_attack: bool,
    /// The bar, slot 0 being key 1: 1–5 weapon skills, 6 heal, 7–9 utilities, 0 elite.
    pub bar: [Option<AbilityId>; 10],
    /// Skills 6–0, chosen from the specialty (kept across weapon swaps).
    pub kit: [Option<AbilityId>; 5],
    /// Weapon sets (main or two-handed item, off-hand item), and which is in hand.
    pub sets: Vec<(Option<String>, Option<String>)>,
    pub active_set: usize,
    pub swap_ready_at: f64,
    /// The basic attack that repeats while there's a target.
    pub auto: Option<AbilityId>,
    pub specialty: Option<Specialty>,
    /// When each ability is ready again (simulation time).
    pub cooldowns: Vec<(AbilityId, f64)>,
    pub gcd_until: f64,
    pub cast: Option<Cast>,
    pub queued: Option<Queued>,
    /// Interrupted: no skills until then.
    pub lockout_until: f64,
    pub resource_kind: ResourceKind,
    pub resource: f32,
    pub max_resource: f32,
    /// When the guard went up (a blow soon after is parried); `None` when it's down.
    pub guard_since: Option<f64>,
    /// The action being animated, and when it began.
    pub action: Action,
    pub action_time: f64,
    /// Enemies this unit hurt recently (felling one rallies you if you're downed).
    pub recent_victims: Vec<(UnitId, f64)>,
    /// Fighting players until then: heals on it take PvP numbers.
    pub pvp_until: f64,
    /// Helping someone up: who, and since when.
    pub reviving: Option<(UnitId, f64)>,
}

impl Fighter {
    pub fn new() -> Fighter {
        Fighter {
            target: None,
            auto_attack: false,
            bar: [None; 10],
            kit: [None; 5],
            sets: Vec::new(),
            active_set: 0,
            swap_ready_at: 0.0,
            auto: None,
            specialty: None,
            cooldowns: Vec::new(),
            gcd_until: 0.0,
            cast: None,
            queued: None,
            lockout_until: 0.0,
            resource_kind: ResourceKind::None,
            resource: 0.0,
            max_resource: 0.0,
            guard_since: None,
            action: Action::None,
            action_time: 0.0,
            recent_victims: Vec::new(),
            pvp_until: -100.0,
            reviving: None,
        }
    }

    pub fn ready_at(&self, ability: AbilityId) -> f64 {
        self.cooldowns.iter().find(|(a, _)| *a == ability).map_or(0.0, |(_, t)| *t)
    }

    pub fn set_cooldown(&mut self, ability: AbilityId, until: f64) {
        match self.cooldowns.iter_mut().find(|(a, _)| *a == ability) {
            Some(c) => c.1 = until,
            None => self.cooldowns.push((ability, until)),
        }
    }
}

impl Default for Fighter {
    fn default() -> Self {
        Fighter::new()
    }
}

/// One aura on a unit, with what it needs to keep working after its source is gone.
#[derive(Clone, Copy, Debug)]
pub struct AuraInstance {
    pub aura: AuraId,
    pub source: UnitId,
    pub power: f32,
    pub level: u8,
    pub expires: f64,
    pub duration: f64,
    pub stacks: u8,
    pub next_tick: f64,
    pub absorb_left: f32,
    /// Put on by a player or their summon (see [`crate::combat::HitSource::player`]).
    pub player: bool,
}

/// Diminishing returns in one category of crowd control.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dr {
    pub count: u8,
    pub reset_at: f64,
}

#[derive(Component, Clone, Debug, Default)]
pub struct Auras {
    pub list: Vec<AuraInstance>,
    pub dr: [Dr; 4],
    /// Elites and bosses: crowd control chips this bar instead of landing.
    pub defiance: f32,
    pub defiance_max: f32,
    pub defiance_back_at: f64,
}

/// What a unit's auras and situation allow this step (worked out before it moves).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Control {
    pub stunned: bool,
    pub rooted: bool,
    /// Feared: running from here.
    pub fleeing: Option<Vec2>,
    pub dazed: bool,
    pub silenced: bool,
    pub blocking: bool,
    pub reflecting: bool,
    pub cc_immune: bool,
    pub invulnerable: bool,
    /// Movement speed multiplier.
    pub speed: f32,
}

impl Control {
    pub fn can_move(&self) -> bool {
        !self.stunned && !self.rooted && self.fleeing.is_none()
    }

    pub fn can_act(&self) -> bool {
        !self.stunned && self.fleeing.is_none() && !self.dazed && !self.silenced
    }
}

/// A monster's grudges: who it wants to hit, and how much.
#[derive(Component, Clone, Debug, Default)]
pub struct Threat {
    pub entries: Vec<(UnitId, f32)>,
    pub taunted_by: Option<UnitId>,
    pub taunt_until: f64,
}

impl Threat {
    pub fn add(&mut self, id: UnitId, amount: f32) {
        match self.entries.iter_mut().find(|(e, _)| *e == id) {
            Some(e) => e.1 += amount,
            None => self.entries.push((id, amount.max(0.0))),
        }
    }

    pub fn of(&self, id: UnitId) -> f32 {
        self.entries.iter().find(|(e, _)| *e == id).map_or(0.0, |e| e.1)
    }

    pub fn contains(&self, id: UnitId) -> bool {
        self.entries.iter().any(|(e, _)| *e == id)
    }

    pub fn remove(&mut self, id: UnitId) {
        self.entries.retain(|(e, _)| *e != id);
        if self.taunted_by == Some(id) {
            self.taunted_by = None;
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.taunted_by = None;
    }

    /// Forced to the top of the list for a while.
    pub fn taunt(&mut self, by: UnitId, until: f64) {
        let top = self.entries.iter().map(|e| e.1).fold(0.0f32, f32::max);
        match self.entries.iter_mut().find(|(e, _)| *e == by) {
            Some(e) => e.1 = e.1.max(top * 1.1 + 1.0),
            None => self.entries.push((by, top * 1.1 + 1.0)),
        }
        self.taunted_by = Some(by);
        self.taunt_until = until;
    }

    /// Who to attack: the taunter while it lasts, else the most threat, but only switching
    /// from the current target when someone pulls ahead by 10% (in melee) or 30% (at range).
    pub fn pick(&self, current: Option<UnitId>, now: f64, valid: impl Fn(UnitId) -> bool, melee: impl Fn(UnitId) -> bool) -> Option<UnitId> {
        if let Some(t) = self.taunted_by && now < self.taunt_until && valid(t) {
            return Some(t);
        }
        let best = self.entries.iter().filter(|(e, _)| valid(*e)).max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))?;
        match current {
            Some(cur) if valid(cur) && cur != best.0 => {
                let needed = self.of(cur) * if melee(best.0) { 1.1 } else { 1.3 };
                Some(if best.1 > needed { best.0 } else { cur })
            }
            _ => Some(best.0),
        }
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

/// Called up by someone (a necromancer's skeleton): fights for its owner until `until`.
#[derive(Component, Clone, Copy, Debug)]
pub struct Summoned {
    pub owner: UnitId,
    pub until: f64,
}

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
    /// Its definition in the content tables.
    pub mob: &'static str,
    pub mode: Mode,
    pub home: Vec2,
    pub aggro_radius: f32,
    pub wander: f32,
    pub wander_target: Option<Vec2>,
    pub next_think: f64,
    pub next_wander: f64,
    /// When the current fight began (a monster that's never been hit still keeps chasing).
    pub engaged_at: f64,
    /// Where round its target a melee monster stands, as a bearing and a distance from the
    /// target's centre, so a pack spreads round its prey instead of queueing on the side it came
    /// from.
    pub slot: Option<(f32, f32)>,
    /// Idle with nobody on the players' side in sight: it stands still and only looks around
    /// once a second.
    pub asleep: bool,
}

impl Brain {
    pub fn new(mob: &'static str, home: Vec2, aggro_radius: f32, wander: f32) -> Brain {
        Brain {
            mob,
            mode: Mode::Idle,
            home,
            aggro_radius,
            wander,
            wander_target: None,
            next_think: 0.0,
            next_wander: 0.0,
            engaged_at: 0.0,
            slot: None,
            asleep: false,
        }
    }
}

/// How a unit looks (see protocol::Look). The snapshot works `drawn` out from the fight, and
/// swapping weapons changes the gear.
///
/// It keeps the look twice, weapons put away and drawn, each shared with every snapshot rather
/// than copied into it (#189).
#[derive(Component, Clone, Debug)]
pub struct Appearance {
    put_away: Arc<protocol::Look>,
    drawn: Arc<protocol::Look>,
}

impl Appearance {
    pub fn new(look: protocol::Look) -> Appearance {
        let drawn = Arc::new(protocol::Look { drawn: true, ..look.clone() });
        Appearance { put_away: Arc::new(protocol::Look { drawn: false, ..look }), drawn }
    }

    pub fn look(&self) -> &protocol::Look {
        &self.put_away
    }

    pub fn edit(&mut self, f: impl FnOnce(&mut protocol::Look)) {
        let mut look = (*self.put_away).clone();
        f(&mut look);
        *self = Appearance::new(look);
    }

    /// The look to send, weapons drawn or not.
    pub fn shared(&self, drawn: bool) -> Arc<protocol::Look> {
        Arc::clone(self.shown(drawn))
    }

    /// The same, borrowed.
    pub fn shown(&self, drawn: bool) -> &Arc<protocol::Look> {
        if drawn { &self.drawn } else { &self.put_away }
    }

    /// What a unit with no look of its own is sent: the body type's defaults.
    pub fn bare() -> &'static Appearance {
        static BARE: std::sync::LazyLock<Appearance> = std::sync::LazyLock::new(|| Appearance::new(protocol::Look::default()));
        &BARE
    }
}

impl Default for Appearance {
    fn default() -> Appearance {
        Appearance::bare().clone()
    }
}

/// The monsters in the content tables that the valley spawns by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonsterKind {
    Wolf,
    Goblin,
}

impl MonsterKind {
    pub fn key(self) -> &'static str {
        match self {
            MonsterKind::Wolf => "wolf",
            MonsterKind::Goblin => "goblin",
        }
    }
}

/// The default player: a farmhand with an arming sword and a round shield.
pub fn player_look() -> protocol::Look {
    protocol::Look {
        art: "male".into(),
        tone: Some("warm".into()),
        hair: Some("hair.simple_parted".into()),
        hair_colour: Some("chestnut".into()),
        brows: Some("brows.strong".into()),
        beard: Some("beard.full".into()),
        pieces: ["peasant.top", "peasant.trousers", "peasant.shoes"].map(String::from).to_vec(),
        gear: vec![("main_hand".into(), "sword.arming".into()), ("off_hand".into(), "shield.round".into())],
        drawn: false,
        ..Default::default()
    }
}
