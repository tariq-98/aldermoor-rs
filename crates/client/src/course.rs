//! `--traverse`: a scripted course over the traversal moves (#213), for captures, as `--gait` is
//! for locomotion. Facing west, the player strafes and rolls each way in turn (left, right, back
//! and ahead), standing up out of two rolls and running on out of the others. They walk north
//! into the field wall west of the north road, climbing onto it, and drop off its far side; run
//! north through the village and east of the bridge, where the banks are gentle, wade into the
//! river, swim across, wade out and swim back. They run back south, climbing the wall from the
//! other side, to the ruined hall, climbing onto its rear wall to lower themselves over its inner
//! edge to hang (#245), shimmy round its end onto the outer face, two corners (#246), and let go.
//! Round to the hall's east side, they sprint at its stepped east wall and run up it onto a step
//! (#246), climb to the lookout under the gable, drop to hang from its edge, shimmy left, eject
//! right along the hold, shimmy back and climb up; then step down, drop to hang from the step
//! below and leap up to the lookout's hold, and let go. Last they run at the wolves in the meadow
//! west of the road, lunging at the first within reach. Each step is logged as it starts, so
//! captures can be cut by frame.
use glam::{Vec2, Vec3, Vec3Swizzles};
use protocol::{Command, UnitId};
use std::f32::consts::FRAC_PI_2;

#[derive(Clone, Copy, PartialEq)]
enum Pace {
    Walk,
    Run,
    /// Shift held: at a wall too tall to climb, runs up it.
    Sprint,
}

#[derive(Clone, Copy)]
enum Step {
    /// Standing for `seconds`, facing `yaw`.
    Stand { seconds: f32, yaw: f32 },
    /// At a run `way` degrees round from facing `yaw` (left is positive) for `seconds`, rolling
    /// that way as it starts if `roll`.
    Strafe {
        seconds: f32,
        yaw: f32,
        way: f32,
        roll: bool,
    },
    /// Rolling `way` degrees round from facing `yaw` (left is positive), then standing for
    /// `seconds`. At an edge that lowers the player over it to hang, and hanging, it lets go.
    Roll { seconds: f32, yaw: f32, way: f32 },
    /// Space with `way` degrees round from facing `yaw` pressed, then nothing for `seconds`.
    /// Hanging, that leaps (into the wall), ejects (along it) or kicks off it (away).
    Jump { seconds: f32, yaw: f32, way: f32 },
    /// To the point (x, z) at `pace`, facing the way it goes.
    Go { to: Vec2, pace: Pace },
    /// At the nearest enemy, lunging at it (the sword's third skill) once it's within reach, then
    /// watching for `seconds`.
    Lunge { seconds: f32 },
}

/// The way the player faces through the rolls: west, the way `--gait` and `--smoke` start, into
/// the open meadow.
const WEST: f32 = -FRAC_PI_2;

const fn strafe(name: &'static str, seconds: f32, way: f32, roll: bool) -> (&'static str, Step) {
    (
        name,
        Step::Strafe {
            seconds,
            yaw: WEST,
            way,
            roll,
        },
    )
}

const fn stand(name: &'static str, seconds: f32) -> (&'static str, Step) {
    (name, Step::Stand { seconds, yaw: WEST })
}

const fn roll(name: &'static str, seconds: f32, way: f32) -> (&'static str, Step) {
    (
        name,
        Step::Roll {
            seconds,
            yaw: WEST,
            way,
        },
    )
}

const fn jump(name: &'static str, seconds: f32, way: f32) -> (&'static str, Step) {
    (
        name,
        Step::Jump {
            seconds,
            yaw: WEST,
            way,
        },
    )
}

const fn go(name: &'static str, x: f32, z: f32, pace: Pace) -> (&'static str, Step) {
    (
        name,
        Step::Go {
            to: Vec2::new(x, z),
            pace,
        },
    )
}

/// The sim's roll: 5 m in 0.45 s (docs/MOVEMENT.md). Each costs half the endurance, which comes
/// back at 8 a second, so they're 5 s apart.
const ROLL: f32 = 0.45;

