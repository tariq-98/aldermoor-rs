//! Read native ASTC KTX2 supplied by the asset build. No compression or transcoding here.
use anyhow::{Context, Result, ensure};
use std::{ops::Range, path::Path};
#[derive(Clone)]
pub struct Ktx {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub srgb: bool,
    pub levels: Vec<Range<usize>>,
}
impl Ktx {
    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(std::fs::read(path)?).with_context(|| format!("KTX2 {}", path.display()))
    }
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        ensure!(
            bytes.len() >= 104 && bytes[..12] == *b"\xABKTX 20\xBB\r\n\x1A\n",
            "invalid KTX2 header"
        );
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let format = u32_at(12);
        ensure!(
            format == 165 || format == 166,
            "expected native ASTC 6x6 UNORM or sRGB"
        );
        ensure!(
            u32_at(16) == 1
                && u32_at(28) == 0
                && u32_at(32) == 0
                && u32_at(36) == 1
                && u32_at(44) == 0,
            "expected 2D ASTC without arrays, cubes or supercompression"
        );
        let (width, height, n) = (u32_at(20), u32_at(24), u32_at(40));
        ensure!(
            width > 0 && height > 0 && width <= 16384 && height <= 16384,
            "invalid KTX extent"
        );
        ensure!(
            n > 0 && n <= 1 + width.max(height).ilog2(),
            "invalid KTX mip count"
        );
        ensure!(bytes.len() >= 80 + n as usize * 24, "truncated mip index");
        let mut levels = Vec::new();
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        for i in 0..n as usize {
            let o = 80 + i * 24;
            let offset = usize::try_from(u64_at(o))?;
            let length = usize::try_from(u64_at(o + 8))?;
            let uncompressed = usize::try_from(u64_at(o + 16))?;
            let w = (width >> i).max(1) as usize;
            let h = (height >> i).max(1) as usize;
            let expected = w.div_ceil(6) * h.div_ceil(6) * 16;
            ensure!(
                length == expected
                    && uncompressed == expected
                    && offset >= 80 + n as usize * 24
                    && offset % 16 == 0,
                "invalid ASTC level layout"
            );
            let end = offset.checked_add(length).context("KTX mip overflow")?;
            ensure!(end <= bytes.len(), "truncated ASTC level");
            ensure!(
                !levels
                    .iter()
                    .any(|r: &Range<usize>| offset < r.end && end > r.start),
                "overlapping KTX levels"
            );
            levels.push(offset..end);
        }
        Ok(Self {
            bytes,
            width,
            height,
            srgb: format == 166,
            levels,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // Structural parser fixture, not an authored image: no on-disk texture is created.
    fn header() -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..12].copy_from_slice(b"\xABKTX 20\xBB\r\n\x1A\n");
        for (o, v) in [(12, 165u32), (16, 1), (20, 6), (24, 6), (36, 1), (40, 1)] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes())
        }
        for (o, v) in [(80, 112u64), (88, 16), (96, 16)] {
            b[o..o + 8].copy_from_slice(&v.to_le_bytes())
        }
        b
    }
    #[test]
    fn reads_native_blocks_and_rejects_truncation() {
        let b = header();
        let k = Ktx::parse(b.clone()).unwrap();
        assert_eq!(k.levels[0], 112..128);
        assert!(!k.srgb);
        assert!(Ktx::parse(b[..120].to_vec()).is_err());
    }
    #[test]
    fn refuses_supercompression_and_bad_lengths() {
        let mut b = header();
        b[44] = 1;
        assert!(Ktx::parse(b).is_err());
        let mut b = header();
        b[88] = 8;
        assert!(Ktx::parse(b).is_err());
    }
}
