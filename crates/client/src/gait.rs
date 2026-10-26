//! `--gait`: a scripted run for seeing locomotion in captures (#110), as `--smoke` is for the sim.
//! Facing one way, the player runs ahead, strafes left, backpedals and strafes right, sweeps the
//! travel once round every direction, then walks, sprints and walks again. Then they turn round,
//! draw a weapon and run the pattern again, mirrored so that it stays on the same side of the
//! line they walked. Last they turn on the spot (#211): a small turn held, a long one, an about
//! face and a spin, then they circle-strafe once round.
use glam::Vec2;
use protocol::Command;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

#[derive(Clone, Copy)]
enum Pace {
    Stand,
    Walk,
    Run,
    Sprint,
}

/// One stretch of the run: `seconds` long, facing `yaw` (radians) and travelling `travel` degrees
/// round from it (left is positive; each turns from the first to the second over the stretch) at
/// `pace`. While `hold`, the player faces `yaw`; otherwise they turn toward the travel as they
/// would on their own.
struct Leg {
    name: &'static str,
    seconds: f32,
    yaw: [f32; 2],
    hold: bool,
    travel: [f32; 2],
    pace: Pace,
}

const fn leg(name: &'static str, seconds: f32, yaw: f32, travel: [f32; 2], pace: Pace) -> Leg {
    Leg {
        name,
        seconds,
        yaw: [yaw, yaw],
        hold: true,
        travel,
        pace,
    }
}

/// A stretch standing still, turning from the first yaw to the second.
const fn look(name: &'static str, seconds: f32, yaw: [f32; 2]) -> Leg {
    Leg {
        yaw,
        ..leg(name, seconds, 0., [0., 0.], Pace::Stand)
    }
}

/// The yaws facing west and east. West first, the way `--smoke` runs: open meadow, away from the
/// river and the wolves until they're needed as a target. Every turn goes north of the line
/// walked, clear of the tree south of it.
const WEST: f32 = -FRAC_PI_2;
const EAST: f32 = FRAC_PI_2;

/// The yaw `degrees` round to the left from east (north is -90).
const fn east(degrees: f32) -> f32 {
    EAST + degrees * PI / 180.
}

const LEGS: [Leg; 26] = [
    leg("stand", 1., WEST, [0., 0.], Pace::Stand),
    leg("run ahead", 1.2, WEST, [0., 0.], Pace::Run),
    leg("strafe left", 1.2, WEST, [90., 90.], Pace::Run),
    leg("backpedal", 2., WEST, [180., 180.], Pace::Run),
    leg("strafe right", 1.2, WEST, [-90., -90.], Pace::Run),
    leg("sweep round", 4., WEST, [0., 360.], Pace::Run),
    leg("walk", 1.5, WEST, [0., 0.], Pace::Walk),
    leg("sprint", 1.5, WEST, [0., 0.], Pace::Sprint),
    leg("walk", 1.5, WEST, [0., 0.], Pace::Walk),
    leg("stand", 1., WEST, [0., 0.], Pace::Stand),
    Leg {
        hold: false,
        ..leg("turn round", 0.6, WEST, [180., 180.], Pace::Walk)
    },
    leg("draw", 1., EAST, [0., 0.], Pace::Stand),
    leg("run ahead drawn", 1.2, EAST, [0., 0.], Pace::Run),
    leg("strafe right drawn", 1.2, EAST, [-90., -90.], Pace::Run),
    leg("backpedal drawn", 2., EAST, [180., 180.], Pace::Run),
    leg("strafe left drawn", 1.2, EAST, [90., 90.], Pace::Run),
    leg("sweep round drawn", 4., EAST, [0., -360.], Pace::Run),
    leg("stand", 1., EAST, [0., 0.], Pace::Stand),
    // The chest takes a small turn and the feet stay, until it is held a moment; a turn past 70°
    // has them step round at once.
    look("look left", 0.4, [east(0.), east(50.)]),
    look("hold left", 1.5, [east(50.), east(50.)]),
    look("look right", 0.8, [east(50.), east(-60.)]),
    look("hold right", 1.2, [east(-60.), east(-60.)]),
    look("about face", 1.5, [east(120.), east(120.)]),
    look("spin round", 3.5, [east(120.), east(630.)]),
    // Facing north, round a point ahead of them, strafing right so that it stays north of the
    // line walked.
    Leg {
        yaw: [east(-90.), east(270.)],
        ..leg("circle strafe", 4., 0., [-90., -90.], Pace::Run)
    },
    look("stand", 1., [east(-90.), east(-90.)]),
];

/// The leg the run is on `seconds` in, and the move it asks for then; `None` once it's over.
pub fn at(seconds: f32) -> Option<(usize, Command)> {
    let mut start = 0.;
    for (i, leg) in LEGS.iter().enumerate() {
        if seconds < start + leg.seconds {
            let along = (seconds - start) / leg.seconds;
            let turned = |[from, to]: [f32; 2]| from + (to - from) * along;
            let yaw = (turned(leg.yaw) + PI).rem_euclid(TAU) - PI;
            let way = yaw + turned(leg.travel).to_radians();
            let direction = match leg.pace {
                Pace::Stand => Vec2::ZERO,
                _ => Vec2::new(way.sin(), way.cos()),
            };
            return Some((
                i,
                Command::Move {
                    direction,
                    walk: matches!(leg.pace, Pace::Walk),
                    sprint: matches!(leg.pace, Pace::Sprint),
                    face_yaw: leg.hold.then_some(yaw),
                },
            ));
        }
        start += leg.seconds;
    }
    None
}

/// What leg `i` does, for the log.
pub fn name(i: usize) -> &'static str {
    LEGS[i].name
}

/// Whether leg `i` draws the weapon as it starts (by targeting the nearest enemy).
pub fn draws(i: usize) -> bool {
    LEGS[i].name == "draw"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_run_holds_its_facing_and_ends() {
        let (mut legs, mut last) = (Vec::new(), None::<f32>);
        for frame in 0.. {
            let Some((
                i,
                Command::Move {
                    direction,
                    face_yaw,
                    ..
                },
            )) = at(frame as f32 / 60.)
            else {
                break;
            };
            assert!(direction.length() < 1. + 1e-5);
            assert_eq!(face_yaw.is_none(), name(i) == "turn round");
            // The facing turns smoothly, but for the about face: half round at once.
            if let (Some(was), Some(now)) = (last, face_yaw) {
                let turned = ((now - was + PI).rem_euclid(TAU) - PI).abs();
                if name(i) == "about face" && legs.last() != Some(&i) {
                    assert!((turned - PI).abs() < 1e-3, "the about face turned {turned}");
                } else {
                    assert!(turned < 0.05, "{}: turned {turned} in a frame", name(i));
                }
            }
            last = face_yaw;
            if legs.last() != Some(&i) {
                legs.push(i);
            }
        }
        assert_eq!(legs, (0..LEGS.len()).collect::<Vec<_>>());
        assert_eq!((0..LEGS.len()).filter(|&i| draws(i)).count(), 1);
    }
}
