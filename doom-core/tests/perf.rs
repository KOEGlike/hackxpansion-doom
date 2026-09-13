//! DOOM engine performance tests.
//!
//! These run the actual C engine on the host: boot into E1M1 (the embedded
//! WAD autostarts the level), simulate player input, and time frames. The
//! device build executes the same engine, so these numbers indicate how the
//! console performs relative to the host; the firmware additionally logs
//! per-frame timings over defmt at runtime.
//!
//! Run with: cargo test --release --test perf -- --nocapture

/// Key codes from `doomkeys.h` (the engine's in-game bindings).
mod keys {
    pub const KEY_UPARROW: u8 = 0xad;
    pub const KEY_DOWNARROW: u8 = 0xaf;
    pub const KEY_LEFTARROW: u8 = 0xac;
    pub const KEY_RIGHTARROW: u8 = 0xae;
    pub const KEY_FIRE: u8 = 0xa3;
    pub const KEY_USE: u8 = 0xa2;
}

use doom_core::Doom;
use std::{cell::RefCell, rc::Rc};

/// Counters shared between the sink and the test body.
#[derive(Default)]
struct Counters {
    frames: u32,
    non_black_pixels: u64,
}

#[derive(Clone, Default)]
struct BenchSink {
    counters: Rc<RefCell<Counters>>,
}

impl doom_core::FrameSink for BenchSink {
    fn draw(&mut self, screen: &[u8], _palette: &[[u8; 3]; 256]) {
        let mut counters = self.counters.borrow_mut();
        counters.frames += 1;
        counters.non_black_pixels += screen.iter().filter(|&&p| p != 0).count() as u64;
    }
}

/// One engine instance per process; run all benchmarking sequentially here.
#[test]
fn doom_engine_benchmark() {
    let sink = BenchSink::default();
    let mut sink_ref = sink.clone();
    let mut doom = Doom::new(&mut sink_ref).expect("engine boots from the embedded WAD");
    doom_core::set_bench_mode(true);
    let (zs, zl, zc, zf) = doom_core::zone_stats();
    println!("zone after boot: static={zs} level={zl} cache={zc} free={zf}");
    doom_core::zone_top();

    // Warm-up: let the level load settle, then hold "forward" while firing,
    // which is close to the worst-case render load (walls + sprites + flats).
    for _ in 0..35 {
        doom.tick();
    }
    let started = std::time::Instant::now();
    let mut hot_frames = 0;
    for _ in 0..35 {
        doom.push_key(true, keys::KEY_UPARROW);
        doom.push_key(true, keys::KEY_FIRE);
        if doom.tick() {
            hot_frames += 1;
        }
        doom.push_key(false, keys::KEY_FIRE);
    }
    let elapsed = started.elapsed();
    let per_frame_ms = elapsed.as_secs_f64() * 1000.0 / hot_frames.max(1) as f64;
    println!("doom: {hot_frames} frames in {elapsed:?} ({per_frame_ms:.2} ms/frame on host)");

    // The engine must produce meaningful output: E1M1's sky, walls and flats
    // should paint a large part of the screen.
    let counters = sink.counters.borrow();
    assert!(
        counters.non_black_pixels > 10_000,
        "expected rendered content, got {} non-black pixels",
        counters.non_black_pixels
    );
    assert!(hot_frames >= 30, "expected at least 30 frames, got {hot_frames}");
    drop(counters);

    // Turn benchmark: spinning around forces every visible sprite and texture
    // to be cached, which is the worst case for zone thrashing.
    for key in [keys::KEY_RIGHTARROW, keys::KEY_LEFTARROW] {
        for _ in 0..70 {
            doom.push_key(true, key);
            let _ = doom.tick();
            doom.push_key(false, key);
        }
    }
    let counters = sink.counters.borrow();
    println!(
        "doom: {} total frames rendered, {} non-black pixels total",
        counters.frames, counters.non_black_pixels
    );
    drop(counters);
    let (zs, zl, zc, zf) = doom_core::zone_stats();
    println!("zone after gameplay: static={zs} level={zl} cache={zc} free={zf}");

    // On the target (RP2354B at 150 MHz) this has to hold ~10 FPS or better;
    // on the host it is an order of magnitude faster. The threshold below is
    // intentionally loose: it guards against catastrophic regressions (e.g.
    // falling off the fast path), not against absolute device performance.
    assert!(
        per_frame_ms < 150.0,
        "rendering became pathological: {per_frame_ms:.2} ms/frame"
    );
}