const STEPS: [(&str, Step); 52] = [
    stand("stand", 1.),
    strafe("strafe left", 0.5, 90., false),
    strafe("roll left", ROLL, 90., true),
    strafe("run on left", 0.6, 90., false),
    stand("stand", 3.45),
    strafe("strafe right", 0.5, -90., false),
    strafe("roll right", ROLL, -90., true),
    stand("stand up right", 4.05),
    strafe("backpedal", 0.5, 180., false),
    strafe("roll back", ROLL, 180., true),
    strafe("run on back", 0.6, 180., false),
    stand("stand", 3.45),
    strafe("run ahead", 0.5, 0., false),
    strafe("roll ahead", ROLL, 0., true),
    stand("stand up ahead", 2.),
    // The field wall along z 21.6 from x -28 to 3.85: a metre high and 0.9 m deep, so it's
    // climbed rather than vaulted, even at a run.
    go("to the wall", -2., 26., Pace::Run),
    go("climb the wall", -2., 18., Pace::Walk),
    // The river runs along z -58 here; the bridge is at x 1.5.
    go("to the village", -2., -30., Pace::Run),
    go("to the river", 17., -45., Pace::Run),
    go("swim across", 17., -63., Pace::Run),
    go("swim back", 17., -45., Pace::Run),
    go("back through the village", -2., -30., Pace::Run),
    go("to the wall again", -2., 16., Pace::Run),
    go("climb it from the north", -2., 25., Pace::Run),
    // The ruined hall on the hill (`world::masonry`): its east wall, along x -14.9, steps up from
    // the rear at z 52 to the lookout under the gable, 4.2 m up, each step a climb from the last.
    // The rear wall's east half, from x -15.35 to -17.15, stands 1.6 m over the hall and 0.9 m
    // over the east wall's lowest step, with a hold along its inner face, round its west end and
    // along its outer face.
    go("to the ruin", -11., 52.2, Pace::Run),
    go("onto its east wall", -14.9, 52.2, Pace::Walk),
    go("onto the rear wall", -16., 52.2, Pace::Walk),
    stand("on the rear wall", 1.),
    // Rolling south lowers the player over the inner edge, to hang facing north; then west round
    // the wall's end onto it, and north round the next corner onto the outer face.
    roll("drop to hang inside", 1.5, -90.),
    strafe("shimmy round the corner", 1.6, 0., false),
    strafe("round the end", 1.2, 90., false),
    roll("let go outside", 1.5, 0.),
    go("round the rear", -12., 54., Pace::Run),
    // The lookout is 4.2 m up, too tall to climb, so sprinting at it runs up the wall, and at the
    // top of the run the hands reach its edge to pull up onto it. The ground under it rises to
    // the north: at z 44 the hands have 3 cm to spare, at its middle less than 1.
    go("east of the ruin", -10.5, 44., Pace::Run),
    go("run up to the lookout", -14.9, 44., Pace::Sprint),
    stand("on the lookout", 1.),
    // Facing in over the hall, so rolling back lowers the player over the outer edge, a hold
    // 2.35 m long from z 44.6 to 42.25, to hang facing the wall.
    roll("drop to hang", 1.5, 180.),
    strafe("shimmy left", 0.6, 90., false),
    // Space pressing right: a side eject to the far end of the same hold.
    jump("eject right", 1., -90.),
    strafe("shimmy back", 1., 90., false),
    // Into the wall climbs onto the top in 0.8 s; pressing on after that would walk off the far
    // side, so this lets go as the climb starts.
    strafe("climb up", 0.5, 0., false),
    stand("back on the lookout", 1.5),
    // Down onto the step below, over its outer edge to hang from its hold, 1.3 m under the
    // lookout's, and right to its end, under the end of the lookout's: Space leaps up to that.
    go("down a step", -14.9, 45.6, Pace::Walk),
    stand("on the step", 1.),
    roll("drop to hang below", 1.5, 180.),
    strafe("shimmy to the end", 0.6, -90., false),
    jump("leap up", 1.5, 0.),
    roll("let go", 2., 0.),
    // Round the gable's buttress.
    go("round the ruin", -12., 38., Pace::Run),
    // The wolves are at (-38, 30) and notice a player 13 m off.
    go("to the meadow", -20., 29., Pace::Run),
    ("lunge", Step::Lunge { seconds: 3. }),
    stand("stand", 0.5),
];

/// Within this of a point, the player has reached it, in metres.
const ARRIVED: f32 = 0.5;
/// The sword's lunge reaches 10 m (crates/content/combat/weapons.ron); it is used a little inside
/// that.
const LUNGE_REACH: f32 = 9.5;
/// The lunge's bar slot: the main hand's third skill.
const LUNGE_SLOT: u8 = 2;
/// How long the player waits for an enemy to come within reach before giving up, in seconds.
const LUNGE_WAIT: f32 = 20.;

