use std::cell::Cell;
use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// MMC2 (Mapper 9) — Punch-Out!! mapper.
/// 8KB switchable PRG bank at $8000, 3 fixed 8KB banks at $A000-$FFFF.
/// 4KB switchable CHR banks with variant latch switching.
pub struct Mmc2Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,

    prg_bank: u8,
    chr_bank_0_l: u8,
    chr_bank_0_r: u8,
    chr_bank_1_l: u8,
    chr_bank_1_r: u8,

    latch_0: Cell<bool>, // false = L, true = R
    latch_1: Cell<bool>,

    mirroring: Mirroring,
}

impl Mmc2Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank_0_l: 0,
            chr_bank_0_r: 0,
            chr_bank_1_l: 0,
            chr_bank_1_r: 0,
            latch_0: Cell::new(true),
            latch_1: Cell::new(true),
            mirroring: Mirroring::Horizontal,
        }
    }
}

impl Mapper for Mmc2Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.prg_bank as usize;
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let offset = (addr as usize - 0x8000) + (bank % num_banks) * 0x2000;
                Some(self.prg_rom[offset])
            }
            0xA000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let bank = match addr {
                    0xA000..=0xBFFF => num_banks.saturating_sub(3),
                    0xC000..=0xDFFF => num_banks.saturating_sub(2),
                    0xE000..=0xFFFF => num_banks.saturating_sub(1),
                    _ => unreachable!(),
                };
                let offset = (addr as usize % 0x2000) + bank * 0x2000;
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0xA000..=0xAFFF => self.prg_bank = val & 0x0F,
            0xB000..=0xBFFF => self.chr_bank_0_l = val & 0x1F,
            0xC000..=0xCFFF => self.chr_bank_0_r = val & 0x1F,
            0xD000..=0xDFFF => self.chr_bank_1_l = val & 0x1F,
            0xE000..=0xEFFF => self.chr_bank_1_r = val & 0x1F,
            0xF000..=0xFFFF => {
                self.mirroring = if val & 0x01 == 0 { Mirroring::Vertical } else { Mirroring::Horizontal };
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 { return None; }

        let num_banks = self.chr_rom.len() / 0x1000;
        if num_banks == 0 { return Some(0); }

        let bank = if addr < 0x1000 {
            if self.latch_0.get() { self.chr_bank_0_r } else { self.chr_bank_0_l }
        } else {
            if self.latch_1.get() { self.chr_bank_1_r } else { self.chr_bank_1_l }
        };

        let offset = (addr as usize % 0x1000) + (bank as usize % num_banks) * 0x1000;
        let val = self.chr_rom[offset % self.chr_rom.len()];

        // Latch switching is side-effect of reading certain tiles.
        // Bank 0: 0x0FD8 -> L, 0x0FE8 -> R
        // Bank 1: 0x1FD8-0x1FDF -> L, 0x1FE8-0x1FEF -> R
        match addr {
            0x0FD8 => self.latch_0.set(false),
            0x0FE8 => self.latch_0.set(true),
            0x1FD8..=0x1FDF => self.latch_1.set(false),
            0x1FE8..=0x1FEF => self.latch_1.set(true),
            _ => {}
        }

        Some(val)
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {}

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Mmc2 {
            prg_bank: self.prg_bank,
            chr_bank_0_l: self.chr_bank_0_l,
            chr_bank_0_r: self.chr_bank_0_r,
            chr_bank_1_l: self.chr_bank_1_l,
            chr_bank_1_r: self.chr_bank_1_r,
            latch_0: self.latch_0.get(),
            latch_1: self.latch_1.get(),
            mirroring: match self.mirroring {
                Mirroring::Horizontal => 0,
                Mirroring::Vertical => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Mmc2 {
            prg_bank, chr_bank_0_l, chr_bank_0_r,
            chr_bank_1_l, chr_bank_1_r, latch_0, latch_1, mirroring,
        } = state
        {
            self.prg_bank = *prg_bank;
            self.chr_bank_0_l = *chr_bank_0_l;
            self.chr_bank_0_r = *chr_bank_0_r;
            self.chr_bank_1_l = *chr_bank_1_l;
            self.chr_bank_1_r = *chr_bank_1_r;
            self.latch_0.set(*latch_0);
            self.latch_1.set(*latch_1);
            self.mirroring = match mirroring {
                0 => Mirroring::Horizontal,
                1 => Mirroring::Vertical,
                2 => Mirroring::OneScreenLow,
                3 => Mirroring::OneScreenHigh,
                _ => Mirroring::Horizontal,
            };
        }
    }
}
