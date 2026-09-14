use std::env;
use std::path::PathBuf;

fn main() {
    let out_dir = env::var_os("OUT_DIR").map(PathBuf::from).unwrap();
    let embedded = out_dir.join("doom.wad");
    println!("cargo:rerun-if-env-changed=DOOM_WAD");

    // Same stripped WAD as the Chocolate port (single source of truth in
    // wadtool). Paths below are relative to this crate root, which is also
    // the build script CWD.
    if let Some(path) = env::var_os("DOOM_WAD").map(PathBuf::from) {
        // Explicit override: compress the given source WAD.
        println!("cargo:rerun-if-changed={}", path.display());
        wadtool::compress_wad(&path, &embedded);
    } else {
        let source = PathBuf::from("../doom-core/assets/doom_e1m1.wad");
        println!("cargo:rerun-if-changed={}", source.display());
        if source.exists() {
            // Workspace checkout: compress from the shared source asset.
            wadtool::compress_wad(&source, &embedded);
        } else {
            // Crates.io checkout (no sibling crates): fall back to the
            // vendored stripped WAD. Regenerate it via the pipeline above
            // whenever the source asset or wadtool changes; `vendored_wad`
            // test guards against drift.
            let vendored = PathBuf::from("assets/doom.wad");
            println!("cargo:rerun-if-changed={}", vendored.display());
            std::fs::copy(&vendored, &embedded).expect("vendored assets/doom.wad missing");
        }
    }

    let literal = embedded.display().to_string();
    std::fs::write(
        out_dir.join("wad.rs"),
        format!("pub static DOOM_WAD: &[u8] = include_bytes!({literal:?});\n"),
    )
    .expect("failed to generate wad.rs");
}
