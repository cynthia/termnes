pub mod mapper;

use crate::ppu::Mirroring;
use mapper::{
    AxromMapper, CnromMapper, Mapper, Mmc1Mapper, Mmc2Mapper, Mmc3Mapper, Mmc5Mapper, NromMapper,
    SplitFetch, SunsoftFme7Mapper, UnromMapper, Vrc6Mapper, Vrc6Variant,
};
use std::fs::File;
use std::io::Read;

pub struct Cartridge {
    pub mapper_id: u8,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mirroring: Mirroring,
    mapper: Box<dyn Mapper>,
    pub has_battery: bool,
    pub path: Option<String>,
}

impl Cartridge {
    pub fn new(data: &[u8]) -> Result<Self, String> {
        if data.len() < 16 {
            return Err("ROM is too short to contain a valid iNES header".to_string());
        }

        if &data[0..4] != b"NES\x1A" {
            return Err("Invalid iNES magic number. Is this a valid NES ROM?".to_string());
        }

        let prg_banks = data[4] as usize;
        let chr_banks = data[5] as usize;
        let flags6 = data[6];
        let flags7 = data[7];

        let mapper_id = (flags6 >> 4) | (flags7 & 0xF0);

        let mirroring = if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_battery = flags6 & 0x02 != 0;
        let has_trainer = flags6 & 0x04 != 0;

        let prg_size = prg_banks * 16384;
        let chr_size = chr_banks * 8192;

        let prg_start = 16 + if has_trainer { 512 } else { 0 };
        let chr_start = prg_start + prg_size;

        if data.len() < chr_start + chr_size {
            return Err("ROM file is truncated".to_string());
        }

        let prg_rom = data[prg_start..prg_start + prg_size].to_vec();
        let chr_rom = data[chr_start..chr_start + chr_size].to_vec();

        let mapper: Box<dyn Mapper> = match mapper_id {
            0 => Box::new(NromMapper::new(prg_rom.clone(), chr_rom.clone(), mirroring)),
            1 => Box::new(Mmc1Mapper::new(prg_rom.clone(), chr_rom.clone())),
            2 => Box::new(UnromMapper::new(prg_rom.clone(), mirroring)),
            3 => Box::new(CnromMapper::new(
                prg_rom.clone(),
                chr_rom.clone(),
                mirroring,
            )),
            4 => Box::new(Mmc3Mapper::new(prg_rom.clone(), chr_rom.clone())),
            5 => Box::new(Mmc5Mapper::new(prg_rom.clone(), chr_rom.clone())),
            7 => Box::new(AxromMapper::new(prg_rom.clone())),
            9 => Box::new(Mmc2Mapper::new(prg_rom.clone(), chr_rom.clone())),
            24 => Box::new(Vrc6Mapper::new(
                prg_rom.clone(),
                chr_rom.clone(),
                Vrc6Variant::Vrc6a,
            )),
            26 => Box::new(Vrc6Mapper::new(
                prg_rom.clone(),
                chr_rom.clone(),
                Vrc6Variant::Vrc6b,
            )),
            69 => Box::new(SunsoftFme7Mapper::new(prg_rom.clone(), chr_rom.clone())),
            _ => return Err(format!("Unsupported mapper: {}", mapper_id)),
        };

        Ok(Self {
            mapper_id,
            prg_rom,
            chr_rom,
            mirroring,
            mapper,
            has_battery,
            path: None,
        })
    }

    pub fn from_ines(data: &[u8]) -> Result<Self, String> {
        Self::new(data)
    }

