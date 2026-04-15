pub trait Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, val: u8);
    fn chr_read(&self, addr: u16) -> Option<u8>;
    fn chr_write(&mut self, addr: u16, val: u8);
}

/// NROM (Mapper 0) — no bank switching.
/// 16KB PRG-ROM mirrors $8000-$BFFF to $C000-$FFFF.
/// 32KB PRG-ROM fills $8000-$FFFF directly.
/// CHR-ROM (or CHR-RAM if no CHR banks) at PPU $0000-$1FFF.
pub struct NromMapper {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_is_ram: bool,
}

impl NromMapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram { vec![0u8; 8192] } else { chr_rom };
        Self { prg_rom, chr, chr_is_ram }
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
}

/// UNROM (Mapper 2) — switches 16KB PRG-ROM banks, 8KB CHR-RAM.
/// Writing any value to $8000-$FFFF selects the bank at $8000-$BFFF.
/// The last 16KB bank is always mapped at $C000-$FFFF.
pub struct UnromMapper {
    prg_rom: Vec<u8>,
    bank_select: usize,
    num_banks: usize,
    chr_ram: [u8; 8192],
}

impl UnromMapper {
    pub fn new(prg_rom: Vec<u8>) -> Self {
        let num_banks = prg_rom.len() / 0x4000;
        Self {
            prg_rom,
            bank_select: 0,
            num_banks,
            chr_ram: [0; 8192],
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
        let m = NromMapper::new(prg, vec![]);
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
        let m = NromMapper::new(prg, vec![]);
        assert_eq!(m.cpu_read(0x8000), Some(0x11));
        assert_eq!(m.cpu_read(0xC000), Some(0x22));
    }

    #[test]
    fn nrom_chr_rom_read() {
        let chr = vec![0xCC; 8192];
        let m = NromMapper::new(make_prg(1, 0), chr);
        assert_eq!(m.chr_read(0x0000), Some(0xCC));
        assert_eq!(m.chr_read(0x1FFF), Some(0xCC));
        assert_eq!(m.chr_read(0x2000), None);
    }

    #[test]
    fn nrom_chr_ram_roundtrip() {
        let mut m = NromMapper::new(make_prg(1, 0), vec![]);
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
        let mut m = UnromMapper::new(prg);

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
        let mut m = UnromMapper::new(prg);

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
        let mut m = UnromMapper::new(prg);
        // Writing 7 wraps to bank 3 (7 % 4)
        m.cpu_write(0x8000, 7);
        assert_eq!(m.bank_select, 3);
    }

    #[test]
    fn unrom_chr_ram_roundtrip() {
        let prg = vec![0u8; 2 * 0x4000];
        let mut m = UnromMapper::new(prg);
        m.chr_write(0x0000, 0xAA);
        m.chr_write(0x1FFF, 0xBB);
        assert_eq!(m.chr_read(0x0000), Some(0xAA));
        assert_eq!(m.chr_read(0x1FFF), Some(0xBB));
    }

    #[test]
    fn unrom_chr_out_of_range() {
        let prg = vec![0u8; 2 * 0x4000];
        let m = UnromMapper::new(prg);
        assert_eq!(m.chr_read(0x2000), None);
    }
}
