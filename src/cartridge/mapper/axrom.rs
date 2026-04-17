use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// AxROM (Mapper 7) — switches 32KB PRG-ROM banks, uses 8KB CHR-RAM.
/// Provides 1-screen nametable mirroring, switchable between the two internal VRAM pages.
pub struct AxromMapper {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    prg_bank: usize,
    mirroring: Mirroring,
}

impl AxromMapper {
    pub fn new(prg_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_ram: [0; 8192],
            prg_bank: 0,
            mirroring: Mirroring::OneScreenLow,
        }
    }
}

impl Mapper for AxromMapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x8000;
                if num_banks == 0 { return None; }
                let offset = (self.prg_bank % num_banks) * 0x8000 + (addr as usize - 0x8000);
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = (val & 0x07) as usize;
            self.mirroring = if val & 0x10 != 0 {
                Mirroring::OneScreenHigh
            } else {
                Mirroring::OneScreenLow
            };
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr < 0x2000 {
            Some(self.chr_ram[addr as usize])
        } else {
            None
        }
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            self.chr_ram[addr as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Axrom {
            prg_bank: self.prg_bank,
            chr_ram: self.chr_ram.to_vec(),
            mirroring: match self.mirroring {
                Mirroring::OneScreenLow => 0,
                Mirroring::OneScreenHigh => 1,
                _ => 0,
            },
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Axrom { prg_bank, chr_ram, mirroring } = state {
            self.prg_bank = *prg_bank;
            if chr_ram.len() == self.chr_ram.len() {
                self.chr_ram.copy_from_slice(chr_ram);
            }
            self.mirroring = match mirroring {
                1 => Mirroring::OneScreenHigh,
                _ => Mirroring::OneScreenLow,
            };
        }
    }
}
