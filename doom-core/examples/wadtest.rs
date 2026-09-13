fn main() {
    // Verify the embedded compressed WAD structure.
    let wad = doom_core::DOOM_WAD;
    let magic = &wad[0..4];
    let numlumps = u32::from_le_bytes(wad[4..8].try_into().unwrap());
    let table = u32::from_le_bytes(wad[8..12].try_into().unwrap());
    println!("magic {magic:?} numlumps {numlumps} tableofs {table}");
    for i in 0..8 {
        let base = table as usize + i * 16;
        let pos = u32::from_le_bytes(wad[base..base + 4].try_into().unwrap());
        let size = u32::from_le_bytes(wad[base + 4..base + 8].try_into().unwrap());
        let name = String::from_utf8_lossy(&wad[base + 8..base + 16]).to_string();
        println!("{name:8} pos={pos:#010x} size={size}");
    }
}
