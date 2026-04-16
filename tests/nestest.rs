//! Drives Kevin Horton's `nestest.nes`, the canonical CPU regression ROM.
//!
//! nestest has two modes:
//!   - "graphical" — enter via the normal reset vector ($C004), runs
//!      interactively, shows results on-screen
//!   - "automation" — jump directly to $C000, runs non-interactively,
//!      writes a test-progress byte to $0000, and error codes to $02/$03
//!      when done. This is what's used by emulator test suites.
//!
//! Final state: $02 = 0 and $03 = 0 means all official-opcode tests passed.
//! Nonzero values are error codes documented in `nestest.txt`. The ROM
//! ultimately hangs in a tight loop once complete, so we cap the run.
//!
//! Skips gracefully if `tests/test_roms/nestest.nes` isn't present.

mod common;

use common::try_rom;
use termnes::Nes;

#[test]
fn nestest_automation_official_opcodes() {
    let Some(path) = try_rom("nestest.nes") else { return };
    let mut nes = Nes::from_ines_file(&path).expect("load nestest");

    // Automation mode: jump over the power-on graphic setup.
    nes.cpu.pc = 0xC000;

    // nestest completes in well under 30 frames; 120 is generous headroom.
    // We drive individual instructions (instead of step_frame) so we stay in
    // CPU-space — nestest doesn't enable rendering and shouldn't need DMA.
    // Cap by instruction count to avoid runaway loops if something is broken.
    const MAX_INSTR: u64 = 100_000_000;
    let mut instr = 0u64;
    while instr < MAX_INSTR {
        // Known "done" sinks: nestest ends with JMP to itself at a terminal
        // address once all tests complete.
        let prev_pc = nes.cpu.pc;
        nes.step_instruction();
        if nes.cpu.pc == prev_pc {
            break; // infinite loop — test has finished reporting
        }
        instr += 1;
    }

    let err_lo = nes.peek(0x0002);
    let err_hi = nes.peek(0x0003);
    assert_eq!(
        (err_lo, err_hi),
        (0, 0),
        "nestest reported errors: $02=${:02X} $03=${:02X} after {} instructions",
        err_lo,
        err_hi,
        instr
    );
    eprintln!("[pass] nestest: {} instructions executed, $02/$03 both 0", instr);
}
