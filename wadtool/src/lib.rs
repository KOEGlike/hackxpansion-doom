//! Host build tool: rewrites the DOOM WAD for the hackxpansion console.
//!
//! The build scripts of `doom-core` (Chocolate engine) and `tiny` (raycaster)
//! both consume this so the stripped WAD format has a single source of truth.
//! Paths are resolved relative to the calling crate root (`../doom-sys/...`).

pub fn parse_header(src: &[u8]) -> (&[u8], usize, usize) {
    let numlumps = u32::from_le_bytes(src[4..8].try_into().unwrap()) as usize;
    let infotableofs = u32::from_le_bytes(src[8..12].try_into().unwrap()) as usize;
    (&src[0..4], numlumps, infotableofs)
}

/// Rewrites the WAD for the console:
/// 1. sprite lumps are quartered in resolution (point-sampled; the
///    renderer scales from the lump header so the on-screen size is unchanged),
/// 2. large stored lumps are LZ4-compressed, flagged via the top bit of their
///    directory position (dg_wadfile.c decompresses on demand),
/// 3. lumps belonging to stubbed-out subsystems (menus, intermission, status
///    bar, automap, demos) are dropped.
/// Map data, palettes and metadata stay raw and keep the zero-copy path.
pub fn compress_wad(input: &std::path::Path, output: &std::path::Path) {
    use lz4_flex::block::compress as lz4_compress;

    let src = std::fs::read(input).expect("failed to read the DOOM WAD");
    if src.len() < 12 || &src[0..4] != b"IWAD" {
        panic!("{} is not an IWAD", input.display());
    }
    let (_, numlumps, infotableofs) = parse_header(&src);

    // Pass 1: collect the kept lumps in directory order, tracking sections.
    let mut kept: Vec<(String, u32, u32)> = Vec::new(); // (name, off, size)
    let mut in_sprites = false;
    for i in 0..numlumps {
        let base = infotableofs + i * 16;
        let off = u32::from_le_bytes(src[base..base + 4].try_into().unwrap());
        let size = i32::from_le_bytes(src[base + 4..base + 8].try_into().unwrap()) as u32;
        let name = String::from_utf8_lossy(&src[base + 8..base + 16])
            .trim_end_matches('\0')
            .to_string();
        if name == "S_START" {
            in_sprites = true;
        } else if name == "S_END" {
            in_sprites = false;
        }
        if disabled_lump(&name) {
            continue;
        }
        kept.push((name, off, size));
        let _ = &mut in_sprites;
        if in_sprites {
            // remember for pass 2 (sprites are quartered)
        }
    }

    // Track sprite range again for the transform decision.
    let mut sprite_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, (name, _, _)) in kept.iter().enumerate() {
        let _ = i;
        if name == "S_START" {
            start = Some(i);
        } else if name == "S_END" && start.is_some() {
            sprite_ranges.push((start.unwrap(), i));
            start = None;
        }
    }
    let is_sprite = |i: usize| sprite_ranges.iter().any(|(a, b)| i > *a && i < *b);

    let mut flat_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, (name, _, _)) in kept.iter().enumerate() {
        if name == "F1_START" {
            start = Some(i);
        } else if name == "F1_END" && start.is_some() {
            flat_ranges.push((start.unwrap(), i));
            start = None;
        }
    }
    let is_flat = |i: usize| flat_ranges.iter().any(|(a, b)| i > *a && i < *b);

    // Wall patches stay raw (never LZ4): the tiny engine samples patch
    // columns straight from flash every frame, which needs random access
    // that a block-compressed blob cannot provide.
    let mut patch_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, (name, _, _)) in kept.iter().enumerate() {
        if name == "P_START" {
            start = Some(i);
        } else if name == "P_END" && start.is_some() {
            patch_ranges.push((start.unwrap(), i));
            start = None;
        }
    }
    let is_patch = |i: usize| patch_ranges.iter().any(|(a, b)| i > *a && i < *b);

    // Textures referenced by E1M1 (plus the sky and switch-animation
    // partners). TEXTURE1 is rebuilt to contain only these: every texture
    // costs static zone memory (column lookup tables) that the 144 KiB
    // console zone cannot afford for the full 125-texture set.
    let keep_textures = e1m1_textures(&src, numlumps, infotableofs);

    // Pass 2: transform and store.
    let mut directory: Vec<u8> = Vec::with_capacity(kept.len() * 16);
    let mut body: Vec<u8> = Vec::new();
    let mut dataofs = (12 + 16 * kept.len()) as u32;
    for (i, (name, off, size)) in kept.iter().enumerate() {
        let raw = &src[*off as usize..(*off + *size) as usize];
        // Sprites are quartered (halved twice): their world scale is
        // restored in r_data.c/r_main.c; wall patches stay full resolution
        // because TEXTURE1 geometry and the fixed 1-texel-per-unit wall
        // mapping depend on it. Flats are halved to 32x32 tiles (the span
        // addressing is adapted in r_draw.c).
        // Sprites are quartered (halved twice), flats and wall patches
        // halved once: uniform low-fi art where 1 texel covers 2 world
        // units on walls (the tiny engine samples with a 0.5 texel
        // scale). TEXTURE1 geometry is halved to match in
        // `halve_texture1` below.
        let transformed = if name == "TEXTURE1" {
            halve_texture1(&prune_texture1(raw, &keep_textures))
        } else if is_sprite(i) {
            halve_patch(&halve_patch(raw))
        } else if is_flat(i) {
            halve_flat(raw)
        } else if is_patch(i) && raw.len() >= 16 {
            halve_patch(raw)
        } else {
            raw.to_vec()
        };
        let stored_size_before_compression = transformed.len() as u32;
        let mut stored = transformed;
        let mut compressed = false;
        if stored.len() > 512 && compressible(name) && !is_patch(i) {
            let packed = lz4_compress(&stored);
            if packed.len() < stored.len() {
                stored = packed;
                compressed = true;
            }
        }
        let position = if compressed { dataofs | 0x8000_0000 } else { dataofs };
        if compressed {
            // length prefix so the reader can decode exactly this lump
            body.extend_from_slice(&((stored.len() as u32).to_le_bytes()));
        }
        body.extend_from_slice(&stored);
        dataofs += stored.len() as u32 + if compressed { 4 } else { 0 };
        let stored_size = stored_size_before_compression;
        directory.extend_from_slice(&position.to_le_bytes());
        directory.extend_from_slice(&stored_size.to_le_bytes());
        let mut name_bytes = [0_u8; 8];
        name_bytes[..name.len().min(8)].copy_from_slice(&name.as_bytes()[..name.len().min(8)]);
        directory.extend_from_slice(&name_bytes);
    }

    let mut out = Vec::with_capacity(12 + directory.len() + body.len());
    out.extend_from_slice(b"IWAD");
    out.extend_from_slice(&((kept.len() as u32).to_le_bytes()));
    out.extend_from_slice(&12_u32.to_le_bytes());
    out.extend_from_slice(&directory);
    out.extend_from_slice(&body);
    std::fs::write(output, &out).expect("failed to write the compressed WAD");
}

