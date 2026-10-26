//! Network views built straight from the world (#286): once a step, everyone goes into one
//! [`NetFrame`], quantised for the wire, and each network player's view is picked out of it by
//! their own [`Encoder`]. Nothing is built twice, and no unit's strings are copied unless its
//! intro changed.

use protocol::wire::{Encoder, Frame, Sight, UnitRef, WireView};
use protocol::{Event, UnitId, UnitKind};

use crate::combat::view::{areas_seen, combat_all, player_state, Warning, VIEW_RANGE};
use crate::vitals::COMBAT_TIMEOUT;
use crate::{movement, region, Anim, Appearance, Clock, Faction, Fighter, Motion, Sim, Transform, Unit, Vitals};

/// Everyone in the region at one step, ready for any number of network players' views.
#[derive(Default)]
pub struct NetFrame {
    pub wire: Frame,
    /// Warned blows winding up.
    warnings: Vec<Warning>,
}

impl Sim {
    /// This step's frame. `prev`, the last one built, lends it whatever hasn't changed since.
    pub fn net_frame(&mut self, prev: Option<&NetFrame>) -> NetFrame {
        let clock = self.world.resource::<Clock>();
        let (tick, time) = (clock.tick, clock.time);
        let mut q = self.world.query::<(&Unit, &Transform, &Motion, &Vitals, &Anim, &Fighter, Option<&Appearance>)>();
        let mut ghosts = self.world.query::<(&Unit, &Transform, &Vitals, &Anim, &region::Ghost, Option<&Appearance>)>();
        let world = &self.world;
        // As `snapshot` has them, wherever they are.
        let gone = |u: &Unit, v: &Vitals| v.dead && time - v.died_at > 8.0 && u.kind != UnitKind::Player;
        let own = q.iter(world).filter(|(u, _, _, v, ..)| !gone(u, v)).map(|(u, t, m, v, a, f, look)| UnitRef {
            id: u.id,
            kind: u.kind,
            body: u.body,
            name: &u.name,
            look: look.unwrap_or(Appearance::bare()).shown(f.target.is_some() || f.guard_since.is_some() || time - v.last_combat < COMBAT_TIMEOUT),
            level: u.level,
            position: t.pos,
            velocity: m.observed,
            yaw: t.yaw,
            pose: a.pose,
            pose_time: (time - a.since) as f32,
            traversal: movement::traversal(m),
            health: v.health,
            max_health: v.max_health,
            hostile: u.faction == Faction::Hostile,
            target: f.target,
        });
        let theirs = ghosts.iter(world).filter(|(u, _, v, ..)| !gone(u, v)).map(|(u, t, v, a, g, look)| UnitRef {
            id: u.id,
            kind: u.kind,
            body: u.body,
            name: &u.name,
            look: look.unwrap_or(Appearance::bare()).shown(g.drawn),
            level: u.level,
            position: t.pos,
            velocity: g.velocity,
            yaw: t.yaw,
            pose: a.pose,
            pose_time: (time - a.since) as f32,
            traversal: g.traversal,
            health: v.health,
            max_health: v.max_health,
            hostile: u.faction == Faction::Hostile,
            target: g.target,
        });
        let (combat, warnings) = combat_all(world);
        let wire = Frame::build(tick, time, own.chain(theirs), combat, prev.map(|p| &p.wire));
        NetFrame { wire, warnings }
    }

    /// `viewer`'s view of `frame` for the wire, with `events` (those since their last view).
    /// The same as encoding their `snapshot` and `combat_view` from the same step.
    pub fn encode_view(&self, frame: &NetFrame, viewer: UnitId, encoder: &mut Encoder, events: &[Event]) -> WireView {
        let e = self.entity(viewer);
        let at = e.and_then(|e| self.world.get::<Transform>(e)).map(|t| t.pos);
        let side = e.and_then(|e| self.world.get::<Unit>(e)).map_or(Faction::Player, |u| u.faction);
        let player = player_state(&self.world, viewer);
        let (projectiles, areas) = areas_seen(&self.world, at, side, &frame.warnings);
        let sight = Sight { at, range: Some(VIEW_RANGE), player: player.as_ref(), projectiles: &projectiles, areas: &areas, events };
        encoder.encode_frame(&frame.wire, &sight)
    }
}
