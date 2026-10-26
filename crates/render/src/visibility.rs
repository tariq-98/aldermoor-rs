//! Camera/frustum maths and coherent projected-size LOD selection (no asset authoring).
use glam::{Mat4, Vec3, Vec4};
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub center: Vec3,
    pub half: Vec3,
}
impl Bounds {
    pub fn from_points(points: impl Iterator<Item = Vec3>) -> Self {
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for p in points {
            lo = lo.min(p);
            hi = hi.max(p)
        }
        if !lo.is_finite() {
            return Self {
                center: Vec3::ZERO,
                half: Vec3::ZERO,
            };
        }
        Self {
            center: (lo + hi) * 0.5,
            half: (hi - lo) * 0.5,
        }
    }
    pub fn transformed(self, m: Mat4) -> Self {
        Self {
            center: m.transform_point3(self.center),
            half: m.x_axis.truncate().abs() * self.half.x
                + m.y_axis.truncate().abs() * self.half.y
                + m.z_axis.truncate().abs() * self.half.z,
        }
    }
    pub fn padded(self, r: f32) -> Self {
        Self {
            half: self.half + Vec3::splat(r),
            ..self
        }
    }
}
pub struct Frustum {
    planes: [Vec4; 6],
}
impl Frustum {
    pub fn new(vp: Mat4) -> Self {
        let r0 = vp.row(0);
        let r1 = vp.row(1);
        let r2 = vp.row(2);
        let r3 = vp.row(3);
        Self {
            planes: [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2]
                .map(|p| p / p.truncate().length()),
        }
    }
    pub fn intersects(&self, b: Bounds) -> bool {
        self.planes
            .iter()
            .all(|p| p.truncate().dot(b.center) + p.w + p.truncate().abs().dot(b.half) >= 0.)
    }
}
pub fn screen_height(height: f32, depth: f32, fov: f32, pixels: u32) -> f32 {
    height * pixels as f32 / (2. * (fov * 0.5).tan() * depth.max(0.1))
}
pub fn lod_for_height(pixels: f32, previous: Option<usize>) -> usize {
    // Whole-model hysteresis prevents mesh/texture popping around 100/300 px.
    match previous {
        Some(0) if pixels >= 270. => 0,
        Some(1) if (90.0..330.).contains(&pixels) => 1,
        Some(2) if pixels <= 110. => 2,
        _ => {
            if pixels > 300. {
                0
            } else if pixels > 100. {
                1
            } else {
                2
            }
        }
    }
}
/// Last-frame selection and LOD state, bounded to characters in the current draws.
#[derive(Default)]
pub(crate) struct CharacterShadowState {
    pub selected: std::collections::HashSet<u32>,
    lods: std::collections::HashMap<u32, usize>,
}

/// Bound real character shadows to 24 near-detail characters (#75).
/// Existing members retain their shadow until a newcomer is appreciably nearer.
/// Distance rather than signed view depth retains nearby offscreen shadow casters.
pub(crate) fn near_character_shadows(
    characters: impl Iterator<Item = (u32, Bounds)>,
    eye: Vec3,
    fov: f32,
    pixels: u32,
    previous: &CharacterShadowState,
) -> CharacterShadowState {
    let mut state = CharacterShadowState::default();
    let mut near: Vec<_> = characters
        .filter_map(|(id, bounds)| {
            let distance = bounds.center.distance_squared(eye);
            let height = bounds.half.y * 2.;
            if !distance.is_finite() || !height.is_finite() || height <= 0. {
                return None;
            }
            let lod = lod_for_height(
                screen_height(height, distance.sqrt(), fov, pixels),
                previous.lods.get(&id).copied(),
            );
            state.lods.insert(id, lod);
            let rank = distance
                * if previous.selected.contains(&id) {
                    0.8
                } else {
                    1.
                };
            (lod < 2).then_some((id, rank))
        })
        .collect();
    near.sort_unstable_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    state.selected = near.into_iter().take(24).map(|(id, _)| id).collect();
    state
}

