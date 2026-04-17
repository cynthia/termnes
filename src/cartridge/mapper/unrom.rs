use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// UNROM (Mapper 2) — switches 16KB PRG-ROM banks, 8KB CHR-RAM.
/// Writing any value to $8000-$FFFF selects the bank at $8000-$BFFF.
/// The last 16KB bank is always mapped at $C000-$FFFF.
pub struct UnromMapper {
    prg_rom: Vec<u8>,
    bank_select: usize,
    num_banks: usize,
    chr_ram: [u8; 8192],
    mirroring: Mirroring,
}

impl UnromMapper {
    pub fn new(prg_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let num_banks = prg_rom.len() / 0x4000;
        Self {
            prg_rom,
            bank_select: 0,
            num_banks,
            chr_ram: [0; 8192],
            mirroring,
        }
    }
}

impl Mapper for UnromMapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let offset = self.bank_select * 0x4000 + (addr as usize - 0x8000);
                Some(self.prg_rom[offset])
            }
            0xC000..=0xFFFF => {
                let last_bank = self.num_banks.saturating_sub(1);
                let offset = last_bank * 0x4000 + (addr as usize - 0xC000);
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 && self.num_banks > 0 {
            self.bank_select = (val as usize) % self.num_banks;
        }
    }

    fn chr_read(&self, addr: u16, _is_sprite: bool) -> Option<u8> {
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
        MapperState::Unrom {
            bank_select: self.bank_select,
            chr_ram: self.chr_ram.to_vec(),
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Unrom { bank_select, chr_ram } = state {
            self.bank_select = *bank_select;
            if chr_ram.len() == self.chr_ram.len() {
                self.chr_ram.copy_from_slice(chr_ram);
            }
        }
    }
}
