//! Validates the Blargg protocol driver in `tests/common` using a synthetic
//! mapper-0 ROM — no real test ROMs required. If these break we know the
//! driver itself is wrong (vs. an actual emulation bug).

mod common;

use common::{run_blargg, BlarggResult};
use nes_tui::Nes;

/// Wraps 6502 program bytes in a minimal 16 KB NROM iNES image. Program is
/// placed at CPU $8000 (ROM offset 16); the reset vector is pre-populated.
fn make_nrom_rom(program: &[u8]) -> Vec<u8> {
    const PRG_LEN: usize = 0x4000;
    let mut rom = vec![0u8; 16 + PRG_LEN];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1; // 1× 16 KB PRG bank (mirrored into $8000 and $C000)
    rom[5] = 0; // 0 CHR banks → CHR-RAM
    rom[6] = 0; // mapper 0, horizontal mirroring
    rom[7] = 0;
    assert!(program.len() <= PRG_LEN - 6, "program too large");
    rom[16..16 + program.len()].copy_from_slice(program);
    // Reset vector at CPU $FFFC → $8000. With 1 PRG bank the bank mirrors,
    // so $FFFC maps to ROM offset 16 + 0x3FFC.
    rom[16 + 0x3FFC] = 0x00;
    rom[16 + 0x3FFD] = 0x80;
    rom
}

/// Small 6502 program that follows the Blargg protocol: publishes the magic,
/// writes an ASCII result, sets status = `final_status`, then spins forever.
fn make_blargg_program(final_status: u8, text: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    // LDA #$80 ; STA $6000  (status = running)
    p.extend_from_slice(&[0xA9, 0x80, 0x8D, 0x00, 0x60]);
    // Publish magic DE B0 61 at $6001..$6003
    for (i, b) in [0xDE_u8, 0xB0, 0x61].iter().enumerate() {
        p.extend_from_slice(&[0xA9, *b, 0x8D, 0x01 + i as u8, 0x60]);
    }
    // Publish ASCII text starting at $6004
    for (i, b) in text.iter().enumerate() {
        p.extend_from_slice(&[0xA9, *b, 0x8D, 0x04 + i as u8, 0x60]);
    }
    // NUL terminator
    p.extend_from_slice(&[0xA9, 0x00, 0x8D, 0x04 + text.len() as u8, 0x60]);
    // Final status byte
    p.extend_from_slice(&[0xA9, final_status, 0x8D, 0x00, 0x60]);
    // JMP * (infinite loop at the current PC)
    let here = 0x8000u16 + p.len() as u16;
    p.push(0x4C);
    p.push(here as u8);
    p.push((here >> 8) as u8);
    p
}

#[test]
fn synthetic_blargg_pass_is_recognized() {
    let prog = make_blargg_program(0x00, b"OK");
    let rom = make_nrom_rom(&prog);
    let mut nes = Nes::from_ines_bytes(&rom).expect("rom parse");
    match run_blargg(&mut nes, 120) {
        BlarggResult::Pass(text) => assert_eq!(text, "OK"),
        other => panic!("expected Pass(\"OK\"), got {:?}", other),
    }
}

#[test]
fn synthetic_blargg_fail_is_recognized() {
    let prog = make_blargg_program(0x42, b"bad vibes");
    let rom = make_nrom_rom(&prog);
    let mut nes = Nes::from_ines_bytes(&rom).expect("rom parse");
    match run_blargg(&mut nes, 120) {
        BlarggResult::Fail(0x42, text) => assert_eq!(text, "bad vibes"),
        other => panic!("expected Fail(0x42, \"bad vibes\"), got {:?}", other),
    }
}

#[test]
fn synthetic_blargg_timeout_if_never_completes() {
    // Program that publishes magic + running status but never finishes.
    let mut p: Vec<u8> = Vec::new();
    p.extend_from_slice(&[0xA9, 0x80, 0x8D, 0x00, 0x60]); // status = 0x80
    for (i, b) in [0xDE_u8, 0xB0, 0x61].iter().enumerate() {
        p.extend_from_slice(&[0xA9, *b, 0x8D, 0x01 + i as u8, 0x60]);
    }
    let here = 0x8000u16 + p.len() as u16;
    p.extend_from_slice(&[0x4C, here as u8, (here >> 8) as u8]);

    let rom = make_nrom_rom(&p);
    let mut nes = Nes::from_ines_bytes(&rom).expect("rom parse");
    match run_blargg(&mut nes, 5) {
        BlarggResult::TimedOut(_) => {}
        other => panic!("expected TimedOut, got {:?}", other),
    }
}
