//! Host walkthrough: boots the engine, walks forward from spawn,
//! screenshotting every N ticks to catch occlusion glitches.
//!
//! Run with: cargo run --example walk
//! Writes /tmp/doom_walk_{step}.ppm

use tinydoom::engine::{keys, Engine};
use tinydoom::render::{Target, VIEW_H, VIEW_W};

include!(concat!(env!("OUT_DIR"), "/wad.rs"));

struct Image {
    px: Vec<u16>,
}

impl Target for Image {
    fn put(&mut self, x: usize, y: usize, rgb: u16) {
        if x < VIEW_W && y < VIEW_H {
            self.px[y * VIEW_W + x] = rgb;
        }
    }
}

fn rgb565_to_rgb888(c: u16) -> (u8, u8, u8) {
    (
        (((c >> 11) & 0x1f) as u32 * 255 / 31) as u8,
        (((c >> 5) & 0x3f) as u32 * 255 / 63) as u8,
        ((c & 0x1f) as u32 * 255 / 31) as u8,
    )
}

fn main() {
    let mut engine = Engine::boot(DOOM_WAD).expect("boots");
    let mut img = Image {
        px: vec![0; VIEW_W * VIEW_H],
    };
    engine.push_key(true, keys::KEY_UPARROW);
    for step in 0..8 {
        for _ in 0..15 {
            assert!(engine.tick(&mut img));
        }
        let mut ppm = format!("P6\n{VIEW_W} {VIEW_H}\n255\n").into_bytes();
        for px in &img.px {
            let (r, g, b) = rgb565_to_rgb888(*px);
            ppm.extend_from_slice(&[r, g, b]);
        }
        let path = format!("/tmp/doom_walk_{step}.ppm");
        std::fs::write(&path, &ppm).expect("write ppm");
        println!("wrote {path}");
    }
}
