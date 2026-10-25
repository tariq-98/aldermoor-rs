use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::UnitId;

use crate::movement::{self, Crowd};
use crate::{Body, Clock, Land, Motion, PlayerControl, Transform, Unit, Vitals};

const CELL: f32 = 8.0;

/// Who is near what: living units bucketed into 8 m cells, rebuilt every step.
#[derive(Resource, Default)]
pub struct Spatial {
    cells: std::collections::HashMap<(i32, i32), Vec<(UnitId, Vec2)>>,
}

impl Spatial {
    fn key(p: Vec2) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
    }

    /// Units whose centres lie within `radius` of `p`, in ID order.
    pub fn near(&self, p: Vec2, radius: f32) -> Vec<(UnitId, Vec2)> {
        let (x0, z0) = Self::key(p - Vec2::splat(radius));
        let (x1, z1) = Self::key(p + Vec2::splat(radius));
        let r2 = radius * radius;
        let mut out = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(cell) = self.cells.get(&(x, z)) {
                    out.extend(cell.iter().filter(|(_, q)| (*q - p).length_squared() <= r2).copied());
                }
            }
        }
        out.sort_unstable_by_key(|(id, _)| *id);
        out
    }
}

/// Also keeps bodies apart (`movement::separate`) every other step, gathering them in the same
/// pass. The grid keeps where units stood before that nudge, a few centimetres at most.
pub fn rebuild(
    mut spatial: ResMut<Spatial>,
    mut crowd: Local<Crowd>,
    clock: Res<Clock>,
    land: Res<Land>,
    mut q: Query<(Entity, &Unit, &mut Transform, &Vitals, Option<&Motion>, Option<&Body>, Has<PlayerControl>)>,
) {
    for cell in spatial.cells.values_mut() {
        cell.clear();
    }
    let separating = Crowd::due(clock.tick);
    if separating {
        crowd.clear();
    }
    for (e, u, t, v, m, body, player) in &q {
        let p = Vec2::new(t.pos.x, t.pos.z);
        if separating {
            crowd.add(e, p, body.map_or(0.4, |b| b.radius), !v.dead && m.is_some_and(|m| m.forced.is_none()), player);
        }
        if v.dead {
            continue;
        }
        spatial.cells.entry(Spatial::key(p)).or_default().push((u.id, p));
    }
    if separating {
        movement::separate(&mut crowd);
        for (e, from, r, push) in crowd.pushes() {
            if let Ok((_, _, mut t, _, Some(m), _, _)) = q.get_mut(e) {
                movement::shift(&land.0, &mut t, m, from, push, r);
            }
        }
    }
}
