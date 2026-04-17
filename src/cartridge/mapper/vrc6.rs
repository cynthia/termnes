use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// VRC6 variant
#[derive(Clone, Copy, PartialEq)]
pub enum Vrc6Variant {
    Vrc6a, // Mapper 24
    Vrc6b, // Mapper 26
}

/// VRC6 (Mappers 24 and 26) — Konami's advanced mapper.
/// Features 16KB/8KB PRG banking, 1KB CHR banking, and an IRQ timer.
pub struct Vrc6Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    variant: Vrc6Variant,

    prg_bank_16k: usize,
    prg_bank_8k: usize,

    chr_banks: [usize; 8],

    mirroring: Mirroring,
    
    // IRQ
    irq_latch: u8,
    irq_counter: u8,
    irq_enable: bool,
    irq_enable_after_ack: bool,
    irq_pending: bool,
}

impl Vrc6Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, variant: Vrc6Variant) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0; 8192],
            variant,
            prg_bank_16k: 0,
            prg_bank_8k: 0,
            chr_banks: [0; 8],
            mirroring: Mirroring::Vertical,
            irq_latch: 0,
            irq_counter: 0,
            irq_enable: false,
            irq_enable_after_ack: false,
            irq_pending: false,
        }
    }

    fn fix_addr(&self, addr: u16) -> u16 {
        if self.variant == Vrc6Variant::Vrc6b {
            let a0 = addr & 0x01;
            let a1 = (addr & 0x02) >> 1;
            (addr & 0xFFFC) | (a0 << 1) | a1
        } else {
            addr
        }
    }
}

impl Mapper for Vrc6Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xBFFF => {
                let num_banks = self.prg_rom.len() / 0x4000;
                if num_banks == 0 { return None; }
                let offset = (addr as usize - 0x8000) + (self.prg_bank_16k % num_banks) * 0x4000;
                Some(self.prg_rom[offset])
            }
            0xC000..=0xDFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let offset = (addr as usize - 0xC000) + (self.prg_bank_8k % num_banks) * 0x2000;
                Some(self.prg_rom[offset])
            }
            0xE000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let last_bank = num_banks.saturating_sub(1);
                let offset = (addr as usize - 0xE000) + last_bank * 0x2000;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        let fixed_addr = self.fix_addr(addr);

        match fixed_addr {
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000] = val,
            0x8000..=0x8003 => self.prg_bank_16k = (val & 0x0F) as usize,
            0xC000..=0xC003 => self.prg_bank_8k = (val & 0x1F) as usize,
            0xD000..=0xD003 => {
                match fixed_addr {
                    0xD000 => self.chr_banks[0] = val as usize,
                    0xD001 => self.chr_banks[1] = val as usize,
                    0xD002 => self.chr_banks[2] = val as usize,
                    0xD003 => self.chr_banks[3] = val as usize,
                    _ => {}
                }
            }
            0xE000..=0xE003 => {
                match fixed_addr {
                    0xE000 => self.chr_banks[4] = val as usize,
                    0xE001 => self.chr_banks[5] = val as usize,
                    0xE002 => self.chr_banks[6] = val as usize,
                    0xE003 => self.chr_banks[7] = val as usize,
                    _ => {}
                }
            }
            0xB003 => {
                self.mirroring = match (val >> 2) & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::OneScreenLow,
                    3 => Mirroring::OneScreenHigh,
                    _ => Mirroring::Vertical,
                };
            }
            0xF000 => {
                self.irq_latch = val;
            }
            0xF001 => {
                self.irq_enable_after_ack = (val & 0x01) != 0;
                self.irq_enable = (val & 0x02) != 0;
                if val & 0x02 != 0 {
                    self.irq_counter = self.irq_latch;
                }
                self.irq_pending = false;
            }
            0xF002 => {
                self.irq_enable = self.irq_enable_after_ack;
                self.irq_pending = false;
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 { return None; }
        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 { return Some(0); }

        let bank = self.chr_banks[(addr / 0x0400) as usize];
        let offset = (addr as usize % 0x0400) + (bank % num_banks) * 0x0400;
        Some(self.chr_rom[offset])
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {}

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn tick_scanline(&mut self) {
        if self.irq_enable {
            if self.irq_counter == 0xFF {
                self.irq_counter = self.irq_latch;
                self.irq_pending = true;
            } else {
                self.irq_counter = self.irq_counter.wrapping_add(1);
            }
        }
    }

    fn check_irq(&self) -> bool {
        self.irq_pending
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Vrc6 {
            prg_bank_16k: self.prg_bank_16k,
            prg_bank_8k: self.prg_bank_8k,
            chr_banks: self.chr_banks.to_vec(),
            mirroring: match self.mirroring {
                Mirroring::Vertical => 0,
                Mirroring::Horizontal => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
            irq_latch: self.irq_latch,
            irq_counter: self.irq_counter,
            irq_enable: self.irq_enable,
            irq_enable_after_ack: self.irq_enable_after_ack,
            irq_pending: self.irq_pending,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Vrc6 {
            prg_bank_16k, prg_bank_8k, chr_banks, mirroring,
            irq_latch, irq_counter, irq_enable, irq_enable_after_ack, irq_pending
        } = state {
            self.prg_bank_16k = *prg_bank_16k;
            self.prg_bank_8k = *prg_bank_8k;
            if chr_banks.len() == 8 {
                self.chr_banks.copy_from_slice(chr_banks);
            }
            self.mirroring = match mirroring {
                0 => Mirroring::Vertical,
                1 => Mirroring::Horizontal,
                2 => Mirroring::OneScreenLow,
                3 => Mirroring::OneScreenHigh,
                _ => Mirroring::Vertical,
            };
            self.irq_latch = *irq_latch;
            self.irq_counter = *irq_counter;
            self.irq_enable = *irq_enable;
            self.irq_enable_after_ack = *irq_enable_after_ack;
            self.irq_pending = *irq_pending;
        }
    }
}
