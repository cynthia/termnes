use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// NROM (Mapper 0) — no bank switching.
/// 16KB PRG-ROM mirrors $8000-$BFFF to $C000-$FFFF.
/// 32KB PRG-ROM fills $8000-$FFFF directly.
/// CHR-ROM (or CHR-RAM if no CHR banks) at PPU $0000-$1FFF.
pub struct NromMapper {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_is_ram: bool,
    mirroring: Mirroring,
}

impl NromMapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self { prg_rom, chr, chr_is_ram, mirroring }
    }
}

impl Mapper for NromMapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                // Mirror 16KB ROM into upper half when only one bank
                let offset = (addr as usize - 0x8000) % self.prg_rom.len();
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, _addr: u16, _val: u8) {
        // NROM PRG-ROM is read-only
    }

    fn chr_read(&self, addr: u16, _is_sprite: bool) -> Option<u8> {
        if (addr as usize) < self.chr.len() {
            Some(self.chr[addr as usize])
        } else {
            None
        }
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr_is_ram && (addr as usize) < self.chr.len() {
            self.chr[addr as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Nrom
    }

    fn load_mapper_state(&mut self, _state: &MapperState) {}
}
