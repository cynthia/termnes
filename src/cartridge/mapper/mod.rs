use crate::ppu::Mirroring;
use crate::savestate::MapperState;

pub trait Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&self, addr: u16) -> Option<u8>;
    fn chr_write(&mut self, addr: u16, val: u8);
    fn mirroring(&self) -> Mirroring {
        Mirroring::Horizontal
    }
    fn tick_scanline(&mut self) {}
    fn check_irq(&self) -> bool { false }
    fn save_mapper_state(&self) -> MapperState;
    fn load_mapper_state(&mut self, state: &MapperState);
}

pub mod nrom;
pub mod unrom;
pub mod cnrom;
pub mod mmc1;
pub mod mmc2;
pub mod mmc3;
pub mod axrom;
pub mod mmc5;

pub use nrom::NromMapper;
pub use unrom::UnromMapper;
pub use cnrom::CnromMapper;
pub use mmc1::Mmc1Mapper;
pub use mmc2::Mmc2Mapper;
pub use mmc3::Mmc3Mapper;
pub use axrom::AxromMapper;
pub use mmc5::Mmc5Mapper;
