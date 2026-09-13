//! DOOM-style port app for the hackxpansion console.
//!
//! Leases the eight button roles and the RGB565 framebuffer from the registry
//! and drives the purpose-built `tinydoom` raycaster (fits 512 KiB RAM where
//! the vanilla engine could not). The engine renders RGB565 directly into a
//! 320x200 session; the display task centers it, leaving 20 pixel borders.
//!
//! Controls: the four-button module provides Up/Down/Left/Right (its face
//! buttons alias the D-pad on the same physical pins) and a second two-button
//! module provides A/B:
//!
//! | role  | in game          |
//! |-------|------------------|
//! | Up    | move forward     |
//! | Down  | move backward    |
//! | Left  | turn left        |
//! | Right | turn right       |
//! | A     | fire (later)     |
//! | B     | use (later)      |
//!
//! Holding Up and Down for 1.5 seconds exits back to the app picker.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::{future::Future, pin::Pin};

use tinydoom::{
    engine::{self, Engine},
    render::{Target, VIEW_H, VIEW_W},
};
use xpanse_api::{
    app::App,
    interfaces::{
        buttons::{A, B, Button, Down, Left, Right, Up},
        video::{Rgb565FrameBuffer, Rgb565FrameSession, Rgb565Pixel},
    },
    reexports::{
        defmt,
        embassy_futures::yield_now,
        embassy_time::{Duration, Instant, Ticker},
    },
    registry::{Registry, ResourceLease},
};

const DOOM_WIDTH: u16 = VIEW_W as u16;
const DOOM_HEIGHT: u16 = VIEW_H as u16;
/// Frames between defmt performance reports (~4 s of game time at 35 Hz).
const STATS_FRAME_INTERVAL: u32 = 140;
/// The engine is paced at 35 Hz like vanilla DOOM.
const FRAME_INTERVAL: Duration = Duration::from_micros(28_571);
/// How long Up+Down must be held to leave the app.
const EXIT_HOLD: Duration = Duration::from_millis(1500);

type AppButton<R> = ResourceLease<Box<dyn Button<R>>>;
/// One distinct physical group per role: Up/Down/Left/Right come from the
/// four-button module (its face buttons alias the D-pad on the same pins), A
/// and B from a second (two-button) module. Leasing both aliases of one pin
/// is impossible (one physical group per pin), so X/Y are not requested.
type DoomResources = (
    Box<dyn Button<Up>>,
    Box<dyn Button<Down>>,
    Box<dyn Button<Left>>,
    Box<dyn Button<Right>>,
    Box<dyn Button<A>>,
    Box<dyn Button<B>>,
    Rgb565FrameBuffer,
);

/// Display session viewed as a tinydoom pixel target.
struct SessionTarget<'a> {
    session: Rgb565FrameSession<'a>,
}

impl Target for SessionTarget<'_> {
    fn put(&mut self, x: usize, y: usize, rgb: u16) {
        self.session
            .set_pixel(x as u16, y as u16, Rgb565Pixel(rgb));
    }
}

/// Button state with edge detection, mapped to engine key events.
struct Keys<'a> {
    up: &'a dyn Button<Up>,
    down: &'a dyn Button<Down>,
    left: &'a dyn Button<Left>,
    right: &'a dyn Button<Right>,
    a: &'a dyn Button<A>,
    b: &'a dyn Button<B>,
    previous: [(bool, u8); 6],
}

impl<'a> Keys<'a> {
    fn new(
        up: &'a dyn Button<Up>,
        down: &'a dyn Button<Down>,
        left: &'a dyn Button<Left>,
        right: &'a dyn Button<Right>,
        a: &'a dyn Button<A>,
        b: &'a dyn Button<B>,
    ) -> Self {
        Self {
            up,
            down,
            left,
            right,
            a,
            b,
            previous: [(false, 0); 6],
        }
    }

