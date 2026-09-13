//! DOOM system bindings - C FFI exports for the DOOM engine

#![cfg_attr(target_os = "none", no_std)]

#[cfg(target_os = "none")]
extern crate alloc;

#[cfg(target_os = "none")]
use alloc::alloc::{alloc, dealloc, Layout};

// Bench state - defined in Rust for both target and host
#[unsafe(no_mangle)]
static mut dg_bench: i32 = 0;
#[unsafe(no_mangle)]
static mut dg_bench_ms: u32 = 0;

/// Line-buffered console log for target builds. C `printf`/`vfprintf`
/// paths emit one byte per `dg_log_str` call, so bytes accumulate until a
/// newline (or a full buffer) and flush as a single defmt frame.
#[cfg(target_os = "none")]
mod target_log {
    const CAP: usize = 256;
    static mut BUF: [u8; CAP] = [0; CAP];
    static mut LEN: usize = 0;

    fn flush() {
        unsafe {
            if LEN == 0 {
                return;
            }
            let chunk = &BUF[..LEN];
            LEN = 0;
            match core::str::from_utf8(chunk) {
                Ok(s) => defmt::info!("doom: {}", s),
                Err(_) => defmt::info!("doom: <{} non-utf8 bytes>", chunk.len()),
            }
        }
    }

