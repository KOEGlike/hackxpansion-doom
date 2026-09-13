use doom_core::Doom;
use std::{cell::RefCell, rc::Rc};
struct Sink { px: Rc<RefCell<Vec<u8>>> }
impl doom_core::FrameSink for Sink {
    fn draw(&mut self, screen: &[u8], _palette: &[[u8; 3]; 256]) {
        *self.px.borrow_mut() = screen.to_vec();
    }
}
fn main() {
    let px = Rc::new(RefCell::new(Vec::new()));
    let mut sink = Sink { px: px.clone() };
    let mut doom = Doom::new(&mut sink).expect("boot");
    doom_core::set_bench_mode(true);
    for _ in 0..5 { doom.tick(); }
    let px = px.borrow();
    // PLAYPAL from the source WAD directly (I_SetPalette is stubbed out).
    let wad = std::fs::read("../doom-core/assets/doom_e1m1.wad").expect("wad");
    let n = u32::from_le_bytes(wad[4..8].try_into().unwrap()) as usize;
    let io = u32::from_le_bytes(wad[8..12].try_into().unwrap()) as usize;
    let mut pal = None;
    for i in 0..n {
        let base = io + i * 16;
        if &wad[base + 8..base + 16] == b"PLAYPAL\0" {
            let off = u32::from_le_bytes(wad[base..base + 4].try_into().unwrap()) as usize;
            pal = Some(wad[off..off + 768].to_vec());
            break;
        }
    }
    let pal = pal.expect("PLAYPAL");
    let mut ppm = format!("P6\n320 200\n255\n").into_bytes();
    for p in px.iter() {
        let o = *p as usize * 3;
        ppm.extend_from_slice(&pal[o..o + 3]);
    }
    std::fs::write("/tmp/choc_ref.ppm", &ppm).unwrap();
    println!("wrote ref");
}
