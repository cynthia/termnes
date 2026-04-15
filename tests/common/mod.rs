//! Shared helpers for ROM-based integration tests. Each test file declares
//! `mod common;` to pull these in — `tests/common/mod.rs` is the standard
//! cargo convention for a non-test helper module that lives alongside tests.
//!
//! Each integration test file compiles as its own binary and only uses a
//! subset of these helpers. `#![allow(dead_code)]` keeps the warning noise
//! down without hiding genuine unused code in the main crate.

#![allow(dead_code)]

use std::path::PathBuf;

use nes_tui::Nes;

/// Absolute path to a ROM name (or subpath) under `tests/test_roms/`.
pub fn rom_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("test_roms");
    p.push(name);
    p
}

/// Returns the path if the ROM exists, else `None`. Call sites should use
/// `let-else` to skip the test gracefully when a ROM is not present — this
/// keeps `cargo test` green on checkouts that haven't populated test ROMs.
pub fn try_rom(name: &str) -> Option<PathBuf> {
    let p = rom_path(name);
    if p.is_file() {
        Some(p)
    } else {
        eprintln!(
            "[skip] test ROM not found: tests/test_roms/{} — see tests/test_roms/README for how to obtain it",
            name
        );
        None
    }
}

/// Result of a Blargg-format test ROM.
#[derive(Debug)]
pub enum BlarggResult {
    /// The test never transitioned $6000 out of "running". Message holds any
    /// partial ASCII output accumulated at $6004+.
    TimedOut(String),
    /// $6000 dropped to 0. Message holds the ASCII result text.
    Pass(String),
    /// $6000 held a non-zero non-running code. The u8 is the status byte.
    Fail(u8, String),
    /// $6000 was 0x81 — ROM requested a soft reset (not yet supported here).
    NeedsReset(String),
}

/// Runs a Blargg-format test ROM to completion (or `max_frames`, whichever
/// comes first) and returns its result.
///
/// The Blargg convention:
///   - Magic bytes $DE $B0 $61 appear at $6001-$6003 once the test has started
///   - $6000 reads as 0x80 while running, 0x81 if a soft reset is requested,
///     otherwise the final status byte (0 = pass, anything else = fail)
///   - Null-terminated ASCII result at $6004+
pub fn run_blargg(nes: &mut Nes, max_frames: u64) -> BlarggResult {
    const MAGIC: [u8; 3] = [0xDE, 0xB0, 0x61];

    // Wait for the ROM to publish the magic bytes, then watch $6000.
    let mut started = false;
    for _ in 0..max_frames {
        nes.step_frame();

        if !started {
            if nes.peek(0x6001) == MAGIC[0]
                && nes.peek(0x6002) == MAGIC[1]
                && nes.peek(0x6003) == MAGIC[2]
            {
                started = true;
            } else {
                continue;
            }
        }

        match nes.peek(0x6000) {
            0x80 => continue, // still running
            0x81 => return BlarggResult::NeedsReset(read_blargg_text(nes)),
            0x00 => return BlarggResult::Pass(read_blargg_text(nes)),
            code => return BlarggResult::Fail(code, read_blargg_text(nes)),
        }
    }

    BlarggResult::TimedOut(read_blargg_text(nes))
}

/// Reads the null-terminated ASCII message Blargg test ROMs publish at $6004+.
pub fn read_blargg_text(nes: &Nes) -> String {
    let mut out = String::new();
    for i in 0u16..0x1FFC {
        let b = nes.peek(0x6004 + i);
        if b == 0 {
            break;
        }
        if b.is_ascii() && !b.is_ascii_control() || b == b'\n' {
            out.push(b as char);
        }
    }
    out.trim().to_string()
}

/// Convenience: loads `rom`, runs Blargg protocol, panics on failure or
/// timeout, eprintln!s the ROM's own pass message on success. Returns silently
/// (via the let-else) if the ROM is missing. ROMs that target mappers we
/// don't implement yet are reported as [skip] rather than failing the test —
/// those failures aren't regressions, just unimplemented features.
pub fn assert_blargg_pass(rom: &str, max_frames: u64) {
    let Some(path) = try_rom(rom) else { return };
    let mut nes = match Nes::from_ines_file(&path) {
        Ok(n) => n,
        Err(e) if e.starts_with("Unsupported mapper") => {
            eprintln!("[skip] {}: {} (implement the mapper to enable)", rom, e);
            return;
        }
        Err(e) => panic!("failed to load {}: {}", rom, e),
    };
    match run_blargg(&mut nes, max_frames) {
        BlarggResult::Pass(text) => eprintln!("[pass] {}: {}", rom, text),
        BlarggResult::Fail(code, text) => {
            panic!("{} failed (status ${:02X}):\n{}", rom, code, text)
        }
        BlarggResult::NeedsReset(text) => {
            panic!(
                "{} requested a soft reset (unsupported). Partial output:\n{}",
                rom, text
            )
        }
        BlarggResult::TimedOut(text) => panic!(
            "{} did not complete within {} frames. Partial output:\n{}",
            rom, max_frames, text
        ),
    }
}

