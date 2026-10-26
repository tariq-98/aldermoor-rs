//! One region of a world split into several (#168, docs/SERVER.md "Regions"). Each region is its
//! own `Sim` on its own thread; the host (`server::regions`) moves what crosses between them,
//! between steps. This is a region's side of it:
//!
//! - its block of unit IDs (`region << 24 | n`), so an ID never changes when a unit crosses;
//! - the ground it owns ([`Bounds`]), past which its monsters give up a chase;
//! - handing a unit over whole ([`Sim::export`], [`Sim::import`]);
//! - read-only ghosts of the neighbours' units near the border ([`Sim::ghosts_near`],
//!   [`Sim::set_ghosts`]), which it can see and target but never changes;
//! - harm aimed at a ghost, sent to the region that owns it ([`Sim::take_requests`],
//!   [`Sim::apply_requests`]) and applied there at the start of its next step.
//!
//! A sim that is never told its region is region 0 owning the whole world, as before.

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3};
use protocol::UnitId;
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::combat::effects::{self, Delivery};
use crate::combat::HitSource;
use crate::content::{AbilityId, Effect};
use crate::{Anim, Appearance, Auras, Body, Brain, Control, Credit, Fighter, Home, Inventory, Motion, PlayerControl, Progress, Sim, Stats, Summoned, Threat};
use crate::{Transform, Unit, Units, Vitals};

const DEFAULT_BODY: Body = Body { radius: 0.4, height: 1.8 };

/// Unit IDs in each region's block.
pub const BLOCK: UnitId = 1 << 24;
/// Region IDs there's room for, with blocks that size.
pub const REGIONS: u32 = (UnitId::MAX / BLOCK) + 1;
/// Units this close to a neighbour's ground show there as ghosts.
pub const GHOST_BAND: f32 = 32.0;
/// How far past its region's edge a unit's centre goes before it hands over, so one strolling
/// along the line doesn't flap between the two.
pub const HANDOFF: f32 = 4.0;

/// A rectangle of ground on the x/z plane: `min` included, `max` not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl Bounds {
    pub fn new(min: Vec2, max: Vec2) -> Bounds {
        Bounds { min, max }
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.x < self.max.x && p.y < self.max.y
    }

    /// How far `p` is outside, 0 inside.
    pub fn distance(&self, p: Vec2) -> f32 {
        (self.min - p).max(p - self.max).max(Vec2::ZERO).length()
    }
}

/// Which region this sim is, and the ground it owns (all of it when `None`).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Region {
    pub id: u32,
    pub bounds: Option<Bounds>,
}

impl Region {
    /// Whether `p` is on this region's ground.
    pub fn owns(&self, p: Vec2) -> bool {
        self.bounds.is_none_or(|b| b.contains(p))
    }
}

/// A neighbour's unit, seen across the border. It carries only what's needed to draw it, aim at
/// it and reach it (`Unit`, `Transform`, `Body`, `Vitals`, `Anim`, its look, and `PlayerControl`
/// for a player), so the systems that move, think and fight pass it by.
#[derive(Component, Clone, Copy, Debug)]
pub struct Ghost {
    pub region: u32,
    pub velocity: Vec3,
    pub traversal: protocol::Traversal,
    pub target: Option<UnitId>,
    pub drawn: bool,
}

/// What a region publishes of one of its units for a neighbour's ghost, every step.
#[derive(Clone, Debug)]
pub struct GhostState {
    pub id: UnitId,
    pub region: u32,
    pub level: u8,
    pub transform: Transform,
    pub vitals: Vitals,
    pub anim: Anim,
    pub velocity: Vec3,
    pub traversal: protocol::Traversal,
    pub target: Option<UnitId>,
    pub drawn: bool,
    /// Who it is and what it looks like, sent only when the neighbour doesn't have it yet.
    pub first: Option<Box<GhostFirst>>,
}

/// The parts of a ghost that don't change from step to step.
#[derive(Clone, Debug)]
pub struct GhostFirst {
    pub unit: Unit,
    pub body: Body,
    pub look: Option<Appearance>,
    pub player: bool,
}

/// Something aimed at a ghost, for the region that owns the unit.
#[derive(Clone, Debug)]
pub struct Request {
    /// The region that owns the target.
    pub region: u32,
    pub target: UnitId,
    pub landing: Landing,
}

#[derive(Clone, Debug)]
pub enum Landing {
    /// What an ability does to one target, from its dodge and guard checks on.
    Effects { effects: Vec<Effect>, src: HitSource, ability: Option<AbilityId>, center: Vec3, delivery: Delivery },
    /// One effect on its own (a combo going off).
    Effect { effect: Effect, src: HitSource, ability: Option<AbilityId>, crit: bool, center: Vec3, mult: f32 },
}

