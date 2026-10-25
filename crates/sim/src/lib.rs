//! The game world, stepped 60 times a second.
//!
//! Deterministic: no wall clock, one seeded random sequence, systems run in a fixed order, and
//! units are visited in the order they were created. The same seed and the same commands always
//! give the same world, which is what replays, tests and multiplayer rely on.
//!
//! Milestone 1 covered movement (running, walking, backpedalling, jumping, dodging, sliding along
//! walls) and monsters that wander, notice you, chase and give up. Milestone 2 is the fight:
//! skills from weapons and specialties, casts and channels, auras and crowd control, projectiles
//! and warned blows, guarding and parrying, threat, and being downed (docs/DESIGN.md).

mod brain;
pub mod combat;
mod components;
pub mod content;
mod movement;
mod rng;
mod spatial;
pub mod stats;
mod vitals;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};
use glam::{Vec2, Vec3};
use protocol::{CombatView, Command, Event, Snapshot, UnitId, UnitKind};

pub use components::*;
pub use content::Specialty;
pub use rng::Rng;
pub use vitals::{COMBAT_TIMEOUT, ENDURANCE_MAX};

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

    /// Every unit, in ID order.
    pub fn all(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities.iter().filter_map(|e| *e)
    }

    /// A unit that's gone for good (a summon's remains).
    pub(crate) fn forget(&mut self, id: UnitId) {
        if let Some(slot) = self.entities.get_mut(id as usize) {
            *slot = None;
        }
    }
}

/// A reused buffer for the systems that visit every unit, so a step doesn't allocate one per system.
#[derive(Resource, Default)]
struct UnitList(Vec<Entity>);

/// Every unit, in ID order, in the shared buffer. Hand it back with [`return_units`].
pub(crate) fn take_units(world: &mut World) -> Vec<Entity> {
    let mut list = std::mem::take(&mut world.resource_mut::<UnitList>().0);
    list.clear();
    list.extend(world.resource::<Units>().all());
    list
}

