//! A staged fight for reviewing combat effects (`--effects-review`). The showcase lineup wears
//! auras and casts, missiles of every look cross in front of it, and warnings, fields and
//! flashes lie on the ground. Everything follows scene time alone, so a capture at a given frame
//! always shows the same thing, and the effects' GPU cost is the difference from `--showcase`,
//! which draws the same scene without them.
//!
//! The reel (`--effects-reel`) shows the same effects one shot at a time instead, close up and
//! captioned, missiles again in slow motion.
use glam::Vec3;
use protocol::{AreaView, AuraView, CastBar, CombatView, Event, ProjectileView, School, Shape};
use protocol::{UnitCombat, UnitState};
use std::f32::consts::FRAC_PI_2;

/// Missiles crossing in front of the lineup: look, school, speed (m/s), height above the
/// ground and lane (z). Speeds are the content's.
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

/// How fast a reel shot's slow replay runs.
const SLOW: f32 = 0.25;

/// What a reel shot puts on stage.
#[derive(Clone, Copy)]
enum On {
    /// A missile crossing the frame (its look, school and speed), at full speed and again in
    /// the shot's slow replay.
    Missile(&'static str, School, f32),
    /// Auras worn by the lineup, from its left.
    Auras(&'static [&'static str]),
    /// Abilities being cast by the lineup, from its left, and their cast times.
    Casts(&'static [(&'static str, f32)]),
    /// A field in front of the lineup: its combo field (none for a school's own), its school
    /// and whether it threatens the viewer.
    Field(Option<&'static str>, School, bool),
    /// A warning (hostile or not) that lands after its seconds, a circle in a blow of its
    /// school.
    Warning(Shape, bool, School, f32),
    Combo,
}

/// One shot of the reel: an effect, or a few that differ only in colour, close up.
pub struct Shot {
    pub caption: &'static str,
    pub seconds: f32,
    /// The camera's target (x, z, height over the ground) and the eye's offset from it.
    pub camera: (f32, f32, f32, [f32; 3]),
    /// Seconds into the shot when its slow replay starts.
    slow_from: Option<f32>,
    on: On,
}

impl Shot {
    /// Effect seconds `local` real seconds into the shot.
    fn clock(&self, local: f32) -> f32 {
        match self.slow_from {
            Some(s) if local > s => s + (local - s) * SLOW,
            _ => local,
        }
    }
}

/// Side on to the missiles' lane, close.
const LANE_VIEW: (f32, f32, f32, [f32; 3]) = (0., 25., 1.35, [0.25, 0.2, 2.5]);
/// Where the reel's missiles fly: from and to (x) along z, this high over the ground.
const LANE: (f32, f32, f32, f32) = (-6., 0.9, 25., 1.4);
/// The first of the lineup, close.
const ONE_VIEW: (f32, f32, f32, [f32; 3]) = (-4., 22., 1., [0.9, 0.35, 3.9]);
/// The first three of the lineup.
const THREE_VIEW: (f32, f32, f32, [f32; 3]) = (-1.2, 22., 1., [0.4, 0.55, 5.6]);
/// The ground in front of the lineup, from above, and the middle of what lies on it.
const GROUND_VIEW: (f32, f32, f32, [f32; 3]) = (0.5, 25.6, 0.3, [0., 3.4, 5.4]);
const GROUND_AT: (f32, f32) = (0.5, 25.6);

