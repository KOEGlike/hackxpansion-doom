//! Vanilla-style BSP raycaster: front-to-back walls with a column clipper,
//! per-column floors/ceilings/sky, all in `f32` (M33 single-precision FPU).
//!
//! Art is sampled straight from flash (raw wall patches, decoded flats);
//! per-frame RAM is a 320-entry depth buffer plus small stack temps.

use crate::assets;
use crate::level::Level;
use crate::map::{Map, Seg, Vertex};
use crate::wad::Wad;

pub const VIEW_W: usize = 320;
pub const VIEW_H: usize = 200;
pub const HORIZON: f32 = 100.0;
/// Focal length in pixels: 90-degree horizontal field of view.
pub const FOCAL: f32 = 160.0;
/// World units per wall texel: wall patches are halved by `wadtool` (like
/// flats, unlike the old full-res assumption), so 1 texel covers 2 units.
/// Sprite/flat sampling is unaffected (their headers carry true sizes).
pub const TEXEL: f32 = 0.5;
/// Line flags (vanilla values).
const ML_DONTPEGTOP: u16 = 0x0008;
const ML_DONTPEGBOTTOM: u16 = 0x0010;

/// Camera pose. `angle` is radians, 0 = east (+x), positive = CCW (north).
/// `z` is the absolute view height (floor + eye).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub angle: f32,
}

/// Pixel sink: display session on target, image buffer on host.
pub trait Target {
    fn put(&mut self, x: usize, y: usize, rgb: u16);
}

/// Mutable per-frame scratch, owned by the engine and reused.
#[derive(Default)]
pub struct Dbg {
    pub paint_calls: u64,
    pub tex_miss: u64,
    pub empty_range: u64,
    pub frags: u64,
}

impl Dbg {
    pub const fn zero() -> Self {
        Self {
            paint_calls: 0,
            tex_miss: 0,
            empty_range: 0,
            frags: 0,
        }
    }
}

/// Drawn-pixel mask for the exact painter's algorithm: far geometry
/// never overwrites near pixels (windows over far walls). 320x200 bits.
pub struct DrawnMask {
    bits: [u8; VIEW_W * VIEW_H / 8],
}

impl DrawnMask {
    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    #[inline]
    pub fn test(&self, x: usize, y: usize) -> bool {
        let i = y * VIEW_W + x;
        self.bits[i >> 3] & (1 << (i & 7)) != 0
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize) {
        let i = y * VIEW_W + x;
        self.bits[i >> 3] |= 1 << (i & 7);
    }
}

pub struct FrameState {
    pub cam: Camera,
    pub view_dir: (f32, f32),
    pub side_vec: (f32, f32),
    /// Perpendicular wall depth per column (sprites + lighting).
    pub perp: [f32; VIEW_W],
    /// Painter mask shared by walls and planes.
    pub mask: DrawnMask,
    /// Scratch wall column (palette indices, 0xff = untouched).
    pub column: [u8; 256],
    pub dbg_hits: u64,
    pub dbg: Dbg,
}

impl FrameState {
    /// Zeroed state for static storage (BSS). Must be paired with
    /// [`FrameState::reset`] before rendering.
    pub const ZERO: Self = Self {
        cam: Camera {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            angle: 0.0,
        },
        view_dir: (0.0, 0.0),
        side_vec: (0.0, 0.0),
        perp: [0.0; VIEW_W],
        column: [0; 256],
        dbg_hits: 0,
        dbg: Dbg::zero(),
        mask: DrawnMask { bits: [0; VIEW_W * VIEW_H / 8] },
    };

    pub fn reset(&mut self, cam: Camera) {
        self.dbg_hits = 0;
        self.cam = cam;
        self.view_dir = (libm::cosf(cam.angle), libm::sinf(cam.angle));
        // Screen-right = view dir rotated -90° (clockwise).
        self.side_vec = (libm::sinf(cam.angle), -libm::cosf(cam.angle));
        self.perp.fill(0.0);
        self.mask.clear();
    }
}

/// Per-frame renderer: clipper plus trig cache.
pub struct Renderer {
    clip: [(i32, i32); 128],
    clip_len: usize,
    pub dbg_segs: u64,
    pub dbg_walls: u64,
    pub dbg_behind: u64,
    pub dbg_facing: u64,
    pub dbg_offscreen: u64,
    pub dbg_cols: u64,
    pub dbg_pixels: u64,
}