pub(crate) fn return_units(world: &mut World, list: Vec<Entity>) {
    world.resource_mut::<UnitList>().0 = list;
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
        world.insert_resource(UnitList::default());
        world.insert_resource(spatial::Spatial::default());
        world.insert_resource(combat::missiles::Missiles::default());
        world.insert_resource(combat::missiles::Areas::default());
        world.insert_resource(combat::abilities::Arrivals::default());
        world.insert_resource(movement::Landings::default());
        let mut schedule = Schedule::new(Step);
        schedule.set_executor(SingleThreadedExecutor::new());
        schedule.add_systems(
            (
                combat::apply_commands,
                brain::think,
                combat::auras::refresh_control,
                movement::move_units,
                movement::after_moving,
                combat::abilities::land_charges,
                combat::abilities::update,
                combat::missiles::update,
                combat::missiles::update_areas,
                combat::auras::update,
                vitals::update,
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
        sim.spawn_mob("goblin_archer", Vec2::new(57.0, 11.0));
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

    /// The land, to add to (tests put walls where they need them).
    pub fn land_mut(&mut self) -> &mut world::Valley {
        &mut self.world.resource_mut::<Land>().into_inner().0
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

    /// Adds a level-1 warrior at `at`, facing `yaw`: a farmhand with an arming sword and a round
    /// shield, and a greatsword to swap to.
    pub fn spawn_player(&mut self, name: &str, at: Vec2, yaw: f32) -> UnitId {
        self.spawn_hero(name, Specialty::Warrior, 1, at, yaw)
    }

    /// Adds a player of `specialty` and `level`, with its default weapons and skills.
    pub fn spawn_hero(&mut self, name: &str, specialty: Specialty, level: u8, at: Vec2, yaw: f32) -> UnitId {
        let cat = content::catalog();
        let def = cat.specialty(specialty);
        let pos = place(&self.world, at);
        let (stats, health) = stats::for_specialty(def, level);
        let mut f = Fighter::new();
        f.specialty = Some(specialty);
        f.resource_kind = def.resource;
        f.max_resource = def.max_resource;
        f.resource = def.start;
        f.sets = def.sets.iter().map(|(m, o)| (m.map(String::from), o.map(String::from))).collect();
        let first = |list: &[&str], n: usize| list.get(n).and_then(|k| cat.ability_id(k));
        f.kit = [first(&def.heals, 0), first(&def.utilities, 0), first(&def.utilities, 1), first(&def.utilities, 2), first(&def.elites, 0)];
        let e = self
            .world
            .spawn((
                Transform { pos, yaw },
                Motion::new(6.0),
                Body { radius: 0.4, height: 1.8 },
                Vitals::new(health),
                stats,
                f,
                Auras::default(),
                Control { speed: 1.0, ..Default::default() },
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
            level,
            faction: Faction::Player,
        });
        combat::abilities::equip(&mut self.world, e);
        id
    }

    /// Puts a specialty skill in slot 6–0 (`slot` 5–9): the heal, a utility or the elite.
    /// Returns false if the specialty has no such skill for that slot.
    pub fn set_skill(&mut self, id: UnitId, slot: usize, key: &str) -> bool {
        let cat = content::catalog();
        let Some(e) = self.entity(id) else { return false };
        let Some(spec) = self.world.get::<Fighter>(e).and_then(|f| f.specialty) else { return false };
        let def = cat.specialty(spec);
        let allowed = match slot {
            5 => def.heals.contains(&key),
            6..=8 => def.utilities.contains(&key),
            9 => def.elites.contains(&key),
            _ => false,
        };
        let Some(ability) = cat.ability_id(key).filter(|_| allowed) else { return false };
        if let Some(mut f) = self.world.get_mut::<Fighter>(e) {
            if f.kit.contains(&Some(ability)) {
                return false;
            }
            f.kit[slot - 5] = Some(ability);
        }
        combat::abilities::equip(&mut self.world, e);
        true
    }

    /// Adds a monster that wanders around `at`.
    pub fn spawn_monster(&mut self, kind: MonsterKind, at: Vec2) -> UnitId {
        self.spawn_mob(kind.key(), at).expect("the valley's monsters are in the content tables")
    }

    /// Adds a monster from the content tables (by key, e.g. "dire_wolf") at its usual level.
    pub fn spawn_mob(&mut self, key: &str, at: Vec2) -> Option<UnitId> {
        spawn_mob(&mut self.world, key, at, None, Faction::Hostile)
    }

    /// A monster at another level than its usual one.
    pub fn spawn_mob_at_level(&mut self, key: &str, at: Vec2, level: u8) -> Option<UnitId> {
        spawn_mob(&mut self.world, key, at, Some(level), Faction::Hostile)
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
            if self.land().blocked(p, 0.6) || self.land().water_depth(p.x, p.y) > 0.0 {
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
            if let Some(c) = center && (t.pos - c).length_squared() > combat::view::VIEW_RANGE * combat::view::VIEW_RANGE {
                continue;
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
                    l.drawn = f.target.is_some() || f.guard_since.is_some() || time - v.last_combat < COMBAT_TIMEOUT;
                    l
                },
            });
        }
        units.sort_by_key(|u| u.id);
        let player = viewer.and_then(|id| combat::view::player_state(&self.world, id));
        let events = std::mem::take(&mut self.world.resource_mut::<Outbox>().0);
        Snapshot { tick, time, units, player, events }
    }

    /// Combat detail for everything `viewer` can see (actions, casts, auras, projectiles,
    /// warnings and fields), published beside each snapshot.
    pub fn combat_view(&self, viewer: Option<UnitId>) -> CombatView {
        combat::view::combat_view(&self.world, viewer)
    }
}

fn place(world: &World, at: Vec2) -> Vec3 {
    Vec3::new(at.x, world.resource::<Land>().0.ground(at.x, at.y), at.y)
}

/// Adds a monster from the content tables at `at`. `level` and `faction` override its own:
/// summons fight at their owner's level, on their owner's side.
pub(crate) fn spawn_mob(world: &mut World, key: &str, at: Vec2, level: Option<u8>, faction: Faction) -> Option<UnitId> {
    let cat = content::catalog();
    let def = cat.mob(key)?;
    let level = level.unwrap_or(def.level);
    let pos = place(world, at);
    let yaw = world.resource_mut::<Rng>().range(-std::f32::consts::PI, std::f32::consts::PI);
    let (stats, health) = stats::for_mob(def, level);
    let mut f = Fighter::new();
    f.auto = cat.ability_id(def.auto);
    let auras = Auras { defiance: def.defiance, defiance_max: def.defiance, ..Default::default() };
    let e = world
        .spawn((
            Transform { pos, yaw },
            Motion::new(def.speed),
            Body { radius: def.radius, height: def.height },
            Vitals::new(health),
            stats,
            f,
            auras,
            Control { speed: 1.0, ..Default::default() },
        ))
        .id();
    world.entity_mut(e).insert((
        Threat::default(),
        Anim::default(),
        Brain::new(def.key, at, def.aggro, def.wander),
        Home { pos, yaw },
        Appearance((def.look)()),
    ));
    let id = world.resource_mut::<Units>().issue(e);
    // Spread monsters' decisions across the quarter-second so they don't all think at once.
    let first = world.resource::<Clock>().time + brain::think_offset(id);
    world.get_mut::<Brain>(e).unwrap().next_think = first;
    world.entity_mut(e).insert(Unit { id, kind: UnitKind::Monster, body: def.body, name: def.name.to_string(), level, faction });
    Some(id)
}
