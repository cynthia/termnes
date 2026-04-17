use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// MMC3 (Mapper 4) — common mapper for many later games.
/// Supports 8KB PRG banks, 1KB/2KB CHR banks.
/// Includes a scanline IRQ counter.
pub struct Mmc3Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    chr_ram: [u8; 8192],
    chr_is_ram: bool,

    registers: [u8; 8],
    bank_select: u8,

    prg_mode: bool, // false: $8000 sw, $C000 fixed; true: $8000 fixed, $C000 sw
    chr_mode: bool, // false: 2KB at $0000, 1KB at $1000; true: 1KB at $0000, 2KB at $1000

    mirroring: Mirroring,

    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_reload: bool,
    irq_pending: bool,
}

impl Mmc3Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0; 8192],
            chr_ram: [0; 8192],
            chr_is_ram,
            registers: [0; 8],
            bank_select: 0,
            prg_mode: false,
            chr_mode: false,
            mirroring: Mirroring::Horizontal,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_reload: false,
            irq_pending: false,
        }
    }
}

impl Mapper for Mmc3Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }

                let bank = match addr {
                    0x8000..=0x9FFF => {
                        if !self.prg_mode { self.registers[6] as usize } else { num_banks.saturating_sub(2) }
                    }
                    0xA000..=0xBFFF => self.registers[7] as usize,
                    0xC000..=0xDFFF => {
                        if self.prg_mode { self.registers[6] as usize } else { num_banks.saturating_sub(2) }
                    }
                    0xE000..=0xFFFF => num_banks.saturating_sub(1),
                    _ => unreachable!(),
                };
                let offset = (addr as usize % 0x2000) + (bank % num_banks) * 0x2000;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000] = val,
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    // Bank Select
                    self.bank_select = val & 0x07;
                    self.prg_mode = (val & 0x40) != 0;
                    self.chr_mode = (val & 0x80) != 0;
                } else {
                    // Bank Data
                    self.registers[self.bank_select as usize] = val;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    self.mirroring = if val & 1 == 0 { Mirroring::Vertical } else { Mirroring::Horizontal };
                } else {
                    // PRG RAM Protect (ignored)
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16, _is_sprite: bool) -> Option<u8> {
        if addr >= 0x2000 { return None; }

        if self.chr_is_ram {
            return Some(self.chr_ram[addr as usize]);
        }

        let num_banks = self.chr_rom.len() / 0x0400; // 1KB banks
        if num_banks == 0 { return Some(0); }

        let bank = if !self.chr_mode {
            match addr {
                0x0000..=0x07FF => (self.registers[0] & 0xFE) as usize + (addr as usize / 0x0400),
                0x0800..=0x0FFF => (self.registers[1] & 0xFE) as usize + (addr as usize / 0x0400 - 2),
                0x1000..=0x13FF => self.registers[2] as usize,
                0x1400..=0x17FF => self.registers[3] as usize,
                0x1800..=0x1BFF => self.registers[4] as usize,
                0x1C00..=0x1FFF => self.registers[5] as usize,
                _ => unreachable!(),
            }
        } else {
            match addr {
                0x0000..=0x03FF => self.registers[2] as usize,
                0x0400..=0x07FF => self.registers[3] as usize,
                0x0800..=0x0BFF => self.registers[4] as usize,
                0x0C00..=0x0FFF => self.registers[5] as usize,
                0x1000..=0x17FF => (self.registers[0] & 0xFE) as usize + (addr as usize / 0x0400 - 4),
                0x1800..=0x1FFF => (self.registers[1] & 0xFE) as usize + (addr as usize / 0x0400 - 6),
                _ => unreachable!(),
            }
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
        self.mirroring
    }

    fn tick_scanline(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn check_irq(&self) -> bool {
        self.irq_pending
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Mmc3 {
            prg_ram: self.prg_ram.to_vec(),
            chr_ram: self.chr_ram.to_vec(),
            registers: self.registers.to_vec(),
            bank_select: self.bank_select,
            prg_mode: self.prg_mode,
            chr_mode: self.chr_mode,
            mirroring: match self.mirroring {
                Mirroring::Horizontal => 0,
                Mirroring::Vertical => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
            irq_latch: self.irq_latch,
            irq_counter: self.irq_counter,
            irq_enabled: self.irq_enabled,
            irq_reload: self.irq_reload,
            irq_pending: self.irq_pending,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Mmc3 {
            prg_ram, chr_ram, registers, bank_select,
            prg_mode, chr_mode, mirroring,
            irq_latch, irq_counter, irq_enabled, irq_reload, irq_pending,
        } = state
        {
            if prg_ram.len() == self.prg_ram.len() {
                self.prg_ram.copy_from_slice(prg_ram);
            }
            if chr_ram.len() == self.chr_ram.len() {
                self.chr_ram.copy_from_slice(chr_ram);
            }
            if registers.len() == self.registers.len() {
                self.registers.copy_from_slice(registers);
            }
            self.bank_select = *bank_select;
            self.prg_mode = *prg_mode;
            self.chr_mode = *chr_mode;
            self.mirroring = match mirroring {
                0 => Mirroring::Horizontal,
                1 => Mirroring::Vertical,
                2 => Mirroring::OneScreenLow,
                3 => Mirroring::OneScreenHigh,
                _ => Mirroring::Horizontal,
            };
            self.irq_latch = *irq_latch;
            self.irq_counter = *irq_counter;
            self.irq_enabled = *irq_enabled;
            self.irq_reload = *irq_reload;
            self.irq_pending = *irq_pending;
        }
    }
}
