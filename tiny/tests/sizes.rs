#[test]
fn print_static_sizes() {
    println!("Map: {} KB", core::mem::size_of::<tinydoom::map::Map>() / 1024);
    println!("Level: {} KB", core::mem::size_of::<tinydoom::level::Level>() / 1024);
    println!("FrameState: {} B", core::mem::size_of::<tinydoom::render::FrameState>());
    println!("Renderer: {} B", core::mem::size_of::<tinydoom::render::Renderer>());
    println!("Wad entries: {} B", core::mem::size_of::<tinydoom::wad::Entry>() * tinydoom::wad::MAX_LUMPS);
}
