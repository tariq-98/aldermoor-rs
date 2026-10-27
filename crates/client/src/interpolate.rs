use glam::Vec3;
use protocol::{Snapshot, Traversal, UnitId, UnitState};
use std::collections::{HashMap, VecDeque};
use std::{sync::Arc, time::Instant};

/// Snapshots kept to blend between: enough for a couple of send intervals at 20 Hz.
const KEPT: usize = 8;
/// Furthest a unit is carried on past the newest snapshot when the next is late.
const EXTRAPOLATE: f64 = 0.1;
/// How fast a unit eases back onto its path after a correction (a late snapshot that
/// disagreed with where it was carried), per second.
const CATCH_UP: f32 = 15.;

/// Snapshots in arrival order, blended at a steady sim time (#198): Hermite curves through
/// each unit's positions and velocities, a little behind the newest snapshot (about two send
/// intervals over the network, one step locally), carried on at most 100 ms when one is late
/// and eased back onto the path rather than snapped when the next arrives.
pub struct Snapshots {
    pub previous: Arc<Snapshot>,
    pub current: Arc<Snapshot>,
    pub received: Instant,
    /// The sim time this frame draws (`pacing::SimClock`). Without it, time is measured from
    /// when the newest snapshot arrived, which jitters with arrival.
    pub draw_time: Option<f64>,
    kept: VecDeque<Arc<Snapshot>>,
    /// Sim seconds between snapshots, smoothed: a step locally, a send interval over the
    /// network.
    interval: f64,
    /// Where each unit was drawn last frame, and when, for easing out corrections.
    shown: HashMap<UnitId, (Vec3, f64)>,
}
impl Snapshots {
    pub fn new(s: Arc<Snapshot>) -> Self {
        Self {
            previous: s.clone(),
            current: s.clone(),
            received: Instant::now(),
            draw_time: None,
            kept: VecDeque::from([s]),
            interval: 1. / 60.,
            shown: HashMap::new(),
        }
    }
    /// The extra delay over the one step `pacing::DRAW_DELAY` already gives, so there are
    /// about two send intervals of snapshots ahead of what's drawn.
    fn extra_delay(&self) -> f64 {
        (2. * self.interval - 2. / 60.).max(0.)
    }
    /// How far past the previous snapshot this frame draws, in seconds: a step's length when
    /// it shows the newest one.
    pub fn since(&self) -> f32 {
        match self.draw_time {
            Some(t) => (t - self.previous.time).max(0.) as f32,
            None => self.received.elapsed().as_secs_f32(),
        }
    }
    pub fn push(&mut self, s: Arc<Snapshot>) {
        if s.tick > self.current.tick {
            let gap = s.time - self.current.time;
            if gap > 0. && gap < 1. {
                self.interval += (gap - self.interval) * 0.1;
            }
            self.previous = self.current.clone();
            self.current = s.clone();
            self.received = Instant::now();
            self.kept.push_back(s);
            while self.kept.len() > KEPT {
                self.kept.pop_front();
            }
        }
    }
    /// Everyone a step behind the newest sim time, so there's a later state to blend
    /// towards, except `own` (the player's character), drawn at the newest time so its
    /// response to the keys shows a step sooner (#219).
    pub fn units(&mut self, indexed: bool, own: Option<UnitId>) -> Vec<UnitState> {
        let Some(draw) = self.draw_time else {
            return interpolate(
                &self.previous,
                &self.current,
                self.since(),
                indexed,
                own.map(|id| (id, crate::pacing::DRAW_DELAY as f32)),
            );
        };
        let others = draw - self.extra_delay();
        let mine = draw + crate::pacing::DRAW_DELAY;
        let (a, b) = self.around(others);
        let (oa, ob) = self.around(mine);
        let (ia, ib) = (index(&a), index(&b));
        let mut out = Vec::with_capacity(self.current.units.len());
        for u in &self.current.units {
            let is_own = own == Some(u.id);
            let (t, a, b, ia, ib) = if is_own {
                (mine, &oa, &ob, None, None)
            } else {
                (others, &a, &b, Some(&ia), Some(&ib))
            };
            let ua = ia.map_or_else(|| a.unit(u.id), |m| m.get(&u.id).copied());
            let ub = ib.map_or_else(|| b.unit(u.id), |m| m.get(&u.id).copied());
            let mut v = blend(ua, ub.unwrap_or(u), a.time, b.time, t);
            // Ease out a jump that isn't the unit's own motion (a correction after a late
            // snapshot); teleports (10 m or more) and owner's body go straight there.
            if let Some(&(was, at)) = self.shown.get(&u.id) {
                let dt = (t - at).clamp(0., 0.1) as f32;
                let expected = was + v.velocity * dt;
                let off = v.position - expected;
                if !is_own && off.length() > 0.02 && off.length() < 10. && dt > 0. {
                    v.position = expected + off * (1. - (-CATCH_UP * dt).exp());
                }
            }
            self.shown.insert(u.id, (v.position, t));
            out.push(v);
        }
        let here: std::collections::HashSet<UnitId> =
            self.current.units.iter().map(|u| u.id).collect();
        self.shown.retain(|id, _| here.contains(id));
        out
    }
    /// The kept snapshots either side of sim time `t` (the newest twice past the end, the
    /// oldest twice before the start).
    fn around(&self, t: f64) -> (Arc<Snapshot>, Arc<Snapshot>) {
        let after = self.kept.iter().position(|s| s.time > t);
        match after {
            Some(0) => (self.kept[0].clone(), self.kept[0].clone()),
            Some(i) => (self.kept[i - 1].clone(), self.kept[i].clone()),
            None => {
                let last = self
                    .kept
                    .back()
                    .cloned()
                    .unwrap_or_else(|| self.current.clone());
                (last.clone(), last)
            }
        }
    }
}

