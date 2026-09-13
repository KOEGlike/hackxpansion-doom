# hackxpansion-doom

Doom for the Hackxpansion handheld console (RP2354B, 512 KiB RAM, 320x240
RGB565 display, button modules).

## Architecture

Vanilla Chocolate-Doom cannot fit: E1M1 alone needs ~300 KiB of zone RAM
against ~190 KiB reclaimable (measured via host zone telemetry; see
`doom-core/tests/perf.rs` + `dg_zone_stats`). Instead this repo ships a
purpose-built raycaster that reuses the Doom WAD art:

| crate | role |
|---|---|
| `tiny/` (`tinydoom`) | no-std raycaster: BSP walls, floors/ceilings/sky, sprites later. All bulk state in BSS statics (~150 KiB); zero heap use. |
| `wadtool/` | host build tool: rewrites `doom_e1m1.wad` for the console (drops unused lumps, LZ4-compresses maps/sprites/flats, halves flats, quarters sprites, halves wall patches + TEXTURE1 geometry, prunes TEXTURE1 to referenced textures, keeps wall patches raw for zero-copy sampling). Single source of truth for the stripped format. |
| `src/` (`doom-app`) | firmware app shell: leases buttons + RGB565 framebuffer, drives `tinydoom::Engine` at 35 Hz. |
| `doom-core/`, `doom-sys/` | legacy Chocolate-Doom port (host tests only now; kept as a rendering/behavior reference, not linked into firmware). |

## Budgets (release firmware)

- Flash: ~1.80 MB / 2 MB (stripped WAD ~218 KiB lives in flash, XIP-mapped).
- RAM: ~395 KiB / 512 KiB; tiny engine statics ~150 KiB (`cargo test -p tinydoom --test sizes`... see `tiny/tests/sizes.rs`).

## Working on it

```sh
cd wadtool && cargo test          # WAD pipeline unit tests
cd ../tiny && cargo test          # engine unit tests + static RAM sizing
cd ../tiny && cargo run --example shot   # E1M1 screenshots -> /tmp/doom_shot_*.ppm
cd ../doom-core && cargo test --test perf  # Chocolate reference (host only)
# device firmware (from the hackxpansion repo):
cargo build --release -p xpanse --target thumbv8m.main-none-eabihf
```

Key engine invariants (all covered by tests/screenshots):

- `SEGS` lump `linedef`/`side` fields are untrusted (this WAD's node
  builder left zeros/garbage); `map::resolve_segs` re-derives them
  geometrically (blockmap + subsector centroids).
- `mapsidedef_t` order is top, BOTTOM, mid.
- Wall texture rows wrap (vanilla `&127`, here at halved height).
- `f32` math only (M33 has single-precision FPU, enabled by firmware);
  no `core`-missing float methods — use `libm` + local `euclid`.
- No heap on target: fixed arrays + `*_ZERO` consts + `load_into`.
- `Level::ZERO` must stay all-zero (else the 106 KiB struct lands in
  `.data` instead of BSS).

## Roadmap (Stage 2)

Sprites (billboard + depth buffer), doors/elevators, exit switch,
weapons, enemy AI, HUD, thing collision. The `Engine::tick`/
`push_key` API already anticipates fire/use keys.
