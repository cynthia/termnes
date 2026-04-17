use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// MMC1 (Mapper 1) — dynamic bank switching for PRG and CHR.
/// Supports 16KB or 32KB PRG banks, 4KB or 8KB CHR banks.
/// Includes 8KB PRG RAM at $6000-$7FFF.
pub struct Mmc1Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    chr_ram: [u8; 8192],
    chr_is_ram: bool,

    // Shift register
    shift_register: u8,
    write_count: u8,

    // Internal registers
    control: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,

    mirroring: Mirroring,
}

impl Mmc1Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let mut m = Self {
            prg_rom,
            chr_rom,
            prg_ram: [0; 8192],
            chr_ram: [0; 8192],
            chr_is_ram,
            shift_register: 0x10,
            write_count: 0,
            control: 0x0C, // PRG mode 3 by default
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            mirroring: Mirroring::Horizontal,
        };
        m.update_mirroring();
        m
    }

    fn update_mirroring(&mut self) {
        self.mirroring = match self.control & 0x03 {
            0 => Mirroring::OneScreenLow,
            1 => Mirroring::OneScreenHigh,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };
    }
}

impl Mapper for Mmc1Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xFFFF => {
                let prg_mode = (self.control >> 2) & 0x03;
                let bank_select = (self.prg_bank & 0x0F) as usize;
                let num_banks = self.prg_rom.len() / 0x4000;

                if num_banks == 0 { return None; }

                match prg_mode {
                    0 | 1 => {
                        // switch 32 KB at $8000, ignore low bit of bank number
                        let bank = (bank_select & 0x0E) % num_banks;
                        let offset = (addr as usize - 0x8000) + bank * 0x4000;
                        Some(self.prg_rom[offset])
                    }
                    2 => {
                        // fix first bank at $8000, switch 16 KB at $C000
                        if addr < 0xC000 {
                            let offset = addr as usize - 0x8000;
                            Some(self.prg_rom[offset % self.prg_rom.len()])
                        } else {
                            let bank = bank_select % num_banks;
                            let offset = (addr as usize - 0xC000) + bank * 0x4000;
                            Some(self.prg_rom[offset % self.prg_rom.len()])
                        }
                    }
                    3 => {
                        // switch 16 KB at $8000, fix last bank at $C000
                        if addr < 0xC000 {
                            let bank = bank_select % num_banks;
                            let offset = (addr as usize - 0x8000) + bank * 0x4000;
                            Some(self.prg_rom[offset % self.prg_rom.len()])
                        } else {
                            let last_bank = num_banks.saturating_sub(1);
                            let offset = (addr as usize - 0xC000) + last_bank * 0x4000;
                            Some(self.prg_rom[offset % self.prg_rom.len()])
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000] = val,
            0x8000..=0xFFFF => {
                if val & 0x80 != 0 {
                    self.shift_register = 0x10;
                    self.write_count = 0;
                    self.control |= 0x0C;
                    self.update_mirroring();
                } else {
                    let bit = (val & 0x01) << self.write_count;
                    self.shift_register = (self.shift_register & !(1 << self.write_count)) | bit;
                    self.write_count += 1;

                    if self.write_count == 5 {
                        let data = self.shift_register & 0x1F;
                        match addr {
                            0x8000..=0x9FFF => {
                                self.control = data;
                                self.update_mirroring();
                            }
                            0xA000..=0xBFFF => self.chr_bank_0 = data,
                            0xC000..=0xDFFF => self.chr_bank_1 = data,
                            0xE000..=0xFFFF => self.prg_bank = data,
                            _ => {}
                        }
                        self.shift_register = 0;
                        self.write_count = 0;
                    }
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

        let chr_mode = (self.control >> 4) & 0x01;
        let num_banks = self.chr_rom.len() / 0x1000;
        if num_banks == 0 { return Some(0); }

        if chr_mode == 0 {
            // switch 8 KB at a time
            let bank = ((self.chr_bank_0 & 0x1E) as usize) % (num_banks / 2).max(1);
            let offset = addr as usize + bank * 0x2000;
            Some(self.chr_rom[offset % self.chr_rom.len()])
        } else {
            // switch two separate 4 KB banks
            let bank = if addr < 0x1000 {
                (self.chr_bank_0 as usize) % num_banks
            } else {
                (self.chr_bank_1 as usize) % num_banks
            };
            let offset = (addr as usize % 0x1000) + bank * 0x1000;
            Some(self.chr_rom[offset % self.chr_rom.len()])
        }
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_is_ram {
            self.chr_ram[addr as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Mmc1 {
            prg_ram: self.prg_ram.to_vec(),
            chr_ram: self.chr_ram.to_vec(),
            shift_register: self.shift_register,
            write_count: self.write_count,
            control: self.control,
            chr_bank_0: self.chr_bank_0,
            chr_bank_1: self.chr_bank_1,
            prg_bank: self.prg_bank,
            mirroring: match self.mirroring {
                Mirroring::Horizontal => 0,
                Mirroring::Vertical => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Mmc1 {
            prg_ram, chr_ram, shift_register, write_count,
            control, chr_bank_0, chr_bank_1, prg_bank, mirroring,
        } = state
        {
            if prg_ram.len() == self.prg_ram.len() {
                self.prg_ram.copy_from_slice(prg_ram);
            }
            if chr_ram.len() == self.chr_ram.len() {
                self.chr_ram.copy_from_slice(chr_ram);
            }
            self.shift_register = *shift_register;
            self.write_count = *write_count;
            self.control = *control;
            self.chr_bank_0 = *chr_bank_0;
            self.chr_bank_1 = *chr_bank_1;
            self.prg_bank = *prg_bank;
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
