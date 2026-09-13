//! E1M1 map data decoded into engine-friendly structs.
//!
//! Coordinates convert from map units (i16) to `f32` world units once at
//! load; the renderer and physics then never touch fixed point.
//!
//! All storage is fixed-size arrays: the console heap cannot back
//! `Vec`-based level data. Capacities cover E1M1 with margin; `load`
//! fails cleanly if a map exceeds them.

use crate::wad::Wad;

pub const MAX_VERTEXES: usize = 640;
pub const MAX_LINES: usize = 640;
pub const MAX_SIDES: usize = 768;
pub const MAX_SECTORS: usize = 160;
pub const MAX_THINGS: usize = 192;
pub const MAX_NODES: usize = 256;
pub const MAX_SUBSECTORS: usize = 320;
pub const MAX_SEGS: usize = 896;
pub const MAX_BMAP_BLOCKS: usize = 1024;
pub const MAX_BMAP_LIST: usize = 4096;

fn le_i16(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}

fn le_u16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

/// Map vertex in world units.
#[derive(Clone, Copy, Debug, Default)]
pub struct Vertex {
    pub x: f32,
    pub y: f32,
}

/// One-sided wall definition.
#[derive(Clone, Copy, Debug, Default)]
pub struct Side {
    pub xoff: i16,
    pub yoff: i16,
    pub top: [u8; 8],
    pub mid: [u8; 8],
    pub bot: [u8; 8],
    pub sector: u16,
}

/// Line segment with front (and optional back) side.
#[derive(Clone, Copy, Debug, Default)]
pub struct Line {
    pub v1: u16,
    pub v2: u16,
    pub flags: u16,
    pub special: u16,
    pub tag: u16,
    pub front: u16,
    /// Back side index, or `u16::MAX` for single-sided lines.
    pub back: u16,
}

/// Sector: floor/ceiling heights, flats, light.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sector {
    pub floor_h: f32,
    pub ceil_h: f32,
    pub floor_pic: [u8; 8],
    pub ceil_pic: [u8; 8],
    pub light: u8,
    pub special: u16,
    pub tag: u16,
}

/// Thing placement (player starts, monsters, items, decorations).
#[derive(Clone, Copy, Debug, Default)]
pub struct Thing {
    pub x: f32,
    pub y: f32,
    pub angle_deg: i16,
    pub kind: u16,
    pub flags: u16,
}

/// BSP node for rendering traversal.
#[derive(Clone, Copy, Debug, Default)]
pub struct Node {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    /// Right then left bounding boxes: [top, bottom, left, right].
    pub bbox: [[f32; 4]; 2],
    /// Child indices: subsector bit (0x8000) set means leaf.
    pub child: [u16; 2],
}

/// Subsector: a run of segs.
#[derive(Clone, Copy, Debug, Default)]
pub struct Subsector {
    pub first_seg: u16,
    pub seg_count: u16,
}

/// A seg: directed edge with linedef/side references.
///
/// The SEGS lump's `linedef`/`side` fields are authoritative (verified
/// against the source WAD: every entry is in range and its direction
/// matches its line). [`Map::resolve_segs`] only culls degenerate segs
/// and off-line mini-segs (partition edges that close subsectors but lie
/// on no linedef); those keep `linedef = u16::MAX` and are skipped at
/// render.
#[derive(Clone, Copy, Debug, Default)]
pub struct Seg {
    pub v1: u16,
    pub v2: u16,
    pub linedef: u16,
    pub side: u8,
}

/// Blockmap for collision queries: per-block line lists.
#[derive(Clone, Debug)]
pub struct Blockmap {
    pub org_x: f32,
    pub org_y: f32,
    pub width: u16,
    pub height: u16,
    /// Byte offsets (into `lists`) of each block's list.
    pub offsets: [u16; MAX_BMAP_BLOCKS],
    pub block_count: usize,
    /// Concatenated u16 line indices, each list 0xffff-terminated.
    pub lists: [u16; MAX_BMAP_LIST],
    pub lists_used: usize,
}

impl Blockmap {
    pub const fn zero() -> Self {
        Self {
            org_x: 0.0,
            org_y: 0.0,
            width: 0,
            height: 0,
            offsets: [0; MAX_BMAP_BLOCKS],
            block_count: 0,
            lists: [0; MAX_BMAP_LIST],
            lists_used: 0,
        }
    }
}

impl Default for Blockmap {
    fn default() -> Self {
        Self {
            org_x: 0.0,
            org_y: 0.0,
            width: 0,
            height: 0,
            offsets: [0; MAX_BMAP_BLOCKS],
            block_count: 0,
            lists: [0; MAX_BMAP_LIST],
            lists_used: 0,
        }
    }
}