    pub fn push(bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' {
                flush();
            } else if b != b'\r' {
                unsafe {
                    if LEN >= CAP {
                        flush();
                    }
                    BUF[LEN] = b;
                    LEN += 1;
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_log_str(s: *const u8, len: u32) {
    let bytes = unsafe { core::slice::from_raw_parts(s, len as usize) };
    #[cfg(target_os = "none")]
    {
        target_log::push(bytes);
    }
    #[cfg(not(target_os = "none"))]
    {
        use std::io::Write;
        let mut out = std::io::stderr().lock();
        let _ = out.write_all(bytes);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_get_ticks_ms() -> u32 {
    #[cfg(target_os = "none")]
    {
        embassy_time::Instant::now()
            .duration_since(embassy_time::Instant::from_secs(0))
            .as_millis() as u32
    }
    #[cfg(not(target_os = "none"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(std::time::Instant::now);
        start.elapsed().as_millis() as u32
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_sleep_ms(ms: u32) {
    #[cfg(target_os = "none")]
    {
        let deadline =
            embassy_time::Instant::now() + embassy_time::Duration::from_millis(ms as u64);
        while embassy_time::Instant::now() < deadline {
            core::hint::spin_loop();
        }
    }
    #[cfg(not(target_os = "none"))]
    {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

#[cfg(not(target_os = "none"))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_fatal(msg: *const i8) {
    if !msg.is_null() {
        let len = unsafe { strlen(msg) };
        dg_log_str(msg as *const u8, len as u32);
    }
    // The actual longjmp is handled in C code
    // We just mark the error; C code will do the jump
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_set_bench(enabled: i32) {
    unsafe {
        dg_bench = enabled;
        if enabled != 0 {
            dg_bench_ms = 0;
        }
    }
}

/// Single-producer/single-consumer key event queue feeding the engine.
/// `Doom::push_key` (app task) enqueues; C `DG_GetKey` (engine tick, same
/// task thread) dequeues via [`dg_pop_key`]. Full-queue pushes are dropped.
use core::sync::atomic::{AtomicU32, Ordering};

static KEY_HEAD: AtomicU32 = AtomicU32::new(0);
static KEY_TAIL: AtomicU32 = AtomicU32::new(0);
static mut KEY_QUEUE: [(u8, u8); 64] = [(0, 0); 64]; // (pressed, key)

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_push_key(pressed: i32, key: u8) {
    let head = KEY_HEAD.load(Ordering::Relaxed);
    let tail = KEY_TAIL.load(Ordering::Relaxed);
    let next = if head + 1 >= 64 { 0 } else { head + 1 };
    if next == tail {
        return; // full: drop oldest input rather than blocking the app
    }
    unsafe {
        KEY_QUEUE[head as usize] = ((pressed != 0) as u8, key);
    }
    KEY_HEAD.store(next, Ordering::Relaxed);
}

/// Pops one queued key event for C `DG_GetKey`. Returns 1 and fills
/// `pressed`/`key` when an event was pending, 0 when the queue is empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_pop_key(pressed: *mut i32, key: *mut u8) -> i32 {
    let head = KEY_HEAD.load(Ordering::Relaxed);
    let tail = KEY_TAIL.load(Ordering::Relaxed);
    if tail == head {
        return 0;
    }
    let (p, k) = unsafe { KEY_QUEUE[tail as usize] };
    unsafe {
        if !pressed.is_null() {
            *pressed = i32::from(p);
        }
        if !key.is_null() {
            *key = k;
        }
    }
    KEY_TAIL.store(
        if tail + 1 >= 64 { 0 } else { tail + 1 },
        Ordering::Relaxed,
    );
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_set_wad(data: *const u8, size: u32) {
    unsafe extern "C" {
        static mut dg_wad_data: *const u8;
        static mut dg_wad_size: u32;
    }
    unsafe {
        dg_wad_data = data;
        dg_wad_size = size;
    }
}

// Heap functions - implemented in Rust, called from C via v-table
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_heap_alloc(size: u32) -> *mut u8 {
    #[cfg(target_os = "none")]
    {
        let layout = Layout::from_size_align_unchecked(size as usize, 8);
        alloc(layout)
    }
    #[cfg(not(target_os = "none"))]
    {
        std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(size as usize, 8))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_heap_alloc_zeroed(size: u32) -> *mut u8 {
    let ptr = dg_heap_alloc(size);
    if !ptr.is_null() {
        core::ptr::write_bytes(ptr, 0, size as usize);
    }
    ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_heap_free(ptr: *mut u8, size: u32) {
    #[cfg(target_os = "none")]
    {
        let layout = Layout::from_size_align_unchecked(size as usize, 8);
        dealloc(ptr, layout);
    }
    #[cfg(not(target_os = "none"))]
    {
        std::alloc::dealloc(ptr, std::alloc::Layout::from_size_align_unchecked(size as usize, 8))
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dg_heap_alloc_size(_ptr: *mut u8) -> u32 {
    0 // Not tracked; caller must know size
}

#[cfg(not(target_os = "none"))]
unsafe extern "C" {
    fn strlen(s: *const i8) -> usize;
}

#[cfg(target_os = "none")]
unsafe extern "C" {
    fn strlen(s: *const i8) -> usize;
}

/// V-table of platform functions for C code to call (host version only).
/// For target, the C code in dg_xpanse.c provides the DG_PLATFORM.
#[cfg(not(target_os = "none"))]
#[unsafe(no_mangle)]
pub static DG_PLATFORM: DgPlatform = DgPlatform {
    log_str: dg_log_str,
    get_ticks_ms: dg_get_ticks_ms,
    sleep_ms: dg_sleep_ms,
    fatal: dg_fatal,
    set_bench: dg_set_bench,
    push_key: dg_push_key,
    set_wad: dg_set_wad,
    heap_alloc: dg_heap_alloc,
    heap_alloc_zeroed: dg_heap_alloc_zeroed,
    heap_free: dg_heap_free,
    heap_alloc_size: dg_heap_alloc_size,
};

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn key_queue_roundtrip() {
        // Drain anything left over from other tests.
        let mut pressed = 0;
        let mut key = 0;
        while unsafe { dg_pop_key(&mut pressed, &mut key) } != 0 {}

        assert_eq!(unsafe { dg_pop_key(&mut pressed, &mut key) }, 0);
        unsafe { dg_push_key(1, 0xad) };
        unsafe { dg_push_key(0, 0xad) };
        assert_eq!(unsafe { dg_pop_key(&mut pressed, &mut key) }, 1);
        assert_eq!((pressed, key), (1, 0xad));
        assert_eq!(unsafe { dg_pop_key(&mut pressed, &mut key) }, 1);
        assert_eq!((pressed, key), (0, 0xad));
        assert_eq!(unsafe { dg_pop_key(&mut pressed, &mut key) }, 0);
    }
}

/// Platform function v-table for C code.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DgPlatform {
    pub log_str: unsafe extern "C" fn(*const u8, u32),
    pub get_ticks_ms: unsafe extern "C" fn() -> u32,
    pub sleep_ms: unsafe extern "C" fn(u32),
    pub fatal: unsafe extern "C" fn(*const i8),
    pub set_bench: unsafe extern "C" fn(i32),
    pub push_key: unsafe extern "C" fn(i32, u8),
    pub set_wad: unsafe extern "C" fn(*const u8, u32),
    pub heap_alloc: unsafe extern "C" fn(u32) -> *mut u8,
    pub heap_alloc_zeroed: unsafe extern "C" fn(u32) -> *mut u8,
    pub heap_free: unsafe extern "C" fn(*mut u8, u32),
    pub heap_alloc_size: unsafe extern "C" fn(*mut u8) -> u32,
}