/// Where the player is on the course.
#[derive(Default)]
pub struct Course {
    step: usize,
    /// When the step began, in course seconds, and how long it may take.
    began: Option<(f32, f32)>,
    target: Option<UnitId>,
    lunged: Option<f32>,
}

impl Course {
    /// The commands for this frame, `now` seconds into the course with the player at `at` and
    /// the living enemies in view at `foes`. Empty once the course is over.
    pub fn update(&mut self, now: f32, at: Vec3, foes: &[(UnitId, Vec3)]) -> Vec<Command> {
        let mut out = Vec::new();
        while let Some(&(_, step)) = STEPS.get(self.step) {
            let (began, limit) = *self.began.get_or_insert_with(|| {
                let limit = match step {
                    Step::Stand { seconds, .. }
                    | Step::Strafe { seconds, .. }
                    | Step::Roll { seconds, .. }
                    | Step::Jump { seconds, .. }
                    | Step::Lunge { seconds } => seconds,
                    // Long enough at a swim, and over walls on the way.
                    Step::Go { to, .. } => to.distance(at.xz()) / 1.5 + 4.,
                };
                (now, limit)
            });
            let first = now == began;
            let over = match step {
                Step::Go { to, .. } => to.distance(at.xz()) < ARRIVED || now - began >= limit,
                Step::Lunge { .. } => match self.lunged {
                    Some(lunged) => now - lunged >= limit,
                    None => now - began >= LUNGE_WAIT,
                },
                _ => now - began >= limit,
            };
            if over && !first {
                self.step += 1;
                self.began = None;
                continue;
            }
            match step {
                Step::Stand { yaw, .. } => out.push(walk(Vec2::ZERO, Pace::Run, Some(yaw))),
                Step::Strafe { yaw, way, roll, .. } => {
                    let way = yaw + way.to_radians();
                    let direction = Vec2::new(way.sin(), way.cos());
                    if roll && first {
                        out.push(Command::Dodge { direction });
                    }
                    out.push(walk(direction, Pace::Run, Some(yaw)));
                }
                Step::Roll { yaw, way, .. } => {
                    if first {
                        let way = yaw + way.to_radians();
                        out.push(Command::Dodge {
                            direction: Vec2::new(way.sin(), way.cos()),
                        });
                    }
                    out.push(walk(Vec2::ZERO, Pace::Run, Some(yaw)));
                }
                Step::Jump { yaw, way, .. } => {
                    let way = yaw + way.to_radians();
                    let direction = Vec2::new(way.sin(), way.cos());
                    if first {
                        out.push(Command::Jump);
                    }
                    out.push(walk(
                        if first { direction } else { Vec2::ZERO },
                        Pace::Run,
                        Some(yaw),
                    ));
                }
                Step::Go { to, pace } => {
                    out.push(walk((to - at.xz()).normalize_or_zero(), pace, None));
                }
                Step::Lunge { .. } => {
                    let nearest = foes.iter().min_by(|a, b| {
                        a.1.distance_squared(at)
                            .total_cmp(&b.1.distance_squared(at))
                    });
                    if self.target.is_none()
                        && let Some(&(id, _)) = nearest
                    {
                        self.target = Some(id);
                        out.push(Command::SetTarget(Some(id)));
                    }
                    let foe = foes.iter().find(|f| Some(f.0) == self.target);
                    match (foe, self.lunged) {
                        (Some(&(_, there)), None) => {
                            let toward = (there - at).xz();
                            if toward.length() < LUNGE_REACH {
                                out.push(Command::UseSlot {
                                    slot: LUNGE_SLOT,
                                    ground: None,
                                });
                                self.lunged = Some(now);
                            }
                            out.push(walk(toward.normalize_or_zero(), Pace::Run, None));
                        }
                        _ => out.push(walk(Vec2::ZERO, Pace::Run, None)),
                    }
                }
            }
            break;
        }
        out
    }

    /// The step the course is on, for the log, or `None` once it's over.
    pub fn step(&self) -> Option<(usize, &'static str)> {
        STEPS.get(self.step).map(|s| (self.step, s.0))
    }
}