fn index(s: &Snapshot) -> HashMap<UnitId, &UnitState> {
    s.units.iter().map(|u| (u.id, u)).collect()
}

/// A unit at sim time `t`, between its state `a` (at `ta`, if it was there) and `b` (at
/// `tb`): a Hermite curve through both positions with their velocities, linear for forced
/// moves (their speed changes sharply), a snap for a teleport, and past `b` carried on along
/// its velocity for at most [`EXTRAPOLATE`].
fn blend(a: Option<&UnitState>, b: &UnitState, ta: f64, tb: f64, t: f64) -> UnitState {
    let mut out = b.clone();
    let span = tb - ta;
    let Some(a) = a.filter(|a| span > 1e-6 && a.position.distance(b.position) < 10.) else {
        let extra = (t - tb).clamp(0., EXTRAPOLATE) as f32;
        out.position += b.velocity * extra;
        out.pose_time += extra;
        return out;
    };
    let s = ((t - ta) / span).clamp(0., 1.) as f32;
    let d = span as f32;
    let forced =
        matches!(a.traversal, Traversal::Forced(_)) || matches!(b.traversal, Traversal::Forced(_));
    if forced {
        out.position = a.position.lerp(b.position, s);
        out.velocity = (b.position - a.position) / d;
    } else {
        let (s2, s3) = (s * s, s * s * s);
        let (h00, h10, h01, h11) = (
            2. * s3 - 3. * s2 + 1.,
            s3 - 2. * s2 + s,
            -2. * s3 + 3. * s2,
            s3 - s2,
        );
        out.position =
            a.position * h00 + a.velocity * d * h10 + b.position * h01 + b.velocity * d * h11;
        let (d00, d10, d01, d11) = (
            6. * s2 - 6. * s,
            3. * s2 - 4. * s + 1.,
            -6. * s2 + 6. * s,
            3. * s2 - 2. * s,
        );
        out.velocity =
            (a.position * d00 + b.position * d01) / d + a.velocity * d10 + b.velocity * d11;
    }
    let dy = (b.yaw - a.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    out.yaw = a.yaw + dy * s;
    if a.pose == b.pose {
        out.pose_time = a.pose_time + (b.pose_time - a.pose_time) * s;
    }
    if t > tb {
        let extra = (t - tb).min(EXTRAPOLATE) as f32;
        out.position += b.velocity * extra;
        out.pose_time += extra;
    }
    out
}
fn interpolate(
    previous: &Snapshot,
    current: &Snapshot,
    elapsed: f32,
    indexed: bool,
    own: Option<(UnitId, f32)>,
) -> Vec<UnitState> {
    let gap = (current.time - previous.time) as f32;
    let dt = gap.max(1. / 60.);
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
            let elapsed = match own {
                Some((id, lead)) if id == u.id => elapsed + lead,
                _ => elapsed,
            };
            let alpha = (elapsed / dt).clamp(0., 1.);
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
                traversal: Default::default(),
                aim_pitch: 0,
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
        let u = &interpolate(&a, &b, 1. / 120., true, None)[0];
        assert!((u.position.x - 0.5).abs() < 0.001);
        assert!((u.yaw.abs() - std::f32::consts::PI).abs() < 0.02);
        assert!((interpolate(&a, &b, 10., true, None)[0].position.x - 1.1).abs() < 0.001);
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
            interpolate(&a, &b, 0.008, true, None),
            interpolate(&a, &b, 0.008, false, None)
        );
    }
    #[test]
    fn respawn_teleport_is_not_smeared() {
        let a = snap(1, 0., 0.);
        let b = snap(2, 100., 0.);
        assert_eq!(interpolate(&a, &b, 0.001, true, None)[0].position.x, 100.);
    }
    #[test]
    fn the_players_own_body_is_drawn_ahead_of_everyone_else() {
        let (a, b) = (snap(1, 0., 0.), snap(2, 1., 0.));
        let id = 1;
        let others = interpolate(&a, &b, 0.004, true, None)[0].position.x;
        let own = interpolate(&a, &b, 0.004, true, Some((id, 1. / 60.)))[0]
            .position
            .x;
        assert!(own > others, "own {own} others {others}");
    }
    #[test]
    fn a_steady_walker_moves_steadily_through_jittery_30_hz_views() {
        // A walker at 1.5 m/s, a view every second step (30 Hz) arriving 50 ms late give or take
        // 20 ms, drawn at 60 frames a second: its drawn speed holds within 0.05 m/s (#198).
        let speed = 1.5;
        let view = |tick: u64| {
            let mut s = snap(tick, speed * tick as f32 / 60., 0.);
            s.units[0].velocity = Vec3::new(speed, 0., 0.);
            Arc::new(s)
        };
        let jitter =
            |tick: u64| ((tick.wrapping_mul(2654435761) % 1000) as f64 / 1000. - 0.5) * 0.04;
        let mut snaps = Snapshots::new(view(0));
        let mut clock = crate::pacing::SimClock::default();
        let mut next = 2;
        let mut last: Option<f32> = None;
        let mut worst: f32 = 0.;
        for frame in 0..(60 * 4) {
            let local = frame as f64 / 60.;
            while next as f64 / 60. + 0.05 + jitter(next) <= local {
                snaps.push(view(next));
                clock.observe(next as f64 / 60., next as f64 / 60. + 0.05 + jitter(next));
                next += 2;
            }
            snaps.draw_time = clock.draw_time(local);
            let x = snaps.units(true, None)[0].position.x;
            if let Some(was) = last
                && frame > 60
            {
                worst = worst.max(((x - was) * 60. - speed).abs());
            }
            last = Some(x);
        }
        assert!(worst < 0.05, "speed strayed by {worst} m/s");
    }
}
