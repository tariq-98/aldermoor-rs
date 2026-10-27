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
/// Conservative receiver/caster volume for one shadow map. Keep the camera's
/// near plane for every cascade so the shader's inter-cascade blend is covered.
pub struct ShadowCasterVolume {
    light: Frustum,
    receivers: Frustum,
    texel_axes: [Vec3; 2],
}
impl ShadowCasterVolume {
    pub fn new(
        light_vp: Mat4,
        camera_vp: Mat4,
        eye: Vec3,
        forward: Vec3,
        receiver_far: f32,
        resolution: u32,
    ) -> Self {
        let forward = forward.normalize();
        let mut receivers = Frustum::new(camera_vp);
        receivers.planes[5] = (-forward).extend(forward.dot(eye) + receiver_far);
        Self {
            light: Frustum::new(light_vp),
            receivers,
            // NDC spans two units. Bounds.half also needs a factor of two,
            // so these axes yield the full projected width/height in texels.
            texel_axes: [0, 1].map(|i| light_vp.row(i).truncate().abs() * resolution as f32),
        }
    }
    pub fn intersects(&self, bounds: Bounds, sun: Vec3, reject_small: bool) -> bool {
        // Preserve the existing 100 m caster-to-receiver sweep, but test it
        // against this cascade's receiver depth rather than the whole camera.
        // An offscreen caster can still shade an onscreen receiver.
        let swept = Bounds {
            center: bounds.center - sun * 50.,
            half: bounds.half + sun.abs() * 50.,
        };
        if !self.light.intersects(bounds) || !self.receivers.intersects(swept) {
            return false;
        }
        // Only mid/far cascades opt into the issue's two-texel size policy.
        // Use the larger dimension: a long thin caster must not disappear.
        // Bounds already include skin/wind padding; invalid sizes fail open.
        let [width, height] = self.texel_axes.map(|axis| axis.dot(bounds.half));
        !reject_small || !width.is_finite() || !height.is_finite() || width.max(height) >= 2.
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

/// The far layer and the matrix used to render it form one cached result.
#[derive(Default)]
pub(crate) struct FarShadowCache {
    rendered: Option<(usize, Mat4)>,
}
impl FarShadowCache {
    /// Reuse for at most one frame, and only within one texel of the stored volume.
    /// Checking all corners also catches sun rotation, FOV/aspect changes and depth drift.
    pub(crate) fn plan(
        &self,
        frame: usize,
        candidate: Mat4,
        resolution: u32,
        invalidated: bool,
    ) -> (Mat4, bool) {
        let Some((rendered_frame, cached)) = self.rendered else {
            return (candidate, true);
        };
        // Retrying an unsubmitted/failed frame must not reuse its encoded-only layer.
        if invalidated || frame <= rendered_frame || frame - rendered_frame >= 2 || resolution == 0
        {
            return (candidate, true);
        }
        let delta = candidate * cached.inverse();
        let texel = 2. / resolution as f32;
        for x in [-1., 1.] {
            for y in [-1., 1.] {
                for z in [0., 1.] {
                    let corner = Vec3::new(x, y, z);
                    let moved = delta.project_point3(corner);
                    if !moved.is_finite() || (moved - corner).abs().max_element() > texel {
                        return (candidate, true);
                    }
                }
            }
        }
        (cached, false)
    }
    /// Called only after the far encoder completes. A same-frame retry forces a redraw.
    pub(crate) fn encoded(&mut self, frame: usize, matrix: Mat4) {
        self.rendered = Some((frame, matrix));
    }
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
    fn shadow_volume(far: f32) -> ShadowCasterVolume {
        let camera = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 800.);
        let light = Mat4::from_scale(Vec3::new(0.01, 0.01, -0.005));
        ShadowCasterVolume::new(light, camera, Vec3::ZERO, -Vec3::Z, far, 1024)
    }
    #[test]
    fn shadow_receivers_are_bounded_per_cascade_but_keep_blend_overlap() {
        let caster = Bounds {
            center: Vec3::new(0., 0., -30.),
            half: Vec3::splat(0.5),
        };
        assert!(!shadow_volume(15.).intersects(caster, Vec3::Y, false));
        assert!(shadow_volume(50.).intersects(caster, Vec3::Y, false));
        // The mid map is sampled before the near split as part of blending.
        let overlap = Bounds {
            center: Vec3::new(0., 0., -14.),
            ..caster
        };
        assert!(shadow_volume(50.).intersects(overlap, Vec3::Y, false));
        assert!(shadow_volume(150.).intersects(caster, Vec3::Y, false));
    }
    #[test]
    fn shadow_volume_retains_offscreen_casters_and_rejects_unreachable_ones() {
        let volume = shadow_volume(50.);
        let caster = Bounds {
            center: Vec3::new(0., 20., -20.),
            half: Vec3::splat(0.5),
        };
        assert!(!volume.receivers.intersects(caster));
        assert!(volume.intersects(caster, Vec3::Y, false));
        assert!(!volume.intersects(caster, -Vec3::Y, false));
        let outside_light = Bounds {
            center: Vec3::new(101., 0., -20.),
            ..caster
        };
        assert!(!volume.intersects(outside_light, Vec3::X, false));
    }
    #[test]
    fn shadow_texel_rejection_preserves_near_and_long_thin_casters() {
        let volume = shadow_volume(50.);
        let small = Bounds {
            center: Vec3::new(0., 0., -20.),
            half: Vec3::splat(0.05),
        };
        assert!(volume.intersects(small, Vec3::Y, false));
        assert!(!volume.intersects(small, Vec3::Y, true));
        let thin = Bounds {
            half: Vec3::new(0.5, 0.01, 0.01),
            ..small
        };
        assert!(volume.intersects(thin, Vec3::Y, true));
        let boundary = Bounds {
            half: Vec3::splat(2. / 10.24),
            ..small
        };
        assert!(volume.intersects(boundary, Vec3::Y, true));
    }
    #[test]
    fn shadow_texel_footprint_includes_rotated_axes() {
        let camera = glam::camera::rh::proj::directx::perspective(1., 1., 0.1, 800.);
        let light = Mat4::from_scale(Vec3::new(0.01, 0.01, -0.005))
            * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4);
        let volume = ShadowCasterVolume::new(light, camera, Vec3::ZERO, -Vec3::Z, 50., 1024);
        let bounds = Bounds {
            center: Vec3::new(0., 0., -20.),
            half: Vec3::new(0.15, 0.15, 0.01),
        };
        assert!(volume.intersects(bounds, Vec3::Y, true));
        assert!(!shadow_volume(50.).intersects(bounds, Vec3::Y, true));
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

    #[test]
    fn far_shadow_refreshes_every_second_frame_and_keeps_the_rendered_matrix() {
        let mut cache = FarShadowCache::default();
        let original = Mat4::IDENTITY;
        assert_eq!(cache.plan(0, original, 1024, false), (original, true));
        cache.encoded(0, original);
        let subtexel = Mat4::from_translation(Vec3::X * 0.0005);
        assert_eq!(cache.plan(1, subtexel, 1024, false), (original, false));
        assert_eq!(cache.plan(2, subtexel, 1024, false), (subtexel, true));
        cache.encoded(2, subtexel);
        assert_eq!(cache.plan(3, original, 1024, false), (subtexel, false));
        assert!(cache.plan(4, original, 1024, false).1);
    }

    #[test]
    fn far_shadow_invalidates_cuts_resets_retries_and_frame_rewinds() {
        let mut cache = FarShadowCache::default();
        cache.encoded(10, Mat4::IDENTITY);
        assert!(cache.plan(11, Mat4::IDENTITY, 1024, true).1);
        assert!(cache.plan(10, Mat4::IDENTITY, 1024, false).1);
        assert!(cache.plan(9, Mat4::IDENTITY, 1024, false).1);
        assert!(!cache.plan(11, Mat4::IDENTITY, 1024, false).1);
    }

    #[test]
    fn far_shadow_measures_translation_rotation_and_projection_drift_in_texels() {
        let mut cache = FarShadowCache::default();
        cache.encoded(0, Mat4::IDENTITY);
        for changed in [
            Mat4::from_translation(Vec3::X * 0.003),
            Mat4::from_translation(Vec3::Y * 0.003),
            Mat4::from_translation(Vec3::Z * 0.003),
            Mat4::from_rotation_y(0.01),
            Mat4::from_scale(Vec3::new(1.01, 1., 1.)),
        ] {
            assert_eq!(cache.plan(1, changed, 1024, false), (changed, true));
        }
        let shift = Mat4::from_translation(Vec3::X * 0.001);
        assert!(!cache.plan(1, shift, 1024, false).1);
        assert!(cache.plan(1, shift, 4096, false).1);
    }

    #[test]
    fn far_shadow_refreshes_for_real_camera_and_sun_changes() {
        let eye = Vec3::new(0., 3., 8.);
        let target = Vec3::new(0., 1., 0.);
        let sun = Vec3::new(0.4, 0.8, 0.2).normalize();
        let original = stable_cascades(eye, target, 1., 1.6, sun, 1024)[2];
        let mut cache = FarShadowCache::default();
        cache.encoded(0, original);
        let tiny_move = stable_cascades(
            eye + Vec3::X * 0.0001,
            target + Vec3::X * 0.0001,
            1.,
            1.6,
            sun,
            1024,
        )[2];
        assert_eq!(cache.plan(1, tiny_move, 1024, false), (original, false));
        for candidate in [
            stable_cascades(
                eye + Vec3::X * 10.,
                target + Vec3::X * 10.,
                1.,
                1.6,
                sun,
                1024,
            )[2],
            stable_cascades(eye, target, 1.2, 1.6, sun, 1024)[2],
            stable_cascades(eye, target, 1., 2., sun, 1024)[2],
            stable_cascades(eye, target, 1., 1.6, Vec3::Y, 1024)[2],
        ] {
            assert!(cache.plan(1, candidate, 1024, false).1);
        }
    }

    #[test]
    fn far_shadow_never_reuses_invalid_projection_data() {
        let mut cache = FarShadowCache::default();
        cache.encoded(0, Mat4::ZERO);
        assert!(cache.plan(1, Mat4::IDENTITY, 1024, false).1);
        cache.encoded(0, Mat4::IDENTITY);
        let invalid = Mat4::from_cols_array(&[f32::NAN; 16]);
        assert!(cache.plan(1, invalid, 1024, false).1);
        assert!(cache.plan(1, Mat4::IDENTITY, 0, false).1);
    }
}
