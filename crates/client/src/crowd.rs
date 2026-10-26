//! `--crowd N`: N more characters round the player, standing about, walking, running and
//! fighting, to measure animation and skinning with 50, 100 or 200 characters in view (#215).
use glam::Vec3;
use protocol::{Look, Pose, UnitState};

/// Unit ids the crowd takes, clear of the sim's.
const FIRST_ID: u32 = 1_000_000;
/// How many are on each ring round the player, and how far apart the rings are. Walkers and
/// runners go round lanes of their own, a third and two thirds of the way out to the next ring,
/// so that nobody passes through anybody.
const RING: usize = 12;
const SPACING: f32 = 3.6;

/// The crowd round `base` (the player), `time` seconds in. A quarter of them stand looking one
/// way and the other, so their feet step round; a quarter walk and a quarter run round their
/// ring; and a quarter swing their weapons, a blow every 1.2 s. They wear five outfits in turn,
/// two of them with capes.
pub fn units(base: &UnitState, count: usize, time: f32) -> Vec<UnitState> {
    (0..count)
        .map(|i| {
            let ring = (i / RING) as f32;
            let start = (i % RING) as f32 / RING as f32 * std::f32::consts::TAU + ring * 0.3;
            let phase = i as f32 * 0.37;
            let mut u = base.clone();
            u.id = FIRST_ID + i as u32;
            u.look = look(i).into();
            u.target = None;
            let (pose, speed, pose_time, lane) = match i % 4 {
                0 => (Pose::Idle, 0., time, 0.),
                1 => (Pose::Walk, 1.6, time, 1.),
                2 => (Pose::Run, 6., time, 2.),
                _ => {
                    let t = (time + phase) % 1.2;
                    if t < 0.6 {
                        (Pose::Attack, 0., t, 0.)
                    } else {
                        (Pose::Idle, 0., t - 0.6, 0.)
                    }
                }
            };
            // Movers go round their lane at their speed, facing the way they go.
            let radius = 6. + (ring + lane / 3.) * SPACING;
            let angle = start + speed / radius * time;
            let at = Vec3::new(angle.cos(), 0., angle.sin()) * radius;
            let ahead = Vec3::new(-angle.sin(), 0., angle.cos());
            u.position = base.position + at;
            u.position.y = world::height(u.position.x, u.position.z);
            u.velocity = ahead * speed;
            u.yaw = if speed > 0. {
                ahead.x.atan2(ahead.z)
            } else {
                // Facing the player, looking 70° one way and the other every few seconds.
                let facing = (-at.x).atan2(-at.z);
                facing + 1.2 * (time * 0.8 + phase).sin()
            };
            u.pose = pose;
            u.pose_time = pose_time;
            u
        })
        .collect()
}

fn look(i: usize) -> Look {
    let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
    let gear = |items: &[(&str, &str)]| {
        items
            .iter()
            .map(|(slot, id)| (slot.to_string(), id.to_string()))
            .collect()
    };
    let (art, pieces, items, drawn) = match i % 5 {
        0 => (
            "male",
            strings(&[
                "plate.cuirass",
                "plate.pauldrons",
                "plate.gauntlets",
                "plate.legs",
            ]),
            gear(&[
                ("main_hand", "sword.arming"),
                ("off_hand", "shield.round"),
                ("back", "cape.traveller"),
            ]),
            true,
        ),
        1 => (
            "female",
            strings(&[
                "ranger.top",
                "ranger.trousers",
                "ranger.boots",
                "ranger.hood",
            ]),
            gear(&[("main_hand", "bow.hunting"), ("back", "quiver.leather")]),
            true,
        ),
        2 => (
            "elf_male",
            strings(&["robe.mage"]),
            gear(&[("main_hand", "staff.oak")]),
            false,
        ),
        3 => (
            "dwarf_male",
            strings(&[
                "peasant.top",
                "peasant.trousers",
                "peasant.shoes",
                "plate.cuirass",
            ]),
            gear(&[("main_hand", "sword.great"), ("back", "cape.traveller")]),
            true,
        ),
        _ => (
            "goblin_male",
            strings(&["ranger.top", "ranger.trousers", "ranger.boots"]),
            gear(&[
                ("main_hand", "dagger.hunting"),
                ("off_hand", "shield.round"),
            ]),
            true,
        ),
    };
    Look {
        art: art.into(),
        pieces,
        gear: items,
        drawn,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use protocol::UnitKind;
    #[test]
    fn the_crowd_stands_apart_round_the_player_and_movers_keep_going() {
        let base = UnitState {
            id: 1,
            kind: UnitKind::Player,
            body: "player".into(),
            name: "Traveller".into(),
            level: 1,
            position: Vec3::new(10., 0., 20.),
            velocity: Vec3::ZERO,
            yaw: 0.,
            pose: Pose::Idle,
            pose_time: 0.,
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look: Default::default(),
            traversal: Default::default(),
            aim_pitch: 0,
        };
        // At any time: nobody passes through anybody, even at 200.
        for time in [0., 3., 17.5, 60.] {
            let now = units(&base, 200, time);
            let next = units(&base, 200, time + 1. / 60.);
            for (i, u) in now.iter().enumerate() {
                assert!(u.id >= FIRST_ID);
                let away = (u.position - base.position).with_y(0.).length();
                assert!(away > 5.9, "{i} stands {away} m from the player");
                for v in &now[i + 1..] {
                    let apart = (u.position - v.position).with_y(0.).length();
                    assert!(
                        apart > 1.,
                        "{} and {} {apart} m apart at {time}",
                        u.id,
                        v.id
                    );
                }
                // Each frame a mover goes as far as its speed takes it, the way it faces.
                let went = (next[i].position - u.position).with_y(0.);
                let speed = u.velocity.length();
                assert!((went.length() * 60. - speed).abs() < 0.05 * speed.max(1.));
                if speed > 0. {
                    let facing = Quat::from_rotation_y(u.yaw) * Vec3::Z;
                    assert!(facing.dot(went.normalize()) > 0.99);
                }
            }
        }
    }
}
