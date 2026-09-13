use std::env;

/// Size of the static DOOM zone buffer on the console (144 KiB of the 512 KiB
/// SRAM; the platform display buffer and its own heap take most of the rest).
const ZONE_SIZE: u32 = 144 * 1024;
/// Host builds get a much larger zone so the perf tests exercise the engine
/// without zone pressure; the target uses the small static buffer above.
const HOST_ZONE_SIZE: u32 = 4 * 1024 * 1024;

/// Engine sources compiled for every target. The platform port files
/// (doomgeneric_{sdl,xlib,win,...}), the SDL sound/music backends and the
/// stdio-based WAD reader are intentionally omitted; dg_wadfile.c provides the
/// in-memory WAD class instead.
const SOURCES: &[&str] = &[
    "d_event.c", "d_items.c", "d_iwad.c", "d_loop.c", "d_main.c", "d_mode.c",
    "d_net.c", "doomdef.c", "doomgeneric.c", "doomstat.c", "dstrings.c", "dummy.c",
    "f_wipe.c", "g_game.c",
    "info.c", "i_sound.c", "i_system.c", "i_timer.c", "i_video.c", "m_argv.c", "m_bbox.c",
    "m_controls.c", "m_fixed.c", "m_misc.c", "m_random.c",
    "p_ceilng.c", "p_doors.c", "p_enemy.c", "p_floor.c", "p_inter.c", "p_lights.c", "p_map.c",
    "p_maputl.c", "p_mobj.c", "p_plats.c", "p_pspr.c", "p_setup.c", "p_sight.c",
    "p_spec.c", "p_switch.c", "p_telept.c", "p_tick.c", "p_user.c", "r_bsp.c", "r_data.c",
    "r_draw.c", "r_main.c", "r_plane.c", "r_segs.c", "r_sky.c", "r_things.c",
    "sounds.c", "tables.c", "v_video.c",
    "w_file.c", "w_main.c", "w_wad.c", "z_zone.c", "dg_wadfile.c",
];

fn main() {
    let out_dir = env::var_os("OUT_DIR").map(std::path::PathBuf::from).unwrap();
    let target = env::var("TARGET").expect("TARGET is set by Cargo");
    let thumb = target.contains("thumbv8m");

    // --- embed the WAD image (LZ4-compressed large lumps) ----------------------
    let wad = match env::var_os("DOOM_WAD") {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            path
        }
        None => {
            let default = std::path::PathBuf::from("assets/doom_e1m1.wad");
            println!("cargo:rerun-if-changed={}", default.display());
            default
        }
    };
    let embedded = out_dir.join("doom.wad");
    wadtool::compress_wad(&wad, &embedded);
    println!("cargo:rerun-if-env-changed=DOOM_WAD");
    println!("cargo:rerun-if-changed=../doom-sys/src/c");
    println!("cargo:rerun-if-changed=../doom-sys/include");
    println!("cargo:rerun-if-changed=src/c");

    // --- compile the C engine -------------------------------------------------
    let zone = if thumb { ZONE_SIZE } else { HOST_ZONE_SIZE };
    let mut build = cc::Build::new();
    build.include("../doom-sys/src/c");
    if !thumb {
        build.include("src/c");
    }
    build
        .define("DOOMGENERIC_RESX", "320")
        .define("DOOMGENERIC_RESY", "200")
        .define("DG_ZONE_SIZE", format!("{}", zone).as_str())
        .define("NDEBUG", None);
    if thumb {
        build
            .compiler("clang")
            .opt_level_str("z")
            .include("../doom-sys/include")
            .flags([
                "-target",
                "thumbv8m.main-none-eabihf",
                "-mfloat-abi=hard",
                "-fno-stack-protector",
                "-ffunction-sections",
                "-fdata-sections",
            ]);
        build.file("../doom-sys/src/c/setjmp.S");
        build.file("../doom-sys/src/c/xpanse_libc.c");
    }
    if !cfg!(feature = "disable-c") {
        for source in SOURCES {
            build.file(format!("../doom-sys/src/c/{source}"));
        }
        // Only compile platform-specific sources for target.
        // For host tests, doom-sys provides the platform functions.
        if thumb {
            build.file("../doom-sys/src/c/dg_xpanse.c");
        } else {
            build.file("src/c/dg_host.c");
        }
    }
    build.warnings(false).compile("doomcore");


    // --- generated Rust module ------------------------------------------------
    let wad_path = out_dir.join("doom.wad");
    let literal = wad_path.display().to_string();
    std::fs::write(
        out_dir.join("wad.rs"),
        format!("pub static DOOM_WAD: &[u8] = include_bytes!({literal:?});\n"),
    )
    .expect("failed to generate wad.rs");
    std::fs::write(
        out_dir.join("zone.rs"),
        format!("pub const ZONE_SIZE: usize = {ZONE_SIZE};\n"),
    )
    .expect("failed to generate zone.rs");
}

