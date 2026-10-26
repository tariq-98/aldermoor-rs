//! What a [`Server`](crate::Server) steps: one `Sim`, or a [`Realm`] of regions (#208). Players
//! see the same either way; in a realm, each player's commands go to the region they live in
//! and their views come from it (its units and its neighbours' ghosts), across borders.

use crate::regions::Realm;
use crate::SEND_EVERY;
use protocol::wire::{Encoder, WireView};
use protocol::{CombatView, Command, Event, Snapshot, UnitId};
use sim::{NetFrame, Sim};
use std::collections::{HashMap, VecDeque};

pub(crate) enum World {
    // Boxed: both are big, and a server holds one World for its life.
    One(Box<One>),
    Many(Box<Many>),
}

pub(crate) struct One {
    pub sim: Sim,
    /// The last step's events.
    events: Vec<Event>,
    /// The last few steps' events, for network players who get a view every few steps.
    recent: VecDeque<Vec<Event>>,
}

pub(crate) struct Many {
    realm: Realm,
    /// Each region's last few steps' events.
    recent: HashMap<u32, VecDeque<Vec<Event>>>,
    /// Each region's last frame, to lend its next what hasn't changed.
    frames: HashMap<u32, NetFrame>,
}

impl World {
    pub fn one(sim: Sim) -> World {
        World::One(Box::new(One { sim, events: Vec::new(), recent: VecDeque::new() }))
    }

    pub fn many(realm: Realm) -> World {
        World::Many(Box::new(Many { realm, recent: HashMap::new(), frames: HashMap::new() }))
    }

    /// Adds a player at `at`. In a realm that's the region (and layer with room) there, or
    /// `None` when there's none.
    pub fn spawn_player(&mut self, name: &str, at: glam::Vec2, yaw: f32) -> Option<UnitId> {
        match self {
            World::One(w) => Some(w.sim.spawn_player(name, at, yaw)),
            World::Many(w) => w.realm.spawn_player(name, at),
        }
    }

    /// Adds a player as a saved record left them (#88). In a realm, in the region where they
    /// stood, or `None` when there's no room there.
    pub fn spawn_record(&mut self, record: &sim::CharacterRecord) -> Option<UnitId> {
        match self {
            World::One(w) => Some(w.sim.spawn_record(record)),
            World::Many(w) => {
                let id = w.realm.spawn_player(&record.name, glam::Vec2::from(record.at))?;
                let record = record.clone();
                w.realm.with_unit(id, move |sim, id| sim.restore(id, &record));
                Some(id)
            }
        }
    }

    /// Player `id`'s character as it stands, to keep.
    pub fn record(&self, id: UnitId) -> Option<sim::CharacterRecord> {
        match self {
            World::One(w) => w.sim.record(id),
            World::Many(w) => w.realm.with_unit(id, |sim, id| sim.record(id)),
        }
    }

    pub fn command(&mut self, id: UnitId, c: Command) {
        match self {
            World::One(w) => w.sim.command(id, c),
            World::Many(w) => w.realm.command(id, c),
        }
    }

    pub fn step(&mut self) {
        match self {
            World::One(w) => {
                w.sim.step();
                w.events = w.sim.take_events();
                keep(&mut w.recent, w.events.clone());
            }
            World::Many(w) => {
                w.realm.step();
                let regions: Vec<u32> = w.realm.regions();
                w.recent.retain(|r, _| regions.contains(r));
                for r in regions {
                    keep(w.recent.entry(r).or_default(), w.realm.events(r).to_vec());
                }
            }
        }
    }

    pub fn tick(&self) -> u64 {
        match self {
            World::One(w) => w.sim.tick(),
            World::Many(w) => w.realm.tick(),
        }
    }

    /// What `viewer` sees after this step, with this step's events where they are.
    pub fn snapshot(&mut self, viewer: UnitId) -> Snapshot {
        match self {
            World::One(w) => {
                let mut snapshot = w.sim.snapshot(Some(viewer));
                snapshot.events = w.events.clone();
                snapshot
            }
            World::Many(w) => w.realm.snapshot(viewer),
        }
    }

    pub fn combat_view(&self, viewer: UnitId) -> CombatView {
        match self {
            World::One(w) => w.sim.combat_view(Some(viewer)),
            World::Many(w) => w.realm.combat_view(viewer),
        }
    }

    /// Network players' views, each from the region they live in, with what happened there
    /// since their last view. Each player's encoder goes in and comes back beside their view.
    pub fn encode_views(&mut self, due: Vec<(UnitId, Encoder)>) -> Vec<(UnitId, Encoder, WireView)> {
        let World::Many(w) = self else {
            unreachable!("one sim encodes its views in place, across cores (`run`)")
        };
        let mut by_region: Vec<(u32, Vec<(UnitId, Encoder)>)> = Vec::new();
        for (id, encoder) in due {
            let region = w.realm.region_of(id);
            match by_region.iter_mut().find(|(r, _)| *r == region) {
                Some((_, list)) => list.push((id, encoder)),
                None => by_region.push((region, vec![(id, encoder)])),
            }
        }
        let mut views = Vec::new();
        for (region, list) in by_region {
            let since: Vec<Event> = w.recent.get(&region).map_or_else(Vec::new, |r| r.iter().flatten().cloned().collect());
            let prev = w.frames.remove(&region);
            let (frame, out) = w.realm.with(region, move |sim| {
                let frame = sim.net_frame(prev.as_ref());
                let out: Vec<_> = list
                    .into_iter()
                    .map(|(id, mut encoder)| {
                        let view = sim.encode_view(&frame, id, &mut encoder, &since);
                        (id, encoder, view)
                    })
                    .collect();
                (frame, out)
            });
            w.frames.insert(region, frame);
            views.extend(out);
        }
        views
    }
}

impl One {
    /// Every event of the last few steps, for the network players due a view.
    pub fn since(&self) -> Vec<Event> {
        self.recent.iter().flatten().cloned().collect()
    }
}

/// Keeps the last [`SEND_EVERY`] steps' events.
fn keep(recent: &mut VecDeque<Vec<Event>>, events: Vec<Event>) {
    recent.push_back(events);
    while recent.len() > SEND_EVERY as usize {
        recent.pop_front();
    }
}
