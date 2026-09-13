//! Software DOOM engine core for the hackxpansion console.
//!
//! Wraps the vendored `doomgeneric` C sources (the original DOOM gameplay code
//! plus the vanilla software renderer) in a small Rust FFI layer:
//!
//! * [`Doom`] drives the engine frame by frame from the app task.
//! * [`FrameSink`] receives every rendered frame (320x200 palette indices plus
//!   the 256-colour palette) for conversion to the console's RGB565 display.
//! * The engine renders at the classic 320x200 resolution and must be paced at
//!   35 Hz by the caller.
//!
//! On host targets the engine runs against the system libc (exercised by the
//! performance tests in `tests/perf.rs`); on the `thumbv8m` target it is built
//! against the freestanding shims in `c/`.

#![cfg_attr(target_os = "none", no_std)]
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "none")]
extern crate alloc;

use core::slice;

include!(concat!(env!("OUT_DIR"), "/wad.rs"));

unsafe extern "C" {
    // Rust -> C (dg_xpanse.c).
    fn dg_start(wad: *const u8, wad_size: u32) -> i32;
    fn dg_run_tick() -> i32;

    // Screen buffer owned by the engine (i_video.c).
    static I_VideoBuffer: *mut u8;

    // Palette bytes as last written by `I_SetPalette` (i_video.c).
    fn I_GetPaletteRGB(out: *mut u8);
}

/// Screen dimensions of the vanilla DOOM renderer.
pub const DOOM_WIDTH: usize = 320;
pub const DOOM_HEIGHT: usize = 200;

/// Receives rendered DOOM frames.
/// `screen` holds one palette index per pixel (`DOOM_WIDTH * DOOM_HEIGHT`
/// bytes); `palette` holds 256 RGB triplets. Implementations convert to RGB565
/// and push the result to the display. Called once per engine frame from
/// within [`Doom::tick`], on the same task thread.
pub trait FrameSink {
    fn draw(&mut self, screen: &[u8], palette: &[[u8; 3]; 256]);
}

/// Booted engine state. All mutable engine state lives in C statics, so at
/// most one `Doom` may exist at a time; the app task must be its only user.
pub struct Doom<'a> {
    sink: &'a mut dyn FrameSink,
}

impl<'a> Doom<'a> {
    /// Boots the engine synchronously (runs `D_DoomMain`).
    /// Returns `None` when the embedded WAD is empty or the engine aborted
    /// during startup.
    pub fn new<S: FrameSink>(sink: &'a mut S) -> Option<Self> {
        unsafe {
            let ok = dg_start(DOOM_WAD.as_ptr(), DOOM_WAD.len() as u32) == 0;
            if ok {
                Some(Self { sink })
            } else {
                None
            }
        }
    }

    /// Queues a key event (key codes per `doomkeys.h`).
    pub fn push_key(&mut self, pressed: bool, key: u8) {
        unsafe { doom_sys::dg_push_key(pressed as i32, key) };
    }

    /// Runs one engine frame (tic update + render). Returns `false` when DOOM
    /// raised a fatal error and the app should exit.
    pub fn tick(&mut self) -> bool {
        let ok = unsafe { dg_run_tick() == 0 };
        if ok {
            let screen = unsafe { slice::from_raw_parts(I_VideoBuffer, DOOM_WIDTH * DOOM_HEIGHT) };
            let palette = unsafe { read_palette() };
            self.sink.draw(screen, &palette);
        }
        ok
    }

    /// Runs `frames` frames back to back without pacing (bench mode).
    pub fn run_bench(&mut self, frames: u32) -> u32 {
        let mut done = 0;
        while done < frames && self.tick() {
            done += 1
        }
        done
    }
}

unsafe fn read_palette() -> [[u8; 3]; 256] {
    let mut palette = [[0u8; 3]; 256];
    I_GetPaletteRGB(palette.as_mut_ptr().cast());
    palette
}

#[cfg(target_os = "none")]
pub fn set_bench_mode(enabled: bool) {
    unsafe { doom_sys::dg_set_bench(enabled as i32) };
}

#[cfg(not(target_os = "none"))]
pub fn set_bench_mode(enabled: bool) {
    let _ = enabled;
}

#[cfg(not(target_os = "none"))]
unsafe extern "C" {
    fn dg_zone_stats(s: *mut i32, l: *mut i32, c: *mut i32, f: *mut i32);
}

/// Zone usage in bytes: (static, level, cache, free). Host tests only.
#[cfg(not(target_os = "none"))]
pub fn zone_stats() -> (i32, i32, i32, i32) {
    let (mut s, mut l, mut c, mut f) = (0, 0, 0, 0);
    unsafe { dg_zone_stats(&mut s, &mut l, &mut c, &mut f) };
    (s, l, c, f)
}

#[cfg(not(target_os = "none"))]
unsafe extern "C" {
    fn dg_zone_top();
}

/// Prints largest live zone blocks. Host tests only.
#[cfg(not(target_os = "none"))]
pub fn zone_top() {
    unsafe { dg_zone_top() };
}

#[cfg(target_os = "none")]
mod platform_symbols {
    // Force the platform symbols from doom-sys to be included in the final binary.
    // The C code in the engine static library calls these functions via the
    // DG_PLATFORM v-table, but the linker's --gc-sections doesn't see the
    // connection. By referencing them here, we ensure they are kept.
    use doom_sys::{
        dg_log_str, dg_get_ticks_ms, dg_sleep_ms,
        dg_set_bench, dg_push_key, dg_pop_key, dg_set_wad,
        dg_heap_alloc, dg_heap_alloc_zeroed, dg_heap_free, dg_heap_alloc_size,
    };

    // Static array of function pointers - #[used] prevents garbage collection
    union FnPtr {
        f: extern "C" fn(),
        log: unsafe extern "C" fn(*const u8, u32),
        ticks: unsafe extern "C" fn() -> u32,
        sleep: unsafe extern "C" fn(u32),
        bench: unsafe extern "C" fn(i32),
        push: unsafe extern "C" fn(i32, u8),
        pop: unsafe extern "C" fn(*mut i32, *mut u8) -> i32,
        wad: unsafe extern "C" fn(*const u8, u32),
        heap_alloc: unsafe extern "C" fn(u32) -> *mut u8,
        heap_alloc_zeroed: unsafe extern "C" fn(u32) -> *mut u8,
        heap_free: unsafe extern "C" fn(*mut u8, u32),
        heap_alloc_size: unsafe extern "C" fn(*mut u8) -> u32,
    }

    #[used]
    static FORCE_PLATFORM_SYMBOLS: [FnPtr; 11] = [
        FnPtr { log: dg_log_str },
        FnPtr { ticks: dg_get_ticks_ms },
        FnPtr { sleep: dg_sleep_ms },
        FnPtr { bench: dg_set_bench },
        FnPtr { push: dg_push_key },
        FnPtr { pop: dg_pop_key },
        FnPtr { wad: dg_set_wad },
        FnPtr { heap_alloc: dg_heap_alloc },
        FnPtr { heap_alloc_zeroed: dg_heap_alloc_zeroed },
        FnPtr { heap_free: dg_heap_free },
        FnPtr { heap_alloc_size: dg_heap_alloc_size },
    ];
}
