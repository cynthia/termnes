use crate::ppu::Mirroring;
use crate::savestate::MapperState;

/// Per-tile override for MMC5 vertical split screen mode ($5200-$5202).
/// When the mapper returns this for a given (scanline, coarse_x), the PPU
/// replaces its normal bg NT/AT/PT fetches with these values.
pub struct SplitFetch {
    pub palette: u8,      // 2-bit palette index
    pub pattern_lo: u8,   // bg pattern low plane byte for this scanline's row
    pub pattern_hi: u8,   // bg pattern high plane byte
}

pub trait Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&self, addr: u16, is_sprite: bool) -> Option<u8>;
    fn chr_write(&mut self, addr: u16, val: u8);
    fn mapper_ppu_read(&self, _addr: u16) -> Option<u8> { None }
    fn mirroring(&self) -> Mirroring {
        Mirroring::Horizontal
    }
    /// Which CIRAM bank (0 or 1) should the given logical nametable index
    /// (0..=3) route to when `mapper_ppu_read` returns `None`? Default is
    /// derived from the coarse `mirroring()` mode, but mappers (notably
    /// MMC5) can express per-NT combinations that don't fit the 4-variant
    /// enum. Used by the PPU's internal CIRAM mirror logic.
    fn nt_ciram_bank(&self, nt_index: u8) -> u8 {
        match self.mirroring() {
            Mirroring::Horizontal => [0, 0, 1, 1][nt_index as usize & 0x03],
            Mirroring::Vertical => [0, 1, 0, 1][nt_index as usize & 0x03],
            Mirroring::OneScreenLow => 0,
            Mirroring::OneScreenHigh => 1,
        }
    }
    /// Called late in each rendering scanline (cycle 260). MMC3 uses this
    /// to approximate its A12 rising-edge IRQ clock.
    fn tick_scanline(&mut self) {}
    /// Called early in each rendering scanline (cycle 4). MMC5 uses this
    /// because its internal scanline counter is documented to compare
    /// against `$5203` at PPU cycle 4 — firing ~85 CPU cycles earlier
    /// than the MMC3-style late tick, which matters for mid-frame CHR
    /// swaps (Metal Slader Glory's portrait is sensitive to this).
    fn tick_scanline_early(&mut self) {}
    fn tick_cpu(&mut self) {}
    fn expansion_audio_sample(&self) -> f32 {
        0.0
    }
    fn check_irq(&self) -> bool {
        false
    }
    /// MMC5 vertical split override. Default: no override.
    fn split_fetch(&self, _scanline: u16, _coarse_x: u8) -> Option<SplitFetch> {
        None
    }
    /// Debug-only accessors for mapper register state. Used by tests/probes.
    /// None when the mapper type doesn't carry this concept.
    #[doc(hidden)]
    fn dbg_exram_mode(&self) -> Option<u8> { None }
    #[doc(hidden)]
    fn dbg_nametable_mapping(&self) -> Option<u8> { None }
    #[doc(hidden)]
    fn dbg_split_mode(&self) -> Option<u8> { None }
    #[doc(hidden)]
    fn dbg_chr_banks_a(&self) -> Option<[usize; 8]> { None }
    #[doc(hidden)]
    fn dbg_chr_banks_b(&self) -> Option<[usize; 4]> { None }
    #[doc(hidden)]
    fn dbg_chr_high(&self) -> Option<usize> { None }
    #[doc(hidden)]
    fn dbg_irq_target(&self) -> Option<u8> { None }
    fn save_mapper_state(&self) -> MapperState;
    fn load_mapper_state(&mut self, state: &MapperState);
}

pub mod axrom;
pub mod cnrom;
pub mod mmc1;
pub mod mmc2;
pub mod mmc3;
pub mod mmc5;
pub mod nrom;
pub mod sunsoft_fme7;
pub mod unrom;
pub mod vrc6;

pub use axrom::AxromMapper;
pub use cnrom::CnromMapper;
pub use mmc1::Mmc1Mapper;
pub use mmc2::Mmc2Mapper;
pub use mmc3::Mmc3Mapper;
pub use mmc5::Mmc5Mapper;
pub use nrom::NromMapper;
pub use sunsoft_fme7::SunsoftFme7Mapper;
pub use unrom::UnromMapper;
pub use vrc6::{Vrc6Mapper, Vrc6Variant};
