pub mod mapper;

use std::fs;

use crate::ppu::Mirroring;
use mapper::{Mapper, Mmc1Mapper, NromMapper, UnromMapper};

pub struct Cartridge {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper_id: u8,
    pub mirroring: Mirroring,
    pub has_battery: bool,
    mapper: Box<dyn Mapper>,
}

impl Cartridge {
    /// Parses an iNES ROM from raw bytes and returns a Cartridge.
    pub fn from_ines(data: &[u8]) -> Result<Self, String> {
        if data.len() < 16 {
            return Err("ROM too small for iNES header".into());
        }
        if &data[0..4] != b"NES\x1A" {
            return Err("Invalid iNES magic number".into());
        }

        let prg_rom_banks = data[4];
        let chr_rom_banks = data[5];
        let flags6 = data[6];
        let flags7 = data[7];

        let mapper_id = (flags6 >> 4) | (flags7 & 0xF0);
        let has_battery = flags6 & 0x02 != 0;
        let has_trainer = flags6 & 0x04 != 0;

        let mirroring = if flags6 & 0x08 != 0 {
            // Four-screen VRAM — not mirrored at all; treat as vertical for now
            Mirroring::Vertical
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        // 16-byte header + optional 512-byte trainer
        let prg_start = 16 + if has_trainer { 512 } else { 0 };
        let prg_size = prg_rom_banks as usize * 0x4000;
        let prg_end = prg_start + prg_size;

        if data.len() < prg_end {
            return Err("ROM file truncated (PRG-ROM)".into());
        }

        let prg_rom = data[prg_start..prg_end].to_vec();

        let chr_size = chr_rom_banks as usize * 0x2000;
        let chr_end = prg_end + chr_size;

        if data.len() < chr_end {
            return Err("ROM file truncated (CHR-ROM)".into());
        }

        let chr_rom = data[prg_end..chr_end].to_vec();

        let mapper: Box<dyn Mapper> = match mapper_id {
            0 => Box::new(NromMapper::new(prg_rom.clone(), chr_rom.clone(), mirroring)),
            1 => Box::new(Mmc1Mapper::new(prg_rom.clone(), chr_rom.clone())),
            2 => Box::new(UnromMapper::new(prg_rom.clone(), mirroring)),
            _ => return Err(format!("Unsupported mapper: {}", mapper_id)),
        };

        Ok(Cartridge {
            prg_rom,
            chr_rom,
            mapper_id,
            mirroring,
            has_battery,
            mapper,
        })
    }

    /// Loads an iNES ROM from disk.
    pub fn from_file(path: &str) -> Result<Self, String> {
        let data = fs::read(path).map_err(|e| format!("Failed to read ROM: {e}"))?;
        Self::from_ines(&data)
    }

    pub fn cpu_read(&self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.mapper.cpu_write(addr, val);
    }

    /// Reads from the CHR address space (pattern tables). Falls back to 0.
    pub fn chr_read(&self, addr: u16) -> u8 {
        self.mapper.chr_read(addr).unwrap_or(0)
    }

    /// Writes to the CHR address space (only meaningful for CHR-RAM).
    pub fn chr_write(&mut self, addr: u16, val: u8) {
        self.mapper.chr_write(addr, val);
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid iNES image in memory.
    fn make_ines(
        mapper_id: u8,
        prg_banks: u8,
        chr_banks: u8,
        flags6_extra: u8,
        prg_fill: u8,
        chr_fill: u8,
    ) -> Vec<u8> {
        let mut rom = Vec::new();
        // Header
        rom.extend_from_slice(b"NES\x1A");
        rom.push(prg_banks);
        rom.push(chr_banks);
        rom.push((mapper_id << 4) | flags6_extra); // flags6: lower nibble of mapper in bits 4-7
        rom.push(mapper_id & 0xF0);                // flags7: upper nibble of mapper in bits 4-7
        rom.extend_from_slice(&[0u8; 8]);           // unused header bytes

        // PRG-ROM
        rom.extend(vec![prg_fill; prg_banks as usize * 0x4000]);
        // CHR-ROM
        rom.extend(vec![chr_fill; chr_banks as usize * 0x2000]);
        rom
    }

    // ── Header parsing ───────────────────────────────────────────────────────

    #[test]
    fn valid_nrom_header_parses() {
        let rom = make_ines(0, 2, 1, 0, 0xAA, 0xBB);
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.mapper_id, 0);
        assert_eq!(cart.prg_rom.len(), 2 * 0x4000);
        assert_eq!(cart.chr_rom.len(), 0x2000);
        assert_eq!(cart.mirroring, Mirroring::Horizontal);
        assert!(!cart.has_battery);
    }

    #[test]
    fn vertical_mirroring_flag() {
        let rom = make_ines(0, 1, 0, 0x01, 0, 0); // bit 0 = vertical
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.mirroring, Mirroring::Vertical);
    }