impl Renderer {
    /// Zeroed renderer for static storage (clipper rebuilt every frame).
    pub const ZERO: Self = Self {
        clip: [(0, 0); 128],
        clip_len: 0,
        dbg_segs: 0,
        dbg_walls: 0,
        dbg_behind: 0,
        dbg_facing: 0,
        dbg_offscreen: 0,
        dbg_cols: 0,
        dbg_pixels: 0,
    };

    /// Backwards-compatible constructor (small; stack-safe).
    pub fn new() -> Self {
        Self::ZERO
    }

    /// Renders one frame from `cam` into `target`.
    pub fn render(
        &mut self,
        level: &Level<'_>,
        wad: &Wad,
        state: &mut FrameState,
        cam: Camera,
        target: &mut impl Target,
    ) {
        state.reset(cam);
        self.clip[0] = (i32::MIN, -1);
        self.clip[1] = (VIEW_W as i32, i32::MAX);
        self.clip_len = 2;
        let map = &level.map;

        // BSP walk from the root (last node), front to back.
        if map.node_count > 0 {
            self.render_node(level, wad, map, map.node_count - 1, state, target);
        }

    }

    fn render_node(
        &mut self,
        level: &Level<'_>,
        wad: &Wad,
        map: &Map,
        node_idx: usize,
        state: &mut FrameState,
        target: &mut impl Target,
    ) {
        let node = &map.nodes[node_idx.min(map.node_count - 1)];
        let side = point_on_side(state.cam.x, state.cam.y, node);
        let front = node.child[side] as usize;
        let back = node.child[1 - side] as usize;
        self.render_child(level, wad, map, front, state, target);
        if Self::child_visible(map, state, back) {
            self.render_child(level, wad, map, back, state, target);
        }
    }

    fn render_child(
        &mut self,
        level: &Level<'_>,
        wad: &Wad,
        map: &Map,
        child: usize,
        state: &mut FrameState,
        target: &mut impl Target,
    ) {
        if child & 0x8000 != 0 {
            let sub = &map.subsectors[(child & 0x7fff) as usize % map.subsector_count.max(1)];
            for i in 0..sub.seg_count as usize {
                let si = sub.first_seg as usize + i;
                let seg = &map.segs[si];
                self.add_line(level, wad, map, seg, state, target);
            }
        } else if child < map.node_count {
            self.render_node(level, wad, map, child, state, target);
        }
    }

