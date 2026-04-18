//! Probe MMC5 state for Metal Slader Glory and Uchuu Keibitai SDF to
//! understand what modes/banks/registers they actually set at various
//! points during startup. Helpful for narrowing where the CHR garbling
//! could be coming from.
//!
//! Run with: cargo test --test mmc5_state_probe -- --ignored --nocapture

use std::path::PathBuf;

fn probe(rom_name: &str, frame_counts: &[u64]) {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("roms");
    path.push(rom_name);
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("[skip] ROM not available at {}", path.display());
        return;
    };

    let mut nes = match termnes::Nes::from_ines_bytes(&bytes) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("[skip] {}: {}", rom_name, e);
            return;
        }
    };

    let mut last = 0u64;
    for &target in frame_counts {
        while last < target {
            nes.step_frame();
            last += 1;
        }
        let cart = &nes.cpu.bus.cartridge;
        let exram_mode = cart.dbg_exram_mode().unwrap_or(0xFF);
        let nt_map = cart.dbg_nametable_mapping().unwrap_or(0);
        let split_mode = cart.dbg_split_mode().unwrap_or(0);
        let banks_a = cart.dbg_chr_banks_a().unwrap_or([0; 8]);
        let banks_b = cart.dbg_chr_banks_b().unwrap_or([0; 4]);
        let chr_high = cart.dbg_chr_high().unwrap_or(0);
        let ppu_ctrl = nes.cpu.bus.ppu.ctrl;
        let ppu_mask = nes.cpu.bus.ppu.mask;

        // Peek CIRAM at each tile row (every 32 bytes) so we can see if
        // different screen regions have different tile indices — which would
        // indicate a bg image is actually laid out in the nametable.
        let mut rows = Vec::new();
        for row in 0..30 {
            let offset = row * 32;
            let b = nes.cpu.bus.ppu.vram[offset];
            let all_same = (0..32).all(|c| nes.cpu.bus.ppu.vram[offset + c] == b);
            rows.push(if all_same { format!("row{row:02}=${b:02X}*") } else { format!("row{row:02}=mixed") });
        }
        let vram_sample = rows.join(" ");
        let ciram1_sample: Vec<u8> = (0..8).map(|i| nes.cpu.bus.ppu.vram[0x400 + i]).collect();

        // Count non-off-screen sprites (Y < 240).
        let onscreen_sprites = (0..64)
            .filter(|&i| nes.cpu.bus.ppu.oam[i * 4] < 240)
            .count();
        // Collect top Y values of on-screen sprites to see if any are near
        // the top of the screen.
        let sprite_ys: Vec<u8> = (0..64)
            .map(|i| nes.cpu.bus.ppu.oam[i * 4])
            .filter(|&y| y < 240)
            .take(16)
            .collect();

        let state = nes.save_state();
        if let termnes::savestate::MapperState::Mmc5 {
            prg_mode,
            chr_mode,
            prg_banks,
            irq_target,
            irq_enable,
            ..
        } = &state.mapper
        {
            eprintln!(
                "[{} @ frame {}]\n  prg_mode={} chr_mode={} prg_banks={:?}\n  \
                 exram_mode={} nt_map=${:02X} split_mode=${:02X}\n  \
                 chr_high={} banks_a={:?} banks_b={:?}\n  \
                 irq_target={} irq_enable={} ppu_ctrl=${:02X} ppu_mask=${:02X}\n  \
                 NT0: {}\n  ciram1[0..8]={:02X?}\n  \
                 onscreen_sprites={} sprite_ys[0..16]={:?}",
                rom_name, last, prg_mode, chr_mode, prg_banks,
                exram_mode, nt_map, split_mode,
                chr_high, banks_a, banks_b,
                irq_target, irq_enable, ppu_ctrl, ppu_mask,
                vram_sample, ciram1_sample,
                onscreen_sprites, sprite_ys
            );
        }
    }
}

#[test]
#[ignore]
fn probe_msg() {
    probe("metalsladerglory.nes", &[120, 600]);
}

#[test]
#[ignore]
fn probe_uksdf() {
    probe("uksdf.nes", &[60, 90, 150, 240, 400, 600]);
}
