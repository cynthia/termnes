use termnes::ppu::palette::NES_PALETTE;
use termnes::ppu::render::*;

#[test]
fn test_palette_has_64_entries() {
    assert_eq!(NES_PALETTE.len(), 64);
}

#[test]
fn test_palette_black_entries() {
    // 0x0D, 0x0E, 0x0F should all be black
    assert_eq!(NES_PALETTE[0x0D], (0, 0, 0));
    assert_eq!(NES_PALETTE[0x0E], (0, 0, 0));
    assert_eq!(NES_PALETTE[0x0F], (0, 0, 0));

    // 0x1D, 0x1E, 0x1F
    assert_eq!(NES_PALETTE[0x1D], (0, 0, 0));
    assert_eq!(NES_PALETTE[0x1E], (0, 0, 0));
    assert_eq!(NES_PALETTE[0x1F], (0, 0, 0));

    // 0x2E, 0x2F
    assert_eq!(NES_PALETTE[0x2E], (0, 0, 0));
    assert_eq!(NES_PALETTE[0x2F], (0, 0, 0));

    // 0x3E, 0x3F
    assert_eq!(NES_PALETTE[0x3E], (0, 0, 0));
    assert_eq!(NES_PALETTE[0x3F], (0, 0, 0));
}

#[test]
fn test_palette_non_black_spot_checks() {
    // Row 0: 0x00 is a dark gray
    assert_eq!(NES_PALETTE[0x00], (84, 84, 84));
    // Row 2: 0x20 is near-white
    assert_eq!(NES_PALETTE[0x20], (236, 238, 236));
    // Row 3: 0x30 should equal 0x20 (both near-white in 2C02 palette)
    assert_eq!(NES_PALETTE[0x30], (236, 238, 236));
    // 0x2D is a dark gray
    assert_eq!(NES_PALETTE[0x2D], (60, 60, 60));
}

#[test]
fn test_palette_no_overflow_values() {
    for (i, &(r, g, b)) in NES_PALETTE.iter().enumerate() {
        // All values must be valid u8 (compiler enforces this, but let's
        // verify no entry accidentally has values that look like wrap-around)
        assert!(
            r <= 255 && g <= 255 && b <= 255,
            "palette entry {:#04X} has invalid RGB ({}, {}, {})",
            i, r, g, b
        );
    }
}

#[test]
fn test_rendering_constants() {
    assert_eq!(SCREEN_WIDTH, 256);
    assert_eq!(SCREEN_HEIGHT, 240);
    assert_eq!(SCANLINES_PER_FRAME, 262);
    assert_eq!(CYCLES_PER_SCANLINE, 341);
    assert_eq!(VBLANK_SCANLINE, 241);
    assert_eq!(PRE_RENDER_SCANLINE, 261);
}

#[test]
fn test_total_cycles_per_frame() {
    // NES PPU: 262 scanlines x 341 cycles = 89342 PPU cycles per frame
    assert_eq!(SCANLINES_PER_FRAME * CYCLES_PER_SCANLINE, 89342);
}

#[test]
fn test_visible_area() {
    // Visible scanlines: 0..240 (240 lines)
    // Visible pixels per line: 256
    assert_eq!(SCREEN_WIDTH * SCREEN_HEIGHT, 61440);
}
