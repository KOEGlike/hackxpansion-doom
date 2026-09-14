//! The crates.io fallback (`assets/doom.wad`) must match the wadtool
//! pipeline output, or registry consumers would boot different art than
//! workspace checkouts. Regenerate it by copying the fresh `OUT_DIR`
//! `doom.wad` after any source-asset or wadtool change.

#[test]
fn vendored_wad_matches_pipeline() {
    // An explicit DOOM_WAD override intentionally diverges from both.
    if std::env::var_os("DOOM_WAD").is_some() {
        return;
    }
    assert_eq!(
        include_bytes!("../assets/doom.wad"),
        tinydoom::DOOM_WAD,
        "assets/doom.wad is stale: rebuild and copy OUT_DIR/doom.wad over it"
    );
}
