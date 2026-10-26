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
