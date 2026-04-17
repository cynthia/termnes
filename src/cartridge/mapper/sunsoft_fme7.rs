use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// Sunsoft FME-7 / Sunsoft 5B (Mapper 69)
/// Features 8KB PRG banking, 1KB CHR banking, and a 16-bit IRQ timer.
pub struct SunsoftFme7Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    chr_is_ram: bool,
    prg_ram: [u8; 8192],

    command: u8,
    chr_banks: [usize; 8],
    prg_banks: [usize; 4], // for $6000, $8000, $A000, $C000
    prg_ram_enable: bool,
    prg_ram_select: bool, // true if $6000 is RAM instead of ROM

    mirroring: Mirroring,

    irq_counter: u16,
    irq_enable: bool,
    irq_counter_enable: bool,
    irq_pending: bool,
}

impl SunsoftFme7Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 8192],
            chr_is_ram,
            prg_ram: [0; 8192],
            command: 0,
            chr_banks: [0; 8],
            prg_banks: [0, 0, 1, 2],
            prg_ram_enable: false,
            prg_ram_select: false,
            mirroring: Mirroring::Vertical,
            irq_counter: 0,
            irq_enable: false,
            irq_counter_enable: false,
            irq_pending: false,
        }
    }

    fn prg_read_8k(&self, bank: usize, offset: usize) -> u8 {
        let num_banks = self.prg_rom.len() / 0x2000;
        if num_banks == 0 { return 0; }
        self.prg_rom[(bank % num_banks) * 0x2000 + offset]
    }
}

impl Mapper for SunsoftFme7Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select {
                    if self.prg_ram_enable {
                        Some(self.prg_ram[addr as usize - 0x6000])
                    } else {
                        Some(0)
                    }
                } else {
                    Some(self.prg_read_8k(self.prg_banks[0], addr as usize - 0x6000))
                }
            }
            0x8000..=0x9FFF => Some(self.prg_read_8k(self.prg_banks[1], addr as usize - 0x8000)),
            0xA000..=0xBFFF => Some(self.prg_read_8k(self.prg_banks[2], addr as usize - 0xA000)),
            0xC000..=0xDFFF => Some(self.prg_read_8k(self.prg_banks[3], addr as usize - 0xC000)),
            0xE000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let last_bank = num_banks.saturating_sub(1);
                Some(self.prg_read_8k(last_bank, addr as usize - 0xE000))
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enable {
                    self.prg_ram[addr as usize - 0x6000] = val;
                }
            }
            0x8000..=0x9FFF => {
                self.command = val & 0x0F;
            }
            0xA000..=0xBFFF => {
                match self.command {
                    0x00..=0x07 => self.chr_banks[self.command as usize] = val as usize,
                    0x08 => {
                        self.prg_ram_enable = (val & 0x80) != 0;
                        self.prg_ram_select = (val & 0x40) != 0;
                        self.prg_banks[0] = (val & 0x3F) as usize;
                    }
                    0x09 => self.prg_banks[1] = (val & 0x3F) as usize,
                    0x0A => self.prg_banks[2] = (val & 0x3F) as usize,
                    0x0B => self.prg_banks[3] = (val & 0x3F) as usize,
                    0x0C => {
                        self.mirroring = match val & 0x03 {
                            0 => Mirroring::Vertical,
                            1 => Mirroring::Horizontal,
                            2 => Mirroring::OneScreenLow,
                            3 => Mirroring::OneScreenHigh,
                            _ => Mirroring::Vertical,
                        };
                    }
                    0x0D => {
                        self.irq_counter_enable = (val & 0x80) != 0;
                        self.irq_enable = (val & 0x01) != 0;
                        self.irq_pending = false;
                    }
                    0x0E => {
                        self.irq_counter = (self.irq_counter & 0xFF00) | (val as u16);
                    }
                    0x0F => {
                        self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8);
                    }
                    _ => {}
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
        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 { return Some(0); }

        let bank = self.chr_banks[(addr / 0x0400) as usize];
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

    fn tick_cpu(&mut self) {
        if !self.irq_counter_enable {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_counter = 0xFFFF;
            if self.irq_enable {
                self.irq_pending = true;
            }
        } else {
            self.irq_counter -= 1;
        }
    }

    fn check_irq(&self) -> bool {
        self.irq_pending
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::SunsoftFme7 {
            command: self.command,
            chr_banks: self.chr_banks.to_vec(),
            prg_banks: self.prg_banks.to_vec(),
            prg_ram_enable: self.prg_ram_enable,
            prg_ram_select: self.prg_ram_select,
            mirroring: match self.mirroring {
                Mirroring::Vertical => 0,
                Mirroring::Horizontal => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
            irq_counter: self.irq_counter,
            irq_enable: self.irq_enable,
            irq_counter_enable: self.irq_counter_enable,
            irq_pending: self.irq_pending,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::SunsoftFme7 {
            command, chr_banks, prg_banks, prg_ram_enable, prg_ram_select, mirroring,
            irq_counter, irq_enable, irq_counter_enable, irq_pending
        } = state {
            self.command = *command;
            if chr_banks.len() == 8 {
                self.chr_banks.copy_from_slice(chr_banks);
            }
            if prg_banks.len() == 4 {
                self.prg_banks.copy_from_slice(prg_banks);
            }
            self.prg_ram_enable = *prg_ram_enable;
            self.prg_ram_select = *prg_ram_select;
            self.mirroring = match mirroring {
                0 => Mirroring::Vertical,
                1 => Mirroring::Horizontal,
                2 => Mirroring::OneScreenLow,
                3 => Mirroring::OneScreenHigh,
                _ => Mirroring::Vertical,
            };
            self.irq_counter = *irq_counter;
            self.irq_enable = *irq_enable;
            self.irq_counter_enable = *irq_counter_enable;
            self.irq_pending = *irq_pending;
        }
    }
}