/// Lumps belonging to subsystems that are stubbed out on the console (menus,
/// intermission, status bar, automap, intro demos). They are not embedded.
pub fn disabled_lump(name: &str) -> bool {
    name.starts_with("M_")
        || name.starts_with("WI")
        || name.starts_with("BR")
        || name.starts_with("DEMO")
        || matches!(name, "TITLEPIC" | "CREDIT" | "HELP1" | "HELP2" | "ENDOOM")
        || name.starts_with("HELP")
        || name == "STBAR"
        || name.starts_with("STT")
        || name.starts_with("STG")
        || name.starts_with("STK")
        || name.starts_with("STY")
        || name.starts_with("STC")
        || name.starts_with("STDISK")
        || name.starts_with("STF") // status bar face graphics (status bar stubbed)
        || name == "STARMS" // ammo panel (status bar stubbed)
        || name.starts_with("STPB") // status bar buttons panel
        || name.starts_with("AMMNUM")
}

/// Which lumps are stored LZ4-compressed.
pub fn compressible(name: &str) -> bool {
    !matches!(name, "TEXTURE1" | "TEXTURE2" | "PNAMES" | "PLAYPAL" | "COLORMAP")
}

/// Half-resolution transform for a patch/sprite blob (point-sampled, keeping
/// the vanilla column layout). Returns the new blob.
pub fn halve_patch(src: &[u8]) -> Vec<u8> {
    fn le_u16(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes([b[at], b[at + 1]]) as usize
    }

    let width = le_u16(src, 0);
    let height = le_u16(src, 2);
    let left = i16::from_le_bytes([src[4], src[5]]);
    let top = i16::from_le_bytes([src[6], src[7]]);
    let columnofs: Vec<usize> = (0..width)
        .map(|i| {
            i32::from_le_bytes(src[8 + i * 4..12 + i * 4].try_into().unwrap()) as usize
        })
        .collect();

    let new_width = width / 2;
    let new_height = height / 2;

    // Reconstruct each source column (opacity + color), then point-sample it.
    let mut new_columns: Vec<Vec<u8>> = Vec::with_capacity(new_width);
    let mut new_opaque: Vec<Vec<bool>> = Vec::with_capacity(new_width);
    for cx in 0..new_width {
        let sx = cx * 2;
        let mut opaque = vec![false; height];
        let mut colors = vec![0_u8; height];
        let mut pos = columnofs[sx];
        loop {
            let topdelta = src[pos];
            if topdelta == 0xff {
                break;
            }
            let len = src[pos + 1] as usize;
            let pixels = pos + 3;
            for k in 0..len {
                if topdelta as usize + k < height {
                    opaque[topdelta as usize + k] = true;
                    colors[topdelta as usize + k] = src[pixels + k];
                }
            }
            pos += 3 + len + 1; // topdelta + length + pad + pixels + pad
        }
        let mut col: Vec<u8> = Vec::with_capacity(new_height);
        let mut col_opaque: Vec<bool> = Vec::with_capacity(new_height);
        for y in 0..new_height {
            if opaque[y * 2] {
                col.push(colors[y * 2]);
                col_opaque.push(true);
            } else {
                col.push(0);
                col_opaque.push(false);
            }
        }
        new_columns.push(col);
        new_opaque.push(col_opaque);
    }

    // Encode: header + columnofs + column posts.
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&(new_width as u16).to_le_bytes());
    out.extend_from_slice(&(new_height as u16).to_le_bytes());
    out.extend_from_slice(&((left / 2).to_le_bytes()));
    out.extend_from_slice(&((top / 2).to_le_bytes()));
    for _ in 0..new_width {
        out.extend_from_slice(&0_u32.to_le_bytes()); // placeholder columnofs
    }
    for (cx, col) in new_columns.iter().enumerate() {
        let opaque_run = &new_opaque[cx];
        let ofs = out.len() as u32;
        out[8 + cx * 4..12 + cx * 4].copy_from_slice(&ofs.to_le_bytes());
        let mut y = 0;
        while y < col.len() {
            if !opaque_run[y] {
                y += 1;
                continue;
            }
            let mut run = y;
            while run < col.len() && opaque_run[run] {
                run += 1;
            }
            let len = run - y;
            out.push(y as u8);
            out.push(len as u8);
            out.push(0_u8); // pad
            out.extend_from_slice(&col[y..run]);
            out.push(0_u8); // pad
            y = run;
        }
        out.push(0xff);
    }
    out
}