/// Requests made this step, for the host to collect.
#[derive(Resource, Default)]
pub(crate) struct Outgoing(pub Vec<Request>);

/// The region owning `e`, if it's a ghost.
pub(crate) fn ghost_region(world: &World, e: Entity) -> Option<u32> {
    world.get::<Ghost>(e).map(|g| g.region)
}

pub(crate) fn send(world: &mut World, region: u32, target: Entity, landing: Landing) {
    let Some(target) = world.get::<Unit>(target).map(|u| u.id) else { return };
    world.resource_mut::<Outgoing>().0.push(Request { region, target, landing });
}

fn set<T: Component<Mutability = bevy_ecs::component::Mutable>>(world: &mut World, e: Entity, value: T) {
    if let Some(mut c) = world.get_mut::<T>(e) {
        *c = value;
    }
}

/// Whether a monster at `p` has gone past its region's edge, where it gives up the chase.
pub(crate) fn beyond(world: &World, p: Vec2) -> bool {
    !world.resource::<Region>().owns(p)
}

macro_rules! record {
    ($($field:ident: $ty:ty),* $(,)?) => {
        /// Everything about one unit, to hand it to another region whole.
        #[derive(Clone, Debug)]
        pub struct UnitRecord {
            pub id: UnitId,
            $(pub $field: Option<$ty>,)*
        }

        impl UnitRecord {
            fn take(world: &mut World, e: Entity, id: UnitId) -> UnitRecord {
                let mut unit = world.entity_mut(e);
                UnitRecord { id, $($field: unit.take::<$ty>(),)* }
            }

            fn spawn(self, world: &mut World) -> Entity {
                let mut unit = world.spawn_empty();
                $(if let Some(c) = self.$field {
                    unit.insert(c);
                })*
                unit.id()
            }

            /// How many components it carries.
            pub fn parts(&self) -> usize {
                0 $(+ self.$field.is_some() as usize)*
            }
        }
    };
}

// Every component a unit can have. A new component goes here too, or it's lost on crossing
// (`a_crossing_carries_every_component` checks).
record! {
    unit: Unit,
    transform: Transform,
    motion: Motion,
    body: Body,
    vitals: Vitals,
    stats: Stats,
    fighter: Fighter,
    auras: Auras,
    control: Control,
    threat: Threat,
    anim: Anim,
    home: Home,
    player: PlayerControl,
    summoned: Summoned,
    brain: Brain,
    appearance: Appearance,
    progress: Progress,
    credit: Credit,
    inventory: Inventory,
    skills: crate::crafting::Skills,
    quests: crate::quests::Quests,
    skinned: crate::crafting::Skinned,
    potion_cooldown: crate::consumables::PotionCooldown,
    trail: crate::rewind::Trail,
}

impl UnitRecord {
    /// How the region it just left sees it for the step it takes to arrive: a ghost owned by
    /// `region`, where it now lives.
    pub fn ghost(&self, region: u32, now: f64) -> Option<GhostState> {
        let (unit, transform, vitals) = (self.unit.clone()?, self.transform?, self.vitals?);
        let target = self.fighter.as_ref().and_then(|f| f.target);
        let guarding = self.fighter.as_ref().is_some_and(|f| f.guard_since.is_some());
        Some(GhostState {
            id: unit.id,
            region,
            level: unit.level,
            transform,
            vitals,
            anim: self.anim.unwrap_or_default(),
            velocity: self.motion.map_or(Vec3::ZERO, |m| m.observed),
            traversal: self.motion.map_or_else(Default::default, |m| crate::movement::traversal(&m)),
            target,
            drawn: target.is_some() || guarding || now - vitals.last_combat < crate::COMBAT_TIMEOUT,
            first: Some(Box::new(GhostFirst {
                unit,
                body: self.body.unwrap_or(DEFAULT_BODY),
                look: self.appearance.clone(),
                player: self.player.is_some(),
            })),
        })
    }
}

impl Sim {
    /// Region `id` of a world, owning `bounds` of `land`. Its units take IDs from its own block.
    pub fn region(land: Arc<world::Valley>, seed: u64, id: u32, bounds: Bounds) -> Sim {
        assert!(id < REGIONS, "unit IDs have room for {REGIONS} regions");
        let mut sim = Sim::on(land, seed);
        sim.world.insert_resource(Region { id, bounds: Some(bounds) });
        let mut units = sim.world.resource_mut::<Units>();
        assert!(units.next == 0, "a region is set before anything is in it");
        units.base = id * BLOCK;
        sim
    }

