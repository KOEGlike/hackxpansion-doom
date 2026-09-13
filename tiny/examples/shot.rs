//! Host screenshot helper: renders E1M1 views to PPM files for review.
//!
//! Run with: cargo run --example shot
//! Writes /tmp/doom_shot_{start,turn,door}.ppm

use std::time::Instant;
use tinydoom::level::{self, Level};
use tinydoom::render::{Camera, FrameState, Renderer, Target, VIEW_H, VIEW_W};
use tinydoom::wad::{Entry, Wad, MAX_LUMPS};
use tinydoom::{game, map};

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
    let mut storage = [Entry::zero(); MAX_LUMPS];
    let wad = Wad::parse(DOOM_WAD, &mut storage).expect("valid wad");
    let mut scratch = [0u8; 24576];
    let mut map_storage = map::Map::ZERO;
    map_storage
        .load_into(&wad, "E1M1", &mut scratch)
        .expect("map");
    let mut level = Level::ZERO;
    level::load_into(&mut level, wad, "E1M1", &mut scratch).expect("level");
    let player = game::spawn(&level).expect("spawn");
    println!(
        "spawn at ({:.0}, {:.0}) angle {:.0} deg sector {}",
        player.x,
        player.y,
        player.angle * 180.0 / std::f32::consts::PI,
        player.sector
    );

    let mut renderer = Renderer::new();
    let mut state = FrameState::ZERO;
    let base = game::camera(&level, &player);

    let views: [(&str, Camera); 3] = [
        ("start", base),
        (
            "turn",
            Camera {
                angle: base.angle + 1.1,
                ..base
            },
        ),
        (
            "door",
            Camera {
                x: base.x + 160.0,
                y: base.y + 40.0,
                angle: base.angle + 0.15,
                ..base
            },
        ),
    ];
    for (name, cam) in views {
        let mut img = Image {
            px: vec![0; VIEW_W * VIEW_H],
        };
        let t = Instant::now();
        renderer.render(&level, &wad, &mut state, cam, &mut img);
        println!("{name}: {:?} (cam {cam:.1?})", t.elapsed());
        let mut ppm = format!("P6\n{VIEW_W} {VIEW_H}\n255\n").into_bytes();
        for px in &img.px {
            let (r, g, b) = rgb565_to_rgb888(*px);
            ppm.extend_from_slice(&[r, g, b]);
        }
        let path = format!("/tmp/doom_shot_{name}.ppm");
        std::fs::write(&path, &ppm).expect("write ppm");
        println!("wrote {path}");
    }
}
