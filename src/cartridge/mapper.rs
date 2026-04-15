use crate::ppu::Mirroring;

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
}

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

    fn chr_read(&self, addr: u16) -> Option<u8> {
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
}

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
}

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

    fn chr_read(&self, addr: u16) -> Option<u8> {
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
}

/// MMC2 (Mapper 9) — Punch-Out!! mapper.
/// 8KB switchable PRG bank at $8000, 3 fixed 8KB banks at $A000-$FFFF.
/// 4KB switchable CHR banks with variant latch switching.
use std::cell::Cell;
pub struct Mmc2Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,

    prg_bank: u8,
    chr_bank_0_l: u8,
    chr_bank_0_r: u8,
    chr_bank_1_l: u8,
    chr_bank_1_r: u8,

    latch_0: Cell<bool>, // false = L, true = R
    latch_1: Cell<bool>,

    mirroring: Mirroring,
}

impl Mmc2Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_rom,
            prg_bank: 0,
            chr_bank_0_l: 0,
            chr_bank_0_r: 0,
            chr_bank_1_l: 0,
            chr_bank_1_r: 0,
            latch_0: Cell::new(true),
            latch_1: Cell::new(true),
            mirroring: Mirroring::Horizontal,
        }
    }
}

impl Mapper for Mmc2Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.prg_bank as usize;
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let offset = (addr as usize - 0x8000) + (bank % num_banks) * 0x2000;
                Some(self.prg_rom[offset])
            }
            0xA000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let bank = match addr {
                    0xA000..=0xBFFF => num_banks.saturating_sub(3),
                    0xC000..=0xDFFF => num_banks.saturating_sub(2),
                    0xE000..=0xFFFF => num_banks.saturating_sub(1),
                    _ => unreachable!(),
                };
                let offset = (addr as usize % 0x2000) + bank * 0x2000;
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0xA000..=0xAFFF => self.prg_bank = val & 0x0F,
            0xB000..=0xBFFF => self.chr_bank_0_l = val & 0x1F,
            0xC000..=0xCFFF => self.chr_bank_0_r = val & 0x1F,
            0xD000..=0xDFFF => self.chr_bank_1_l = val & 0x1F,
            0xE000..=0xEFFF => self.chr_bank_1_r = val & 0x1F,
            0xF000..=0xFFFF => {
                self.mirroring = if val & 0x01 == 0 { Mirroring::Vertical } else { Mirroring::Horizontal };
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 { return None; }

        let num_banks = self.chr_rom.len() / 0x1000;
        if num_banks == 0 { return Some(0); }

        let bank = if addr < 0x1000 {
            if self.latch_0.get() { self.chr_bank_0_r } else { self.chr_bank_0_l }
        } else {
            if self.latch_1.get() { self.chr_bank_1_r } else { self.chr_bank_1_l }
        };

        let offset = (addr as usize % 0x1000) + (bank as usize % num_banks) * 0x1000;
        let val = self.chr_rom[offset % self.chr_rom.len()];

        // Latch switching is side-effect of reading certain tiles.
        // Bank 0: 0x0FD8 -> L, 0x0FE8 -> R
        // Bank 1: 0x1FD8-0x1FDF -> L, 0x1FE8-0x1FEF -> R
        match addr {
            0x0FD8 => self.latch_0.set(false),
            0x0FE8 => self.latch_0.set(true),
            0x1FD8..=0x1FDF => self.latch_1.set(false),
            0x1FE8..=0x1FEF => self.latch_1.set(true),
            _ => {}
        }

        Some(val)
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {}

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}

/// MMC3 (Mapper 4) — common mapper for many later games.
/// Supports 8KB PRG banks, 1KB/2KB CHR banks.
/// Includes a scanline IRQ counter.
pub struct Mmc3Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    prg_ram: [u8; 8192],
    chr_ram: [u8; 8192],
    chr_is_ram: bool,

    registers: [u8; 8],
    bank_select: u8,

    prg_mode: bool, // false: $8000 sw, $C000 fixed; true: $8000 fixed, $C000 sw
    chr_mode: bool, // false: 2KB at $0000, 1KB at $1000; true: 1KB at $0000, 2KB at $1000

    mirroring: Mirroring,

    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_reload: bool,
    irq_pending: bool,
}