    /// Conservative visibility test for a back child: project the full
    /// leaf bbox (all its segs); visible unless fully behind.
    fn child_visible(map: &Map, state: &FrameState, child: usize) -> bool {
        let (mut x0, mut y0, mut x1, mut y1);
        if child & 0x8000 != 0 {
            let sub = &map.subsectors[(child & 0x7fff) as usize % map.subsector_count.max(1)];
            if sub.seg_count == 0 {
                return false;
            }
            x0 = f32::INFINITY;
            y0 = f32::INFINITY;
            x1 = f32::NEG_INFINITY;
            y1 = f32::NEG_INFINITY;
            let nv = map.vertex_count.max(1);
            for i in 0..sub.seg_count as usize {
                let seg = &map.segs[sub.first_seg as usize + i];
                for v in [
                    map.vertexes[seg.v1 as usize % nv],
                    map.vertexes[seg.v2 as usize % nv],
                ] {
                    x0 = x0.min(v.x);
                    y0 = y0.min(v.y);
                    x1 = x1.max(v.x);
                    y1 = y1.max(v.y);
                }
            }
        } else {
            // Nested nodes carry their own boxes only relative to their
            // parent; over-traverse rather than risk a miss.
            return true;
        }
        let (vx, vy) = state.view_dir;
        for (cx, cy) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            if (cx - state.cam.x) * vx + (cy - state.cam.y) * vy > 0.1 {
                return true;
            }
        }
        false
    }

    /// Projects a seg, clips to viewport + clipper, draws wall ranges.
    fn add_line(
        &mut self,
        level: &Level<'_>,
        wad: &Wad,
        map: &Map,
        seg: &Seg,
        state: &mut FrameState,
        target: &mut impl Target,
    ) {
        if seg.linedef == u16::MAX {
            // Unresolvable seg (degenerate or orphaned): skip.
            self.dbg_facing += 1;
            return;
        }
        let nv = map.vertex_count.max(1);
        let nl = map.line_count.max(1);
        let line = &map.lines[seg.linedef as usize % nl];
        let lv1 = map.vertexes[line.v1 as usize % nv];
        let lv2 = map.vertexes[line.v2 as usize % nv];
        // Camera must be on the seg's owning side (front = right of v1->v2,
        // resolved geometrically at load since lump fields are untrusted).
        let ldx = lv2.x - lv1.x;
        let ldy = lv2.y - lv1.y;
        let cam_front =
            (state.cam.x - lv1.x) * ldy - (state.cam.y - lv1.y) * ldx > 0.0;
        if cam_front != (seg.side == 0) {
            self.dbg_facing += 1;
            return;
        }
        let a = map.vertexes[seg.v1 as usize % nv];
        let b = map.vertexes[seg.v2 as usize % nv];
        let (vx, vy) = state.view_dir;
        let (sx, sy) = state.side_vec;
        let (mut ax, mut ay) = (a.x - state.cam.x, a.y - state.cam.y);
        let (mut bx, mut by) = (b.x - state.cam.x, b.y - state.cam.y);
        let (mut af, mut bf) = (ax * vx + ay * vy, bx * vx + by * vy);
        if af <= 0.1 && bf <= 0.1 {
            self.dbg_behind += 1;
            return;
        }
        if af <= 0.1 || bf <= 0.1 {
            let t = (0.1 - af) / (bf - af);
            if af <= 0.1 {
                ax += (bx - ax) * t;
                ay += (by - ay) * t;
                af = 0.1;
            } else {
                bx = ax + (bx - ax) * t;
                by = ay + (by - ay) * t;
                bf = 0.1;
            }
        }
        let half = VIEW_W as f32 / 2.0;
        let mut x1 = (half + FOCAL * (ax * sx + ay * sy) / af) as i32;
        let mut x2 = (half + FOCAL * (bx * sx + by * sy) / bf) as i32;
        // Order the screen range left-to-right; the ray math below stays
        // in original seg orientation.
        if x1 > x2 {
            core::mem::swap(&mut x1, &mut x2);
        }
        x1 = x1.max(0);
        x2 = x2.min(VIEW_W as i32 - 1);
        if x2 < x1 {
            self.dbg_offscreen += 1;
            return;
        }
        // Walk the sorted clipper, drawing each unclaimed fragment.
        // Only solid segs (single-sided, closed doors) claim ranges;
        // see-through openings stay open for farther geometry while the
        // drawn-pixel mask keeps near pixels exact.
        self.dbg_segs += 1;
        let mut x = x1;
        let mut ci = 0;
        while x <= x2 && ci < self.clip_len {
            let (c1, c2) = self.clip[ci];
            if x < c1 {
                let xe = x2.min(c1 - 1);
                if self.draw_wall(level, wad, map, seg, state, a, b, x, xe, target) {
                    self.clip_claim(x, xe);
                }
                x = xe + 1;
                continue;
            }
            if x <= c2 {
                x = c2.saturating_add(1);
                if x > x2 {
                    break;
                }
            }
            ci += 1;
        }
        if x <= x2 {
            self.dbg_walls += 1;
            if self.draw_wall(level, wad, map, seg, state, a, b, x, x2, target) {
                self.clip_claim(x, x2);
            }
        }
    }

    /// Inserts a claimed range into the sorted, merged clipper.
    fn clip_claim(&mut self, x1: i32, x2: i32) {
        // Find insertion point (clipper[0] is the (-inf,-1) sentinel).
        let mut i = 1;
        while i < self.clip_len && self.clip[i].1 < x1 - 1 {
            i += 1;
        }
        let mut a = x1;
        let mut b = x2;
        // Merge all overlapping/adjacent entries.
        let mut j = i;
        while j < self.clip_len && self.clip[j].0 <= b + 1 {
            a = a.min(self.clip[j].0);
            b = b.max(self.clip[j].1);
            j += 1;
        }
        let merged = j - i;
        if merged == 0 {
            if self.clip_len < self.clip.len() {
                self.clip[i..].rotate_right(1);
                self.clip[i] = (a, b);
                self.clip_len += 1;
            }
        } else {
            self.clip[i] = (a, b);
            if merged > 1 {
                self.clip.copy_within(i + merged.., i + 1);
            }
            self.clip_len -= merged - 1;
        }
    }

    /// Draws wall parts for columns `[x1, x2]` of one seg.
    /// Returns true when the seg is solid (claims clip ranges).
    #[allow(clippy::too_many_arguments)]
    fn draw_wall(
        &mut self,
        level: &Level<'_>,
        wad: &Wad,
        map: &Map,
        seg: &Seg,
        state: &mut FrameState,
        a: Vertex,
        b: Vertex,
        x1: i32,
        x2: i32,
        target: &mut impl Target,
    ) -> bool {
        let nl = map.line_count.max(1);
        let ns = map.side_count.max(1);
        let line = &map.lines[seg.linedef as usize % nl];
        let nv = map.vertex_count.max(1);
        let lv1 = map.vertexes[line.v1 as usize % nv];
        let lv2 = map.vertexes[line.v2 as usize % nv];
        // Owning side was resolved geometrically at load.
        let side_idx = if seg.side == 0 || line.back == u16::MAX {
            line.front
        } else {
            line.back
        };
        let side = &map.sides[side_idx as usize % ns];
        let nsec = map.sector_count.max(1);
        let front = &map.sectors[side.sector as usize % nsec];
        // The far sector is the OTHER side of the line (not line.back:
        // for a seg owned by side 1 the far side is side 0).
        let back = if line.back != u16::MAX {
            let other = if seg.side == 0 { line.back } else { line.front };
            let bs = &map.sides[other as usize % ns];
            Some(&map.sectors[bs.sector as usize % nsec])
        } else {
            None
        };

        // Seg length + offset from the linedef start (for split segs).
        // Split segs may run opposite to the linedef; measure from v1.
        let ex = b.x - a.x;
        let ey = b.y - a.y;
        let seg_len = libm::sqrtf(ex * ex + ey * ey).max(0.001);
        let ox = a.x - lv1.x;
        let oy = a.y - lv1.y;
        let seg_off = libm::sqrtf(ox * ox + oy * oy);
        let ldx = lv2.x - lv1.x;
        let ldy = lv2.y - lv1.y;
        let same_dir = ex * ldx + ey * ldy > 0.0;

        let (vx, vy) = state.view_dir;
        let (sx, sy) = state.side_vec;
        let half = VIEW_W as f32 / 2.0;

        for x in x1..=x2 {
            let xu = x as f32;
            // Pinhole ray for this column (unnormalized).
            let rx = vx * FOCAL + sx * (xu - half);
            let ry = vy * FOCAL + sy * (xu - half);
            // Intersect with the seg line.
            let denom = rx * ey - ry * ex;
            if denom.abs() < 1e-6 {
                continue;
            }
            let t = ((a.x - state.cam.x) * ey - (a.y - state.cam.y) * ex) / denom;
            if t <= 0.0 {
                continue;
            }
            // Perpendicular distance + fractional position along the seg.
            let perp = t * FOCAL;
            let ix = state.cam.x + rx * t - a.x;
            let iy = state.cam.y + ry * t - a.y;
            let frac = ((ix * ex + iy * ey) / (seg_len * seg_len)).clamp(0.0, 1.0);
            let from_v1 = if same_dir {
                seg_off + frac * seg_len
            } else {
                (seg_off - frac * seg_len).abs()
            };
            let along = from_v1 + side.xoff as f32;
            let scale = FOCAL / perp.max(0.5);
            state.perp[x as usize] = perp;
            let drew = Self::draw_parts(
                level, wad, line, side, front, back, along, scale, state, x, target,
            );
            if !drew {
                // See-through opening (or textureless barrier): the rows
                // belong to the near sector's planes. Bounds from the
                // sector heights at this column's depth.
                let viewz = state.cam.z;
                let y_of = |world_y: f32| (HORIZON - (world_y - viewz) * scale) as i32;
                Self::span_ceiling(level, state, x, y_of(front.ceil_h), front, target);
                Self::span_floor(level, state, x, y_of(front.floor_h), front, target);
            }
        }
        // Solid (claims clip ranges): single-sided lines and closed doors.
        // See-through openings stay open for farther geometry; the
        // drawn-pixel mask keeps near pixels exact.
        match back {
            None => true,
            Some(back) => {
                back.ceil_h <= front.floor_h || back.floor_h >= front.ceil_h
            }
        }
    }

    /// Draws upper/mid/lower parts of one wall column.
    #[allow(clippy::too_many_arguments)]
    /// Returns true when any part drew (for opening-span fallback).
    fn draw_parts(
        level: &Level<'_>,
        wad: &Wad,
        line: &crate::map::Line,
        side: &crate::map::Side,
        front: &crate::map::Sector,
        back: Option<&crate::map::Sector>,
        along: f32,
        scale: f32,
        state: &mut FrameState,
        x: i32,
        target: &mut impl Target,
    ) -> bool {
        let viewz = state.cam.z;
        let y_of = |world_y: f32| (HORIZON - (world_y - viewz) * scale) as i32;

        // One wall part: expand the texture column, blit its rows, then
        // emit floor/ceiling edge spans for the rows it bounds. Spans draw
        // immediately (front-to-back) through the drawn-pixel mask, so near
        // geometry always wins: steps, sills and door frames all land
        // their own treads. A part whose edge floats mid-air (window tops,
        // masked mids) emits no span, leaving the opening to farther
        // content. Returns true when anything drew.
        macro_rules! part {
            ($tex:expr, $wtop:expr, $wbot:expr, $anchor:expr, $sec:expr) => {{
                state.column.fill(0xff);
                let tex_h = paint_part(level, wad, $tex, along, &mut state.column, &mut state.dbg);
                let y1 = y_of($wtop).max(0);
                let y2 = y_of($wbot).min(VIEW_H as i32 - 1);
                if tex_h > 0 && y2 >= y1 {
                    blit_column(
                        level, state, x, y1, y2, $anchor, tex_h, viewz, scale,
                        $sec.light, target,
                    );
                    if $wtop >= $sec.ceil_h - 0.5 {
                        Self::span_ceiling(level, state, x, y1, $sec, target);
                    }
                    if $wbot <= $sec.floor_h + 0.5 {
                        Self::span_floor(level, state, x, y2, $sec, target);
                    }
                    true
                } else {
                    false
                }
            }};
        }

        let mut drew = false;

        match back {
            None => {
                // Single-sided: mid texture spans the full wall.
                if name_present(&side.mid) {
                    let h = tex_height(level, &side.mid);
                    let anchor = if line.flags & ML_DONTPEGBOTTOM != 0 {
                        front.floor_h + h + side.yoff as f32
                    } else {
                        front.ceil_h + side.yoff as f32
                    };
                    drew = part!(&side.mid, front.ceil_h, front.floor_h, anchor, front);
                }
            }
            Some(back) => {
                // Upper texture.
                if back.ceil_h < front.ceil_h && name_present(&side.top) {
                    let h = tex_height(level, &side.top);
                    let anchor = if line.flags & ML_DONTPEGTOP != 0 {
                        front.ceil_h + side.yoff as f32
                    } else {
                        back.ceil_h + h + side.yoff as f32
                    };
                    drew |= part!(&side.top, front.ceil_h, back.ceil_h, anchor, front);
                }
                // Masked midtexture across the opening (no spans: the
                // opening shows farther content).
                if name_present(&side.mid) {
                    let otop = front.ceil_h.min(back.ceil_h);
                    let obot = front.floor_h.max(back.floor_h);
                    if otop > obot {
                        let anchor = otop + side.yoff as f32;
                        drew |= part!(&side.mid, otop, obot, anchor, front);
                    }
                }
                // Lower texture.
                if back.floor_h > front.floor_h && name_present(&side.bot) {
                    let anchor = if line.flags & ML_DONTPEGBOTTOM != 0 {
                        front.ceil_h + side.yoff as f32
                    } else {
                        back.floor_h + side.yoff as f32
                    };
                    drew |= part!(&side.bot, back.floor_h, front.floor_h, anchor, front);
                }
            }
        }
        drew
    }

    /// Ceiling span for rows [0, y1): sky or the sector's ceiling flat.
    #[allow(clippy::too_many_arguments)]
    fn span_ceiling(
        level: &Level<'_>,
        state: &mut FrameState,
        x: i32,
        y1: i32,
        sec: &crate::map::Sector,
        target: &mut impl Target,
    ) {
        if y1 <= 0 {
            return;
        }
        if Level::is_sky(&sec.ceil_pic) {
            Self::draw_sky(level, state, x, 0, y1, target);
        } else if let Some(fi) = level.flat_num(&sec.ceil_pic) {
            Self::draw_flat(
                level, state, x, 0, y1, sec.ceil_h, state.cam.z, true, sec.light, fi,
                target,
            );
        }
    }

    /// Floor span for rows (y2, H) with the sector's floor flat.
    fn span_floor(
        level: &Level<'_>,
        state: &mut FrameState,
        x: i32,
        y2: i32,
        sec: &crate::map::Sector,
        target: &mut impl Target,
    ) {
        if y2 >= VIEW_H as i32 - 1 {
            return;
        }
        if let Some(fi) = level.flat_num(&sec.floor_pic) {
            Self::draw_flat(
                level, state, x, y2 + 1, VIEW_H as i32, sec.floor_h, state.cam.z, false,
                sec.light, fi, target,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_flat(
        level: &Level<'_>,
        state: &mut FrameState,
        x: i32,
        y1: i32,
        y2: i32,
        plane_h: f32,
        viewz: f32,
        ceiling: bool,
        light: u8,
        flat_idx: usize,
        target: &mut impl Target,
    ) {
        let h_diff = if ceiling { plane_h - viewz } else { viewz - plane_h };
        if h_diff <= 0.0 {
            return;
        }
        let xu = x as f32;
        let (vx, vy) = state.view_dir;
        let (sx, sy) = state.side_vec;
        let flat = level.flat(flat_idx);
        for y in y1..y2 {
            let dy = if ceiling {
                HORIZON - y as f32
            } else {
                y as f32 - HORIZON
            };
            if dy <= 0.0 {
                continue;
            }
            let perp = h_diff * FOCAL / dy;
            let side_off = (xu - VIEW_W as f32 / 2.0) * perp / FOCAL;
            let wx = state.cam.x + vx * perp + sx * side_off;
            let wy = state.cam.y + vy * perp + sy * side_off;
            let pal = assets::flat_pixel(
                flat,
                euclid(wx, 64.0) as usize,
                euclid(wy, 64.0) as usize,
            );
            if state.mask.test(x as usize, y as usize) {
                continue;
            }
            let l = shade_light(light, perp);
            let (cmap, maps) = level.cmap();
            let rgb =
                level.rgb_lut[assets::shade(cmap, maps, l, pal) as usize];
            target.put(x as usize, y as usize, rgb);
            state.mask.set(x as usize, y as usize);
        }
    }

    fn draw_sky(
        level: &Level<'_>,
        state: &mut FrameState,
        x: i32,
        y1: i32,
        y2: i32,
        target: &mut impl Target,
    ) {
        // Sky column from the F_SKY1 flat, panned by view angle.
        let sky = *b"F_SKY1\0\0";
        let Some(fi) = level.flat_num(&sky) else {
            return;
        };
        let flat = level.flat(fi);
        // One full turn pans across the sky 4 times.
        let pan = euclid(state.cam.angle * 81.0 + x as f32 * 0.35, 32.0);
        for y in y1..y2 {
            if state.mask.test(x as usize, y as usize) {
                continue;
            }
            let pal = assets::flat_pixel(flat, pan as usize, (y as usize * 2) / 3);
            target.put(x as usize, y as usize, level.rgb_lut[pal as usize]);
            state.mask.set(x as usize, y as usize);
        }
    }
}

/// Expands one texture column (all patch layers, later overwriting) into
/// `col` indexed by TEXTURE ROW. Returns the texture height, or 0 when the
/// texture is missing. Screen rows iterate in `blit_column` (vanilla-style
/// backward mapping: no gaps when minified).
fn paint_part(
    level: &Level<'_>,
    wad: &Wad,
    tex_name: &[u8; 8],
    along: f32,
    col: &mut [u8; 256],
    dbg: &mut Dbg,
) -> u16 {
    dbg.paint_calls += 1;
    let Some(ti) = level.texture_num(tex_name) else {
        dbg.tex_miss += 1;
        return 0;
    };
    let tex = &level.textures[ti];
    if tex.height == 0 || tex.height > 256 {
        return 0;
    }
    let tex_x = euclid(along * TEXEL, tex.width as f32);
    let pool = &level.texpatches;
    for p in 0..tex.patch_count as usize {
        let patch = &pool[tex.patch_start as usize + p];
        if patch.lump == u16::MAX {
            continue;
        }
        let Some(data) = wad.raw(patch.lump as usize) else {
            continue;
        };
        if data.len() < 8 {
            continue;
        }
        let pw = u16::from_le_bytes([data[0], data[1]]) as f32;
        let pc = tex_x - patch.ox as f32;
        if pc < 0.0 || pc >= pw {
            continue;
        }
        let oy = patch.oy as i32;
        assets::draw_column(data, pc as usize, |py, pal| {
            let r = oy + py as i32;
            if (0..tex.height as i32).contains(&r) {
                col[r as usize] = pal;
            }
        });
    }
    tex.height
}

/// Writes scratch column rows [y1, y2] through colormap + RGB LUT.
/// Blits screen rows [y1, y2] of a wall part: maps each row back to a
/// texture row (`anchor` = world height of texture row 0) and shades it.
#[allow(clippy::too_many_arguments)]
fn blit_column(
    level: &Level<'_>,
    state: &mut FrameState,
    x: i32,
    y1: i32,
    y2: i32,
    anchor: f32,
    tex_h: u16,
    viewz: f32,
    scale: f32,
    light: u8,
    target: &mut impl Target,
) {
    if y2 < y1 || tex_h == 0 {
        return;
    }
    let perp = state.perp[x as usize].max(1.0);
    let l = shade_light(light, perp);
    let (cmap, maps) = level.cmap();
    let inv_scale = 1.0 / scale.max(1e-6);
    for y in y1..=y2 {
        // World height of this row, then texture row (backward mapping).
        // Rows wrap like vanilla R_DrawColumn (`&127`, here at the halved
        // height), so shifted textures (row offsets) tile seamlessly.
        let world_y = viewz + (HORIZON - y as f32) * inv_scale;
        let trow = if tex_h > 0 {
            euclid((anchor - world_y) * TEXEL, tex_h as f32) as i32
        } else {
            -1
        };
        if trow < 0 || trow >= tex_h as i32 {
            continue;
        }
        if state.mask.test(x as usize, y as usize) {
            continue;
        }
        let pal = state.column[trow as usize];
        if pal == 0xff {
            continue;
        }
        let rgb =
            level.rgb_lut[assets::shade(cmap, maps, l, pal) as usize];
        target.put(x as usize, y as usize, rgb);
        state.mask.set(x as usize, y as usize);
        state.dbg_hits += 1;
    }
}

/// Positive floating-point remainder (Euclidean modulus).
fn euclid(x: f32, y: f32) -> f32 {
    let r = libm::fmodf(x, y);
    if r < 0.0 {
        r + y
    } else {
        r
    }
}

/// Light level with distance falloff (vanilla-flavored).
fn shade_light(light: u8, perp: f32) -> u8 {
    (light as f32 - perp * 0.06).clamp(0.0, 255.0) as u8
}

fn tex_height(level: &Level<'_>, name: &[u8; 8]) -> f32 {
    level
        .texture_num(name)
        .map(|ti| level.textures[ti].height as f32)
        .unwrap_or(128.0)
}

fn name_present(name: &[u8; 8]) -> bool {
    !(name[0] == b'-' || name[0] == 0)
}

use crate::map::point_on_side;

#[cfg(test)]
mod mask_tests {
    use super::DrawnMask;
    use super::{VIEW_H, VIEW_W};

    #[test]
    fn mask_set_test_roundtrip() {
        let mut m = DrawnMask { bits: [0; VIEW_W * VIEW_H / 8] };
        assert!(!m.test(0, 0));
        assert!(!m.test(319, 199));
        m.set(0, 0);
        m.set(319, 199);
        m.set(160, 100);
        assert!(m.test(0, 0));
        assert!(m.test(319, 199));
        assert!(m.test(160, 100));
        assert!(!m.test(1, 0));
        assert!(!m.test(0, 1));
        assert!(!m.test(159, 100));
        m.clear();
        assert!(!m.test(160, 100));
    }
}
