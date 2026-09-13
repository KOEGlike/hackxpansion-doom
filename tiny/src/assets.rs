//! Decoders for WAD art: palette, colormaps, textures and patches.
//!
//! All lump data stays in flash; decoders borrow it. Lookup tables fill
//! caller-provided fixed arrays: no heap allocation anywhere here.

use crate::wad::Wad;

fn le_i16(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}

fn le_u16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn le_i32(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// First palette (256 RGB triplets) from PLAYPAL, filled in place.
pub fn palette_into(wad: Wad, out: &mut [[u8; 3]; 256]) -> Option<()> {
    let idx = wad.find("PLAYPAL")?;
    let raw = wad.raw(idx)?;
    if raw.len() < 768 {
        return None;
    }
    for (i, entry) in out.iter_mut().enumerate() {
        entry.copy_from_slice(&raw[i * 3..i * 3 + 3]);
    }
    Some(())
}

/// Colormap rows (each 256 bytes mapping palette index -> shaded index).
/// Shareware COLORMAP holds 34 maps; index 0 is full-bright.
pub fn colormaps(wad: Wad<'_>) -> Option<&'_ [u8]> {
    let idx = wad.find("COLORMAP")?;
    wad.raw(idx)
}

/// Shade a palette index by light level 0..255 using the colormap ladder.
/// `light` follows the vanilla convention (higher = brighter); colormap
/// row 0 is full-bright, so bright lights use low rows.
pub fn shade(colormaps: &[u8], maps: usize, light: u8, color: u8) -> u8 {
    let map = 31u8.saturating_sub(light >> 3) as usize;
    let map = map.min(maps.saturating_sub(1));
    colormaps[map * 256 + color as usize]
}

/// Maximum textures (E1M1 pruned set is ~70).
pub const MAX_TEXTURES: usize = 128;
/// Patch-layer pool across all textures (kept set uses ~500 records;
/// LITE3 alone has 64). Fails closed on exhaustion.
pub const MAX_TEXPATCHES: usize = 768;

/// One resolved texture patch layer.
#[derive(Clone, Copy, Debug, Default)]
pub struct TexPatch {
    /// WAD lump index, or `u16::MAX` when the patch is missing from the WAD
    /// (tolerated: the layer is skipped).
    pub lump: u16,
    pub ox: i16,
    pub oy: i16,
}

impl TexPatch {
    pub const fn zero() -> Self {
        Self {
            lump: 0,
            ox: 0,
            oy: 0,
        }
    }
}

/// A resolved texture definition from TEXTURE1. Patch layers live in a
/// shared pool ([`MAX_TEXPATCHES`]) at `patch_start`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Texture {
    pub name: [u8; 8],
    pub width: u16,
    pub height: u16,
    pub patch_start: u16,
    pub patch_count: u16,
}

impl Texture {
    pub const fn zero() -> Self {
        Self {
            name: [0; 8],
            width: 0,
            height: 0,
            patch_start: 0,
            patch_count: 0,
        }
    }
}

/// Parses the (possibly pruned) TEXTURE1 lump into `out`, resolving patch
/// names to lump indices (`u16::MAX` when absent) into `pool`.
/// `tmp` decodes the lump. Returns the texture count.
pub fn textures_into(
    wad: Wad,
    tmp: &mut [u8],
    out: &mut [Texture; MAX_TEXTURES],
    pool: &mut [TexPatch; MAX_TEXPATCHES],
) -> Option<usize> {
    let idx = wad.find("TEXTURE1")?;
    let raw: &[u8] = if wad.entry(idx)?.compressed {
        let n = wad.read_into(idx, tmp)?;
        &tmp[..n]
    } else {
        wad.raw(idx)?
    };
    if raw.len() < 4 {
        return None;
    }
    // PNAMES stays zero-copy in flash.
    let pn_idx = wad.find("PNAMES")?;
    let pn_raw = wad.raw(pn_idx)?;
    if pn_raw.len() < 4 {
        return None;
    }
    let npnames = le_i32(&pn_raw[0..4]) as usize;
    if 4 + npnames * 8 > pn_raw.len() {
        return None;
    }
    let pname = |i: usize| -> Option<[u8; 8]> {
        if i >= npnames {
            return None;
        }
        let mut name = [0u8; 8];
        name.copy_from_slice(&pn_raw[4 + i * 8..12 + i * 8]);
        Some(name)
    };
    let n = le_i32(&raw[0..4]) as usize;
    if n > MAX_TEXTURES {
        return None;
    }
    let mut pool_used = 0usize;
    for (i, slot) in out.iter_mut().take(n).enumerate() {
        let o = le_i32(&raw[4 + i * 4..8 + i * 4]) as usize;
        if o + 22 > raw.len() {
            return None;
        }
        slot.name.copy_from_slice(&raw[o..o + 8]);
        slot.width = le_u16(&raw[o + 12..o + 14]);
        slot.height = le_u16(&raw[o + 14..o + 16]);
        let patchcount = le_i16(&raw[o + 20..o + 22]) as usize;
        if o + 22 + patchcount * 10 > raw.len() {
            return None;
        }
        if pool_used + patchcount > MAX_TEXPATCHES {
            return None;
        }
        slot.patch_start = pool_used as u16;
        slot.patch_count = patchcount as u16;
        for j in 0..patchcount {
            let p = o + 22 + j * 10;
            let ox = le_i16(&raw[p..p + 2]);
            let oy = le_i16(&raw[p + 2..p + 4]);
            let pi = le_i16(&raw[p + 4..p + 6]) as usize;
            let lump = pname(pi)
                .and_then(|pn| {
                    let s = crate::map::name_str(&pn);
                    wad.find(&s)
                })
                .unwrap_or(u16::MAX as usize) as u16;
            pool[pool_used] = TexPatch { lump, ox, oy };
            pool_used += 1;
        }
    }
    Some(n)
}

/// Walks the posts of column `col` of a patch lump, calling `f` for
/// every opaque pixel `(y, palette_index)`. Reads straight from flash.
pub fn draw_column<F>(patch: &[u8], col: usize, mut f: F) -> bool
where
    F: FnMut(usize, u8),
{
    if patch.len() < 8 {
        return false;
    }
    let w = le_u16(&patch[0..2]) as usize;
    let h = le_u16(&patch[2..4]) as usize;
    if col >= w || 8 + w * 4 > patch.len() {
        return false;
    }
    let mut pos = le_i32(&patch[8 + col * 4..12 + col * 4]) as usize;
    if pos >= patch.len() {
        return false;
    }
    loop {
        let top = patch[pos] as usize;
        if top == 0xff {
            break;
        }
        if pos + 2 >= patch.len() {
            return false;
        }
        let len = patch[pos + 1] as usize;
        if pos + 3 + len > patch.len() {
            return false;
        }
        for k in 0..len {
            if top + k < h {
                f(top + k, patch[pos + 3 + k]);
            }
        }
        pos += 3 + len + 1;
    }
    true
}

/// Flat pixel at (x, y) with power-of-two wrapping. Flats in the stripped
/// WAD are halved to 32x32 by `wadtool` (`F1_START`..`F1_END` section).
pub fn flat_pixel(flat: &[u8], x: usize, y: usize) -> u8 {
    flat[(y & 31) * 32 + (x & 31)]
}

/// Size class of a flat blob.
pub fn flat_size(flat: &[u8]) -> usize {
    if flat.len() == 1024 {
        32
    } else {
        64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wad::Wad;

    include!(concat!(env!("OUT_DIR"), "/wad.rs"));

    #[test]
    fn palette_and_colormaps() {
        use crate::wad::{Entry, MAX_LUMPS};
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
        let mut pal = [[0u8; 3]; 256];
        palette_into(wad, &mut pal).expect("palette");
        assert_eq!(pal[0], [0, 0, 0]);
        assert!(pal.iter().any(|e| *e != [0, 0, 0]));
        let maps = colormaps(wad).expect("colormaps");
        assert!(maps.len() >= 32 * 256);
        // Map 0 is the identity map.
        assert_eq!(maps[0], 0);
        assert_eq!(maps[4], 4);
        assert_eq!(maps[255], 255);
        let _ = shade(maps, maps.len() / 256, 200, 4);
    }

    #[test]
    fn texture_directory_parses() {
        use crate::wad::{Entry, MAX_LUMPS};
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
        let mut tmp = [0u8; 32768];
        let mut tex = [Texture::zero(); MAX_TEXTURES];
        let mut pool = [TexPatch::zero(); MAX_TEXPATCHES];
        let n = textures_into(wad, &mut tmp, &mut tex, &mut pool).expect("textures");
        assert!(n > 30);
        let mut sky = false;
        let mut lite3_full = false;
        for t in &tex[..n] {
            assert!(t.width > 0 && t.width <= 512);
            assert!(t.height > 0 && t.height <= 256);
            assert!(t.patch_count > 0);
            if t.name == *b"SKY1\0\0\0\0" {
                sky = true;
            }
            // LITE3 exercises the pooled record path (64 layers).
            if t.name == *b"LITE3\0\0\0" {
                assert_eq!(t.patch_count, 64);
                lite3_full = true;
            }
        }
        assert!(sky);
        assert!(lite3_full, "LITE3 must load all 64 layers");
    }

    #[test]
    fn patch_columns_decode() {
        use crate::wad::{Entry, MAX_LUMPS};
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
        // Resolve any present patch lump via PNAMES-free scan of P_ section.
        let mut idx = None;
        let mut in_p = false;
        for i in 0..wad.len() {
            let n = wad.entry(i).unwrap().name_str();
            if n == "P_START" {
                in_p = true;
            } else if n == "P_END" {
                break;
            } else if in_p && wad.entry(i).unwrap().size > 0 {
                idx = Some(i);
                break;
            }
        }
        let idx = idx.expect("a patch lump");
        let mut buf = [0u8; 32768];
        let n = wad.read_into(idx, &mut buf).expect("read patch");
        let w = le_u16(&buf[0..2]) as usize;
        assert!(w > 0 && w <= 256);
        let mut pixels = 0;
        for col in 0..w {
            let mut count = 0;
            assert!(draw_column(&buf[..n], col, |_, _| {
                count += 1;
            }));
            pixels += count;
        }
        assert!(pixels > 0, "patch produced no pixels");
    }

    #[test]
    fn flats_sample() {
        use crate::wad::{Entry, MAX_LUMPS};
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
        // First flat between F_START and F_END.
        let mut idx = None;
        let mut inside = false;
        for i in 0..wad.len() {
            let n = wad.entry(i).unwrap().name_str();
            if n == "F_START" {
                inside = true;
            } else if n == "F_END" {
                break;
            } else if inside && wad.entry(i).unwrap().size > 0 {
                idx = Some(i);
                break;
            }
        }
        let idx = idx.expect("flat lump");
        let mut buf = [0u8; 4096];
        let n = wad.read_into(idx, &mut buf).expect("read flat");
        assert_eq!(n, 1024, "expected a halved 32x32 flat");
        assert_eq!(flat_size(&buf[..n]), 32);
        let _ = flat_pixel(&buf, 3, 5);
        let _ = flat_pixel(&buf, 70, 130);
    }
}
