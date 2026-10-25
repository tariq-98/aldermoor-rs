//! Noise and randomness matching the Swift original bit for bit, so terrain and placements agree.

#[inline]
pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smoothstep: 0 below `a`, 1 above `b`, an S-curve between.
#[inline]
pub fn smooth(a: f32, b: f32, v: f32) -> f32 {
    let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A hash of a grid cell to 0…1.
#[inline]
pub fn hash(x: i64, y: i64, seed: i64) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(374_761_393)
        .wrapping_add((y as u32).wrapping_mul(668_265_263))
        .wrapping_add((seed as u32).wrapping_mul(2_246_822_519));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

/// Smooth value noise.
#[inline]
pub fn noise(x: f32, y: f32, seed: i64) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i64, fy as i64);
    let a = smooth(0.0, 1.0, x - fx);
    let b = smooth(0.0, 1.0, y - fy);
    mix(
        mix(hash(ix, iy, seed), hash(ix + 1, iy, seed), a),
        mix(hash(ix, iy + 1, seed), hash(ix + 1, iy + 1, seed), a),
        b,
    )
}

/// Three octaves of noise.
#[inline]
pub fn fbm(x: f32, y: f32, seed: i64) -> f32 {
    noise(x, y, seed) * 0.57 + noise(x * 2.1, y * 2.1, seed + 3) * 0.28 + noise(x * 4.3, y * 4.3, seed + 9) * 0.15
}

/// The renderer's placement sequence (a 64-bit linear congruential generator).
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next_unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 32) as u32) as f32 / u32::MAX as f32
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        mix(a, b, self.next_unit())
    }
}