    /// Whether any unit with an ID from `region`'s block lives here, or is a ghost here.
    pub fn holds_block(&self, region: u32) -> bool {
        let units = self.world.resource::<Units>();
        let ids = region * BLOCK..=region * BLOCK + (BLOCK - 1);
        units.foreign.range(ids.clone()).next().is_some() || units.ghosts.range(ids).next().is_some() || (units.base == region * BLOCK && units.entities.iter().any(Option::is_some))
    }

    pub fn region_info(&self) -> Region {
        *self.world.resource::<Region>()
    }

    /// The players living here (not neighbours' ghosts), in ID order.
    pub fn players(&mut self) -> Vec<UnitId> {
        let mut q = self.world.query_filtered::<&Unit, (With<PlayerControl>, Without<Ghost>)>();
        let mut ids: Vec<UnitId> = q.iter(&self.world).map(|u| u.id).collect();
        ids.sort_unstable();
        ids
    }

    /// Puts a region that hasn't stepped yet at `tick`, so a layer opened later steps with the
    /// rest of its world.
    pub fn set_tick(&mut self, tick: u64) {
        let mut clock = self.world.resource_mut::<crate::Clock>();
        assert!(clock.tick == 0, "only a new region is set to a tick");
        clock.tick = tick;
        clock.time = tick as f64 / crate::TICK_RATE as f64;
    }

    /// Units at least [`HANDOFF`] past this region's edge and on one of `others`' ground, with
    /// where each goes, in ID order.
    pub fn departures(&mut self, others: &[(u32, Bounds)]) -> Vec<(UnitId, u32)> {
        let Some(own) = self.region_info().bounds else { return Vec::new() };
        let mut out: Vec<(UnitId, u32)> = crate::queries::with(&mut self.world, |q| &mut q.publish, |world, q| {
            q.iter(world)
                .filter_map(|(u, t, ..)| {
                    let p = t.xz();
                    if own.distance(p) < HANDOFF {
                        return None;
                    }
                    others.iter().find(|(_, b)| b.contains(p)).map(|&(r, _)| (u.id, r))
                })
                .collect()
        });
        out.sort_unstable_by_key(|d| d.0);
        out
    }

    /// Takes a unit out of this region with all its state, to [`import`](Sim::import) into
    /// another.
    pub fn export(&mut self, id: UnitId) -> Option<UnitRecord> {
        let e = self.world.resource::<Units>().entity(id)?;
        if self.world.get::<Ghost>(e).is_some() {
            return None;
        }
        let record = UnitRecord::take(&mut self.world, e, id);
        self.world.despawn(e);
        self.world.resource_mut::<Units>().forget(id);
        Some(record)
    }

    /// Adds units handed over by other regions, in ID order, each replacing its ghost here.
    pub fn import(&mut self, mut arrivals: Vec<UnitRecord>) {
        arrivals.sort_by_key(|r| r.id);
        for r in arrivals {
            let id = r.id;
            if let Some(g) = self.world.resource_mut::<Units>().ghosts.remove(&id) {
                self.world.despawn(g);
            }
            let e = r.spawn(&mut self.world);
            self.world.resource_mut::<Units>().put(id, e);
            crate::quests::arrived(&mut self.world, e);
        }
    }

    /// What this region publishes of its units within [`GHOST_BAND`] of `neighbour`'s ground,
    /// in ID order. Those in `known` (published to it last step) go without the parts that
    /// don't change, which the neighbour already has.
    pub fn ghosts_near(&mut self, neighbour: Bounds, known: &BTreeSet<UnitId>) -> Vec<GhostState> {
        let region = self.region_info().id;
        let t = self.time();
        crate::queries::with(&mut self.world, |q| &mut q.publish, |world, q| {
            let world: &World = world;
            world
                .resource::<Units>()
                .all()
                .filter_map(|e| q.get(world, e).ok())
                .filter(|(_, tr, ..)| neighbour.distance(tr.xz()) <= GHOST_BAND)
                .map(|(u, &transform, &vitals, body, anim, look, player, motion, f)| {
                    let target = f.and_then(|f| f.target);
                    let guarding = f.is_some_and(|f| f.guard_since.is_some());
                    let first = (!known.contains(&u.id)).then(|| {
                        Box::new(GhostFirst { unit: u.clone(), body: body.copied().unwrap_or(DEFAULT_BODY), look: look.cloned(), player })
                    });
                    GhostState {
                        id: u.id,
                        region,
                        level: u.level,
                        transform,
                        vitals,
                        anim: anim.copied().unwrap_or_default(),
                        velocity: motion.map_or(Vec3::ZERO, |m| m.observed),
                        traversal: motion.map_or_else(Default::default, crate::movement::traversal),
                        target,
                        drawn: target.is_some() || guarding || t - vitals.last_combat < crate::COMBAT_TIMEOUT,
                        first,
                    }
                })
                .collect()
        })
    }

