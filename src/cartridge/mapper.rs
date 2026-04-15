use crate::ppu::Mirroring;

pub trait Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&self, addr: u16) -> Option<u8>;
    fn chr_write(&mut self, addr: u16, val: u8);
    fn mirroring(&self) -> Mirroring {
        Mirroring::Horizontal
    }
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
    }

