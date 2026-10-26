use std::hash::{BuildHasherDefault, Hasher};

use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::UnitId;

use crate::movement::{self, Crowd};
use crate::{Body, Clock, Faction, Land, Motion, PlayerControl, Transform, Unit, Vitals};

const CELL: f32 = 8.0;

type Cells = std::collections::HashMap<(i32, i32), Vec<(UnitId, Vec2)>, BuildHasherDefault<CellHash>>;

/// Who is near what: living units bucketed into 8 m cells, rebuilt every step. Units on the
/// players' side are also bucketed on their own, since that's all a monster looks for: a crowd
/// of idle monsters never has to sift through each other.
#[derive(Resource, Default)]
pub struct Spatial {
    cells: Cells,
    players: Cells,
}

/// A quick hash for cell keys (the multiply-and-rotate one rustc uses). Cell keys aren't chosen
/// by anyone hostile, so the default hasher's flood resistance would only cost time; every unit
/// lands in a cell each step and every nearby search looks a few up.
#[derive(Default)]
struct CellHash(u64);

impl Hasher for CellHash {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u32(b as u32);
        }
    }

    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl Spatial {
    fn key(p: Vec2) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
    }

    /// Units whose centres lie within `radius` of `p`, in ID order.
    pub fn near(&self, p: Vec2, radius: f32) -> Vec<(UnitId, Vec2)> {
        Self::near_in(&self.cells, p, radius)
    }

    /// Like [`Spatial::near`], but only units that could be enemies of `side`: for monsters,
    /// just the players' side. Everything it leaves out is on `side`'s own.
    pub fn near_foes(&self, p: Vec2, radius: f32, side: Faction) -> Vec<(UnitId, Vec2)> {
        match side {
            Faction::Hostile => Self::near_in(&self.players, p, radius),
            Faction::Player => self.near(p, radius),
        }
    }

    /// Whether any unit on the players' side stands within `radius` of `p`.
    pub fn player_side_near(&self, p: Vec2, radius: f32) -> bool {
        let (x0, z0) = Self::key(p - Vec2::splat(radius));
        let (x1, z1) = Self::key(p + Vec2::splat(radius));
        let r2 = radius * radius;
        let close = |c: &Vec<(UnitId, Vec2)>| c.iter().any(|(_, q)| (*q - p).length_squared() <= r2);
        // The players' cells are few; walking them all beats looking up every cell in a wide
        // radius until there are as many of them as there are cells to look up.
        if self.players.len() < ((x1 - x0 + 1) * (z1 - z0 + 1)) as usize {
            return self.players.values().any(close);
        }
        (z0..=z1).any(|z| (x0..=x1).any(|x| self.players.get(&(x, z)).is_some_and(close)))
    }

    fn near_in(cells: &Cells, p: Vec2, radius: f32) -> Vec<(UnitId, Vec2)> {
        let (x0, z0) = Self::key(p - Vec2::splat(radius));
        let (x1, z1) = Self::key(p + Vec2::splat(radius));
        let r2 = radius * radius;
        let mut out = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                if let Some(cell) = cells.get(&(x, z)) {
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
    for cell in spatial.players.values_mut() {
        cell.clear();
    }
    let separating = Crowd::due(clock.tick);
    if separating {
        crowd.clear();
    }
    for (e, u, t, v, m, body, player) in &q {
        let p = Vec2::new(t.pos.x, t.pos.z);
        if separating {
            crowd.add(e, p, body.map_or(0.4, |b| b.radius), !v.dead && m.is_some_and(|m| m.forced.is_none() && m.hang.is_none()), player.then_some(u.faction));
        }
        if v.dead {
            continue;
        }
        spatial.cells.entry(Spatial::key(p)).or_default().push((u.id, p));
        if u.faction == Faction::Player {
            spatial.players.entry(Spatial::key(p)).or_default().push((u.id, p));
        }
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
