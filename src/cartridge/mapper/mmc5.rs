use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// MMC5 (Mapper 5) — Advanced mapper with EXRAM, large ROM support, and extra audio.
/// This is a basic implementation supporting PRG/CHR banking to allow games to boot.
pub struct Mmc5Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    chr_is_ram: bool,
    prg_ram: [u8; 64 * 1024], // Up to 64KB PRG RAM
    exram: [u8; 1024],        // 1KB Expansion RAM

    prg_mode: u8,
    chr_mode: u8,
    exram_mode: u8,

    prg_ram_bank: usize,
    prg_bank_8000: usize,
    prg_bank_a000: usize,
    prg_bank_c000: usize,
    prg_bank_e000: usize,
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

    irq_target: u8,
    irq_enable: bool,
    irq_pending: std::cell::Cell<bool>,
    in_frame: std::cell::Cell<bool>,
    scanline_counter: u8,
    watchdog: std::cell::Cell<u8>,

    ppu_ctrl: u8,
    ppu_mask: u8,
}

impl Mmc5Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let last_bank = prg_rom.len().saturating_div(0x2000).saturating_sub(1);
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 8192],
            chr_is_ram,
            prg_ram: [0; 64 * 1024],
            exram: [0; 1024],
            prg_mode: 3,
            chr_mode: 0,
            exram_mode: 0,
            prg_ram_bank: 0,
            prg_bank_8000: last_bank | 0x80,
            prg_bank_a000: last_bank | 0x80,
            prg_bank_c000: last_bank | 0x80,
            prg_bank_e000: last_bank | 0x80,
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
            irq_target: 0,
            irq_enable: false,
            irq_pending: std::cell::Cell::new(false),
            in_frame: std::cell::Cell::new(false),
            scanline_counter: 0,
            watchdog: std::cell::Cell::new(0),
            ppu_ctrl: 0,
            ppu_mask: 0,
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

    fn get_prg_bank(&self, addr: u16) -> usize {
        match self.prg_mode {
            0 => {
                // 32KB mode
                (self.prg_bank_e000 & 0x7C) | (self.prg_bank_e000 & 0x80) | ((addr as usize - 0x8000) / 0x2000)
            }
            1 => {
                // 16KB mode
                if addr < 0xC000 {
                    (self.prg_bank_a000 & 0x7E) | (self.prg_bank_a000 & 0x80) | ((addr as usize - 0x8000) / 0x2000)
                } else {
                    (self.prg_bank_e000 & 0x7E) | (self.prg_bank_e000 & 0x80) | ((addr as usize - 0xC000) / 0x2000)
                }
            }
            2 => {
                // 16KB-8KB mode
                if addr < 0xC000 {
                    (self.prg_bank_a000 & 0x7E) | (self.prg_bank_a000 & 0x80) | ((addr as usize - 0x8000) / 0x2000)
                } else if addr < 0xE000 {
                    self.prg_bank_c000
                } else {
                    self.prg_bank_e000 | 0x80
                }
            }
            3 => {
                // 8KB mode
                match addr {
                    0x8000..=0x9FFF => self.prg_bank_8000,
                    0xA000..=0xBFFF => self.prg_bank_a000,
                    0xC000..=0xDFFF => self.prg_bank_c000,
                    0xE000..=0xFFFF => self.prg_bank_e000 | 0x80,
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Mapper for Mmc5Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x5204 => {
                let mut status = 0;
                if self.irq_pending.get() { status |= 0x80; }
                if self.in_frame.get() { status |= 0x40; }
                self.irq_pending.set(false);
                Some(status)
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
                Some(self.prg_read_8k(self.prg_ram_bank, addr as usize - 0x6000))
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                Some(self.prg_read_8k(bank, addr as usize % 0x2000))
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x2000 => self.ppu_ctrl = val,
            0x2001 => self.ppu_mask = val,
            0x5100 => self.prg_mode = val & 0x03,
            0x5101 => self.chr_mode = val & 0x03,
            0x5102 => self.ram_protect_1 = val & 0x03,
            0x5103 => self.ram_protect_2 = val & 0x03,
            0x5104 => self.exram_mode = val & 0x03,
            0x5105 => self.nametable_mapping = val,
            0x5106 => self.fill_tile = val,
            0x5107 => self.fill_color = val & 0x03,
            0x5113 => self.prg_ram_bank = val as usize,
            0x5114 => self.prg_bank_8000 = val as usize,
            0x5115 => self.prg_bank_a000 = val as usize,
            0x5116 => self.prg_bank_c000 = val as usize,
            0x5117 => self.prg_bank_e000 = (val | 0x80) as usize,
            0x5120..=0x5127 => self.chr_banks_a[addr as usize - 0x5120] = val as usize | (self.chr_high << 8),
            0x5128..=0x512B => self.chr_banks_b[addr as usize - 0x5128] = val as usize | (self.chr_high << 8),
            0x5130 => self.chr_high = (val & 0x03) as usize,
            0x5203 => self.irq_target = val,
            0x5204 => self.irq_enable = (val & 0x80) != 0,
            0x5205 => self.multiplier_1 = val,
            0x5206 => self.multiplier_2 = val,
            0x5C00..=0x5FFF => {
                if self.exram_mode == 0 || self.exram_mode == 1 || self.exram_mode == 2 {
                    self.exram[addr as usize - 0x5C00] = val;
                }
            }
            0x6000..=0x7FFF => {
                if self.ram_protect_1 == 2 && self.ram_protect_2 == 1 {
                    let bank = self.prg_ram_bank & 0x07;
                    self.prg_ram[bank * 0x2000 + (addr as usize - 0x6000)] = val;
                }
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                if bank & 0x80 == 0 && self.ram_protect_1 == 2 && self.ram_protect_2 == 1 {
                    let ram_bank = bank & 0x07;
                    self.prg_ram[ram_bank * 0x2000 + (addr as usize % 0x2000)] = val;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16, is_sprite: bool) -> Option<u8> {
        if addr >= 0x2000 { return None; }

        if self.chr_is_ram {
            return Some(self.chr_ram[addr as usize]);
        }

        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 { return Some(0); }

        let use_chr_b = !is_sprite && (self.ppu_ctrl & 0x20 != 0);
        let chr_banks: &[usize] = if use_chr_b { &self.chr_banks_b } else { &self.chr_banks_a };

        let bank = match self.chr_mode {
            0 => {
                let base = if use_chr_b { chr_banks[3] } else { chr_banks[7] };
                (base & 0xFF8) | (addr as usize / 0x0400)
            }
            1 => {
                let base = if use_chr_b {
                    chr_banks[3]
                } else if addr < 0x1000 {
                    chr_banks[3]
                } else {
                    chr_banks[7]
                };
                (base & 0xFFC) | ((addr as usize % 0x1000) / 0x0400)
            }
            2 => {
                let idx = if use_chr_b {
                    (addr as usize / 0x0800) * 2 + 1
                } else {
                    (addr as usize / 0x0800) * 2 + 1
                };
                (chr_banks[idx.min(chr_banks.len() - 1)] & 0xFFE) | ((addr as usize % 0x0800) / 0x0400)
            }
            3 => chr_banks[(addr as usize / 0x0400).min(chr_banks.len() - 1)],
            _ => unreachable!(),
        };

        let offset = (addr as usize % 0x0400) + (bank % num_banks) * 0x0400;
        Some(self.chr_rom[offset])
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_is_ram {
            self.chr_ram[addr as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.nametable_mapping {
            0x44 => Mirroring::Vertical,
            0x50 => Mirroring::Horizontal,
            // Fallback for games assuming default boot state
            _ => Mirroring::Vertical,
        }
    }

    fn check_irq(&self) -> bool {
        self.irq_pending.get() && self.irq_enable
    }

    fn tick_scanline(&mut self) {
        let wd = self.watchdog.get();
        if wd == 0 {
            self.in_frame.set(true);
            self.scanline_counter = 0;
            self.irq_pending.set(false);
        } else {
            self.scanline_counter = self.scanline_counter.saturating_add(1);
            if self.scanline_counter == self.irq_target {
                self.irq_pending.set(true);
            }
        }
        self.watchdog.set(114);
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Mmc5 {
            prg_mode: self.prg_mode,
            chr_mode: self.chr_mode,
            prg_banks: vec![
                self.prg_ram_bank,
                self.prg_bank_8000,
                self.prg_bank_a000,
                self.prg_bank_c000,
                self.prg_bank_e000,
            ],
            irq_target: self.irq_target,
            irq_enable: self.irq_enable,
            irq_pending: self.irq_pending.get(),
            in_frame: self.in_frame.get(),
            scanline_counter: self.scanline_counter,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Mmc5 { prg_mode, chr_mode, prg_banks, irq_target, irq_enable, irq_pending, in_frame, scanline_counter } = state {
            self.prg_mode = *prg_mode;
            self.chr_mode = *chr_mode;
            if prg_banks.len() >= 5 {
                self.prg_ram_bank = prg_banks[0];
                self.prg_bank_8000 = prg_banks[1];
                self.prg_bank_a000 = prg_banks[2];
                self.prg_bank_c000 = prg_banks[3];
                self.prg_bank_e000 = prg_banks[4];
            }
            self.irq_target = *irq_target;
            self.irq_enable = *irq_enable;
            self.irq_pending.set(*irq_pending);
            self.in_frame.set(*in_frame);
            self.scanline_counter = *scanline_counter;
        }
    }
}