    #[test]
    fn battery_flag_detected() {
        let rom = make_ines(0, 1, 0, 0x02, 0, 0); // bit 1 = battery
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert!(cart.has_battery);
    }

    #[test]
    fn trainer_is_skipped() {
        // Build a ROM with a 512-byte trainer; put a sentinel at start of PRG-ROM.
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1);   // 1 PRG bank
        rom.push(0);   // 0 CHR banks
        rom.push(0x04); // flags6: bit 2 = trainer present
        rom.push(0x00);
        rom.extend_from_slice(&[0u8; 8]);
        rom.extend(vec![0xFFu8; 512]); // trainer (should be ignored)
        rom.extend(vec![0xABu8; 0x4000]); // actual PRG-ROM
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.prg_rom[0], 0xAB);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut rom = make_ines(0, 1, 0, 0, 0, 0);
        rom[0] = 0x00; // corrupt magic
        assert!(Cartridge::from_ines(&rom).is_err());
    }

    #[test]
    fn rejects_too_short() {
        assert!(Cartridge::from_ines(&[0u8; 4]).is_err());
    }

    #[test]
    fn rejects_truncated_prg() {
        let mut rom = make_ines(0, 2, 0, 0, 0, 0);
        rom.truncate(16 + 0x4000); // cut off second bank
        assert!(Cartridge::from_ines(&rom).is_err());
    }

    #[test]
    fn rejects_unsupported_mapper() {
        let rom = make_ines(4, 1, 0, 0, 0, 0); // mapper 4 = MMC3, not supported
        assert!(Cartridge::from_ines(&rom).is_err());
    }

    // ── NROM via Cartridge ───────────────────────────────────────────────────

    #[test]
    fn nrom_cpu_read() {
        let rom = make_ines(0, 1, 0, 0, 0x55, 0);
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.cpu_read(0x8000), Some(0x55));
        // 16KB mirrors to upper half
        assert_eq!(cart.cpu_read(0xC000), Some(0x55));
    }

    #[test]
    fn nrom_chr_read() {
        let rom = make_ines(0, 1, 1, 0, 0, 0xCC);
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.chr_read(0x0000), 0xCC);
        assert_eq!(cart.chr_read(0x1FFF), 0xCC);
    }

    // ── UNROM via Cartridge ──────────────────────────────────────────────────

    #[test]
    fn unrom_bank_switching() {
        // 4 banks, each byte == bank index
        let mut prg = vec![0u8; 4 * 0x4000];
        for bank in 0..4usize {
            for b in &mut prg[bank * 0x4000..(bank + 1) * 0x4000] {
                *b = bank as u8;
            }
        }
        let mut header = Vec::new();
        header.extend_from_slice(b"NES\x1A");
        header.push(4); // 4 PRG banks
        header.push(0); // CHR-RAM
        header.push(0x20); // flags6: mapper id lower nibble = 2
        header.push(0x00);
        header.extend_from_slice(&[0u8; 8]);
        let mut rom = header;
        rom.extend_from_slice(&prg);

        let mut cart = Cartridge::from_ines(&rom).unwrap();

        // Default: bank 0 at $8000
        assert_eq!(cart.cpu_read(0x8000), Some(0));

        // Select bank 2
        cart.cpu_write(0x8000, 2);
        assert_eq!(cart.cpu_read(0x8000), Some(2));
    }

    #[test]
    fn unrom_last_bank_hardwired() {
        let mut prg = vec![0u8; 4 * 0x4000];
        for b in &mut prg[3 * 0x4000..] {
            *b = 0xFF;
        }
        let mut header = Vec::new();
        header.extend_from_slice(b"NES\x1A");
        header.push(4);
        header.push(0);
        header.push(0x20);
        header.push(0x00);
        header.extend_from_slice(&[0u8; 8]);
        let mut rom = header;
        rom.extend_from_slice(&prg);

        let mut cart = Cartridge::from_ines(&rom).unwrap();
        cart.cpu_write(0x8000, 0); // select bank 0 at $8000
        // $C000 must still be last bank
        assert_eq!(cart.cpu_read(0xC000), Some(0xFF));
    }

    #[test]
    fn unrom_chr_ram_roundtrip() {
        let rom = make_ines(2, 2, 0, 0, 0, 0);
        let mut cart = Cartridge::from_ines(&rom).unwrap();
        cart.chr_write(0x0010, 0xAB);
        assert_eq!(cart.chr_read(0x0010), 0xAB);
        cart.chr_write(0x1FFF, 0x77);
        assert_eq!(cart.chr_read(0x1FFF), 0x77);
    }
}
