use crate::savestate::MapperState;
use super::Mapper;

/// MMC5 (Mapper 5) — Advanced mapper with EXRAM, large ROM support, and extra audio.
/// This is a basic implementation supporting PRG/CHR banking to allow games to boot.
pub struct Mmc5Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: [u8; 64 * 1024], // Up to 64KB PRG RAM
    exram: [u8; 1024],        // 1KB Expansion RAM

    prg_mode: u8,
    chr_mode: u8,
    exram_mode: u8,

    prg_banks: [usize; 4],
    chr_banks_a: [usize; 8],
    chr_banks_b: [usize; 4],
    chr_high: usize,

    nametable_mapping: u8,
    fill_tile: u8,
    fill_color: u8,

    ram_protect_1: u8,
    ram_protect_2: u8,

    multiplier_1: u8,
    multiplier_2: u8,
}

impl Mmc5Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0; 64 * 1024],
            exram: [0; 1024],
            prg_mode: 3,
            chr_mode: 0,
            exram_mode: 0,
            prg_banks: [0, 0, 0, 0xFF], // Last bank defaults to last PRG bank
            chr_banks_a: [0; 8],
            chr_banks_b: [0; 4],
            chr_high: 0,
            nametable_mapping: 0,
            fill_tile: 0,
            fill_color: 0,
            ram_protect_1: 0,
            ram_protect_2: 0,
            multiplier_1: 0,
            multiplier_2: 0,
        }
    }

    fn prg_read_8k(&self, bank: usize, offset: usize) -> u8 {
        // High bit 7 indicates ROM vs RAM
        if bank & 0x80 != 0 {
            let rom_bank = bank & 0x7F;
            let num_banks = self.prg_rom.len() / 0x2000;
            if num_banks == 0 { return 0; }
            self.prg_rom[((rom_bank % num_banks) * 0x2000) + offset]
        } else {
            let ram_bank = bank & 0x07;
            self.prg_ram[(ram_bank * 0x2000) + offset]
        }
    }
}

