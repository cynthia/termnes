//! Blargg-format test-ROM drivers. Each test silently skips if the ROM isn't
//! present under `tests/test_roms/` — see that directory's README for how to
//! obtain the ROMs. This keeps a bare `cargo test` green on clean checkouts
//! while giving full ROM-based validation to anyone who drops the files in.

mod common;

use common::assert_blargg_pass;

// ── CPU: instruction behavior ───────────────────────────────────────────────

/// Exercises every official 6502 opcode across all addressing modes.
/// Part of Blargg's `instr_test-v5`. The "official_only" variant skips the
/// unofficial opcodes, which this emulator doesn't implement yet.
#[test]
fn blargg_instr_test_official_only() {
    assert_blargg_pass("official_only.nes", 60 * 120);
}

/// Individual instruction tests from the `instr_test-v5/rom_singles/` folder.
/// These are smaller and produce more targeted failure messages. If you only
/// want a subset, drop in just the ones you care about.
#[test]
fn blargg_instr_01_basics() {
    assert_blargg_pass("01-basics.nes", 60 * 60);
}

#[test]
fn blargg_instr_02_implied() {
    assert_blargg_pass("02-implied.nes", 60 * 60);
}

#[test]
fn blargg_instr_03_immediate() {
    assert_blargg_pass("03-immediate.nes", 60 * 60);
}

#[test]
fn blargg_instr_04_zero_page() {
    assert_blargg_pass("04-zero_page.nes", 60 * 60);
}

#[test]
fn blargg_instr_05_zp_xy() {
    assert_blargg_pass("05-zp_xy.nes", 60 * 60);
}

#[test]
fn blargg_instr_06_absolute() {
    assert_blargg_pass("06-absolute.nes", 60 * 60);
}

#[test]
fn blargg_instr_07_abs_xy() {
    assert_blargg_pass("07-abs_xy.nes", 60 * 60);
}

#[test]
fn blargg_instr_08_ind_x() {
    assert_blargg_pass("08-ind_x.nes", 60 * 60);
}

#[test]
fn blargg_instr_09_ind_y() {
    assert_blargg_pass("09-ind_y.nes", 60 * 60);
}

#[test]
fn blargg_instr_10_branches() {
    assert_blargg_pass("10-branches.nes", 60 * 60);
}

#[test]
fn blargg_instr_11_stack() {
    assert_blargg_pass("11-stack.nes", 60 * 60);
}

#[test]
fn blargg_instr_12_jmp_jsr() {
    assert_blargg_pass("12-jmp_jsr.nes", 60 * 60);
}

#[test]
fn blargg_instr_13_rts() {
    assert_blargg_pass("13-rts.nes", 60 * 60);
}

#[test]
fn blargg_instr_14_rti() {
    assert_blargg_pass("14-rti.nes", 60 * 60);
}

#[test]
fn blargg_instr_15_brk() {
    assert_blargg_pass("15-brk.nes", 60 * 60);
}

#[test]
fn blargg_instr_16_special() {
    assert_blargg_pass("16-special.nes", 60 * 60);
}

// ── CPU: timing ─────────────────────────────────────────────────────────────
// We use the newer `instr_timing` suite (reports via $6000) rather than the
// 2006-era `cpu_timing_test6` which only draws to the PPU screen and can't
// be polled programmatically.

#[test]
fn blargg_instr_timing() {
    assert_blargg_pass("1-instr_timing.nes", 60 * 60);
}

#[test]
fn blargg_branch_timing() {
    assert_blargg_pass("2-branch_timing.nes", 60 * 60);
}

// ── PPU: VBlank + NMI behavior ──────────────────────────────────────────────

#[test]
fn blargg_ppu_vbl_nmi() {
    assert_blargg_pass("ppu_vbl_nmi.nes", 60 * 180);
}

// ── PPU: sprite 0 hit ───────────────────────────────────────────────────────
// sprite_hit_tests_2005.10.05 predates Blargg's $6000 reporting protocol —
// results are printed only to the PPU output, so they can't be verified
// programmatically without OCR or a reference pixel comparison. The ROMs
// are fetched anyway for manual inspection via the TUI renderer.
// (Newer sprite-0 timing coverage is part of ppu_vbl_nmi.)

// ── PPU: OAM stress ─────────────────────────────────────────────────────────

#[test]
fn blargg_oam_stress() {
    assert_blargg_pass("oam_stress.nes", 60 * 120);
}

// ── APU ─────────────────────────────────────────────────────────────────────

#[test]
fn blargg_apu_test() {
    assert_blargg_pass("apu_test.nes", 60 * 120);
}
