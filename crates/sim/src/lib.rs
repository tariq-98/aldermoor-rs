//! The game world, stepped 60 times a second.
//!
//! Deterministic: no wall clock, one seeded random sequence, systems run in a fixed order, and
//! units are visited in the order they were created. The same seed and the same commands always
//! give the same world, which is what replays, tests and multiplayer rely on.
//!
//! Milestone 1 covers movement (running, walking, backpedalling, jumping, dodging, sliding along
//! walls), monsters that wander, notice you, chase, fight and give up, and a simple fight.

mod brain;
mod combat;
mod components;
mod movement;
mod rng;
mod spatial;
mod vitals;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
use glam::{Vec2, Vec3};
use protocol::{Command, Event, PlayerState, Snapshot, UnitId, UnitKind};

pub use components::*;
pub use rng::Rng;

/// Steps per second.
pub const TICK_RATE: u32 = 60;
/// Seconds per step.
pub const DT: f32 = 1.0 / TICK_RATE as f32;

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Step;

/// Simulation time.
#[derive(Resource, Default)]
pub struct Clock {
    pub tick: u64,
    pub time: f64,
}

/// The land everyone walks on.
#[derive(Resource)]
pub struct Land(pub world::Valley);

/// Commands waiting for the next step, in arrival order.
#[derive(Resource, Default)]
pub struct Inbox(pub Vec<(UnitId, Command)>);

/// Events produced this step.
#[derive(Resource, Default)]
pub struct Outbox(pub Vec<Event>);

/// Unit IDs, handed out in order, and where each unit lives.
#[derive(Resource, Default)]
pub struct Units {
    next: UnitId,
    entities: Vec<Option<Entity>>,
}

impl Units {
    fn issue(&mut self, e: Entity) -> UnitId {
        self.next += 1;
        let id = self.next;
        if self.entities.len() <= id as usize {
            self.entities.resize(id as usize + 1, None);
        }
        self.entities[id as usize] = Some(e);
        id
    }

    pub fn entity(&self, id: UnitId) -> Option<Entity> {
        self.entities.get(id as usize).copied().flatten()
    }
}

pub struct Sim {
    pub world: World,
    schedule: Schedule,
}

impl Sim {
    /// A fresh Brambleford valley.
    pub fn new(seed: u64) -> Sim {
        let mut world = World::new();
        world.insert_resource(Clock::default());
        world.insert_resource(Land(world::Valley::new()));
        world.insert_resource(Rng::new(seed));
        world.insert_resource(Inbox::default());
        world.insert_resource(Outbox::default());
        world.insert_resource(Units::default());
        world.insert_resource(spatial::Spatial::default());
        let mut schedule = Schedule::new(Step);
        schedule.set_executor(SingleThreadedExecutor::new());
        schedule.add_systems(
            (
                combat::apply_commands,
                brain::think,
                movement::move_units,
                combat::fight,
                vitals::update_vitals,
                spatial::rebuild,
                movement::update_poses,
            )
                .chain(),
        );
        Sim { world, schedule }
    }

    /// The valley with its monsters in their usual places.
    pub fn brambleford(seed: u64) -> Sim {
        let mut sim = Sim::new(seed);
        // A wolf pack in the meadow west of the north road, and goblins by the eastern ridge.
        for (dx, dz) in [(0.0, 0.0), (3.0, 2.0), (-2.5, 3.5)] {
            sim.spawn_monster(MonsterKind::Wolf, Vec2::new(-38.0 + dx, 30.0 + dz));
        }
        for (dx, dz) in [(0.0, 0.0), (4.0, -3.0)] {
            sim.spawn_monster(MonsterKind::Goblin, Vec2::new(52.0 + dx, 8.0 + dz));
        }
        sim
    }

    pub fn tick(&self) -> u64 {
        self.world.resource::<Clock>().tick
    }

    pub fn time(&self) -> f64 {
        self.world.resource::<Clock>().time
    }

    pub fn land(&self) -> &world::Valley {
        &self.world.resource::<Land>().0
    }

    /// Queues a command from a unit's controller for the next step.
    pub fn command(&mut self, from: UnitId, c: Command) {
        self.world.resource_mut::<Inbox>().0.push((from, c));
    }

    /// Advances the world by one step.
    pub fn step(&mut self) {
        {
            let mut clock = self.world.resource_mut::<Clock>();
            clock.tick += 1;
            clock.time = clock.tick as f64 / TICK_RATE as f64;
        }
        self.schedule.run(&mut self.world);
    }

    /// Advances the world by about `seconds`.
    pub fn run_for(&mut self, seconds: f32) {
        for _ in 0..(seconds * TICK_RATE as f32).round() as u32 {
            self.step();
        }
    }

    fn place(&self, at: Vec2) -> Vec3 {
        Vec3::new(at.x, self.land().ground(at.x, at.y), at.y)
    }

    /// Adds a player at `at`, facing `yaw`.
    pub fn spawn_player(&mut self, name: &str, at: Vec2, yaw: f32) -> UnitId {
        let pos = self.place(at);
        let e = self
            .world
            .spawn((
                Transform { pos, yaw },
                Motion::new(6.0),
                Body { radius: 0.4, height: 1.8 },
                Vitals::new(250.0),
                Fighter::new(Attack { range: 2.4, interval: 1.6, damage: 18.0 }),
                Anim::default(),
                PlayerControl,
                Home { pos, yaw },
                Appearance(components::player_look()),
            ))
            .id();
        let id = self.world.resource_mut::<Units>().issue(e);
        self.world.entity_mut(e).insert(Unit {
            id,
            kind: UnitKind::Player,
            body: "player",
            name: name.to_string(),
            level: 1,
            faction: Faction::Player,
        });
        id
    }

