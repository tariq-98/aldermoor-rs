//! Lag compensation for player-against-player hit tests (#169, docs/SERVER.md, "PvP hit checks
//! and lag compensation").
//!
//! A player sees others where they stood a little while ago. Each player keeps a [`Trail`] of
//! where they stood over the last second, and remembers how far behind their own view is (sent
//! as `Command::Viewing`). When one player's shot, cone or ground attack is tested against
//! another, the target is tested where the shooter saw it, up to [`MAX_REWIND`] steps back. Only
//! players carry a trail, so monsters are never rewound and the record costs nothing for them.

use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::combat::HitSource;
use crate::{Clock, Transform};

/// Steps of history kept: one second.
pub const HISTORY: usize = 60;
/// The furthest a hit test looks back: 250 ms. A sender further behind is tested at this.
pub const MAX_REWIND: u64 = 15;

/// Where a player stood over the last [`HISTORY`] steps, and how far behind their view is.
#[derive(Component, Clone, Debug)]
pub struct Trail {
    at: [Vec3; HISTORY],
    /// The first tick recorded, so a new trail isn't read before it has history.
    since: u64,
    /// Steps between the tick this player last said they were viewing and the step it arrived.
    lag: u64,
}

impl Default for Trail {
    fn default() -> Trail {
        Trail { at: [Vec3::ZERO; HISTORY], since: u64::MAX, lag: 0 }
    }
}

impl Trail {
    /// Steps behind the server the sender's hit tests are checked, at most [`MAX_REWIND`].
    pub fn lag(&self) -> u64 {
        self.lag
    }
}

/// `Command::Viewing`: the sender is looking at `tick`.
pub(crate) fn viewing(world: &mut World, e: Entity, tick: u64) {
    let now = world.resource::<Clock>().tick;
    if let Some(mut t) = world.get_mut::<Trail>(e) {
        t.lag = now.saturating_sub(tick).min(MAX_REWIND);
    }
}

/// Notes where every player stands this step.
pub fn record(world: &mut World, mut q: Local<Option<QueryState<(&Transform, &mut Trail)>>>) {
    let tick = world.resource::<Clock>().tick;
    let q = q.get_or_insert_with(|| world.query());
    for (t, mut trail) in q.iter_mut(world) {
        trail.at[(tick % HISTORY as u64) as usize] = t.pos;
        trail.since = trail.since.min(tick);
    }
}

/// Where `target` stood when `src`'s sender saw it, if this hit test is rewound: both are
/// players and the sender's view is behind.
pub fn seen_at(world: &World, src: &HitSource, target: Entity) -> Option<Vec3> {
    let trail = world.get::<Trail>(target)?;
    let shooter = crate::combat::entity(world, src.unit)?;
    let lag = world.get::<Trail>(shooter)?.lag;
    let tick = world.resource::<Clock>().tick.checked_sub(lag).filter(|_| lag > 0)?;
    (tick >= trail.since).then(|| trail.at[(tick % HISTORY as u64) as usize])
}