    pub fn from_file(path: &str) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("Failed to open ROM: {}", e))?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| format!("Failed to read ROM: {}", e))?;
        let mut cart = Self::new(&data)?;
        cart.path = Some(path.to_string());
        Ok(cart)
    }

    pub fn sav_path(&self) -> Option<std::path::PathBuf> {
        self.path.as_ref().map(|p| {
            let mut path = std::path::PathBuf::from(p);
            path.set_extension("sav");
            path
        })
    }

    pub fn cpu_read(&self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.mapper.cpu_write(addr, val);
    }

    pub fn chr_read(&self, addr: u16, is_sprite: bool) -> u8 {
        self.mapper.chr_read(addr, is_sprite).unwrap_or(0)
    }

    pub fn chr_write(&mut self, addr: u16, val: u8) {
        self.mapper.chr_write(addr, val);
    }

    pub fn mapper_ppu_read(&self, addr: u16) -> Option<u8> {
        self.mapper.mapper_ppu_read(addr)
    }

    pub fn split_fetch(&self, scanline: u16, coarse_x: u8) -> Option<SplitFetch> {
        self.mapper.split_fetch(scanline, coarse_x)
    }

    #[doc(hidden)]
    pub fn dbg_exram_mode(&self) -> Option<u8> { self.mapper.dbg_exram_mode() }
    #[doc(hidden)]
    pub fn dbg_nametable_mapping(&self) -> Option<u8> { self.mapper.dbg_nametable_mapping() }
    #[doc(hidden)]
    pub fn dbg_split_mode(&self) -> Option<u8> { self.mapper.dbg_split_mode() }
    #[doc(hidden)]
    pub fn dbg_chr_banks_a(&self) -> Option<[usize; 8]> { self.mapper.dbg_chr_banks_a() }
    #[doc(hidden)]
    pub fn dbg_chr_banks_b(&self) -> Option<[usize; 4]> { self.mapper.dbg_chr_banks_b() }
    #[doc(hidden)]
    pub fn dbg_chr_high(&self) -> Option<usize> { self.mapper.dbg_chr_high() }
    #[doc(hidden)]
    pub fn dbg_irq_target(&self) -> Option<u8> { self.mapper.dbg_irq_target() }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    pub fn nt_ciram_bank(&self, nt_index: u8) -> u8 {
        self.mapper.nt_ciram_bank(nt_index)
    }

    pub fn tick_scanline(&mut self) {
        self.mapper.tick_scanline();
    }

    pub fn tick_scanline_early(&mut self) {
        self.mapper.tick_scanline_early();
    }

    pub fn tick_cpu(&mut self) {
        self.mapper.tick_cpu();
    }

    pub fn expansion_audio_sample(&self) -> f32 {
        self.mapper.expansion_audio_sample()
    }

    pub fn check_irq(&self) -> bool {
        self.mapper.check_irq()
    }

    pub fn save_mapper_state(&self) -> crate::savestate::MapperState {
        self.mapper.save_mapper_state()
    }

    pub fn load_mapper_state(&mut self, state: &crate::savestate::MapperState) {
        self.mapper.load_mapper_state(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ines(
        mapper_id: u8,
        prg_banks: u8,
        chr_banks: u8,
        mirroring_flag: u8,
        battery_flag: u8,
        trainer_flag: u8,
    ) -> Vec<u8> {
        let mut rom = b"NES\x1A".to_vec();
        rom.push(prg_banks);
        rom.push(chr_banks);

        let flags6_extra =
            (mirroring_flag & 0x01) | ((battery_flag & 0x01) << 1) | ((trainer_flag & 0x01) << 2);
        rom.push((mapper_id << 4) | flags6_extra); // flags6: lower nibble of mapper in bits 4-7
        rom.push(mapper_id & 0xF0); // flags7: upper nibble of mapper in bits 4-7

        rom.extend(vec![0; 8]); // padding

        if trainer_flag != 0 {
            rom.extend(vec![0; 512]);
        }
        rom.extend(vec![0; prg_banks as usize * 16384]);
        rom.extend(vec![0; chr_banks as usize * 8192]);

        rom
    }

    #[test]
    fn valid_nrom_header_parses() {
        let rom = make_ines(0, 1, 1, 0, 0, 0); // NROM (0), 16KB PRG, 8KB CHR, Horiz
        let cart = Cartridge::new(&rom).unwrap();
        assert_eq!(cart.mapper_id, 0);
        assert_eq!(cart.prg_rom.len(), 16384);
        assert_eq!(cart.chr_rom.len(), 8192);
        assert_eq!(cart.mirroring, Mirroring::Horizontal);
        assert!(!cart.has_battery);
        assert_eq!(cart.path, None);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut rom = make_ines(0, 1, 1, 0, 0, 0);
        rom[0] = b'M';
        let result = Cartridge::new(&rom);
        assert!(result.is_err(), "should reject invalid magic");
    }

    #[test]
    fn rejects_too_short() {
        let rom = vec![0; 10]; // header is 16 bytes
        let result = Cartridge::new(&rom);
        assert!(result.is_err(), "should reject short file");
    }

    #[test]
    fn rejects_truncated_prg() {
        let mut rom = make_ines(0, 1, 1, 0, 0, 0);
        rom.pop(); // remove one byte
        let result = Cartridge::new(&rom);
        assert!(result.is_err(), "should reject truncated ROM");
    }

    #[test]
    fn rejects_unsupported_mapper() {
        let rom = make_ines(255, 1, 0, 0, 0, 0); // mapper 255 is not supported
        let result = Cartridge::new(&rom);
        assert!(result.is_err(), "should reject unsupported mapper");
    }

    #[test]
    fn vertical_mirroring_flag() {
        let rom = make_ines(0, 1, 0, 1, 0, 0);
        let cart = Cartridge::new(&rom).unwrap();
        assert_eq!(cart.mirroring, Mirroring::Vertical);
    }

    #[test]
    fn battery_flag_detected() {
        let rom = make_ines(0, 1, 0, 0, 1, 0);
        let cart = Cartridge::new(&rom).unwrap();
        assert!(cart.has_battery);
    }

    #[test]
    fn trainer_is_skipped() {
        let mut rom = b"NES\x1A".to_vec();
        rom.push(1); // 1 PRG
        rom.push(0); // 0 CHR
        rom.push(0x04); // Trainer flag on
        rom.push(0);
        rom.extend(vec![0; 8]); // padding
        rom.extend(vec![0xAA; 512]); // trainer data
        rom.extend(vec![0xBB; 16384]); // PRG data

        let cart = Cartridge::new(&rom).unwrap();
        // The first byte of PRG should be 0xBB, skipping the 512-byte trainer
        assert_eq!(cart.prg_rom[0], 0xBB);
    }
}
