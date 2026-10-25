use protocol::{Snapshot, UnitState};
use std::{sync::Arc, time::Instant};
/// One tick of interpolation delay, with at most 100 ms of bounded extrapolation.
pub struct Snapshots {
    pub previous: Arc<Snapshot>,
    pub current: Arc<Snapshot>,
    pub received: Instant,
}
impl Snapshots {
    pub fn new(s: Arc<Snapshot>) -> Self {
        Self {
            previous: s.clone(),
            current: s,
            received: Instant::now(),
        }
    }
    pub fn push(&mut self, s: Arc<Snapshot>) {
        if s.tick > self.current.tick {
            self.previous = self.current.clone();
            self.current = s;
            self.received = Instant::now();
        }
    }
    pub fn units(&self, indexed: bool) -> Vec<UnitState> {
        interpolate(
            &self.previous,
            &self.current,
            self.received.elapsed().as_secs_f32(),
            indexed,
        )
    }
}
fn interpolate(
    previous: &Snapshot,
    current: &Snapshot,
    elapsed: f32,
    indexed: bool,
) -> Vec<UnitState> {
    let gap = (current.time - previous.time) as f32;
    let dt = gap.max(1. / 60.);
    let alpha = (elapsed / dt).clamp(0., 1.);
    let previous_units: std::collections::HashMap<_, _> = previous
        .units
        .iter()
        .filter(|_| indexed)
        .map(|u| (u.id, u))
        .collect();
    current
        .units
        .iter()
        .map(|u| {
            let mut out = u.clone();
            if let Some(old) = if indexed {
                previous_units.get(&u.id).copied()
            } else {
                previous.unit(u.id)
            } {
                if old.position.distance(u.position) < 10. {
                    out.position = old.position.lerp(u.position, alpha);
                    let dy = (u.yaw - old.yaw + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI;
                    out.yaw = old.yaw + dy * alpha;
                }
                if u.pose == old.pose {
                    out.pose_time = old.pose_time + (u.pose_time - old.pose_time) * alpha;
                }
            }
            if elapsed > dt {
                let extra = (elapsed - dt).min(0.1);
                out.position += u.velocity * extra;
                out.pose_time += extra;
            }
            out
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use protocol::*;
    fn snap(tick: u64, x: f32, yaw: f32) -> Snapshot {
        Snapshot {
            tick,
            time: tick as f64 / 60.,
            units: vec![UnitState {
                id: 1,
                kind: UnitKind::Player,
                body: "player".into(),
                name: "Hero".into(),
                level: 1,
                position: Vec3::new(x, 0., 0.),
                velocity: Vec3::X,
                yaw,
                pose: Pose::Run,
                pose_time: tick as f32 / 60.,
                health: 100.,
                max_health: 100.,
                hostile: false,
                target: None,
                look: Default::default(),
            }],
            player: None,
            events: Vec::new(),
        }
    }
    #[test]
    fn interpolates_across_yaw_wrap_and_caps_stale_motion() {
        let a = snap(1, 0., 3.13);
        let b = snap(2, 1., -3.13);
        let u = &interpolate(&a, &b, 1. / 120., true)[0];
        assert!((u.position.x - 0.5).abs() < 0.001);
        assert!((u.yaw.abs() - std::f32::consts::PI).abs() < 0.02);
        assert!((interpolate(&a, &b, 10., true)[0].position.x - 1.1).abs() < 0.001);
    }
    #[test]
    fn indexed_and_linear_paths_agree_for_reordered_and_missing_units() {
        let mut a = snap(1, 0., 0.);
        let mut b = snap(2, 1., 0.);
        for id in 2..200 {
            let mut u = a.units[0].clone();
            u.id = id;
            u.position.x = id as f32;
            a.units.push(u.clone());
            u.position.x += 1.;
            b.units.push(u);
        }
        b.units.reverse();
        a.units.remove(33);
        assert_eq!(
            interpolate(&a, &b, 0.008, true),
            interpolate(&a, &b, 0.008, false)
        );
    }
    #[test]
    fn respawn_teleport_is_not_smeared() {
        let a = snap(1, 0., 0.);
        let b = snap(2, 100., 0.);
        assert_eq!(interpolate(&a, &b, 0.001, true)[0].position.x, 100.);
    }
}
