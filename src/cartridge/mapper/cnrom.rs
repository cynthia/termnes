use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// CNROM (Mapper 3) — switches 8KB CHR-ROM banks, fixed PRG-ROM.
/// Writing any value to $8000-$FFFF selects the CHR bank.
/// PRG-ROM is either 16KB (mirrored) or 32KB.
pub struct CnromMapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    bank_select: usize,
    num_chr_banks: usize,
    mirroring: Mirroring,
}

impl CnromMapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let num_chr_banks = (chr_rom.len() / 0x2000).max(1);
        Self {
            prg_rom,
            chr_rom,
            bank_select: 0,
            num_chr_banks,
            mirroring,
        }
    }
}

impl Mapper for CnromMapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr as usize - 0x8000) % self.prg_rom.len();
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.bank_select = (val as usize) % self.num_chr_banks;
        }
    }

    fn chr_read(&self, addr: u16, _is_sprite: bool) -> Option<u8> {
        if addr < 0x2000 {
            let offset = self.bank_select * 0x2000 + addr as usize;
            Some(self.chr_rom[offset])
        } else {
            None
        }
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {
        // CHR-ROM is not writable
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Cnrom {
            bank_select: self.bank_select,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Cnrom { bank_select } = state {
            self.bank_select = *bank_select;
        }
    }
}
