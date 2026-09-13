//! Tiny Doom-like raycaster for the hackxpansion console.
//!
//! A purpose-built engine that reuses the stripped console WAD (see
//! `wadtool`) and fits comfortably in 512 KiB RAM: map + tables live in
//! small `Vec`s, art is decoded straight from flash on demand, and there
//! is no zone allocator, no thinkers heap-churn, no floating FPU need.
//!
//! Math is `f32` throughout (the Cortex-M33 single-precision FPU is
//! enabled by the firmware on both cores); map units are Doom units.

#![cfg_attr(target_os = "none", no_std)]

extern crate alloc;

pub mod assets;
pub mod engine;
pub mod game;
pub mod level;
pub mod map;
pub mod render;
pub mod wad;

include!(concat!(env!("OUT_DIR"), "/wad.rs"));
