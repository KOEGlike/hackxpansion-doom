//! Playable engine facade: owns nothing big (all bulk state lives in
//! module statics below, zero-initialized BSS), drives one player.
//!
//! Movement uses axis-separated slide against blockmap lines with vanilla
//! step/height rules (step <= 24, headroom >= 56). Doors, switches,
//! monsters and pickups are Stage 2 and intentionally absent.

use crate::game::{self, Player, EYE};
use crate::level::{self, Level};
use crate::map::Map;
use crate::render::{Camera, FrameState, Renderer, Target, VIEW_H, VIEW_W};
use crate::wad::{Entry, Wad, MAX_LUMPS};

/// Player radius / height / max step, in map units (vanilla values).
pub const PLAYER_R: f32 = 16.0;
pub const PLAYER_H: f32 = 56.0;
pub const STEP_H: f32 = 24.0;
/// Decode scratch: largest single map lump is SIDEDEFS (~19 KiB).
pub const SCRATCH_SIZE: usize = 24576;
/// Walk/turn rates per tick at 35 Hz.
pub const MOVE_SPEED: f32 = 8.0;
pub const TURN_RATE: f32 = 0.09;

// Bulk state: BSS statics, filled by [`Engine::boot`]. Sizes: entries
// 8 KiB, level ~106 KiB, frame state ~10 KiB, scratch 24 KiB.
static mut ENTRIES: [Entry; MAX_LUMPS] = [Entry::zero(); MAX_LUMPS];
static mut ENTRY_COUNT: usize = 0;
static mut LEVEL: Level<'static> = Level::ZERO;
static mut STATE: FrameState = FrameState::ZERO;
static mut RENDERER: Renderer = Renderer::ZERO;
static mut SCRATCH: [u8; SCRATCH_SIZE] = [0; SCRATCH_SIZE];

/// Key codes (doomkeys.h values, matching the app's button map).
pub mod keys {
    pub const KEY_UPARROW: u8 = 0xad;
    pub const KEY_DOWNARROW: u8 = 0xaf;
    pub const KEY_LEFTARROW: u8 = 0xac;
    pub const KEY_RIGHTARROW: u8 = 0xae;
    pub const KEY_FIRE: u8 = 0xa3;
    pub const KEY_USE: u8 = 0xa2;
}

/// Small owned engine handle; safe to hold on task stacks.
pub struct Engine {
    wad_bytes: &'static [u8],
    player: Player,
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

/// Module statics are only touched by the single app task, in program
/// order (boot fully initializes before tick accesses). This makes the
/// `static_mut` accesses sound; the lint is allowed locally so real
/// warnings stay visible.
#[allow(static_mut_refs)]
impl Engine {
    /// Boots the engine from the embedded WAD (full re-init; safe to call
    /// again when re-entering the app). Needs `wad_bytes: 'static` (the
    /// `include_bytes!` image) so the level can borrow colormaps.
    pub fn boot(wad_bytes: &'static [u8]) -> Option<Self> {
        // SAFETY: single app task owns these; boot fully re-initializes
        // before any other access in program order.
        unsafe {
            let entries: &'static mut [Entry; MAX_LUMPS] =
                &mut *core::ptr::addr_of_mut!(ENTRIES);
            let wad = Wad::parse(wad_bytes, entries)?;
            ENTRY_COUNT = wad.len();
            level::load_into(&mut LEVEL, wad, "E1M1", &mut SCRATCH)?;
            let player = game::spawn(&LEVEL)?;
            Some(Self {
                wad_bytes,
                player,
                up: false,
                down: false,
                left: false,
                right: false,
            })
        }
    }

    /// Reader over the boot-parsed directory (shared borrows only).
    fn wad(&self) -> Option<Wad<'static>> {
        // SAFETY: filled by `boot` before any tick; shared refs never alias
        // a live exclusive borrow (parse completed before first use).
        unsafe {
            let n = ENTRY_COUNT;
            if n == 0 || n > MAX_LUMPS {
                return None;
            }
            let entries: &'static [Entry; MAX_LUMPS] = &*core::ptr::addr_of!(ENTRIES);
            Some(Wad::from_parts(self.wad_bytes, &entries[..n]))
        }
    }

    /// Queues a key event (edge-driven like the Chocolate binding).
    pub fn push_key(&mut self, pressed: bool, key: u8) {
        match key {
            keys::KEY_UPARROW => self.up = pressed,
            keys::KEY_DOWNARROW => self.down = pressed,
            keys::KEY_LEFTARROW => self.left = pressed,
            keys::KEY_RIGHTARROW => self.right = pressed,
            _ => {}
        }
    }

    /// Runs one 35 Hz tick: movement then render. Returns false only if
    /// the frame could not render (OOM-safe paths return `None` instead
    /// of panicking; the caller should exit the app).
    pub fn tick(&mut self, target: &mut impl Target) -> bool {
        // SAFETY: same single-task ownership as `boot`.
        unsafe {
            if self.left {
                self.player.angle += TURN_RATE;
            }
            if self.right {
                self.player.angle -= TURN_RATE;
            }
            let mut mx = 0.0;
            if self.up {
                mx += MOVE_SPEED;
            }
            if self.down {
                mx -= MOVE_SPEED;
            }
            if mx != 0.0 {
                self.try_move(
                    mx * libm::cosf(self.player.angle),
                    mx * libm::sinf(self.player.angle),
                );
            }
            // Refresh sector + eye height (snaps steps, no falling physics).
            if let Some(sec) = game::sector_at(&LEVEL.map, self.player.x, self.player.y) {
                self.player.sector = sec;
            }
            let cam = Camera {
                x: self.player.x,
                y: self.player.y,
                z: LEVEL.map.sectors()[self.player.sector].floor_h + EYE,
                angle: self.player.angle,
            };
            let Some(wad) = self.wad() else {
                return false;
            };
            RENDERER.render(&LEVEL, &wad, &mut STATE, cam, target);
        }
        true
    }

