use bevy_ecs::prelude::*;
use glam::Vec2;

use crate::movement::heading;
use crate::spatial::Spatial;
use crate::{Body, Brain, Clock, Fighter, Land, Mode, Motion, MoveIntent, PlayerControl, Rng, Transform, Unit, Units, Vitals};

/// How far a monster follows before giving up and walking home.
pub const LEASH: f32 = 45.0;
/// Monsters give up on a fight nobody has touched for this long.
const BORED: f64 = 12.0;

/// Monster decisions. Choices (who to chase, where to wander) are made four times a second;
/// steering toward the current goal happens every step.
pub fn think(
    clock: Res<Clock>,
    land: Res<Land>,
    spatial: Res<Spatial>,
    units: Res<Units>,
    mut rng: ResMut<Rng>,
    mut mobs: Query<(&Unit, &Transform, &mut Motion, &mut Brain, &mut Fighter, &Vitals, &Body), Without<PlayerControl>>,
    others: Query<(&Unit, &Transform, &Vitals)>,
) {
    let now = clock.time;
    for (u, t, mut m, mut brain, mut f, v, body) in &mut mobs {
        if v.dead {
            continue;
        }
        let here = Vec2::new(t.pos.x, t.pos.z);
        let deciding = now >= brain.next_think;
        if deciding {
            brain.next_think = now + 0.25;
        }
        match brain.mode {
            Mode::Return => {
                let to_home = brain.home - here;
                if to_home.length() < 1.5 {
                    brain.mode = Mode::Idle;
                    m.intent = MoveIntent::default();
                } else {
                    m.intent = MoveIntent { direction: to_home.normalize(), walk: false, face_yaw: None };
                }
            }
            Mode::Chase => {
                let target = f.target.and_then(|id| units.entity(id)).and_then(|e| others.get(e).ok());
                let keep = match target {
                    Some((_, tt, tv)) => {
                        !tv.dead
                            && (brain.home - Vec2::new(tt.pos.x, tt.pos.z)).length() < LEASH
                            && (here - brain.home).length() < LEASH
                            && now - v.last_combat.max(brain.engaged_at) < BORED
                    }
                    None => false,
                };
                if !keep {
                    give_up(&mut brain, &mut f, &mut m);
                    continue;
                }
                let (_, tt, _) = target.unwrap();
                let to = Vec2::new(tt.pos.x, tt.pos.z) - here;
                let reach = f.attack.range + body.radius - 0.3;
                if to.length() > reach {
                    m.intent = MoveIntent { direction: to.normalize(), walk: false, face_yaw: None };
                } else {
                    m.intent = MoveIntent { direction: Vec2::ZERO, walk: false, face_yaw: Some(heading(to)) };
                }
            }
            Mode::Idle => {
                if deciding {
                    // Notice the nearest enemy inside the aggro circle.
                    let seen = spatial
                        .near(here, brain.aggro_radius)
                        .into_iter()
                        .filter_map(|(id, p)| {
                            let (ou, _, ov) = others.get(units.entity(id)?).ok()?;
                            (ou.faction.is_enemy(u.faction) && !ov.dead).then_some((id, (p - here).length_squared()))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                    if let Some((id, _)) = seen {
                        engage(&mut brain, &mut f, id, now);
                        continue;
                    }
                }
                // Wander about home now and then.
                match brain.wander_target {
                    Some(w) if (w - here).length() > 0.5 && now < brain.next_wander + 12.0 => {
                        m.intent = MoveIntent { direction: (w - here).normalize(), walk: true, face_yaw: None };
                    }
                    Some(_) => {
                        brain.wander_target = None;
                        brain.next_wander = now + rng.range(4.0, 12.0) as f64;
                        m.intent = MoveIntent::default();
                    }
                    None if now >= brain.next_wander && brain.wander > 0.0 => {
                        let a = rng.range(0.0, std::f32::consts::TAU);
                        let r = rng.range(1.0, brain.wander);
                        let w = brain.home + Vec2::new(a.sin(), a.cos()) * r;
                        if !land.0.blocked(w, body.radius) {
                            brain.wander_target = Some(w);
                        }
                        brain.next_wander = now;
                    }
                    None => m.intent = MoveIntent::default(),
                }
            }
        }
    }
}

/// When a monster first thinks: decisions are spread over the 15 steps of a quarter-second.
pub fn think_offset(id: protocol::UnitId) -> f64 {
    (id % 15) as f64 / crate::TICK_RATE as f64
}

/// Starts a fight with `target`.
pub fn engage(brain: &mut Brain, f: &mut Fighter, target: protocol::UnitId, now: f64) {
    brain.mode = Mode::Chase;
    brain.engaged_at = now;
    brain.wander_target = None;
    f.target = Some(target);
    f.auto_attack = true;
}

fn give_up(brain: &mut Brain, f: &mut Fighter, m: &mut Motion) {
    brain.mode = Mode::Return;
    f.target = None;
    f.auto_attack = false;
    m.intent = MoveIntent::default();
}