const fn missile(caption: &'static str, visual: &'static str, school: School, speed: f32) -> Shot {
    Shot {
        caption,
        seconds: 3.7,
        camera: LANE_VIEW,
        slow_from: Some(1.1),
        on: On::Missile(visual, school, speed),
    }
}
const fn worn(caption: &'static str, auras: &'static [&'static str]) -> Shot {
    Shot {
        caption,
        seconds: 2.6,
        camera: if auras.len() == 1 {
            ONE_VIEW
        } else {
            THREE_VIEW
        },
        slow_from: None,
        on: On::Auras(auras),
    }
}
const fn field(
    caption: &'static str,
    field: Option<&'static str>,
    school: School,
    hostile: bool,
) -> Shot {
    Shot {
        caption,
        seconds: 2.8,
        camera: GROUND_VIEW,
        slow_from: None,
        on: On::Field(field, school, hostile),
    }
}
const fn warning(caption: &'static str, shape: Shape, hostile: bool, school: School) -> Shot {
    Shot {
        caption,
        seconds: 3.1,
        camera: GROUND_VIEW,
        slow_from: None,
        on: On::Warning(shape, hostile, school, 2.),
    }
}
const BLAST: Shape = Shape::Circle { radius: 1.8 };

const REEL: &[Shot] = &[
    missile("Fire bolt", "fire_bolt", School::Fire, 26.),
    missile("Frost lance", "frost_lance", School::Frost, 30.),
    missile("Shadow bolt", "shadow_bolt", School::Shadow, 24.),
    missile("Holy bolt", "holy_bolt", School::Holy, 28.),
    missile("Arcane missile", "arcane_missile", School::Arcane, 32.),
    missile("Arcane spark", "arcane_spark", School::Arcane, 34.),
    missile("Arrow", "arrow", School::Physical, 40.),
    missile("Thrown axe", "axe", School::Physical, 22.),
    Shot {
        caption: "Casting fire, frost and holy",
        seconds: 3.,
        camera: THREE_VIEW,
        slow_from: None,
        on: On::Casts(&[
            ("staff_firebolt", 1.5),
            ("staff_frostlance", 1.4),
            ("holy_bolt", 1.5),
        ]),
    },
    Shot {
        caption: "Casting shadow and nature",
        seconds: 3.,
        camera: THREE_VIEW,
        slow_from: None,
        on: On::Casts(&[("necro_bolt", 1.5), ("necro_plague", 1.5)]),
    },
    worn("Shield wall", &["shield_wall"]),
    worn(
        "Arcane ward, frost barrier, bone armour",
        &["arcane_ward", "frost_barrier", "bone_armor"],
    ),
    worn(
        "Shield of faith, divine shield, unbreakable",
        &["shield_of_faith", "divine_shield", "unbreakable"],
    ),
    worn("Stunned", &["stunned"]),
    worn(
        "Feared, asleep, silenced",
        &["feared", "asleep", "silenced"],
    ),
    worn("Burning", &["burning"]),
    worn(
        "Bleeding, poisoned, chilled",
        &["bleeding", "poisoned", "chilled"],
    ),
    worn("Frozen", &["frozen"]),
    worn(
        "Rooted, crippled, taunted",
        &["rooted", "crippled", "taunted"],
    ),
    worn(
        "Vulnerable, weakness, riposte",
        &["vulnerable", "weakness", "riposte"],
    ),
    worn("Vanished", &["vanished"]),
    worn("Might, fury, quickness", &["might", "fury", "quickness"]),
    worn(
        "Protection, regeneration, guardian",
        &["protection", "regeneration", "guardian"],
    ),
    worn(
        "Arcane power, lich form, evasion",
        &["arcane_power", "lich_form", "evasion"],
    ),
    field("Fire field", Some("fire"), School::Fire, true),
    field("Ice field", Some("ice"), School::Frost, false),
    field("Light field", Some("light"), School::Holy, false),
    field("Water field", Some("water"), School::Frost, false),
    field("Poison field", Some("poison"), School::Nature, true),
    field("Lightning field", Some("lightning"), School::Arcane, true),
    field("Arcane ground", None, School::Arcane, true),
    field("Shadow ground", None, School::Shadow, true),
    field("Nature ground", None, School::Nature, false),
    warning("Enemy blow: physical", BLAST, true, School::Physical),
    warning("Enemy blow: fire", BLAST, true, School::Fire),
    warning("Enemy blow: frost", BLAST, true, School::Frost),
    warning("Enemy blow: holy", BLAST, true, School::Holy),
    warning("Enemy blow: shadow", BLAST, true, School::Shadow),
    warning("Enemy blow: arcane", BLAST, true, School::Arcane),
    warning("Enemy blow: nature", BLAST, true, School::Nature),
    warning(
        "Enemy cone",
        Shape::Cone {
            range: 4.,
            angle: 1.,
        },
        true,
        School::Physical,
    ),
    warning(
        "Friendly charge",
        Shape::Line {
            length: 5.,
            width: 1.2,
        },
        false,
        School::Physical,
    ),
    Shot {
        caption: "Combo finisher",
        seconds: 2.4,
        camera: GROUND_VIEW,
        slow_from: None,
        on: On::Combo,
    },
];

/// The reel's length in seconds.
pub fn reel_seconds() -> f32 {
    REEL.iter().map(|s| s.seconds).sum()
}

/// The reel's shot at `time` (it loops): its index, how far into it, and the effects' clock
/// when it started (slow replays run it slower than real time).
fn shot_at(time: f32) -> (usize, &'static Shot, f32, f32) {
    let length = reel_seconds();
    let paced: f32 = REEL.iter().map(|s| s.clock(s.seconds)).sum();
    let lap = (time.max(0.) / length).floor();
    let mut local = time.max(0.) - lap * length;
    let mut start = lap * paced;
    for (i, s) in REEL.iter().enumerate() {
        if local < s.seconds {
            return (i, s, local, start);
        }
        local -= s.seconds;
        start += s.clock(s.seconds);
    }
    (REEL.len() - 1, &REEL[REEL.len() - 1], local, start)
}

/// The reel's shot at `time`.
pub fn shot(time: f32) -> &'static Shot {
    shot_at(time).1
}