/// Point-sample a 64x64 flat tile down to 32x32 (the span renderers index
/// 32x32 tiles; see the xpanse changes in r_draw.c).
pub fn halve_flat(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() / 4);
    for y in (0..32).map(|y| y * 2) {
        for x in (0..32).map(|x| x * 2) {
            out.push(src[y * 64 + x]);
        }
    }
    out
}

pub fn lump_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_end_matches('\0').to_string()
}

/// Textures the engine may look up by name on the console: every texture
/// referenced by E1M1 sidedefs (P_SetupLevel fatals on missing names),
/// the episode sky (G_DoLoadLevel fatals), and the SW2 partners of used
/// SW1 switch textures so the exit switch still animates. Everything else
/// is dropped from TEXTURE1 to fit the 144 KiB zone.
pub fn e1m1_textures(src: &[u8], numlumps: usize, infotableofs: usize) -> std::collections::HashSet<String> {
    let mut keep = std::collections::HashSet::new();
    keep.insert("SKY1".to_string());
    // Locate the E1M1 map marker; SIDEDEFS is the third lump after it.
    let mut map_at = None;
    for i in 0..numlumps {
        let base = infotableofs + i * 16;
        if lump_name(&src[base + 8..base + 16]) == "E1M1" {
            map_at = Some(i);
            break;
        }
    }
    let map_at = map_at.expect("source WAD has no E1M1 map");
    let base = infotableofs + (map_at + 3) * 16;
    assert_eq!(lump_name(&src[base + 8..base + 16]), "SIDEDEFS");
    let off = u32::from_le_bytes(src[base..base + 4].try_into().unwrap()) as usize;
    let size = i32::from_le_bytes(src[base + 4..base + 8].try_into().unwrap()) as usize;
    assert_eq!(size % 30, 0);
    for j in 0..size / 30 {
        let e = &src[off + j * 30..off + (j + 1) * 30];
        for k in [4, 12, 20] {
            let t = lump_name(&e[k..k + 8]);
            if t != "-" && !t.is_empty() {
                keep.insert(t);
            }
        }
    }
    // Switch-animation partners (P_UseSpecial flips SW1 -> SW2).
    for name in keep.clone() {
        if let Some(rest) = name.strip_prefix("SW1") {
            keep.insert(format!("SW2{rest}"));
        }
    }
    // Every episode-1 switch pair: P_InitSwitchList resolves them with the
    // fatal R_TextureNumForName (its missing-texture guard is #if 0'd out).
    // Parsed from the C source so the list cannot drift out of sync.
    let switch_src = std::fs::read_to_string("../doom-sys/src/c/p_switch.c")
        .expect("failed to read p_switch.c for switch texture list");
    for line in switch_src.lines() {
        let line = line.trim();
        if !line.starts_with("{\"SW1") {
            continue;
        }
        let parts: Vec<&str> = line.split('"').collect();
        if parts.len() < 5 {
            continue;
        }
        let episode: u32 = line
            .trim_end_matches(|c| c == ',' || c == '}')
            .rsplit(',')
            .next()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(99);
        if episode <= 1 {
            keep.insert(parts[1].to_string());
            keep.insert(parts[3].to_string());
        }
    }
    keep
}

