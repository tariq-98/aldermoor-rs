use bevy_ecs::prelude::*;
use glam::Vec2;
use protocol::Event;

use crate::{Brain, Clock, Fighter, Home, Mode, Motion, Outbox, Transform, Unit, Vitals, DT};

pub const ENDURANCE_MAX: f32 = 100.0;
pub const ENDURANCE_REGEN: f32 = 8.0;
/// Seconds after the last blow before a unit counts as out of combat.
pub const COMBAT_TIMEOUT: f64 = 6.0;
/// Monsters come back this long after dying.
pub const MONSTER_RESPAWN: f64 = 20.0;

/// Endurance and health regeneration, monsters healing on the way home, and monster respawns.
pub fn update_vitals(
    clock: Res<Clock>,
    mut out: ResMut<Outbox>,
    mut q: Query<(&Unit, &mut Vitals, &mut Transform, &mut Motion, &mut Fighter, &Home, Option<&mut Brain>)>,
) {
    let now = clock.time;
    for (u, mut v, mut t, mut m, mut f, home, brain) in &mut q {
        if v.dead {
            if let Some(at) = v.respawn_at {
                if now >= at {
                    *v = Vitals::new(v.max_health);
                    t.pos = home.pos;
                    t.yaw = home.yaw;
                    *m = Motion::new(m.run_speed);
                    f.target = None;
                    f.auto_attack = false;
                    if let Some(mut b) = brain {
                        *b = Brain::new(Vec2::new(home.pos.x, home.pos.z), b.aggro_radius, b.wander);
                        b.next_think = now + crate::brain::think_offset(u.id);
                    }
                    out.0.push(Event::Respawned { unit: u.id });
                }
            }
            continue;
        }
        if m.forced.is_none() {
            v.endurance = (v.endurance + ENDURANCE_REGEN * DT).min(ENDURANCE_MAX);
        }
        let returning = brain.as_ref().is_some_and(|b| b.mode == Mode::Return);
        if returning {
            // Giving up a fight heals a monster as it walks home, so pulling and dropping it
            // can't wear it down.
            v.health = (v.health + v.max_health * 0.25 * DT).min(v.max_health);
            v.invulnerable_until = now + 0.1;
        } else if now - v.last_combat > COMBAT_TIMEOUT {
            v.health = (v.health + v.max_health * 0.04 * DT).min(v.max_health);
        }
    }
}