    /// Axis-separated slide move with radius vs blockmap lines.
    fn try_move(&mut self, dx: f32, dy: f32) {
        // SAFETY: called from `tick`'s exclusive section.
        let map = unsafe { &LEVEL.map };
        let r = PLAYER_R;
        // X axis, then Y axis (classic slide approximation).
        let nx = self.player.x + dx;
        if !Self::blocked(map, self.player.sector, nx, self.player.y, r) {
            self.player.x = nx;
        }
        let ny = self.player.y + dy;
        if !Self::blocked(map, self.player.sector, self.player.x, ny, r) {
            self.player.y = ny;
        }
    }

    /// True when a circle at (`x`,`y`) with radius `r` collides.
    fn blocked(map: &Map, sector: usize, x: f32, y: f32, r: f32) -> bool {
        let cur_floor = map.sectors()[sector].floor_h;
        let bm = &map.blockmap;
        // Blocks overlapped by the circle bbox.
        let bx0 = ((x - r - bm.org_x) / 128.0) as isize;
        let bx1 = ((x + r - bm.org_x) / 128.0) as isize;
        let by0 = ((y - r - bm.org_y) / 128.0) as isize;
        let by1 = ((y + r - bm.org_y) / 128.0) as isize;
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                let Some(list) = bm.lines_in(bx, by) else {
                    continue;
                };
                for &li in list {
                    let nl = map.line_count.max(1);
                    let ns = map.side_count.max(1);
                    let nv = map.vertex_count.max(1);
                    let nsec = map.sector_count.max(1);
                    let line = &map.lines()[li as usize % nl];
                    let a = map.vertexes[line.v1 as usize % nv];
                    let b = map.vertexes[line.v2 as usize % nv];
                    if dist_to_seg(x, y, a.x, a.y, b.x, b.y) >= r {
                        continue;
                    }
                    if line.back == u16::MAX {
                        return true;
                    }
                    // Two-sided: opening must fit + step must be climbable.
                    let fs = &map.sectors()[map.sides()[line.front as usize % ns].sector as usize % nsec];
                    let bs = &map.sectors()[map.sides()[line.back as usize % ns].sector as usize % nsec];
                    let top = fs.ceil_h.min(bs.ceil_h);
                    let bottom = fs.floor_h.max(bs.floor_h);
                    if top - bottom < PLAYER_H {
                        return true;
                    }
                    if bottom - cur_floor > STEP_H {
                        return true;
                    }
                }
            }
        }
        false
    }

}

/// Distance from point to segment.
fn dist_to_seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-6 {
        return libm::sqrtf((px - ax) * (px - ax) + (py - ay) * (py - ay));
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let cx = ax + dx * t - px;
    let cy = ay + dy * t - py;
    libm::sqrtf(cx * cx + cy * cy)
}

/// Frame dimensions for the app shell.
pub const DOOM_WIDTH: usize = VIEW_W;
pub const DOOM_HEIGHT: usize = VIEW_H;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Target, VIEW_H, VIEW_W};

    include!(concat!(env!("OUT_DIR"), "/wad.rs"));

    struct NullTarget;
    impl Target for NullTarget {
        fn put(&mut self, _x: usize, _y: usize, _rgb: u16) {}
    }

    #[test]
    fn boots_and_walks_without_clipping() {
        let mut engine = Engine::boot(DOOM_WAD).expect("boots");
        let (x0, y0) = (engine.player.x, engine.player.y);
        let mut target = NullTarget;
        // Walk forward for 3 simulated seconds.
        engine.push_key(true, keys::KEY_UPARROW);
        for _ in 0..105 {
            assert!(engine.tick(&mut target), "tick alive");
        }
        engine.push_key(false, keys::KEY_UPARROW);
        let moved =
            libm::sqrtf((engine.player.x - x0) * (engine.player.x - x0) + (engine.player.y - y0) * (engine.player.y - y0));
        assert!(moved > 100.0, "player must advance, moved {moved:.0}");
        // Turn around and walk back; must not end inside a wall: every
        // blockmap cell overlapped by the player circle must be passable.
        engine.push_key(true, keys::KEY_LEFTARROW);
        for _ in 0..20 {
            assert!(engine.tick(&mut target));
        }
        engine.push_key(false, keys::KEY_LEFTARROW);
        // Player circle must not intersect a solid line.
        unsafe {
            let map = &LEVEL.map;
            let (px, py) = (engine.player.x, engine.player.y);
            let bm = &map.blockmap;
            let bx = ((px - bm.org_x) / 128.0) as isize;
            let by = ((py - bm.org_y) / 128.0) as isize;
            for oy in -1..=1 {
                for ox in -1..=1 {
                    if let Some(list) = bm.lines_in(bx + ox, by + oy) {
                        for &li in list {
                            let line = &map.lines()[li as usize % map.line_count.max(1)];
                            let a = map.vertexes[line.v1 as usize % map.vertex_count.max(1)];
                            let b = map.vertexes[line.v2 as usize % map.vertex_count.max(1)];
                            if line.back == u16::MAX
                                && dist_to_seg(px, py, a.x, a.y, b.x, b.y) < PLAYER_R - 0.5
                            {
                                panic!("player inside solid line {li}");
                            }
                        }
                    }
                }
            }
        }
    }
}
