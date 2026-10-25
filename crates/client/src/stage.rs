//! A staged fight for reviewing combat effects (`--effects-review`). The showcase lineup wears
//! auras and casts, missiles of every look cross in front of it, and warnings, fields and
//! flashes lie on the ground. Everything follows scene time alone, so a capture at a given frame
//! always shows the same thing, and the effects' GPU cost is the difference from `--showcase`,
//! which draws the same scene without them.
use glam::Vec3;
use protocol::{AreaView, AuraView, CastBar, CombatView, Event, ProjectileView, School, Shape};
use protocol::{UnitCombat, UnitState};
use std::f32::consts::FRAC_PI_2;

/// Missiles crossing in front of the lineup: look, school, speed (m/s), height and lane (z).
/// Speeds are the content's.
const LANES: [(&str, School, f32, f32, f32); 8] = [
    ("fire_bolt", School::Fire, 26., 1.5, 25.),
    ("frost_lance", School::Frost, 30., 1.9, 24.4),
    ("shadow_bolt", School::Shadow, 24., 1.2, 25.6),
    ("holy_bolt", School::Holy, 28., 2.2, 24.),
    ("arcane_missile", School::Arcane, 32., 1.7, 26.2),
    ("arcane_spark", School::Arcane, 34., 1., 24.8),
    ("arrow", School::Physical, 40., 1.4, 23.6),
    ("axe", School::Physical, 22., 1.3, 26.8),
];
/// Where missiles leave and land (x), and the pause before a lane fires again.
const FROM: f32 = -7.;
const TO: f32 = 9.;
const GAP: f32 = 0.5;

/// What each of the showcase lineup wears: the male, elf, dwarf, healer, goblin and wolf.
const AURAS: [&[&str]; 6] = [
    &["shield_wall", "might"],
    &["swiftness"],
    &["stunned", "bleeding"],
    &["regeneration"],
    &["burning", "feared"],
    &["frozen", "dazed"],
];
/// Who casts what, and for how long (the content's times): lineup index, ability, seconds,
/// whether it's a channel.
const CASTS: [(usize, &str, f32, bool); 3] = [
    (1, "staff_frostlance", 1.4, false),
    (3, "healing_light", 1.5, false),
    (4, "necro_drain", 2., true),
];

/// A repeating warning: shape, where, facing, who it threatens, its period and phase.
struct Warning {
    id: u32,
    shape: Shape,
    at: (f32, f32),
    yaw: f32,
    hostile: bool,
    period: f32,
    phase: f32,
}
const WARNINGS: [Warning; 3] = [
    Warning {
        id: 1,
        shape: Shape::Circle { radius: 2. },
        at: (-2.5, 25.6),
        yaw: 0.,
        hostile: true,
        period: 2.4,
        phase: 0.,
    },
    // From the wolf, towards the camera's left.
    Warning {
        id: 2,
        shape: Shape::Cone {
            range: 4.5,
            angle: 1.,
        },
        at: (8., 24.),
        yaw: -0.64,
        hostile: true,
        period: 1.6,
        phase: 0.5,
    },
    // A friend's charge: it threatens others, so it takes the friendly mark.
    Warning {
        id: 3,
        shape: Shape::Line {
            length: 6.,
            width: 1.2,
        },
        at: (-3., 28.5),
        yaw: FRAC_PI_2,
        hostile: false,
        period: 2.,
        phase: 1.1,
    },
];
/// A lasting combo field, and whether it hurts the viewer.
struct Field {
    id: u32,
    radius: f32,
    at: (f32, f32),
    field: &'static str,
    school: School,
    hostile: bool,
}
const FIELDS: [Field; 2] = [
    Field {
        id: 4,
        radius: 1.8,
        at: (2.5, 26.5),
        field: "fire",
        school: School::Fire,
        hostile: true,
    },
    Field {
        id: 5,
        radius: 1.5,
        at: (6.5, 28.),
        field: "ice",
        school: School::Frost,
        hostile: false,
    },
];
const COMBO_AT: (f32, f32) = (0.3, 29.5);
const COMBO_EVERY: f32 = 3.;
/// The ground skill being aimed: where and its radius.
const AIM: ((f32, f32), f32) = ((-5.5, 25.), 1.5);

