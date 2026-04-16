use termnes::cartridge::Cartridge;

/// Builds a minimal iNES ROM with the given PRG bank count and flags.
fn make_ines_rom(prg_banks: u8, chr_banks: u8, flags6: u8, flags7: u8) -> Vec<u8> {
    let mut rom = Vec::new();

    // iNES header (16 bytes)
    rom.extend_from_slice(b"NES\x1A");
    rom.push(prg_banks);
    rom.push(chr_banks);
    rom.push(flags6);
    rom.push(flags7);
    rom.extend_from_slice(&[0u8; 8]);

    // PRG-ROM data
    let prg_size = prg_banks as usize * 0x4000;
    rom.resize(16 + prg_size, 0xEA); // fill with NOP

    // CHR-ROM data
    let chr_size = chr_banks as usize * 0x2000;
    rom.resize(16 + prg_size + chr_size, 0x00);

    rom
}

#[test]
fn test_ines_header_valid_mapper2() {
    // Mapper 2, vertical mirroring, 2 PRG banks, 0 CHR (CHR-RAM)
    let rom = make_ines_rom(2, 0, 0x21, 0x00); // flags6=0x21: mapper_lo=2, vertical mirror
    let cart = Cartridge::from_ines(&rom);
    assert!(cart.is_ok(), "should parse valid mapper 2 ROM");
}

#[test]
fn test_ines_header_vertical_mirroring() {
    // flags6 bit 0 = 1 => vertical mirroring
    let rom = make_ines_rom(2, 0, 0x21, 0x00);
    let cart = Cartridge::from_ines(&rom).unwrap();
    assert_eq!(cart.mirroring, termnes::ppu::Mirroring::Vertical);
}

#[test]
fn test_ines_header_horizontal_mirroring() {
    // flags6 bit 0 = 0 => horizontal mirroring
    let rom = make_ines_rom(2, 0, 0x20, 0x00);
    let cart = Cartridge::from_ines(&rom).unwrap();
    assert_eq!(cart.mirroring, termnes::ppu::Mirroring::Horizontal);
}

#[test]
fn test_ines_header_bad_magic() {
    let mut rom = make_ines_rom(2, 0, 0x20, 0x00);
    rom[0] = b'X'; // corrupt magic
    let result = Cartridge::from_ines(&rom);
    assert!(result.is_err(), "should reject invalid magic");
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(err.contains("magic"), "error should mention magic");
}

#[test]
fn test_ines_header_too_small() {
    let rom = vec![0u8; 10]; // less than 16 bytes
    let result = Cartridge::from_ines(&rom);
    assert!(result.is_err(), "should reject truncated header");
}

#[test]
fn test_ines_header_truncated_prg() {
    // Header says 2 banks but file only has data for 1
    let mut rom = make_ines_rom(1, 0, 0x20, 0x00);
    rom[4] = 2; // lie: say 2 PRG banks
    let result = Cartridge::from_ines(&rom);
    assert!(result.is_err(), "should reject truncated PRG data");
}

#[test]
fn test_ines_unsupported_mapper() {
    // Mapper 4 (MMC3) — not supported
    let rom = make_ines_rom(2, 0, 0x40, 0x00); // flags6 high nibble = 4 => mapper 4
    let result = Cartridge::from_ines(&rom);
    assert!(result.is_err(), "should reject unsupported mapper");
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected error"),
    };
    assert!(err.contains("mapper"), "error should mention mapper");
}

#[test]
fn test_ines_mapper_id_extraction() {
    // Mapper 2: flags6 high nibble = 0x2, flags7 high nibble = 0x0
    // mapper_id = (flags6 >> 4) | (flags7 & 0xF0) = 2 | 0 = 2
    let rom = make_ines_rom(2, 0, 0x20, 0x00);
    let result = Cartridge::from_ines(&rom);
    assert!(result.is_ok(), "mapper 2 should be supported");
}