/// Rebuilds a TEXTURE1 lump containing only `keep` (in original order).
/// Entry layout: name[8], masked i32, width/height i16, columndirectory
/// i32 (obsolete), patchcount i16, then 10-byte patch records.
pub fn prune_texture1(src: &[u8], keep: &std::collections::HashSet<String>) -> Vec<u8> {
    fn le_i32(b: &[u8], at: usize) -> i32 {
        i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn le_i16(b: &[u8], at: usize) -> i16 {
        i16::from_le_bytes(b[at..at + 2].try_into().unwrap())
    }
    let numtextures = le_i32(src, 0) as usize;
    let mut entries: Vec<&[u8]> = Vec::new();
    for i in 0..numtextures {
        let o = le_i32(src, 4 + i * 4) as usize;
        let name = lump_name(&src[o..o + 8]);
        if !keep.contains(&name) {
            continue;
        }
        let patchcount = le_i16(src, o + 20) as usize;
        entries.push(&src[o..o + 22 + patchcount * 10]);
    }
    assert!(
        !entries.is_empty(),
        "texture prune removed every texture"
    );
    let mut out = Vec::new();
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    let mut ofs = 4 + entries.len() * 4;
    for e in &entries {
        out.extend_from_slice(&(ofs as u32).to_le_bytes());
        ofs += e.len();
    }
    for e in &entries {
        out.extend_from_slice(e);
    }
    out
}

/// Halves TEXTURE1 geometry to match halved wall patches: entry
/// width/height and every patch record's origin are integer-halved.
/// Patch data itself is halved separately by `halve_patch`.
pub fn halve_texture1(src: &[u8]) -> Vec<u8> {
    fn le_i32(b: &[u8], at: usize) -> i32 {
        i32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }
    fn le_i16(b: &[u8], at: usize) -> i16 {
        i16::from_le_bytes(b[at..at + 2].try_into().unwrap())
    }
    let numtextures = le_i32(src, 0) as usize;
    let mut out = src.to_vec();
    for i in 0..numtextures {
        let o = le_i32(src, 4 + i * 4) as usize;
        let w = le_i16(src, o + 12);
        let h = le_i16(src, o + 14);
        out[o + 12..o + 14].copy_from_slice(&(w / 2).to_le_bytes());
        out[o + 14..o + 16].copy_from_slice(&(h / 2).to_le_bytes());
        let patchcount = le_i16(src, o + 20) as usize;
        for j in 0..patchcount {
            let p = o + 22 + j * 10;
            let ox = le_i16(src, p);
            let oy = le_i16(src, p + 2);
            out[p..p + 2].copy_from_slice(&(ox / 2).to_le_bytes());
            out[p + 2..p + 4].copy_from_slice(&(oy / 2).to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn source_wad() -> Vec<u8> {
        // Tests run with CWD at the wadtool crate root.
        std::fs::read("../doom-core/assets/doom_e1m1.wad").expect("source WAD")
    }

    fn dir(src: &[u8]) -> Vec<(String, u32, u32)> {
        let (_, numlumps, infotableofs) = parse_header(src);
        (0..numlumps)
            .map(|i| {
                let base = infotableofs + i * 16;
                let off = u32::from_le_bytes(src[base..base + 4].try_into().unwrap());
                let size =
                    i32::from_le_bytes(src[base + 4..base + 8].try_into().unwrap()) as u32;
                (lump_name(&src[base + 8..base + 16]), off, size)
            })
            .collect()
    }

    #[test]
    fn texture_keep_set_covers_engine_lookups() {
        let src = source_wad();
        let (_, numlumps, infotableofs) = parse_header(&src);
        let keep = e1m1_textures(&src, numlumps, infotableofs);
        // Full shareware TEXTURE1 has 125 entries; E1M1 needs far fewer.
        assert!(keep.len() < 125 && !keep.is_empty(), "keep={}", keep.len());
        assert!(keep.contains("SKY1"), "episode sky must survive pruning");
        // Every episode-1 switch pair from the C source must resolve.
        let switch_src =
            std::fs::read_to_string("../doom-sys/src/c/p_switch.c").expect("p_switch.c");
        for line in switch_src.lines() {
            let line = line.trim();
            if !line.starts_with("{\"SW1") {
                continue;
            }
            let parts: Vec<&str> = line.split('"').collect();
            let episode: u32 = line
                .trim_end_matches(|c| c == ',' || c == '}')
                .rsplit(',')
                .next()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(99);
            if episode <= 1 {
                assert!(keep.contains(parts[1]), "missing {}", parts[1]);
                assert!(keep.contains(parts[3]), "missing {}", parts[3]);
            }
        }
    }

    #[test]
    fn pruned_texture1_is_verbatim_subset() {
        let src = source_wad();
        let (_, numlumps, infotableofs) = parse_header(&src);
        let keep = e1m1_textures(&src, numlumps, infotableofs);
        let raw = dir(&src)
            .iter()
            .find(|(n, _, _)| n == "TEXTURE1")
            .map(|(_, off, size)| src[*off as usize..*off as usize + *size as usize].to_vec())
            .expect("TEXTURE1");
        let pruned = prune_texture1(&raw, &keep);
        let nt = i32::from_le_bytes(pruned[0..4].try_into().unwrap()) as usize;
        assert_eq!(nt, keep.len());
        // Every surviving entry is byte-identical to its source entry.
        for i in 0..nt {
            let o = i32::from_le_bytes(pruned[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
            let name = lump_name(&pruned[o..o + 8]);
            assert!(keep.contains(&name));
            let pc = i16::from_le_bytes(pruned[o + 20..o + 22].try_into().unwrap()) as usize;
            let entry = &pruned[o..o + 22 + pc * 10];
            assert!(raw.windows(entry.len()).any(|w| w == entry));
        }
    }

    #[test]
    fn quartered_sprite_stays_decodable() {
        let src = source_wad();
        let lumps = dir(&src);
        // First sprite lump between S_START and S_END.
        let start = lumps.iter().position(|(n, _, _)| n == "S_START").unwrap();
        let (name, off, size) = lumps
            .iter()
            .skip(start + 1)
            .find(|(n, _, _)| n != "S_END")
            .unwrap()
            .clone();
        let raw = &src[off as usize..off as usize + size as usize];
        let w = u16::from_le_bytes([raw[0], raw[1]]) as usize;
        let h = u16::from_le_bytes([raw[2], raw[3]]) as usize;
        let q = halve_patch(&halve_patch(raw));
        let w2 = u16::from_le_bytes([q[0], q[1]]) as usize;
        let h2 = u16::from_le_bytes([q[2], q[3]]) as usize;
        assert_eq!((w2, h2), (w / 4, h / 4), "sprite {name}");
        // Walk every column post list; all reads must stay in bounds.
        for cx in 0..w2 {
            let mut pos =
                i32::from_le_bytes(q[8 + cx * 4..12 + cx * 4].try_into().unwrap()) as usize;
            assert!(pos < q.len());
            loop {
                let top = q[pos] as usize;
                if top == 0xff {
                    break;
                }
                let len = q[pos + 1] as usize;
                assert!(pos + 3 + len + 1 <= q.len());
                assert!(top + len <= h2 + 1);
                pos += 3 + len + 1;
            }
        }
    }

    #[test]
    fn compressed_wad_roundtrip() {
        let out = std::env::temp_dir().join("wadtool_test_doom.wad");
        compress_wad(
            std::path::Path::new("../doom-core/assets/doom_e1m1.wad"),
            &out,
        );
        let packed = std::fs::read(&out).expect("compressed wad");
        assert_eq!(&packed[0..4], b"IWAD");
        let lumps = dir(&packed);
        let names: HashSet<String> = lumps.iter().map(|(n, _, _)| n.clone()).collect();
        for need in ["E1M1", "THINGS", "TEXTURE1", "PNAMES", "PLAYPAL", "S_START", "F_START"] {
            assert!(names.contains(need), "missing {need}");
        }
        // LZ4-flagged lumps carry a 4-byte packed-length prefix that fits.
        let mut flagged = 0;
        for (name, off, size) in &lumps {
            if (*off as i32) < 0 {
                let pos = (*off & 0x7fff_ffff) as usize;
                let packed_len =
                    u32::from_le_bytes(packed[pos..pos + 4].try_into().unwrap()) as usize;
                assert!(pos + 4 + packed_len <= packed.len(), "{name}");
                assert!(*size > 0);
                flagged += 1;
            }
        }
        assert!(flagged > 0, "expected some LZ4 lumps");
        std::fs::remove_file(&out).ok();
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;

    fn tiny_wad() -> Vec<u8> {
        let out = std::env::temp_dir().join("wadtool_patch_test.wad");
        compress_wad(
            std::path::Path::new("../doom-core/assets/doom_e1m1.wad"),
            &out,
        );
        let bytes = std::fs::read(&out).expect("wad");
        std::fs::remove_file(&out).ok();
        bytes
    }

    #[test]
    fn wall_patches_stay_raw() {
        let packed = tiny_wad();
        let (_, numlumps, infotableofs) = parse_header(&packed);
        let mut in_patches = false;
        let mut raw_patches = 0;
        let mut total = 0;
        for i in 0..numlumps {
            let base = infotableofs + i * 16;
            let pos = u32::from_le_bytes(packed[base..base + 4].try_into().unwrap());
            let name = lump_name(&packed[base + 8..base + 16]);
            if name == "P_START" {
                in_patches = true;
                continue;
            }
            if name == "P_END" {
                in_patches = false;
                continue;
            }
            if in_patches {
                total += 1;
                assert_eq!(pos & 0x8000_0000, 0, "patch {name} must stay raw");
                raw_patches += 1;
            }
        }
        assert!(total > 50 && raw_patches == total);
    }
}

#[cfg(test)]
mod halving_tests {
    use super::*;

    fn source_wad() -> Vec<u8> {
        std::fs::read("../doom-core/assets/doom_e1m1.wad").expect("source WAD")
    }

    #[test]
    fn texture_geometry_halves_coherently() {
        let src = source_wad();
        let (_, numlumps, infotableofs) = parse_header(&src);
        let keep = e1m1_textures(&src, numlumps, infotableofs);
        let t1off = (0..numlumps)
            .map(|i| {
                let base = infotableofs + i * 16;
                (
                    lump_name(&src[base + 8..base + 16]),
                    u32::from_le_bytes(src[base..base + 4].try_into().unwrap()) as usize,
                )
            })
            .find(|(n, _)| n == "TEXTURE1")
            .expect("TEXTURE1")
            .1;
        // TEXTURE1 raw size from source dir
        let t1size = (0..numlumps)
            .map(|i| {
                let base = infotableofs + i * 16;
                (
                    lump_name(&src[base + 8..base + 16]),
                    i32::from_le_bytes(src[base + 4..base + 8].try_into().unwrap()) as usize,
                )
            })
            .find(|(n, _)| n == "TEXTURE1")
            .expect("size")
            .1;
        let raw = src[t1off..t1off + t1size].to_vec();
        let pruned = prune_texture1(&raw, &keep);
        let halved = halve_texture1(&pruned);
        let nt = i32::from_le_bytes(pruned[0..4].try_into().unwrap());
        assert_eq!(&halved[0..4], &pruned[0..4], "count preserved");
        assert_eq!(nt as usize, keep.len());
        for i in 0..nt as usize {
            let po = i32::from_le_bytes(pruned[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
            let ho = i32::from_le_bytes(halved[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
            // Entry widths/heights halved, patch origins halved, names kept.
            assert_eq!(&halved[ho..ho + 8], &pruned[po..po + 8]);
            let (pw, ph) = (
                i16::from_le_bytes(pruned[po + 12..po + 14].try_into().unwrap()),
                i16::from_le_bytes(pruned[po + 14..po + 16].try_into().unwrap()),
            );
            let (hw, hh) = (
                i16::from_le_bytes(halved[ho + 12..ho + 14].try_into().unwrap()),
                i16::from_le_bytes(halved[ho + 14..ho + 16].try_into().unwrap()),
            );
            assert_eq!((hw, hh), (pw / 2, ph / 2));
            let pc = i16::from_le_bytes(pruned[po + 20..po + 22].try_into().unwrap()) as usize;
            for j in 0..pc {
                let (pox, poy) = (
                    i16::from_le_bytes(pruned[po + 22 + j * 10..po + 24 + j * 10].try_into().unwrap()),
                    i16::from_le_bytes(pruned[po + 24 + j * 10..po + 26 + j * 10].try_into().unwrap()),
                );
                let (hox, hoy) = (
                    i16::from_le_bytes(halved[ho + 22 + j * 10..ho + 24 + j * 10].try_into().unwrap()),
                    i16::from_le_bytes(halved[ho + 24 + j * 10..ho + 26 + j * 10].try_into().unwrap()),
                );
                assert_eq!((hox, hoy), (pox / 2, poy / 2));
            }
        }
    }
}