impl Mapper for Mmc5Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x5204 => {
                // IRQ status stub
                Some(0)
            }
            0x5205 => Some((self.multiplier_1 as u16 * self.multiplier_2 as u16) as u8),
            0x5206 => Some(((self.multiplier_1 as u16 * self.multiplier_2 as u16) >> 8) as u8),
            0x5C00..=0x5FFF => {
                if self.exram_mode == 2 || self.exram_mode == 3 {
                    Some(self.exram[addr as usize - 0x5C00])
                } else {
                    Some(0) // Read 0 if not accessible by CPU
                }
            }
            0x6000..=0x7FFF => {
                Some(self.prg_read_8k(self.prg_banks[0], addr as usize - 0x6000))
            }
            0x8000..=0x9FFF => {
                let bank = match self.prg_mode {
                    0 | 1 => (self.prg_banks[3] & 0x7C) | 0x80,
                    2 => (self.prg_banks[1] & 0x7E) | 0x80,
                    3 => self.prg_banks[1] | 0x80,
                    _ => unreachable!(),
                };
                Some(self.prg_read_8k(bank, addr as usize - 0x8000))
            }
            0xA000..=0xBFFF => {
                let bank = match self.prg_mode {
                    0 | 1 => (self.prg_banks[3] & 0x7C) | 0x81,
                    2 => (self.prg_banks[1] & 0x7E) | 0x81,
                    3 => self.prg_banks[2] | 0x80,
                    _ => unreachable!(),
                };
                Some(self.prg_read_8k(bank, addr as usize - 0xA000))
            }
            0xC000..=0xDFFF => {
                let bank = match self.prg_mode {
                    0 | 1 => (self.prg_banks[3] & 0x7C) | 0x82,
                    2 | 3 => (self.prg_banks[3] & 0x7E) | 0x80,
                    _ => unreachable!(),
                };
                Some(self.prg_read_8k(bank, addr as usize - 0xC000))
            }
            0xE000..=0xFFFF => {
                let bank = match self.prg_mode {
                    0 | 1 => (self.prg_banks[3] & 0x7C) | 0x83,
                    2 | 3 => (self.prg_banks[3] & 0x7E) | 0x81,
                    _ => unreachable!(),
                };
                Some(self.prg_read_8k(bank | 0x80, addr as usize - 0xE000))
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x5100 => self.prg_mode = val & 0x03,
            0x5101 => self.chr_mode = val & 0x03,
            0x5102 => self.ram_protect_1 = val & 0x03,
            0x5103 => self.ram_protect_2 = val & 0x03,
            0x5104 => self.exram_mode = val & 0x03,
            0x5105 => self.nametable_mapping = val,
            0x5106 => self.fill_tile = val,
            0x5107 => self.fill_color = val & 0x03,
            0x5113 => self.prg_banks[0] = val as usize, // RAM Bank
            0x5114 => self.prg_banks[1] = val as usize,
            0x5115 => self.prg_banks[2] = val as usize,
            0x5116 => self.prg_banks[3] = val as usize,
            0x5117 => self.prg_banks[3] = (val | 0x80) as usize, // Always ROM
            0x5120..=0x5127 => self.chr_banks_a[addr as usize - 0x5120] = val as usize | (self.chr_high << 8),
            0x5128..=0x512B => self.chr_banks_b[addr as usize - 0x5128] = val as usize | (self.chr_high << 8),
            0x5130 => self.chr_high = (val & 0x03) as usize,
            0x5205 => self.multiplier_1 = val,
            0x5206 => self.multiplier_2 = val,
            0x5C00..=0x5FFF => {
                if self.exram_mode == 0 || self.exram_mode == 1 {
                    // EXRAM writeable during rendering if rendering is enabled, etc.
                    self.exram[addr as usize - 0x5C00] = val;
                } else if self.exram_mode == 2 {
                    self.exram[addr as usize - 0x5C00] = val;
                }
            }
            0x6000..=0x7FFF => {
                if self.ram_protect_1 == 2 && self.ram_protect_2 == 1 {
                    let bank = self.prg_banks[0] & 0x07;
                    self.prg_ram[bank * 0x2000 + (addr as usize - 0x6000)] = val;
                }
            }
            0x8000..=0x9FFF => {
                // If mapped to RAM, write
                if self.prg_mode == 3 && self.prg_banks[1] & 0x80 == 0 {
                    let bank = self.prg_banks[1] & 0x07;
                    self.prg_ram[bank * 0x2000 + (addr as usize - 0x8000)] = val;
                }
            }
            0xA000..=0xBFFF => {
                if self.prg_mode == 3 && self.prg_banks[2] & 0x80 == 0 {
                    let bank = self.prg_banks[2] & 0x07;
                    self.prg_ram[bank * 0x2000 + (addr as usize - 0xA000)] = val;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 { return None; }
        
        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 { return Some(0); }

        // Simplify for testing: use banks_a
        let bank = match self.chr_mode {
            0 => (self.chr_banks_a[7] & 0xFF8) + (addr as usize / 0x0400),
            1 => {
                let b = if addr < 0x1000 { self.chr_banks_a[3] & 0xFFC } else { self.chr_banks_a[7] & 0xFFC };
                b + (addr as usize % 0x1000) / 0x0400
            }
            2 => {
                let idx = (addr as usize / 0x0800) * 2 + 1;
                (self.chr_banks_a[idx] & 0xFFE) + (addr as usize % 0x0800) / 0x0400
            }
            3 => self.chr_banks_a[addr as usize / 0x0400],
            _ => unreachable!(),
        };

        let offset = (addr as usize % 0x0400) + (bank % num_banks) * 0x0400;
        Some(self.chr_rom[offset])
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {
        // CHR RAM not currently stubbed
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Mmc5 {
            prg_mode: self.prg_mode,
            chr_mode: self.chr_mode,
            prg_banks: self.prg_banks.to_vec(),
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Mmc5 { prg_mode, chr_mode, prg_banks } = state {
            self.prg_mode = *prg_mode;
            self.chr_mode = *chr_mode;
            if prg_banks.len() == 4 {
                self.prg_banks.copy_from_slice(prg_banks);
            }
        }
    }
}