    /// Adds a monster that wanders around `at`.
    pub fn spawn_monster(&mut self, kind: MonsterKind, at: Vec2) -> UnitId {
        let pos = self.place(at);
        let def = kind.def();
        let yaw = self.world.resource_mut::<Rng>().range(-std::f32::consts::PI, std::f32::consts::PI);
        let e = self
            .world
            .spawn((
                Transform { pos, yaw },
                Motion::new(def.run_speed),
                Body { radius: def.radius, height: def.height },
                Vitals::new(def.health),
                Fighter::new(def.attack),
                Anim::default(),
                Brain::new(at, def.aggro_radius, def.wander),
                Home { pos, yaw },
                Appearance(kind.look()),
            ))
            .id();
        let id = self.world.resource_mut::<Units>().issue(e);
        // Spread monsters' decisions across the quarter-second so they don't all think at once.
        self.world.get_mut::<Brain>(e).unwrap().next_think = brain::think_offset(id);
        self.world.entity_mut(e).insert(Unit {
            id,
            kind: UnitKind::Monster,
            body: def.body,
            name: def.name.to_string(),
            level: def.level,
            faction: Faction::Hostile,
        });
        id
    }

    /// Fills the valley with `n` wandering monsters, for stress tests and benchmarks.
    pub fn spawn_crowd(&mut self, n: usize) {
        self.spawn_crowd_in(n, Vec2::new(0.0, -40.0), Vec2::new(200.0, 160.0));
    }

    /// Spawns `n` wandering monsters in the box `center ± half`.
    pub fn spawn_crowd_in(&mut self, n: usize, center: Vec2, half: Vec2) {
        let mut placed = 0;
        let mut attempts = 0;
        while placed < n && attempts < n * 50 {
            attempts += 1;
            let (x, z) = {
                let mut rng = self.world.resource_mut::<Rng>();
                (rng.range(-half.x, half.x), rng.range(-half.y, half.y))
            };
            let p = center + Vec2::new(x, z);
            if self.land().blocked(p, 0.6) {
                continue;
            }
            let kind = if placed % 3 == 0 { MonsterKind::Goblin } else { MonsterKind::Wolf };
            self.spawn_monster(kind, p);
            placed += 1;
        }
    }

    pub fn entity(&self, id: UnitId) -> Option<Entity> {
        self.world.resource::<Units>().entity(id)
    }

    /// A unit's components, for tests and tools.
    pub fn get<T: Component>(&self, id: UnitId) -> Option<&T> {
        self.entity(id).and_then(|e| self.world.get::<T>(e))
    }

    pub fn get_mut<T: Component<Mutability = bevy_ecs::component::Mutable>>(&mut self, id: UnitId) -> Option<Mut<'_, T>> {
        let e = self.entity(id)?;
        self.world.get_mut::<T>(e)
    }

    /// The world as `viewer` sees it: everyone within 160 m, their own numbers, and this step's events.
    pub fn snapshot(&mut self, viewer: Option<UnitId>) -> Snapshot {
        let clock = self.world.resource::<Clock>();
        let (tick, time) = (clock.tick, clock.time);
        let center = viewer.and_then(|v| self.get::<Transform>(v)).map(|t| t.pos);
        let mut units = Vec::new();
        let mut q = self.world.query::<(&Unit, &Transform, &Motion, &Vitals, &Anim, &Fighter, Option<&Appearance>)>();
        for (u, t, m, v, a, f, look) in q.iter(&self.world) {
            if let Some(c) = center {
                if (t.pos - c).length_squared() > 160.0 * 160.0 {
                    continue;
                }
            }
            if v.dead && time - v.died_at > 8.0 && u.kind != UnitKind::Player {
                continue;
            }
            units.push(protocol::UnitState {
                id: u.id,
                kind: u.kind,
                body: u.body.to_string(),
                name: u.name.clone(),
                level: u.level,
                position: t.pos,
                velocity: m.observed,
                yaw: t.yaw,
                pose: a.pose,
                pose_time: (time - a.since) as f32,
                health: v.health,
                max_health: v.max_health,
                hostile: u.faction == Faction::Hostile,
                target: f.target,
                look: {
                    let mut l = look.map(|a| a.0.clone()).unwrap_or_default();
                    // Weapons come out for a fight and go away once it's over.
                    l.drawn = f.target.is_some() || time - v.last_combat < vitals::COMBAT_TIMEOUT;
                    l
                },
            });
        }
        units.sort_by_key(|u| u.id);
        let player = viewer.and_then(|id| {
            let v = self.get::<Vitals>(id)?;
            let f = self.get::<Fighter>(id)?;
            Some(PlayerState {
                unit: id,
                endurance: v.endurance,
                max_endurance: vitals::ENDURANCE_MAX,
                cooldowns: f.cooldowns.iter().map(|&c| (c - time).max(0.0) as f32).collect(),
                auto_attack: f.auto_attack,
                in_combat: time - v.last_combat < vitals::COMBAT_TIMEOUT,
                can_respawn: v.dead,
            })
        });
        let events = std::mem::take(&mut self.world.resource_mut::<Outbox>().0);
        Snapshot { tick, time, units, player, events }
    }
}
