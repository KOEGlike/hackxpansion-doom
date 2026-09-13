//! Reader for the stripped console WAD produced by `wadtool`.
//!
//! Directory entries keep the vanilla 16-byte layout, except the top bit of
//! the position flags an LZ4-compressed lump: the stored blob is then
//! `[u32 packed_len][lz4 block]` and the size field is the *uncompressed*
//! size. Everything else is raw and safe to reference zero-copy.
//!
//! Zero-heap design: the directory parses into caller-provided storage and
//! lump decoding writes into caller buffers (`lz4_flex::decompress_into`).
//! The console heap cannot back `Vec`-based WAD access.

/// Maximum directory entries (E1M1 uses ~404).
pub const MAX_LUMPS: usize = 512;

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// One directory entry.
#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    /// Raw 8-byte lump name (NUL-padded).
    pub name: [u8; 8],
    /// Byte offset of the stored blob from the start of the file.
    pub pos: u32,
    /// Uncompressed size in bytes.
    pub size: u32,
    /// True when the blob is `[u32 packed_len][lz4 block]`.
    pub compressed: bool,
}

impl Entry {
    /// Lump name without trailing NULs, lossy-decoded.
    pub fn name_str(&self) -> alloc::string::String {
        let s = alloc::string::String::from_utf8_lossy(&self.name);
        alloc::string::String::from(s.trim_end_matches('\0'))
    }

    pub const fn zero() -> Self {
        Self {
            name: [0; 8],
            pos: 0,
            size: 0,
            compressed: false,
        }
    }
}

/// Parsed WAD directory over borrowed bytes (plus borrowed entry storage).
/// Small and `Copy`: pass by value.
#[derive(Clone, Copy)]
pub struct Wad<'a> {
    bytes: &'a [u8],
    entries: &'a [Entry],
}

impl<'a> Wad<'a> {
    /// Parses the header + directory into `storage`. Returns the reader plus
    /// the entry count. `None` on malformed input or directory overflow.
    pub fn parse(bytes: &'a [u8], storage: &'a mut [Entry; MAX_LUMPS]) -> Option<Self> {
        if bytes.len() < 12 || &bytes[0..4] != b"IWAD" {
            return None;
        }
        let n = le_u32(&bytes[4..8]) as usize;
        let io = le_u32(&bytes[8..12]) as usize;
        if n > MAX_LUMPS {
            return None;
        }
        if io.checked_add(n.checked_mul(16)?)? > bytes.len() {
            return None;
        }
        for (i, slot) in storage.iter_mut().take(n).enumerate() {
            let base = io + i * 16;
            let pos = le_u32(&bytes[base..base + 4]);
            let size = le_u32(&bytes[base + 4..base + 8]);
            // NOTE: sizes never set the top bit (lumps are far below 2 GiB).
            let mut name = [0u8; 8];
            name.copy_from_slice(&bytes[base + 8..base + 16]);
            *slot = Entry {
                name,
                pos: pos & 0x7fff_ffff,
                size,
                compressed: pos & 0x8000_0000 != 0,
            };
        }
        Some(Self {
            bytes,
            entries: &storage[..n],
        })
    }

    /// Rebuilds a reader over already-parsed storage (e.g. static storage
    /// filled once at boot). Shared borrows only: always sound to remake.
    pub fn from_parts(bytes: &'a [u8], entries: &'a [Entry]) -> Self {
        Self { bytes, entries }
    }