impl Mmc3Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        Self {
            prg_rom,
            chr_rom,
            prg_ram: [0; 8192],
            chr_ram: [0; 8192],
            chr_is_ram,
            registers: [0; 8],
            bank_select: 0,
            prg_mode: false,
            chr_mode: false,
            mirroring: Mirroring::Horizontal,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_reload: false,
            irq_pending: false,
        }
    }
}

impl Mapper for Mmc3Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }

                let bank = match addr {
                    0x8000..=0x9FFF => {
                        if !self.prg_mode { self.registers[6] as usize } else { num_banks.saturating_sub(2) }
                    }
                    0xA000..=0xBFFF => self.registers[7] as usize,
                    0xC000..=0xDFFF => {
                        if self.prg_mode { self.registers[6] as usize } else { num_banks.saturating_sub(2) }
                    }
                    0xE000..=0xFFFF => num_banks.saturating_sub(1),
                    _ => unreachable!(),
                };
                let offset = (addr as usize % 0x2000) + (bank % num_banks) * 0x2000;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000] = val,
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    // Bank Select
                    self.bank_select = val & 0x07;
                    self.prg_mode = (val & 0x40) != 0;
                    self.chr_mode = (val & 0x80) != 0;
                } else {
                    // Bank Data
                    self.registers[self.bank_select as usize] = val;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    self.mirroring = if val & 1 == 0 { Mirroring::Vertical } else { Mirroring::Horizontal };
                } else {
                    // PRG RAM Protect (ignored)
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 { return None; }

        if self.chr_is_ram {
            return Some(self.chr_ram[addr as usize]);
        }

        let num_banks = self.chr_rom.len() / 0x0400; // 1KB banks
        if num_banks == 0 { return Some(0); }

        let bank = if !self.chr_mode {
            match addr {
                0x0000..=0x07FF => (self.registers[0] & 0xFE) as usize + (addr as usize / 0x0400),
                0x0800..=0x0FFF => (self.registers[1] & 0xFE) as usize + (addr as usize / 0x0400 - 2),
                0x1000..=0x13FF => self.registers[2] as usize,
                0x1400..=0x17FF => self.registers[3] as usize,
                0x1800..=0x1BFF => self.registers[4] as usize,
                0x1C00..=0x1FFF => self.registers[5] as usize,
                _ => unreachable!(),
            }
        } else {
            match addr {
                0x0000..=0x03FF => self.registers[2] as usize,
                0x0400..=0x07FF => self.registers[3] as usize,
                0x0800..=0x0BFF => self.registers[4] as usize,
                0x0C00..=0x0FFF => self.registers[5] as usize,
                0x1000..=0x17FF => (self.registers[0] & 0xFE) as usize + (addr as usize / 0x0400 - 4),
                0x1800..=0x1FFF => (self.registers[1] & 0xFE) as usize + (addr as usize / 0x0400 - 6),
                _ => unreachable!(),
            }
        };

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

    fn tick_scanline(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }

    fn check_irq(&self) -> bool {
        self.irq_pending
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn make_prg(num_banks: usize, fill: u8) -> Vec<u8> {
        vec![fill; num_banks * 0x4000]
    }

    // ── NROM ────────────────────────────────────────────────────────────────

    #[test]
    fn nrom_16kb_mirrors_upper_bank() {
        let mut prg = make_prg(1, 0x00);
        prg[0x0000] = 0xAB; // first byte of the single bank
        let m = NromMapper::new(prg, vec![], Mirroring::Horizontal);
        // $8000 reads first byte of bank
        assert_eq!(m.cpu_read(0x8000), Some(0xAB));
        // $C000 should mirror to same offset
        assert_eq!(m.cpu_read(0xC000), Some(0xAB));
    }

    #[test]
    fn nrom_32kb_no_mirror() {
        let mut prg = make_prg(2, 0x00);
        prg[0x0000] = 0x11; // bank 0 first byte ($8000)
        prg[0x4000] = 0x22; // bank 1 first byte ($C000)
        let m = NromMapper::new(prg, vec![], Mirroring::Horizontal);
        assert_eq!(m.cpu_read(0x8000), Some(0x11));
        assert_eq!(m.cpu_read(0xC000), Some(0x22));
    }

    #[test]
    fn nrom_chr_rom_read() {
        let chr = vec![0xCC; 8192];
        let m = NromMapper::new(make_prg(1, 0), chr, Mirroring::Horizontal);
        assert_eq!(m.chr_read(0x0000), Some(0xCC));
        assert_eq!(m.chr_read(0x1FFF), Some(0xCC));
        assert_eq!(m.chr_read(0x2000), None);
    }

    #[test]
    fn nrom_chr_ram_roundtrip() {
        let mut m = NromMapper::new(make_prg(1, 0), vec![], Mirroring::Horizontal);
        m.chr_write(0x0010, 0x55);
        assert_eq!(m.chr_read(0x0010), Some(0x55));
    }

    // ── UNROM ───────────────────────────────────────────────────────────────

    #[test]
    fn unrom_bank_switching() {
        // 4 banks; each bank filled with its index byte
        let mut prg = vec![0u8; 4 * 0x4000];
        for bank in 0..4usize {
            let fill = bank as u8;
            for b in &mut prg[bank * 0x4000..(bank + 1) * 0x4000] {
                *b = fill;
            }
        }
        let mut m = UnromMapper::new(prg, Mirroring::Horizontal);

        // Default: bank 0 at $8000
        assert_eq!(m.cpu_read(0x8000), Some(0x00));

        // Switch to bank 2
        m.cpu_write(0x8000, 2);
        assert_eq!(m.cpu_read(0x8000), Some(0x02));

        // Switch to bank 1
        m.cpu_write(0xC000, 1);
        assert_eq!(m.cpu_read(0x8000), Some(0x01));
    }

    #[test]
    fn unrom_last_bank_hardwired() {
        // 4 banks; last bank filled with 0xFF
        let mut prg = vec![0u8; 4 * 0x4000];
        for b in &mut prg[3 * 0x4000..] {
            *b = 0xFF;
        }
        let mut m = UnromMapper::new(prg, Mirroring::Horizontal);

        // Switch switchable bank away from last
        m.cpu_write(0x8000, 0);
        // $C000 must still read from last bank
        assert_eq!(m.cpu_read(0xC000), Some(0xFF));

        m.cpu_write(0x8000, 2);
        assert_eq!(m.cpu_read(0xC000), Some(0xFF));
    }

    #[test]
    fn unrom_bank_select_wraps() {
        // 4 banks
        let prg = vec![0u8; 4 * 0x4000];
        let mut m = UnromMapper::new(prg, Mirroring::Horizontal);
        // Writing 7 wraps to bank 3 (7 % 4)
        m.cpu_write(0x8000, 7);
        assert_eq!(m.bank_select, 3);
    }

    #[test]
    fn unrom_chr_ram_roundtrip() {
        let prg = vec![0u8; 2 * 0x4000];
        let mut m = UnromMapper::new(prg, Mirroring::Horizontal);
        m.chr_write(0x0000, 0xAA);
        m.chr_write(0x1FFF, 0xBB);
        assert_eq!(m.chr_read(0x0000), Some(0xAA));
        assert_eq!(m.chr_read(0x1FFF), Some(0xBB));
    }

    #[test]
    fn unrom_chr_out_of_range() {
        let prg = vec![0u8; 2 * 0x4000];
        let m = UnromMapper::new(prg, Mirroring::Horizontal);
        assert_eq!(m.chr_read(0x2000), None);
    }

    // ── MMC1 ────────────────────────────────────────────────────────────────

    #[test]
    fn mmc1_prg_banking() {
        // 8 banks of 16KB
        let mut prg = vec![0u8; 8 * 0x4000];
        for b in 0..8 {
            for i in 0..0x4000 {
                prg[b * 0x4000 + i] = b as u8;
            }
        }
        let mut m = Mmc1Mapper::new(prg, vec![]);

        // Mode 3: $C000 is last bank (7)
        assert_eq!(m.cpu_read(0xC000), Some(7));

        // Switch $8000 to bank 2
        // Write 00010 (binary) to $E000
        for i in 0..5 {
            let val = (2 >> i) & 0x01;
            m.cpu_write(0xE000, val as u8);
        }
        assert_eq!(m.cpu_read(0x8000), Some(2));
    }

    #[test]
    fn mmc1_mirroring() {
        let mut m = Mmc1Mapper::new(vec![0; 0x4000], vec![]);
        // Write 00000 (one-screen low)
        for _ in 0..5 {
            m.cpu_write(0x8000, 0);
        }
        assert_eq!(m.mirroring(), Mirroring::OneScreenLow);

        // Write 00001 (one-screen high)
        for i in 0..5 {
            m.cpu_write(0x8000, if i == 0 { 1 } else { 0 });
        }
        assert_eq!(m.mirroring(), Mirroring::OneScreenHigh);

        // Write 00010 (vertical)
        for i in 0..5 {
            m.cpu_write(0x8000, if i == 1 { 1 } else { 0 });
        }
        assert_eq!(m.mirroring(), Mirroring::Vertical);

        // Write 00011 (horizontal)
        for i in 0..5 {
            m.cpu_write(0x8000, if i < 2 { 1 } else { 0 });
        }
        assert_eq!(m.mirroring(), Mirroring::Horizontal);
        }

        // ── MMC2 ────────────────────────────────────────────────────────────────

    #[test]
    fn mmc2_latch_switching() {
        let prg = vec![0; 0x8000]; // 32KB
        let mut chr = vec![0; 0x8000]; // 32KB
        chr[0x0000] = 0xA1; // Bank 0, L
        chr[0x1000] = 0xB2; // Bank 1, R (default is R)
        chr[0x2000] = 0xC3; // Bank 2, L
        chr[0x3000] = 0xD4; // Bank 3, R

        let mut m = Mmc2Mapper::new(prg, chr);
        // Default: latch_0=R, latch_1=R
        // Bank 0 R = bank 0 (default registers are 0)
        // Bank 1 R = bank 0
        m.cpu_write(0xB000, 0); // Bank 0 L = 0
        m.cpu_write(0xC000, 1); // Bank 0 R = 1
        m.cpu_write(0xD000, 2); // Bank 1 L = 2
        m.cpu_write(0xE000, 3); // Bank 1 R = 3

        // Initially R/R
        assert_eq!(m.chr_read(0x0000), Some(0xB2)); // Bank 1 (0x1000 in chr_rom)
        assert_eq!(m.chr_read(0x1000), Some(0xD4)); // Bank 3 (0x3000 in chr_rom)

        // Switch latch 0 to L
        m.chr_read(0x0FD8);
        assert_eq!(m.chr_read(0x0000), Some(0xA1)); // Bank 0 (0x0000 in chr_rom)

        // Switch latch 0 back to R
        m.chr_read(0x0FE8);
        assert_eq!(m.chr_read(0x0000), Some(0xB2)); // Bank 1 (0x1000 in chr_rom)
    }

    // ── MMC3 ────────────────────────────────────────────────────────────────

    #[test]
    fn mmc3_irq_counter() {
        let mut m = Mmc3Mapper::new(vec![0; 0x8000], vec![]);
        m.cpu_write(0xC000, 5); // Latch = 5
        m.cpu_write(0xC001, 0); // Reload
        m.cpu_write(0xE001, 1); // Enable

        m.tick_scanline(); // First tick reloads to 5
        for _ in 0..4 {
            m.tick_scanline();
            assert!(!m.check_irq());
        }
        m.tick_scanline(); // 5th decrement (6th tick total) -> 0
        assert!(m.check_irq());

        // Disable clears pending? spec says E000 clears.
        m.cpu_write(0xE000, 0);
        assert!(!m.check_irq());
    }

    #[test]
    fn mmc3_prg_banking() {
        let mut prg = vec![0u8; 32 * 0x2000]; // 256KB
        for b in 0..32 {
            for i in 0..0x2000 {
                prg[b * 0x2000 + i] = b as u8;
            }
        }
        let mut m = Mmc3Mapper::new(prg, vec![]);

        // Default: last bank at $E000
        assert_eq!(m.cpu_read(0xE000), Some(31));
        // Second to last at $C000 (if prg_mode=0)
        assert_eq!(m.cpu_read(0xC000), Some(30));

        // Switch $8000 to bank 5
        m.cpu_write(0x8000, 6); // Select R6
        m.cpu_write(0x8001, 5); // Bank 5
        assert_eq!(m.cpu_read(0x8000), Some(5));

        // Switch $A000 to bank 10
        m.cpu_write(0x8000, 7); // Select R7
        m.cpu_write(0x8001, 10); // Bank 10
        assert_eq!(m.cpu_read(0xA000), Some(10));
    }
}