fn walk(direction: Vec2, pace: Pace, face_yaw: Option<f32>) -> Command {
    Command::Move {
        direction,
        walk: pace == Pace::Walk,
        sprint: pace == Pace::Sprint,
        face_yaw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ForcedKind, Traversal};

    /// The course run against the valley as the client's own server starts it, the player at its
    /// spawn facing north: every step is reached, every place arrived at, and on the way the
    /// player rolls four times, climbs the field wall twice, the ruin's east and rear walls and
    /// back up from a hang, swims, hangs from the rear wall and goes round both its corners, runs
    /// up the wall to the lookout and pulls onto it, hangs from the lookout, shimmies and ejects
    /// along it, leaps up to it from the step below, takes no harm letting go, and lunges.
    #[test]
    fn the_course_rolls_climbs_swims_hangs_and_lunges() {
        let mut sim = sim::Sim::brambleford(0xa1de);
        let me = sim.spawn_player("Traveller", Vec2::new(6., 28.), std::f32::consts::PI);
        let mut course = Course::default();
        let start = sim.tick();
        let (mut moves, mut swam, mut reached) = (vec![], false, Vec::<usize>::new());
        let (mut last, mut hung, mut unharmed) = (None, Vec::<f32>::new(), true);
        let wolves = STEPS.iter().position(|s| s.0 == "to the meadow").unwrap();
        while let Some((step, _)) = course.step() {
            let snapshot = sim.snapshot(Some(me));
            let u = snapshot.units.iter().find(|u| u.id == me).unwrap();
            let foes: Vec<_> = (snapshot.units.iter())
                .filter(|f| f.hostile && f.health > 0.)
                .map(|f| (f.id, f.position))
                .collect();
            let now = (sim.tick() - start) as f32 / 60.;
            assert!(now < 150., "still on {} after 150 s", STEPS[step].0);
            let commands = course.update(now, u.position, &foes);
            if let Some(was) = reached.last().copied().filter(|was| *was != step)
                && let Step::Go { to, .. } = STEPS[was].1
            {
                let off = to.distance(u.position.xz());
                assert!(off < ARRIVED + 0.2, "{} ended {off} m short", STEPS[was].0);
            }
            if reached.last() != Some(&step) {
                reached.push(step);
            }
            if let Traversal::Forced(m) = u.traversal
                && last != Some(m.start)
            {
                last = Some(m.start);
                moves.push(m);
            }
            swam |= u.traversal == Traversal::Swimming;
            // From the lookout's hold, 4.2 m up the east wall.
            if u.traversal == Traversal::Hanging && u.position.y > 12.5 {
                hung.push(u.position.z);
            }
            if step < wolves {
                unharmed &= u.health == u.max_health;
            }
            for c in commands {
                sim.command(me, c);
            }
            sim.step();
        }
        assert_eq!(reached, (0..STEPS.len()).collect::<Vec<_>>());
        let count = |kind| moves.iter().filter(|m| m.kind == kind).count();
        assert_eq!(count(ForcedKind::Roll), 4, "{moves:?}");
        assert_eq!(count(ForcedKind::Climb), 5, "{moves:?}");
        assert_eq!(count(ForcedKind::Hold), 5, "{moves:?}");
        assert_eq!(count(ForcedKind::Corner), 2, "{moves:?}");
        assert_eq!(count(ForcedKind::WallRun), 1, "{moves:?}");
        assert_eq!(count(ForcedKind::Mantle), 1, "{moves:?}");
        assert_eq!(count(ForcedKind::Charge), 1, "{moves:?}");
        let onto = |m: &&protocol::ForcedMove, seconds: f64| {
            m.kind == ForcedKind::Hold && (f64::from(m.duration) - seconds).abs() < 1e-3
        };
        let leap = moves.iter().find(|m| onto(m, sim::climbing::LEAP_TIME));
        assert!(
            leap.is_some_and(|m| m.to.y - m.from.y > 1.),
            "no leap: {moves:?}"
        );
        let eject = moves.iter().find(|m| onto(m, sim::climbing::EJECT_TIME));
        assert!(
            eject.is_some_and(|m| (m.to - m.from).with_y(0.).length() > 0.8),
            "no eject: {moves:?}"
        );
        assert!(swam, "never swam");
        let along = hung.iter().fold(f32::MIN, |a, &z| a.max(z))
            - hung.iter().fold(f32::MAX, |a, &z| a.min(z));
        assert!(along > 1., "shimmied {along} m along the hold");
        assert!(unharmed, "hurt before the wolves");
    }
}