impl Blockmap {
    /// Line indices in block `(bx, by)`, or `None` outside the map.
    pub fn lines_in(&self, bx: isize, by: isize) -> Option<&[u16]> {
        if bx < 0 || by < 0 || bx >= self.width as isize || by >= self.height as isize {
            return None;
        }
        let start = self.offsets[(by as usize) * (self.width as usize) + (bx as usize)] as usize;
        let mut end = start;
        while end < self.lists_used && self.lists[end] != 0xffff {
            end += 1;
        }
        Some(&self.lists[start..end])
    }
}

fn name8(b: &[u8]) -> [u8; 8] {
    let mut n = [0u8; 8];
    n.copy_from_slice(&b[..8]);
    n
}

pub fn name_str(n: &[u8; 8]) -> alloc::string::String {
    let s = alloc::string::String::from_utf8_lossy(n);
    alloc::string::String::from(s.trim_end_matches('\0'))
}

/// Which side of a partition line a point is on (vanilla R_PointOnSide).
/// Returns 0 or 1, matching the `child` slot of [`Node`].
pub fn point_on_side(px: f32, py: f32, n: &Node) -> usize {
    if n.dx == 0.0 {
        return if px <= n.x {
            (n.dy > 0.0) as usize
        } else {
            (n.dy < 0.0) as usize
        };
    }
    if n.dy == 0.0 {
        return if py <= n.y {
            (n.dx < 0.0) as usize
        } else {
            (n.dx > 0.0) as usize
        };
    }
    let x = px - n.x;
    let y = py - n.y;
    (y * n.dx >= x * n.dy) as usize
}


impl Vertex { pub const fn zero() -> Self { Self { x: 0.0, y: 0.0 } } }
impl Line { pub const fn zero() -> Self { Self { v1: 0, v2: 0, flags: 0, special: 0, tag: 0, front: 0, back: 0 } } }
impl Side {
    pub const fn zero() -> Self {
        Self { xoff: 0, yoff: 0, top: [0; 8], mid: [0; 8], bot: [0; 8], sector: 0 }
    }
}
impl Sector {
    pub const fn zero() -> Self {
        Self { floor_h: 0.0, ceil_h: 0.0, floor_pic: [0; 8], ceil_pic: [0; 8], light: 0, special: 0, tag: 0 }
    }
}
impl Thing { pub const fn zero() -> Self { Self { x: 0.0, y: 0.0, angle_deg: 0, kind: 0, flags: 0 } } }
impl Node {
    pub const fn zero() -> Self {
        Self { x: 0.0, y: 0.0, dx: 0.0, dy: 0.0, bbox: [[0.0; 4]; 2], child: [0; 2] }
    }
}
impl Subsector { pub const fn zero() -> Self { Self { first_seg: 0, seg_count: 0 } } }
impl Seg { pub const fn zero() -> Self { Self { v1: 0, v2: 0, linedef: 0, side: 0 } } }

/// Macro-free fixed-array push helper.
macro_rules! push {
    ($arr:expr, $len:expr, $cap:expr, $val:expr) => {{
        if $len >= $cap {
            return None;
        }
        $arr[$len] = $val;
        $len += 1;
    }};
}

/// The loaded level geometry.
#[derive(Debug)]
pub struct Map {
    pub vertexes: [Vertex; MAX_VERTEXES],
    pub vertex_count: usize,
    pub lines: [Line; MAX_LINES],
    pub line_count: usize,
    pub sides: [Side; MAX_SIDES],
    pub side_count: usize,
    pub sectors: [Sector; MAX_SECTORS],
    pub sector_count: usize,
    pub things: [Thing; MAX_THINGS],
    pub thing_count: usize,
    pub nodes: [Node; MAX_NODES],
    pub node_count: usize,
    pub subsectors: [Subsector; MAX_SUBSECTORS],
    pub subsector_count: usize,
    pub segs: [Seg; MAX_SEGS],
    pub seg_count: usize,
    pub blockmap: Blockmap,
}

impl Default for Map {
    fn default() -> Self {
        Self {
            vertexes: [Vertex::default(); MAX_VERTEXES],
            vertex_count: 0,
            lines: core::array::from_fn(|_| Line::default()),
            line_count: 0,
            sides: core::array::from_fn(|_| Side::default()),
            side_count: 0,
            sectors: core::array::from_fn(|_| Sector::default()),
            sector_count: 0,
            things: [Thing::default(); MAX_THINGS],
            thing_count: 0,
            nodes: [Node::default(); MAX_NODES],
            node_count: 0,
            subsectors: [Subsector::default(); MAX_SUBSECTORS],
            subsector_count: 0,
            segs: [Seg::default(); MAX_SEGS],
            seg_count: 0,
            blockmap: Blockmap::default(),
        }
    }
}