#[derive(Default)]
pub struct Stage {
    /// Scene time of the previous frame, for the events between frames.
    last: Option<f32>,
}

/// One frame of the staged fight: the view, what happened since the last frame (each with the
/// scene time it happened at), and the aiming circle.
pub struct Frame {
    pub fight: CombatView,
    pub events: Vec<(Event, f32)>,
    pub aim: (Vec3, f32),
}

impl Stage {
    pub fn frame(
        &mut self,
        time: f32,
        units: &[UnitState],
        ground: &dyn Fn(f32, f32) -> f32,
    ) -> Frame {
        let last = self.last.replace(time);
        let on = |(x, z): (f32, f32)| Vec3::new(x, ground(x, z), z);
        let mut fight = CombatView {
            tick: (time * 60.) as u64,
            ..Default::default()
        };
        let mut events = Vec::new();
        // Each lane fires on its own period, offset so the lanes don't move together.
        for (lane, &(visual, school, speed, height, z)) in LANES.iter().enumerate() {
            let flight = (TO - FROM) / speed;
            let period = flight + GAP;
            let offset = period * (lane as f32 * 0.37).fract();
            let shot = ((time + offset) / period).floor();
            let launched = shot * period - offset;
            let id = (lane as u32 + 1) * 100_000 + shot as u32;
            let from = units.get(lane % units.len().max(1)).map_or(0, |u| u.id);
            let elapsed = time - launched;
            if elapsed < flight {
                // A first frame sees missiles already in the air: they were launched earlier.
                if last.is_none_or(|l| l < launched) {
                    events.push((
                        Event::ProjectileLaunched {
                            id,
                            from,
                            visual: visual.into(),
                        },
                        launched,
                    ));
                }
                fight.projectiles.push(ProjectileView {
                    id,
                    position: Vec3::new(FROM + speed * elapsed, height, z),
                    velocity: Vec3::new(speed, 0., 0.),
                    visual: visual.into(),
                    school,
                });
            } else if let Some(l) = last
                && l < launched + flight
            {
                events.push((
                    Event::ProjectileEnded {
                        id,
                        at: Vec3::new(TO, height, z),
                        hit: None,
                    },
                    launched + flight,
                ));
            }
        }
        for w in &WARNINGS {
            let cycle = (time + w.phase) / w.period;
            let center = on(w.at);
            fight.areas.push(AreaView {
                id: w.id,
                warning: true,
                shape: w.shape,
                center,
                yaw: w.yaw,
                elapsed: cycle.fract() * w.period,
                duration: w.period,
                hostile: w.hostile,
                school: School::Physical,
                field: None,
            });
            // It lands as it wraps round; circles flash where they land.
            let landed = cycle.floor() * w.period - w.phase;
            if let (Some(l), Shape::Circle { radius }) = (last, w.shape)
                && l < landed
            {
                events.push((
                    Event::AreaImpact {
                        id: w.id,
                        at: center,
                        radius,
                        school: School::Physical,
                    },
                    landed,
                ));
            }
        }
        for f in &FIELDS {
            fight.areas.push(AreaView {
                id: f.id,
                warning: false,
                shape: Shape::Circle { radius: f.radius },
                center: on(f.at),
                yaw: 0.,
                elapsed: time,
                duration: 600.,
                hostile: f.hostile,
                school: f.school,
                field: Some(f.field.into()),
            });
        }
        let combo = (time / COMBO_EVERY).floor() * COMBO_EVERY;
        if let Some(l) = last
            && l < combo
        {
            events.push((
                Event::Combo {
                    unit: units.first().map_or(0, |u| u.id),
                    name: "Fire field blast".into(),
                    at: on(COMBO_AT),
                },
                combo,
            ));
        }
        for (i, u) in units.iter().enumerate() {
            let cast = CASTS
                .iter()
                .find(|c| c.0 == i)
                .map(|&(_, ability, duration, channel)| CastBar {
                    ability: ability.into(),
                    name: ability.into(),
                    elapsed: (time + i as f32 * 0.3) % duration,
                    duration,
                    channel,
                    interruptible: true,
                });
            let auras = AURAS.get(i).copied().unwrap_or_default();
            fight.units.push(UnitCombat {
                id: u.id,
                action: Default::default(),
                action_time: 0.,
                cast,
                // No duration: they neither fade in nor out.
                auras: auras
                    .iter()
                    .map(|&key| AuraView {
                        key: key.into(),
                        name: key.into(),
                        stacks: 1,
                        remaining: 0.,
                        duration: 0.,
                        debuff: false,
                    })
                    .collect(),
                resource_kind: Default::default(),
                resource: 0.,
                max_resource: 0.,
                guarding: false,
                downed: false,
                defiance: None,
            });
        }
        Frame {
            fight,
            events,
            aim: (on(AIM.0), AIM.1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn lineup() -> Vec<UnitState> {
        (0..6)
            .map(|i| UnitState {
                id: 10 + i,
                kind: protocol::UnitKind::Player,
                body: "player".into(),
                name: "Review".into(),
                level: 1,
                position: Vec3::new(-4. + i as f32 * 2.8, 0., 22.),
                velocity: Vec3::ZERO,
                yaw: 0.25,
                pose: protocol::Pose::Idle,
                pose_time: 0.,
                health: 100.,
                max_health: 100.,
                hostile: false,
                target: None,
                look: Default::default(),
            })
            .collect()
    }

    #[test]
    fn every_look_in_the_effects_table_is_on_stage() {
        let table = assets::Catalog::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .unwrap()
            .effects
            .expect("effects.json");
        let flown: HashSet<_> = LANES.iter().map(|l| l.0).collect();
        for visual in table.projectiles.keys() {
            assert!(
                flown.contains(visual.as_str()),
                "{visual} never flies in the review"
            );
        }
        let kinds: HashSet<_> = AURAS
            .iter()
            .flat_map(|keys| keys.iter())
            .map(|key| format!("{:?}", table.auras[*key].kind))
            .collect();
        assert_eq!(
            kinds.len(),
            3,
            "a shell, a ring and a halo are all worn: {kinds:?}"
        );
        for (_, ability, ..) in CASTS {
            assert!(
                table.casts.contains_key(ability),
                "{ability} doesn't glow when cast"
            );
        }
    }

    #[test]
    fn missiles_launch_once_and_land_after_their_flight() {
        let units = lineup();
        let mut stage = Stage::default();
        let mut launched = HashMap::new();
        let mut landed = HashSet::new();
        let (mut impacts, mut combos) = (0, 0);
        for frame in 0..600 {
            let f = stage.frame(frame as f32 / 60., &units, &|_, _| 0.);
            for (e, at) in &f.events {
                match e {
                    Event::ProjectileLaunched { id, .. } => {
                        assert!(launched.insert(*id, *at).is_none(), "{id} launched twice")
                    }
                    Event::ProjectileEnded { id, .. } => {
                        assert!(*at > launched[id], "{id} landed before it left");
                        assert!(landed.insert(*id), "{id} landed twice");
                    }
                    Event::AreaImpact { .. } => impacts += 1,
                    Event::Combo { .. } => combos += 1,
                    _ => {}
                }
                assert!(*at <= frame as f32 / 60. + 1e-4, "events are never early");
            }
            // Whatever is in flight was announced.
            for p in &f.fight.projectiles {
                assert!(launched.contains_key(&p.id));
            }
        }
        // Ten seconds: each lane fires several times, the circle lands four times.
        assert!(landed.len() >= 8 * 4, "{}", landed.len());
        assert_eq!((impacts, combos), (4, 3));
    }

    #[test]
    fn a_frame_is_the_same_whenever_it_is_drawn() {
        let units = lineup();
        let mut a = Stage::default();
        let mut b = Stage::default();
        for frame in 0..90 {
            a.frame(frame as f32 / 60., &units, &|_, _| 0.);
        }
        let ground = |x: f32, z: f32| 0.1 * x + 0.05 * z;
        assert_eq!(
            a.frame(1.5, &units, &ground).fight,
            b.frame(1.5, &units, &ground).fight
        );
    }
}