    /// Replaces the neighbours' ghosts with what they published this step. A ghost of a unit
    /// that lives here (it just crossed in) is left out.
    pub fn set_ghosts(&mut self, mut ghosts: Vec<GhostState>) {
        ghosts.sort_unstable_by_key(|g| g.id);
        let old = std::mem::take(&mut self.world.resource_mut::<Units>().ghosts);
        let mut kept = std::collections::BTreeMap::new();
        let mut old = old.into_iter().peekable();
        for g in ghosts {
            let id = g.id;
            // Drop the old ghosts that are gone.
            while let Some(&(o, e)) = old.peek() {
                if o >= id {
                    break;
                }
                self.world.despawn(e);
                old.next();
            }
            let existing = old.next_if(|&(o, _)| o == id).map(|(_, e)| e);
            if kept.contains_key(&id) || (existing.is_none() && self.world.resource::<Units>().entity(id).is_some()) {
                continue;
            }
            let ghost = Ghost { region: g.region, velocity: g.velocity, traversal: g.traversal, target: g.target, drawn: g.drawn };
            let e = match (existing, g.first) {
                (Some(e), first) => {
                    // In place, so nothing moves between archetypes.
                    let w = &mut self.world;
                    set(w, e, g.transform);
                    set(w, e, g.vitals);
                    set(w, e, g.anim);
                    set(w, e, ghost);
                    match first {
                        Some(f) => {
                            set(w, e, f.unit);
                            if let Some(l) = f.look {
                                w.entity_mut(e).insert(l);
                            }
                        }
                        None => {
                            if let Some(mut u) = w.get_mut::<Unit>(e) {
                                u.level = g.level;
                            }
                        }
                    }
                    e
                }
                (None, Some(f)) => {
                    let mut ent = self.world.spawn((f.unit, f.body, g.transform, g.vitals, g.anim, ghost));
                    if let Some(l) = f.look {
                        ent.insert(l);
                    }
                    if f.player {
                        ent.insert(PlayerControl);
                    }
                    ent.id()
                }
                // Nothing to make it from: the neighbour thought it was here already.
                (None, None) => continue,
            };
            kept.insert(id, e);
        }
        for (_, e) in old {
            self.world.despawn(e);
        }
        self.world.resource_mut::<Units>().ghosts = kept;
    }

    /// Harm and help aimed at ghosts this step, for the regions that own them.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.world.resource_mut::<Outgoing>().0)
    }

    /// Lands what other regions aimed at this one's units, in the order given. A target that
    /// has since left or gone is missed.
    pub fn apply_requests(&mut self, requests: Vec<Request>) {
        for r in requests {
            let Some(te) = self.world.resource::<Units>().entity(r.target) else { continue };
            if self.world.get::<Ghost>(te).is_some() {
                continue;
            }
            effects::land_request(&mut self.world, te, r.landing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Specialty;

    #[test]
    fn a_crossing_carries_every_component() {
        let mut sim = Sim::new(1);
        let hero = sim.spawn_hero("Hero", Specialty::Mage, 5, Vec2::new(5.0, 10.0), 0.0);
        let wolf = sim.spawn_mob("wolf", Vec2::new(-38.0, 30.0)).unwrap();
        for id in [hero, wolf] {
            let e = sim.entity(id).unwrap();
            let had = sim.world.inspect_entity(e).unwrap().count();
            let record = sim.export(id).unwrap();
            assert_eq!(record.parts(), had, "unit {id} has a component the record doesn't carry");
        }
    }

    #[test]
    fn region_blocks_and_bounds() {
        let b = Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0));
        assert!(b.contains(Vec2::new(0.0, 9.9)) && !b.contains(Vec2::new(10.0, 5.0)));
        assert_eq!(b.distance(Vec2::new(13.0, 14.0)), 5.0);
        let mut sim = Sim::region(crate::brambleford_land(), 1, 3, b);
        let id = sim.spawn_player("Hero", Vec2::new(5.0, 5.0), 0.0);
        assert_eq!(id, 3 * BLOCK + 1);
        assert!(sim.entity(id).is_some());
    }
}
