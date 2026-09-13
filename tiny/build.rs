use std::env;

fn main() {
    let out_dir = env::var_os("OUT_DIR").map(std::path::PathBuf::from).unwrap();

    // Same stripped WAD as the Chocolate port (single source of truth in
    // wadtool). DOOM_WAD overrides the default E1M1 asset. Paths below are
    // relative to this crate root, which is also the build script CWD.
    let wad = match env::var_os("DOOM_WAD") {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            path
        }
        None => {
            let default = std::path::PathBuf::from("../doom-core/assets/doom_e1m1.wad");
            println!("cargo:rerun-if-changed={}", default.display());
            default
        }
    };
    println!("cargo:rerun-if-env-changed=DOOM_WAD");
    wadtool::compress_wad(&wad, &out_dir.join("doom.wad"));

    let wad_path = out_dir.join("doom.wad");
    let literal = wad_path.display().to_string();
    std::fs::write(
        out_dir.join("wad.rs"),
        format!("pub static DOOM_WAD: &[u8] = include_bytes!({literal:?});\n"),
    )
    .expect("failed to generate wad.rs");
}