pub fn stable_cascades(
    eye: Vec3,
    target: Vec3,
    fov: f32,
    aspect: f32,
    sun: Vec3,
    resolution: u32,
) -> [Mat4; 3] {
    let forward = (target - eye).normalize();
    let sun = sun.normalize();
    let up = if sun.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let light = glam::camera::rh::view::look_at_mat4(Vec3::ZERO, -sun, up);
    let splits = [0.1, 15., 50., 150.];
    std::array::from_fn(|i| {
        let near = splits[i];
        let far = splits[i + 1];
        let center = eye + forward * (near + far) * 0.5;
        let tan = (fov * 0.5).tan();
        let radius =
            ((far * far * tan * tan * (1. + aspect * aspect) + (far - near).powi(2) * 0.25).sqrt()
                * 16.)
                .ceil()
                / 16.;
        let c = light.transform_point3(center);
        let texel = radius * 2. / resolution as f32;
        let x = (c.x / texel).round() * texel;
        let y = (c.y / texel).round() * texel;
        glam::camera::rh::proj::directx::orthographic(
            x - radius,
            x + radius,
            y - radius,
            y + radius,
            -c.z - radius - 100.,
            -c.z + radius + 100.,
        ) * light
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frustum_checks_near_far_and_side_planes() {
        let f = Frustum::new(Mat4::IDENTITY);
        assert!(f.intersects(Bounds {
            center: Vec3::new(0., 0., 0.5),
            half: Vec3::splat(0.1)
        }));
        for p in [
            Vec3::new(2., 0., 0.5),
            Vec3::new(0., 0., -1.),
            Vec3::new(0., 0., 2.),
        ] {
            assert!(!f.intersects(Bounds {
                center: p,
                half: Vec3::splat(0.1)
            }));
        }
    }
    #[test]
    fn lod_thresholds_have_hysteresis() {
        assert_eq!(lod_for_height(350., None), 0);
        assert_eq!(lod_for_height(200., None), 1);
        assert_eq!(lod_for_height(70., None), 2);
        assert_eq!(lod_for_height(290., Some(0)), 0);
        assert_eq!(lod_for_height(310., Some(1)), 1);
        assert_eq!(lod_for_height(105., Some(2)), 2);
    }
    #[test]
    fn crowd_shadows_choose_nearest_characters_independent_of_input_order() {
        let bounds = |distance| Bounds {
            center: Vec3::Z * distance,
            half: Vec3::new(0.4, 0.9, 0.4),
        };
        let input: Vec<_> = (0..40)
            .map(|id| (id, bounds(1. + id as f32 * 0.1)))
            .collect();
        let a = near_character_shadows(
            input.iter().copied(),
            Vec3::ZERO,
            1.,
            1200,
            &CharacterShadowState::default(),
        )
        .selected;
        let b = near_character_shadows(
            input.iter().rev().copied(),
            Vec3::ZERO,
            1.,
            1200,
            &CharacterShadowState::default(),
        )
        .selected;
        assert_eq!(a, b);
        assert_eq!(a, (0..24).collect());
        let same_distance = (0..40).rev().map(|id| (id, bounds(2.)));
        assert_eq!(
            near_character_shadows(
                same_distance,
                Vec3::ZERO,
                1.,
                1200,
                &CharacterShadowState::default()
            )
            .selected,
            (0..24).collect()
        );
        let outside = [(0, bounds(100.)), (1, bounds(f32::NAN)), (2, bounds(-2.))];
        assert_eq!(
            near_character_shadows(
                outside.into_iter(),
                Vec3::ZERO,
                1.,
                1200,
                &CharacterShadowState::default()
            )
            .selected,
            [2].into_iter().collect()
        );
    }

    #[test]
    fn crowd_shadow_rank_hysteresis_survives_oscillation_but_admits_nearer_casters() {
        let bounds = |distance| Bounds {
            center: Vec3::Z * distance,
            half: Vec3::new(0.4, 0.9, 0.4),
        };
        let input = |last, newcomer| {
            (0..23)
                .map(|id| (id, bounds(2.)))
                .chain([(23, bounds(last)), (24, bounds(newcomer))])
        };
        let mut state = near_character_shadows(
            input(5., 5.01),
            Vec3::ZERO,
            1.,
            1200,
            &CharacterShadowState::default(),
        );
        let initial = state.selected.clone();
        for frame in 0..32 {
            let movement = if frame % 2 == 0 { 0.02 } else { -0.02 };
            state = near_character_shadows(
                input(5. + movement, 5. - movement),
                Vec3::ZERO,
                1.,
                1200,
                &state,
            );
            assert_eq!(state.selected, initial);
        }
        state = near_character_shadows(input(5., 4.), Vec3::ZERO, 1., 1200, &state);
        assert_eq!(state.selected.len(), 24);
        assert!(state.selected.contains(&24));
        assert!(!state.selected.contains(&23));
        // Disappearing entities cannot leave stale state or reserve a slot.
        state =
            near_character_shadows([(23, bounds(5.))].into_iter(), Vec3::ZERO, 1., 1200, &state);
        assert_eq!(state.selected, [23].into_iter().collect());
        assert_eq!(state.lods.len(), 1);
    }
    #[test]
    fn crowd_shadow_lod_hysteresis_survives_boundary_oscillation() {
        let bounds = |pixels| Bounds {
            center: Vec3::Z * (1.8 * 1200. / (2. * (0.5_f32).tan() * pixels)),
            half: Vec3::new(0.4, 0.9, 0.4),
        };
        let select = |pixels, previous: &CharacterShadowState| {
            near_character_shadows(
                [(7, bounds(pixels))].into_iter(),
                Vec3::ZERO,
                1.,
                1200,
                previous,
            )
        };
        let mut state = select(120., &CharacterShadowState::default());
        for pixels in [99., 101., 95., 105., 91.] {
            state = select(pixels, &state);
            assert!(state.selected.contains(&7));
        }
        state = select(89., &state);
        assert!(state.selected.is_empty());
        for pixels in [99., 101., 105., 109.] {
            state = select(pixels, &state);
            assert!(state.selected.is_empty());
        }
        state = select(111., &state);
        assert!(state.selected.contains(&7));
    }

    #[test]
    fn cascade_xy_stays_still_for_subtexel_translation() {
        let eye = Vec3::new(0., 3., 8.);
        let target = Vec3::new(0., 1., 0.);
        let a = stable_cascades(eye, target, 1., 3.5, Vec3::Y, 1024);
        let b = stable_cascades(
            eye + Vec3::X * 0.0001,
            target + Vec3::X * 0.0001,
            1.,
            3.5,
            Vec3::Y,
            1024,
        );
        for (i, j) in a.iter().zip(b) {
            assert!((i.row(0) - j.row(0)).length() < 1e-6);
            assert!((i.row(1) - j.row(1)).length() < 1e-6);
        }
    }
}