#[derive(Default)]
pub struct Stage {
    /// Scene time of the previous frame (the effects' clock in the reel), for the events
    /// between frames.
    last: Option<f32>,
    /// Shows the reel, and which shot the last frame showed.
    reel: bool,
    shot: Option<usize>,
}

/// One frame of the staged fight: the view, what happened since the last frame (each with the
/// effects' clock when it happened), the aiming circle, the effects' clock (scene time but in
/// the reel's slow replays) and whether the reel just cut to a new shot.
pub struct Frame {
    pub fight: CombatView,
    pub events: Vec<(Event, f32)>,
    pub aim: Option<(Vec3, f32)>,
    pub clock: f32,
    pub cut: bool,
}

impl Stage {
    pub fn reel() -> Self {
        Self {
            reel: true,
            ..Default::default()
        }
    }

    pub fn frame(
        &mut self,
        time: f32,
        units: &[UnitState],
        ground: &dyn Fn(f32, f32) -> f32,
    ) -> Frame {
        if self.reel {
            return self.shoot(time, units, ground);
        }
        let last = self.last.replace(time);
        let on = |(x, z): (f32, f32)| Vec3::new(x, ground(x, z), z);
        let mut fight = CombatView {
            tick: (time * 60.) as u64,
            ..Default::default()
        };
        let mut events = Vec::new();
        // Each lane fires on its own period, offset so the lanes don't move together.
        for (lane, &(visual, school, speed, height, z)) in LANES.iter().enumerate() {
            // Each lane flies straight at its height over the ground at either end.
            let start = Vec3::new(FROM, ground(FROM, z) + height, z);
            let end = Vec3::new(TO, ground(TO, z) + height, z);
            let flight = (TO - FROM) / speed;
            let velocity = (end - start) / flight;
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
                    position: start + velocity * elapsed,
                    velocity,
                    visual: visual.into(),
                    school,
                });
            } else if let Some(l) = last
                && l < launched + flight
            {
                events.push((
                    Event::ProjectileEnded {
                        id,
                        at: end,
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
            aim: Some((on(AIM.0), AIM.1)),
            clock: time,
            cut: false,
        }
    }

    /// The reel's frame at `time`: its shot's effects alone, timed from the shot's start.
    fn shoot(&mut self, time: f32, units: &[UnitState], ground: &dyn Fn(f32, f32) -> f32) -> Frame {
        let (index, shot, local, start) = shot_at(time);
        let at = shot.clock(local);
        let clock = start + at;
        let last = self.last.replace(clock);
        let cut = self.shot.replace(index) != Some(index);
        // A shot's moments, in seconds of effect time from its start, that passed since the
        // last frame.
        let due = |t: f32| start + t <= clock && last.is_none_or(|l| l < start + t);
        let on = |(x, z): (f32, f32)| Vec3::new(x, ground(x, z), z);
        let mut fight = CombatView {
            tick: (time * 60.) as u64,
            ..Default::default()
        };
        let mut events = Vec::new();
        let id = (index as u32 + 1) * 1000;
        match shot.on {
            On::Missile(visual, school, speed) => {
                let (from, to, z, height) = LANE;
                let begin = Vec3::new(from, ground(from, z) + height, z);
                let end = Vec3::new(to, ground(to, z) + height, z);
                let flight = (to - from) / speed;
                let velocity = (end - begin) / flight;
                let launches = [0.1, shot.slow_from.unwrap_or(0.) + 0.1];
                for (n, launched) in launches.into_iter().enumerate() {
                    let id = id + n as u32;
                    if due(launched) {
                        events.push((
                            Event::ProjectileLaunched {
                                id,
                                from: units.first().map_or(0, |u| u.id),
                                visual: visual.into(),
                            },
                            start + launched,
                        ));
                    }
                    let elapsed = at - launched;
                    if (0. ..flight).contains(&elapsed) {
                        fight.projectiles.push(ProjectileView {
                            id,
                            position: begin + velocity * elapsed,
                            velocity,
                            visual: visual.into(),
                            school,
                        });
                    } else if due(launched + flight) {
                        events.push((
                            Event::ProjectileEnded {
                                id,
                                at: end,
                                hit: None,
                            },
                            start + launched + flight,
                        ));
                    }
                }
            }
            On::Field(field, school, hostile) => fight.areas.push(AreaView {
                id,
                warning: false,
                shape: Shape::Circle { radius: 1.8 },
                center: on(GROUND_AT),
                yaw: 0.,
                elapsed: at,
                duration: 600.,
                hostile,
                school,
                field: field.map(Into::into),
            }),
            On::Warning(shape, hostile, school, seconds) => {
                let (yaw, centre) = match shape {
                    // Cones and lines start behind the middle and reach across it.
                    Shape::Circle { .. } => (0., GROUND_AT),
                    _ => (FRAC_PI_2, (GROUND_AT.0 - 2.2, GROUND_AT.1)),
                };
                let elapsed = at - 0.2;
                if (0. ..seconds).contains(&elapsed) {
                    fight.areas.push(AreaView {
                        id,
                        warning: true,
                        shape,
                        center: on(centre),
                        yaw,
                        elapsed,
                        duration: seconds,
                        hostile,
                        school,
                        field: None,
                    });
                }
                if let Shape::Circle { radius } = shape
                    && due(0.2 + seconds)
                {
                    events.push((
                        Event::AreaImpact {
                            id,
                            at: on(centre),
                            radius,
                            school,
                        },
                        start + 0.2 + seconds,
                    ));
                }
            }
            On::Combo => {
                if due(0.3) {
                    events.push((
                        Event::Combo {
                            unit: units.first().map_or(0, |u| u.id),
                            name: "Combo".into(),
                            at: on(GROUND_AT),
                        },
                        start + 0.3,
                    ));
                }
            }
            On::Auras(_) | On::Casts(_) => {}
        }
        for (i, u) in units.iter().enumerate() {
            let auras = match shot.on {
                On::Auras(keys) => keys.get(i).map(|&key| AuraView {
                    key: key.into(),
                    name: key.into(),
                    stacks: 1,
                    remaining: 0.,
                    duration: 0.,
                    debuff: false,
                }),
                _ => None,
            };
            let cast = match shot.on {
                On::Casts(casts) => casts.get(i).map(|&(ability, duration)| CastBar {
                    ability: ability.into(),
                    name: ability.into(),
                    elapsed: at % duration,
                    duration,
                    channel: false,
                    interruptible: true,
                }),
                _ => None,
            };
            fight.units.push(UnitCombat {
                id: u.id,
                action: Default::default(),
                action_time: 0.,
                cast,
                auras: auras.into_iter().collect(),
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
            aim: None,
            clock,
            cut,
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
        // Worn between them: a shell (a mesh), marks at the feet (decals) and something
        // circling the head.
        let layers: Vec<_> = AURAS
            .iter()
            .flat_map(|keys| keys.iter())
            .flat_map(|key| table.layers(&table.auras[*key]).expect("an aura's effect"))
            .collect();
        assert!(
            layers.iter().any(|l| l.kind == assets::LayerKind::Mesh),
            "a shell"
        );
        assert!(
            layers.iter().any(|l| l.kind == assets::LayerKind::Decal),
            "a mark"
        );
        assert!(layers.iter().any(|l| l.orbit != 0.), "a halo");
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
    fn the_reel_shows_every_missile_and_what_it_names_exists() {
        let table = assets::Catalog::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
            .unwrap()
            .effects
            .expect("effects.json");
        let shown: HashSet<_> = REEL
            .iter()
            .filter_map(|s| match s.on {
                On::Missile(visual, ..) => Some(visual),
                _ => None,
            })
            .collect();
        for visual in table.projectiles.keys() {
            assert!(shown.contains(visual.as_str()), "{visual} has no shot");
        }
        for shot in REEL {
            match shot.on {
                On::Auras(keys) => {
                    for key in keys {
                        assert!(table.auras.contains_key(*key), "{key} isn't an aura");
                    }
                }
                On::Casts(casts) => {
                    for (ability, _) in casts {
                        assert!(table.casts.contains_key(*ability), "{ability} doesn't glow");
                    }
                }
                On::Field(Some(field), ..) => {
                    assert!(table.fields.contains_key(field), "{field} isn't a field")
                }
                _ => {}
            }
        }
    }

    #[test]
    fn the_reel_cuts_to_each_shot_once_and_its_missiles_land() {
        let units = lineup();
        let mut stage = Stage::reel();
        let mut launched = HashMap::new();
        let mut landed = HashSet::new();
        let (mut cuts, mut clock) = (0, f32::MIN);
        for frame in 0..(reel_seconds() * 60.) as u32 {
            let f = stage.frame(frame as f32 / 60., &units, &|_, _| 0.);
            assert!(f.clock >= clock, "the effects' clock never runs back");
            clock = f.clock;
            cuts += f.cut as usize;
            for (e, at) in &f.events {
                match e {
                    Event::ProjectileLaunched { id, .. } => {
                        assert!(launched.insert(*id, *at).is_none(), "{id} launched twice")
                    }
                    Event::ProjectileEnded { id, .. } => {
                        assert!(*at > launched[id], "{id} landed before it left");
                        assert!(landed.insert(*id), "{id} landed twice");
                    }
                    _ => {}
                }
                assert!(*at <= f.clock + 1e-4, "events are never early");
            }
        }
        assert_eq!(cuts, REEL.len());
        assert_eq!(
            landed.len(),
            launched.len(),
            "every missile lands in its shot"
        );
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