    fn push(&mut self, engine: &mut Engine) {
        let states = [
            (self.up.is_pressed(), engine::keys::KEY_UPARROW),
            (self.down.is_pressed(), engine::keys::KEY_DOWNARROW),
            (self.left.is_pressed(), engine::keys::KEY_LEFTARROW),
            (self.right.is_pressed(), engine::keys::KEY_RIGHTARROW),
            (self.a.is_pressed(), engine::keys::KEY_FIRE),
            (self.b.is_pressed(), engine::keys::KEY_USE),
        ];
        for slot in 0..states.len() {
            let (pressed, key) = states[slot];
            let (previous, _) = self.previous[slot];
            if pressed != previous {
                engine.push_key(pressed, key);
                self.previous[slot] = (pressed, key);
            }
        }
    }

    fn exit_held(&self) -> bool {
        self.up.is_pressed() && self.down.is_pressed()
    }
}

pub struct DoomApp {
    up: AppButton<Up>,
    down: AppButton<Down>,
    left: AppButton<Left>,
    right: AppButton<Right>,
    a: AppButton<A>,
    b: AppButton<B>,
    frame_buffer: ResourceLease<Rgb565FrameBuffer>,
}

impl App for DoomApp {
    const NAME: &'static str = "DOOM";

    fn can_run(registry: &Registry) -> bool {
        registry.has_resource_set::<DoomResources>()
    }

    fn new(registry: &mut Registry) -> Option<Self> {
        let (up, down, left, right, a, b, frame_buffer) =
            registry.take_resource_set::<DoomResources>()?;
        Some(Self {
            up,
            down,
            left,
            right,
            a,
            b,
            frame_buffer,
        })
    }

    fn run<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = ()> + 'a>> {
        Box::pin(async move {
            let frame_buffer = match self
                .frame_buffer
                .resource_mut()
                .start(DOOM_WIDTH, DOOM_HEIGHT)
            {
                Ok(frame_buffer) => frame_buffer,
                Err(error) => {
                    defmt::error!("DOOM: failed to start framebuffer: {}", error);
                    return;
                }
            };

            let mut target = SessionTarget {
                session: frame_buffer,
            };
            let mut engine = match Engine::boot(tinydoom::DOOM_WAD) {
                Some(engine) => engine,
                None => {
                    defmt::error!("DOOM: engine failed to boot");
                    return;
                }
            };

            let mut keys = Keys::new(
                self.up.resource().as_ref(),
                self.down.resource().as_ref(),
                self.left.resource().as_ref(),
                self.right.resource().as_ref(),
                self.a.resource().as_ref(),
                self.b.resource().as_ref(),
            );

            let mut stats_started = Instant::now();
            let mut stats_frames = 0_u32;
            let mut exit_hold = Instant::now();
            let mut ticker = Ticker::every(FRAME_INTERVAL);

            loop {
                keys.push(&mut engine);
                let running = engine.tick(&mut target);
                target.session.present();

                stats_frames += 1;
                if stats_frames == STATS_FRAME_INTERVAL {
                    let elapsed = Instant::now().duration_since(stats_started);
                    let fps = (stats_frames * 100) / elapsed.as_secs() as u32;
                    defmt::info!(
                        "DOOM: {} frames in {} ms ({} fps x100)",
                        stats_frames,
                        elapsed.as_millis(),
                        fps,
                    );
                    stats_started = Instant::now();
                    stats_frames = 0;
                }

                if keys.exit_held() {
                    if Instant::now().duration_since(exit_hold) >= EXIT_HOLD {
                        break;
                    }
                } else {
                    exit_hold = Instant::now();
                }

                if !running {
                    defmt::warn!("DOOM: engine stopped");
                    break;
                }

                // A ticker that has fallen behind completes immediately, so it
                // cannot guarantee that the sibling display future is polled.
                yield_now().await;
                ticker.next().await;
            }
        })
    }

    fn release(self, registry: &mut Registry) {
        registry.return_resource(self.up);
        registry.return_resource(self.down);
        registry.return_resource(self.left);
        registry.return_resource(self.right);
        registry.return_resource(self.a);
        registry.return_resource(self.b);
        registry.return_resource(self.frame_buffer);
    }
}