impl Map {
    /// Zeroed map: valid empty state for static storage (BSS).
    pub const ZERO: Self = Self {
        vertexes: [Vertex::zero(); MAX_VERTEXES],
        vertex_count: 0,
        lines: [Line::zero(); MAX_LINES],
        line_count: 0,
        sides: [Side::zero(); MAX_SIDES],
        side_count: 0,
        sectors: [Sector::zero(); MAX_SECTORS],
        sector_count: 0,
        things: [Thing::zero(); MAX_THINGS],
        thing_count: 0,
        nodes: [Node::zero(); MAX_NODES],
        node_count: 0,
        subsectors: [Subsector::zero(); MAX_SUBSECTORS],
        subsector_count: 0,
        segs: [Seg::zero(); MAX_SEGS],
        seg_count: 0,
        blockmap: Blockmap::zero(),
    };

    /// Loads the map starting at `marker` (e.g. `"E1M1"`) into `self`
    /// (which must be zeroed: [`Map::ZERO`]).
    /// `scratch` is reusable lump-decoding storage (>= largest map lump).
    pub fn load_into(&mut self, wad: &Wad, marker: &str, scratch: &mut [u8]) -> Option<()> {
        let map = self;
        let lumps = wad.map_lumps(marker)?;

        // Order after the marker: THINGS LINEDEFS SIDEDEFS VERTEXES SEGS
        // SSECTORS NODES SECTORS REJECT BLOCKMAP.

        let blob_3 = match wad.read_into(lumps[3], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_3].chunks_exact(4) {
            push!(
                map.vertexes,
                map.vertex_count,
                MAX_VERTEXES,
                Vertex {
                    x: le_i16(&c[0..2]) as f32,
                    y: le_i16(&c[2..4]) as f32,
                }
            );
        }
        let blob_2 = match wad.read_into(lumps[2], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_2].chunks_exact(30) {
            push!(
                map.sides,
                map.side_count,
                MAX_SIDES,
                Side {
                    xoff: le_i16(&c[0..2]),
                    yoff: le_i16(&c[2..4]),
                    // mapsidedef_t order: top, BOTTOM, mid.
                    top: name8(&c[4..12]),
                    bot: name8(&c[12..20]),
                    mid: name8(&c[20..28]),
                    sector: le_u16(&c[28..30]),
                }
            );
        }
        let blob_1 = match wad.read_into(lumps[1], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_1].chunks_exact(14) {
            let back = le_u16(&c[12..14]);
            push!(
                map.lines,
                map.line_count,
                MAX_LINES,
                Line {
                    v1: le_u16(&c[0..2]),
                    v2: le_u16(&c[2..4]),
                    flags: le_u16(&c[4..6]),
                    special: le_u16(&c[6..8]),
                    tag: le_u16(&c[8..10]),
                    front: le_u16(&c[10..12]),
                    back: if back == 0xffff { u16::MAX } else { back },
                }
            );
        }
        let blob_7 = match wad.read_into(lumps[7], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_7].chunks_exact(26) {
            push!(
                map.sectors,
                map.sector_count,
                MAX_SECTORS,
                Sector {
                    floor_h: le_i16(&c[0..2]) as f32,
                    ceil_h: le_i16(&c[2..4]) as f32,
                    floor_pic: name8(&c[4..12]),
                    ceil_pic: name8(&c[12..20]),
                    light: le_u16(&c[20..22]) as u8,
                    special: le_u16(&c[22..24]),
                    tag: le_u16(&c[24..26]),
                }
            );
        }
        let blob_0 = match wad.read_into(lumps[0], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_0].chunks_exact(10) {
            push!(
                map.things,
                map.thing_count,
                MAX_THINGS,
                Thing {
                    x: le_i16(&c[0..2]) as f32,
                    y: le_i16(&c[2..4]) as f32,
                    angle_deg: le_i16(&c[4..6]),
                    kind: le_u16(&c[6..8]),
                    flags: le_u16(&c[8..10]),
                }
            );
        }
        let blob_4 = match wad.read_into(lumps[4], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_4].chunks_exact(12) {
            push!(
                map.segs,
                map.seg_count,
                MAX_SEGS,
                // seg_t: v1, v2, angle, linedef, side, offset (int16 each).
                Seg {
                    v1: le_u16(&c[0..2]),
                    v2: le_u16(&c[2..4]),
                    linedef: le_u16(&c[6..8]),
                    side: c[8],
                }
            );
        }
        let blob_5 = match wad.read_into(lumps[5], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_5].chunks_exact(4) {
            push!(
                map.subsectors,
                map.subsector_count,
                MAX_SUBSECTORS,
                Subsector {
                    first_seg: le_u16(&c[2..4]),
                    seg_count: le_u16(&c[0..2]),
                }
            );
        }
        let blob_6 = match wad.read_into(lumps[6], scratch) {
            Some(n) => n,
            None => return None,
        };
        for c in scratch[..blob_6].chunks_exact(28) {
            let mut bbox = [[0.0; 4]; 2];
            for b in 0..2 {
                for k in 0..4 {
                    bbox[b][k] = le_i16(&c[8 + b * 8 + k * 2..10 + b * 8 + k * 2]) as f32;
                }
            }
            push!(
                map.nodes,
                map.node_count,
                MAX_NODES,
                Node {
                    x: le_i16(&c[0..2]) as f32,
                    y: le_i16(&c[2..4]) as f32,
                    dx: le_i16(&c[4..6]) as f32,
                    dy: le_i16(&c[6..8]) as f32,
                    bbox,
                    child: [le_u16(&c[24..26]), le_u16(&c[26..28])],
                }
            );
        }
        // BLOCKMAP: header (orgx, orgy, width, height) then per-block
        // offsets (in shorts from lump start); each list starts with a 0
        // word and ends with 0xffff.
        let blob_9 = match wad.read_into(lumps[9], scratch) {
            Some(n) => n,
            None => return None,
        };
        let lump = &scratch[..blob_9];
        if lump.len() < 8 {
            return None;
        }
        let width = le_u16(&lump[4..6]) as usize;
        let height = le_u16(&lump[6..8]) as usize;
        if width * height > MAX_BMAP_BLOCKS {
            return None;
        }
        let bm = &mut map.blockmap;
        bm.org_x = le_i16(&lump[0..2]) as f32;
        bm.org_y = le_i16(&lump[2..4]) as f32;
        bm.width = width as u16;
        bm.height = height as u16;
        bm.block_count = width * height;
        for b in 0..width * height {
            let at = le_u16(&lump[8 + b * 2..10 + b * 2]) as usize * 2;
            if at + 2 > lump.len() {
                return None;
            }
            bm.offsets[b] = bm.lists_used as u16;
            let mut p = at;
            if le_u16(&lump[p..p + 2]) == 0 {
                p += 2;
            }
            loop {
                if p + 2 > lump.len() || bm.lists_used >= MAX_BMAP_LIST {
                    return None;
                }
                let v = le_u16(&lump[p..p + 2]);
                p += 2;
                bm.lists[bm.lists_used] = v;
                bm.lists_used += 1;
                if v == 0xffff {
                    break;
                }
            }
        }

        if map.vertex_count == 0 || map.line_count == 0 || map.sector_count == 0 {
            return None;
        }
        map.resolve_segs();
        Some(())
    }

