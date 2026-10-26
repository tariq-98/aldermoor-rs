//! Two-bone IK: turns a thigh and calf (or an upper arm and forearm) so the end of the chain
//! reaches a target, the middle joint bending toward a pole. Daniel Holden's analytic solver
//! ("Simple Two Joint IK", theorangeduck.com) with a pole: bend the chain in its own plane until
//! it is as long as the target is far, swing it onto the target, then roll it about the line to
//! the target until the middle joint points the pole's way.
use glam::{Quat, Vec3};

/// How close to straight a chain may get, as a share of its full length, so the middle joint
/// always knows which way it bends.
const REACH: f32 = 0.999;

/// The turns that bring a chain whose joints are at `root`, `mid` and `end` to `target`, its
/// middle joint bending toward `pole` (a direction, which needn't be square to the chain): the
/// first for the root's bone and the second for the middle's, both in the frame the positions are
/// in, to apply on the left of each bone's rotation in that frame. The chain keeps its lengths;
/// a target out of reach, or too close, gets the end as near as the chain can bring it, along the
/// line to it.
pub fn two_bone(root: Vec3, mid: Vec3, end: Vec3, target: Vec3, pole: Vec3) -> (Quat, Quat) {
    let [upper, lower] = [mid.distance(root), end.distance(mid)];
    if upper < 1e-6 || lower < 1e-6 {
        return (Quat::IDENTITY, Quat::IDENTITY);
    }
    let reach = target.distance(root).clamp(
        (upper - lower).abs() / REACH + 1e-5,
        (upper + lower) * REACH,
    );
    // The plane the chain bends in, or the pole's if it is straight.
    let along = (end - root).normalize_or(Vec3::Y);
    let normal = along
        .cross(mid - root)
        .try_normalize()
        .or_else(|| along.cross(pole).try_normalize())
        .unwrap_or(along.any_orthonormal_vector());
    let angle = |a: Vec3, b: Vec3| a.normalize().dot(b.normalize()).clamp(-1., 1.).acos();
    // The angles at the root (between the chain's line and the upper bone) and at the middle
    // joint, now and at the target's distance (the law of cosines).
    let cosine = |a: f32, b: f32, c: f32| ((a * a + b * b - c * c) / (2. * a * b)).clamp(-1., 1.);
    let [root_now, mid_now] = [angle(end - root, mid - root), angle(root - mid, end - mid)];
    let root_then = cosine(upper, reach, lower).acos();
    let mid_then = cosine(upper, lower, reach).acos();
    // Bending at both joints in the chain's plane keeps the line from root to end where it was.
    let bend_root = Quat::from_axis_angle(normal, root_then - root_now);
    let bend_mid = Quat::from_axis_angle(normal, mid_then - mid_now);
    // Then the whole chain swings onto the target...
    let swing = Quat::from_rotation_arc(along, (target - root).normalize_or(along));
    let line = swing * along;
    // ...and rolls about the line to it until the middle joint is on the pole's side.
    let knee = swing * bend_root * (mid - root);
    let square = |v: Vec3| v - line * v.dot(line);
    let roll = match (square(knee).try_normalize(), square(pole).try_normalize()) {
        (Some(from), Some(to)) => {
            Quat::from_axis_angle(line, from.cross(to).dot(line).atan2(from.dot(to)))
        }
        _ => Quat::IDENTITY,
    };
    let upper_turn = roll * swing * bend_root;
    (upper_turn, upper_turn * bend_mid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the chain's joints go when its bones turn as `two_bone` says.
    fn solved(root: Vec3, mid: Vec3, end: Vec3, target: Vec3, pole: Vec3) -> (Vec3, Vec3) {
        let (upper, lower) = two_bone(root, mid, end, target, pole);
        let mid_after = root + upper * (mid - root);
        (mid_after, mid_after + lower * (end - mid))
    }

    /// A leg like the male body's: hip, knee a little forward, ankle.
    const LEG: [Vec3; 3] = [
        Vec3::new(0.09, 0.97, -0.04),
        Vec3::new(0.09, 0.54, 0.),
        Vec3::new(0.09, 0.09, -0.09),
    ];

    #[test]
    fn the_end_reaches_targets_in_reach_and_the_bones_keep_their_lengths() {
        let [root, mid, end] = LEG;
        for target in [
            Vec3::new(0.09, 0.09, -0.09),
            Vec3::new(0.2, 0.15, 0.1),
            Vec3::new(-0.1, 0.3, 0.2),
            Vec3::new(0.3, 0.6, -0.3),
            Vec3::new(0.09, 0.5, 0.),
        ] {
            let (m, e) = solved(root, mid, end, target, Vec3::Z);
            assert!(e.distance(target) < 1e-4, "{target}: end at {e}");
            assert!((m.distance(root) - mid.distance(root)).abs() < 1e-5);
            assert!((e.distance(m) - end.distance(mid)).abs() < 1e-5);
        }
    }

    #[test]
    fn the_middle_joint_bends_toward_the_pole() {
        let [root, mid, end] = LEG;
        let target = Vec3::new(0.09, 0.3, 0.);
        for pole in [
            Vec3::Z,
            Vec3::X,
            Vec3::new(-1., 0., 1.),
            Vec3::new(0.3, 0.2, -1.),
        ] {
            let (m, e) = solved(root, mid, end, target, pole);
            assert!(e.distance(target) < 1e-4);
            // The knee sticks out from the line hip to ankle the pole's way.
            let line = (target - root).normalize();
            let out = (m - root) - line * (m - root).dot(line);
            let want = pole - line * pole.dot(line);
            assert!(
                out.normalize().dot(want.normalize()) > 0.999,
                "pole {pole}: knee {m}"
            );
        }
    }

    #[test]
    fn a_target_out_of_reach_gets_the_end_as_near_as_the_chain_goes() {
        let [root, mid, end] = LEG;
        let length = mid.distance(root) + end.distance(mid);
        for target in [Vec3::new(0.09, -1., 0.), Vec3::new(1., 1., 1.)] {
            let (_, e) = solved(root, mid, end, target, Vec3::Z);
            let line = (target - root).normalize();
            assert!((e - root).normalize().dot(line) > 0.9999);
            assert!((e.distance(root) - length * REACH).abs() < 1e-4);
        }
        // Too close to fold that far: as folded as it goes, toward the target.
        let (_, e) = solved(root, mid, end, root + Vec3::new(0., -0.001, 0.), Vec3::Z);
        assert!(e.distance(root) < 0.05, "{e}");
    }

    #[test]
    fn a_chain_already_at_its_target_stays_put() {
        let [root, mid, end] = LEG;
        let (upper, lower) = two_bone(root, mid, end, end, Vec3::Z);
        assert!(upper.angle_between(Quat::IDENTITY) < 1e-3);
        assert!(lower.angle_between(Quat::IDENTITY) < 1e-3);
    }
}
