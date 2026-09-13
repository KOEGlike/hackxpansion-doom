//! Runtime level data: map structs, resolved textures, decoded flats.
//!
//! Everything lives in fixed-size static-friendly arrays with const-zero
//! initializers: the console heap cannot back `Vec`-based level data, and
//! big stack temporaries would overflow task stacks. Callers keep a
//! zeroed [`Level`] (e.g. in static storage) and fill it with
//! [`load_into`].

use crate::assets::{self, TexPatch, Texture};
use crate::map::{self, Map};
use crate::wad::Wad;

pub use crate::assets::{MAX_TEXPATCHES, MAX_TEXTURES};

/// Maximum decoded flats (E1M1 sectors reference ~22).
pub const MAX_FLATS: usize = 24;
/// Decoded flat pool: 24 halved 32x32 flats.
pub const FLAT_POOL: usize = MAX_FLATS * 1024;

/// Decoded flat directory entry.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlatEntry {
    pub name: [u8; 8],
    /// Byte offset into the flat pool.
    pub offset: u16,
}

impl FlatEntry {
    pub const fn zero() -> Self {
        Self {
            name: [0; 8],
            offset: 0,
        }
    }
}

/// The loaded level: map, textures, flats, palette.
///
/// Borrows colormaps from the WAD image, which is `'static` on firmware
/// (and in host tests via `include_bytes!`), so a `Level<'static>` can
/// live in static storage.
pub struct Level<'a> {
    pub map: Map,
    /// RGB565 conversion LUT, computed once from PLAYPAL.
    pub rgb_lut: [u16; 256],
    /// Colormap ladder straight from flash (zero-copy). `None` until
    /// boot; `Option` keeps the zero initializer BSS-resident (a `&[]`
    /// fat pointer would drag the whole `Level` into `.data`).
    pub colormaps: Option<&'a [u8]>,
    pub colormap_count: usize,
    pub textures: [Texture; MAX_TEXTURES],
    pub texture_count: usize,
    pub texpatches: [TexPatch; MAX_TEXPATCHES],
    pub flats: [FlatEntry; MAX_FLATS],
    pub flat_count: usize,
    pub flat_pool: [u8; FLAT_POOL],
    pub flat_pool_used: usize,
}

impl Level<'_> {
    /// Zeroed level: valid empty state for static storage (BSS).
    pub const ZERO: Self = Self {
        map: Map::ZERO,
        rgb_lut: [0; 256],
        colormaps: None,
        colormap_count: 0,
        textures: [Texture::zero(); MAX_TEXTURES],
        texture_count: 0,
        texpatches: [TexPatch::zero(); MAX_TEXPATCHES],
        flats: [FlatEntry::zero(); MAX_FLATS],
        flat_count: 0,
        flat_pool: [0; FLAT_POOL],
        flat_pool_used: 0,
    };

    /// Borrowed colormap ladder + row count. Only valid after boot
    /// (`load_into` fails closed without COLORMAP, so this is reachable
    /// exactly when rendering runs).
    pub fn cmap(&self) -> (&[u8], usize) {
        (self.colormaps.expect("level not booted"), self.colormap_count)
    }

    /// Finds a texture index by raw name.
    pub fn texture_num(&self, name: &[u8; 8]) -> Option<usize> {
        self.textures[..self.texture_count]
            .iter()
            .position(|t| &t.name == name)
    }

    /// Finds a decoded flat by raw name.
    pub fn flat_num(&self, name: &[u8; 8]) -> Option<usize> {
        self.flats[..self.flat_count]
            .iter()
            .position(|f| &f.name == name)
    }

    /// Decoded flat pixels (32x32).
    pub fn flat(&self, idx: usize) -> &[u8] {
        let e = &self.flats[idx];
        &self.flat_pool[e.offset as usize..e.offset as usize + 1024]
    }

    /// True when the ceiling pic is the sky flat.
    pub fn is_sky(name: &[u8; 8]) -> bool {
        name == b"F_SKY1\0\0"
    }
}

/// Loads everything for `marker` (e.g. `"E1M1"`) into `level` (zeroed):
/// map, pruned TEXTURE1, flats referenced by sectors plus the sky flat.
/// `scratch` decodes one lump at a time (>= largest map lump, 20 KiB+).
pub fn load_into<'a>(level: &mut Level<'a>, wad: Wad<'a>, marker: &str, scratch: &mut [u8]) -> Option<()> {
    level.map.load_into(&wad, marker, scratch)?;
    let cmap_raw = assets::colormaps(wad)?;
    level.colormaps = Some(cmap_raw);
    level.colormap_count = cmap_raw.len() / 256;

    // RGB565 LUT from the first palette.
    let mut pal = [[0u8; 3]; 256];
    assets::palette_into(wad, &mut pal)?;
    for (i, e) in pal.iter().enumerate() {
        level.rgb_lut[i] = ((u16::from(e[0] >> 3)) << 11)
            | ((u16::from(e[1] >> 2)) << 5)
            | (u16::from(e[2] >> 3));
    }

    // Resolve texture definitions against PNAMES + lump directory.
    let ntex = assets::textures_into(wad, scratch, &mut level.textures, &mut level.texpatches)?;
    level.texture_count = ntex;

    // Decode every flat referenced by the map plus the sky flat.
    level.flat_count = 0;
    level.flat_pool_used = 0;
    // Collect wanted names without allocating: bounded scan, dedupe inline.
    let mut want: [[u8; 8]; MAX_FLATS + 1] = [[0; 8]; MAX_FLATS + 1];
    let mut want_count = 0usize;
    let mut push_want = |name: [u8; 8]| {
        if want[..want_count].iter().any(|w| w == &name) {
            return true;
        }
        if want_count >= want.len() {
            return false;
        }
        want[want_count] = name;
        want_count += 1;
        true
    };
    for s in level.map.sectors() {
        if !push_want(s.floor_pic) || !push_want(s.ceil_pic) {
            return None;
        }
    }
    if !push_want(*b"F_SKY1\0\0") {
        return None;
    }
    for name in want.iter().take(want_count) {
        if level.flat_count >= MAX_FLATS {
            return None;
        }
        let idx = wad.find(&map::name_str(name))?;
        let n = wad.read_into(idx, scratch)?;
        if n != 1024 {
            return None;
        }
        let off = level.flat_pool_used;
        level.flat_pool[off..off + 1024].copy_from_slice(&scratch[..1024]);
        level.flats[level.flat_count] = FlatEntry {
            name: *name,
            offset: off as u16,
        };
        level.flat_count += 1;
        level.flat_pool_used += 1024;
    }

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wad::{Entry, Wad, MAX_LUMPS};

    include!(concat!(env!("OUT_DIR"), "/wad.rs"));

    #[test]
    fn level_loads() {
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
        let mut scratch = [0u8; 24576];
        let mut level = Level::ZERO;
        load_into(&mut level, wad, "E1M1", &mut scratch).expect("level loads");
        assert!(level.texture_count > 30);
        assert!(level.texture_num(b"SKY1\0\0\0\0").is_some());
        assert!(level.flat_count > 5);
        // Every sector flat resolved.
        for s in level.map.sectors() {
            assert!(level.flat_num(&s.floor_pic).is_some());
            assert!(level.flat_num(&s.ceil_pic).is_some());
        }
    }
}