    /// Validates every seg's stored linedef + side (see the [`Seg`]
    /// note). Degenerate segs and off-line mini-segs keep
    /// `linedef = u16::MAX` (skipped at render).
    fn resolve_segs(&mut self) {
        let nv = self.vertex_count.max(1);
        for si in 0..self.seg_count {
            let seg = self.segs[si];
            if (seg.v1 as usize) >= self.vertex_count
                || (seg.v2 as usize) >= self.vertex_count
                || (seg.linedef as usize) >= self.line_count
                || seg.side > 1
            {
                self.segs[si].linedef = u16::MAX;
                continue;
            }
            let a = self.vertexes[seg.v1 as usize % nv];
            let b = self.vertexes[seg.v2 as usize % nv];
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            if dx * dx + dy * dy < 0.01 {
                self.segs[si].linedef = u16::MAX;
                continue;
            }
            // The stored linedef must actually contain the seg (mini-segs
            // close subsectors along partition lines and lie on no
            // linedef: skip them so they never draw as fake walls).
            let line = self.lines[seg.linedef as usize];
            if (line.v1 as usize) >= self.vertex_count
                || (line.v2 as usize) >= self.vertex_count
            {
                self.segs[si].linedef = u16::MAX;
                continue;
            }
            let lv1 = self.vertexes[line.v1 as usize % nv];
            let lv2 = self.vertexes[line.v2 as usize % nv];
            let ldx = lv2.x - lv1.x;
            let ldy = lv2.y - lv1.y;
            let len2 = ldx * ldx + ldy * ldy;
            if len2 < 0.01 {
                self.segs[si].linedef = u16::MAX;
                continue;
            }
            let mut on_line = true;
            for p in [a, b] {
                let t = ((p.x - lv1.x) * ldx + (p.y - lv1.y) * ldy) / len2;
                if t < -0.02 || t > 1.02 {
                    on_line = false;
                    break;
                }
                let cross = (p.x - lv1.x) * ldy - (p.y - lv1.y) * ldx;
                if cross.abs() > 1.5 {
                    on_line = false;
                    break;
                }
            }
            if !on_line {
                self.segs[si].linedef = u16::MAX;
            }
            // Otherwise the stored linedef/side stand as read.
        }
    }

