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

/// Terrain detail follows distance to the closest horizontal point of the chunk,
/// keeping the ground under the camera full resolution. Ten-percent bands avoid
/// toggling around the 64 m / 160 m mesh-density thresholds.
pub(crate) fn terrain_lod(bounds: Bounds, eye: Vec3, previous: Option<usize>) -> usize {
    let delta = ((eye - bounds.center).abs() - bounds.half).max(Vec3::ZERO);
    let distance = Vec3::new(delta.x, 0., delta.z).length();
    if !distance.is_finite() {
        return 0;
    }
    match previous {
        Some(0) if distance <= 70.4 => 0,
        Some(1) if (57.6..=176.).contains(&distance) => 1,
        Some(2) if distance >= 144. => 2,
        _ if distance <= 64. => 0,
        _ if distance <= 160. => 1,
        _ => 2,
    }
}

/// Near matches the scene, mid is at least half detail, far is quarter detail.
pub(crate) fn terrain_shadow_lod(scene: usize, cascade: usize) -> usize {
    scene.max(cascade)
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
    fn terrain_lod_retains_levels_around_both_boundaries() {
        let bounds = Bounds {
            center: Vec3::ZERO,
            half: Vec3::new(32., 10., 32.),
        };
        let select =
            |distance, previous| terrain_lod(bounds, Vec3::new(32. + distance, 20., 0.), previous);
        assert_eq!(terrain_lod(bounds, Vec3::new(0., 100., 0.), None), 0);
        assert_eq!(select(80., None), 1);
        assert_eq!(select(200., None), 2);
        let mut level = 0;
        for d in [63., 65., 64., 70.] {
            level = select(d, Some(level));
            assert_eq!(level, 0);
        }
        level = select(71., Some(level));
        assert_eq!(level, 1);
        for d in [63., 65., 58., 159., 161., 175.] {
            level = select(d, Some(level));
            assert_eq!(level, 1);
        }
        level = select(177., Some(level));
        assert_eq!(level, 2);
        for d in [159., 161., 145.] {
            level = select(d, Some(level));
            assert_eq!(level, 2);
        }
        assert_eq!(select(143., Some(level)), 1);
        assert_eq!(select(57., Some(1)), 0);
        assert_eq!(select(300., Some(0)), 2); // camera teleport
        assert_eq!(select(0., Some(2)), 0);
        for scene in 0..=2 {
            for cascade in 0..3 {
                let shadow = terrain_shadow_lod(scene, cascade);
                assert!(shadow >= scene && shadow <= 2);
                if cascade == 2 {
                    assert_eq!(shadow, 2);
                }
            }
        }
    }

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
}