    /// Number of directory entries (including markers).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Index of the first lump with exactly this name.
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut key = [0u8; 8];
        let n = name.len().min(8);
        key[..n].copy_from_slice(&name.as_bytes()[..n]);
        self.entries.iter().position(|e| e.name == key)
    }

    /// Directory entry by index.
    pub fn entry(&self, idx: usize) -> Option<&Entry> {
        self.entries.get(idx)
    }

    /// Raw stored bytes of an *uncompressed* lump (zero-copy into flash).
    /// Returns `None` for compressed lumps; use [`Wad::read_into`] there.
    pub fn raw(&self, idx: usize) -> Option<&'a [u8]> {
        let e = self.entries.get(idx)?;
        if e.compressed {
            return None;
        }
        let end = e.pos as usize + e.size as usize;
        if end > self.bytes.len() {
            return None;
        }
        Some(&self.bytes[e.pos as usize..end])
    }

    /// Reads a lump into `out`, decompressing LZ4 blobs on demand.
    /// Returns the byte count on success, `None` on malformed data,
    /// undersized `out`, or size mismatch.
    pub fn read_into(&self, idx: usize, out: &mut [u8]) -> Option<usize> {
        let e = self.entries.get(idx)?;
        let size = e.size as usize;
        if !e.compressed {
            let pos = e.pos as usize;
            if pos + size > self.bytes.len() || out.len() < size {
                return None;
            }
            out[..size].copy_from_slice(&self.bytes[pos..pos + size]);
            return Some(size);
        }
        let pos = e.pos as usize;
        if pos + 4 > self.bytes.len() || out.len() < size {
            return None;
        }
        let packed_len = le_u32(&self.bytes[pos..pos + 4]) as usize;
        if pos + 4 + packed_len > self.bytes.len() {
            return None;
        }
        match lz4_flex::block::decompress_into(
            &self.bytes[pos + 4..pos + 4 + packed_len],
            &mut out[..size],
        ) {
            Ok(written) if written == size => Some(size),
            _ => None,
        }
    }

    /// Indices of the 10 map lumps following a map marker (`E1M1`, ...).
    pub fn map_lumps(&self, marker: &str) -> Option<[usize; 10]> {
        let at = self.find(marker)?;
        if at + 10 >= self.entries.len() {
            return None;
        }
        let mut out = [0usize; 10];
        for (k, slot) in out.iter_mut().enumerate() {
            *slot = at + 1 + k;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    include!(concat!(env!("OUT_DIR"), "/wad.rs"));

    fn open<'a>(entries: &'a mut [Entry; MAX_LUMPS]) -> Wad<'a> {
        Wad::parse(DOOM_WAD, entries).expect("valid wad")
    }

    #[test]
    fn parses_stripped_wad() {
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = open(&mut storage);
        assert!(wad.len() > 300);
        for need in ["E1M1", "TEXTURE1", "PNAMES", "PLAYPAL", "COLORMAP"] {
            assert!(wad.find(need).is_some(), "missing {need}");
        }
        // Raw lumps are zero-copy views into the image.
        let pal = wad.find("PLAYPAL").unwrap();
        assert!(!wad.entry(pal).unwrap().compressed);
        assert_eq!(wad.raw(pal).unwrap().len() as u32, wad.entry(pal).unwrap().size);
        // Oversized directories are rejected, not truncated.
        let mut fake = [0u8; 12];
        fake[0..4].copy_from_slice(b"IWAD");
        fake[4..8].copy_from_slice(&(MAX_LUMPS as u32 + 1).to_le_bytes());
        let mut storage = [Entry::zero(); MAX_LUMPS];
        assert!(Wad::parse(&fake, &mut storage).is_none());
        assert!(Wad::parse(&DOOM_WAD[..11], &mut storage).is_none());
    }

    #[test]
    fn every_lump_reads_back() {
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = open(&mut storage);
        let mut buf = [0u8; 32768];
        let mut compressed = 0;
        for i in 0..wad.len() {
            let e = wad.entry(i).unwrap();
            if e.size == 0 {
                continue;
            }
            assert!(
                (e.size as usize) <= buf.len(),
                "lump {} exceeds scratch",
                e.name_str()
            );
            let n = wad
                .read_into(i, &mut buf)
                .unwrap_or_else(|| panic!("lump {} failed", e.name_str()));
            assert_eq!(n as u32, e.size, "size mismatch {}", e.name_str());
            if e.compressed {
                compressed += 1;
            }
        }
        // Wall patches stay raw for zero-copy sampling; sprites, flats
        // and map data remain LZ4-compressed.
        assert!(compressed > 10, "expected some LZ4 lumps");
    }

    #[test]
    fn map_lumps_resolve() {
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = open(&mut storage);
        let lumps = wad.map_lumps("E1M1").expect("E1M1");
        let mut buf = [0u8; 32768];
        for idx in lumps {
            let e = wad.entry(idx).unwrap();
            assert!((e.size as usize) <= buf.len(), "map lump too big");
            let n = wad.read_into(idx, &mut buf).expect("map lump");
            assert!(n > 0);
        }
    }
}