    pub fn vertexes(&self) -> &[Vertex] {
        &self.vertexes[..self.vertex_count]
    }
    pub fn lines(&self) -> &[Line] {
        &self.lines[..self.line_count]
    }
    pub fn sides(&self) -> &[Side] {
        &self.sides[..self.side_count]
    }
    pub fn sectors(&self) -> &[Sector] {
        &self.sectors[..self.sector_count]
    }
    pub fn things(&self) -> &[Thing] {
        &self.things[..self.thing_count]
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes[..self.node_count]
    }
    pub fn subsectors(&self) -> &[Subsector] {
        &self.subsectors[..self.subsector_count]
    }
    pub fn segs(&self) -> &[Seg] {
        &self.segs[..self.seg_count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wad::Wad;

    include!(concat!(env!("OUT_DIR"), "/wad.rs"));

    #[test]
    fn e1m1_loads_and_links() {
        use crate::wad::{Entry, MAX_LUMPS};
        let mut storage = [Entry::zero(); MAX_LUMPS];
        let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
        let mut scratch = [0u8; 24576];
        let mut map = Map::ZERO;
        map.load_into(&wad, "E1M1", &mut scratch)
            .expect("E1M1 parses");
        assert!(map.vertex_count > 400);
        assert!(map.line_count > 400);
        assert!(map.sector_count > 50);
        assert!(map.node_count > 100);
        // Every index referenced anywhere must resolve.
        for l in map.lines() {
            assert!((l.v1 as usize) < map.vertex_count && (l.v2 as usize) < map.vertex_count);
            assert!((l.front as usize) < map.side_count);
            if l.back != u16::MAX {
                assert!((l.back as usize) < map.side_count);
            }
        }
        for s in map.sides() {
            assert!((s.sector as usize) < map.sector_count);
        }
        let mut unresolved = 0;
        for s in map.segs() {
            assert!((s.v1 as usize) < map.vertex_count && (s.v2 as usize) < map.vertex_count);
            if s.linedef == u16::MAX {
                // Degenerate or orphaned seg: skipped at render.
                unresolved += 1;
                continue;
            }
            assert!((s.linedef as usize) < map.line_count);
            assert!(s.side <= 1);
            // The resolved line must actually contain the seg.
            let line = &map.lines()[s.linedef as usize];
            let a = map.vertexes[s.v1 as usize];
            let b = map.vertexes[s.v2 as usize];
            let lv1 = map.vertexes[line.v1 as usize];
            let lv2 = map.vertexes[line.v2 as usize];
            let ldx = lv2.x - lv1.x;
            let ldy = lv2.y - lv1.y;
            let len2 = ldx * ldx + ldy * ldy;
            assert!(len2 > 0.01);
            for p in [a, b] {
                let t = ((p.x - lv1.x) * ldx + (p.y - lv1.y) * ldy) / len2;
                assert!((-0.02..=1.02).contains(&t), "seg off its line");
                let cross = (p.x - lv1.x) * ldy - (p.y - lv1.y) * ldx;
                assert!(cross.abs() <= 1.5, "seg off its line");
            }
        }
        // Nearly every seg must resolve; the rest are degenerate scraps.
        assert!(unresolved < 45, "too many unresolved segs: {unresolved}");
        for s in map.subsectors() {
            assert!(s.first_seg as usize + s.seg_count as usize <= map.seg_count);
        }
        for n in map.nodes() {
            for c in n.child {
                if (c & 0x8000) == 0 {
                    assert!((c as usize) < map.node_count);
                } else {
                    assert!(((c & 0x7fff) as usize) < map.subsector_count);
                }
            }
        }
        // Player 1 start exists.
        assert!(map.things().iter().any(|t| t.kind == 1));
        // Exit room sector + exit switch line exist (E1M1 ends on S1).
        assert!(map.lines().iter().any(|l| l.special == 11));
        assert!(map.blockmap.width > 0 && map.blockmap.height > 0);
        assert_eq!(
            map.blockmap.block_count,
            map.blockmap.width as usize * map.blockmap.height as usize
        );
    }
}
