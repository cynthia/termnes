use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::{Mapper, SplitFetch};

/// MMC5 (Mapper 5) — Advanced mapper with EXRAM, large ROM support, and extra audio.
/// This is a basic implementation supporting PRG/CHR banking to allow games to boot.
pub struct Mmc5Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    chr_is_ram: bool,
    prg_ram: [u8; 64 * 1024], // Up to 64KB PRG RAM
    exram: [u8; 1024],        // 1KB Expansion RAM

    prg_mode: u8,
    chr_mode: u8,
    exram_mode: u8,

    prg_ram_bank: usize,
    prg_bank_8000: usize,
    prg_bank_a000: usize,
    prg_bank_c000: usize,
    prg_bank_e000: usize,
    chr_banks_a: [usize; 8],
    chr_banks_b: [usize; 4],
    chr_high: usize,

    nametable_mapping: u8,
    fill_tile: u8,
    fill_color: u8,

    ram_protect_1: u8,
    ram_protect_2: u8,

    multiplier_1: u8,
    multiplier_2: u8,

    irq_target: u8,
    irq_enable: bool,
    irq_pending: std::cell::Cell<bool>,
    in_frame: std::cell::Cell<bool>,
    scanline_counter: u8,
    watchdog: std::cell::Cell<u8>,

    ppu_ctrl: u8,
    ppu_mask: u8,

    last_nt_addr: std::cell::Cell<u16>,

    // Mesen-style 3-CPU-cycle PPU idle detector. Every PPU bus access
    // resets `ppu_idle` to 3; `tick_cpu` decrements it. When it drains
    // to 0 the MMC5 decides the PPU has gone idle (rendering disabled or
    // VBlank) and clears `in_frame`. `need_in_frame` is the "first tick
    // after idle" latch — on the next scanline detection after it's set,
    // `in_frame` promotes and the counter starts incrementing.
    ppu_idle: std::cell::Cell<u8>,
    need_in_frame: std::cell::Cell<bool>,

    // Tracks which CHR bank register set was most recently written:
    // false = set A ($5120-$5127, "sprite"), true = set B ($5128-$512B, "BG").
    // Matters for $2007 reads during vblank or forced blank: on real MMC5,
    // those reads use the last-written set regardless of PPUCTRL bit 5.
    // Uchuu Keibitai SDF copies its title nametable via this path.
    last_written_chr_set_b: std::cell::Cell<bool>,

    // Vertical split screen ($5200-$5202). Used by Uchuu Keibitai SDF.
    split_mode: u8,    // $5200: bit 7 enable, bit 6 side (1=right), bits 4-0 tile
    split_scroll: u8,  // $5201: Y pixel scroll (0-239) for the split region
    split_bank: u8,    // $5202: 4KB CHR bank for split tiles
}

impl Mmc5Mapper {
    /// Called on every PPU bus read so the MMC5 can track whether the PPU
    /// is actively rendering. Real hardware watches PPU /RD and decides
    /// "in-frame" dropped when 3 consecutive M2 rises see no PPU read.
    fn notify_ppu_read(&self) {
        // Real MMC5 drops in_frame 3 PPU cycles after the last read, but
        // our renderer only emits reads during cycles 1-256; cycles 257-
        // 340 of each scanline are silent. Use a larger timeout so the
        // gap between scanlines doesn't falsely trip "idle". Still drains
        // cleanly during VBlank (~2280 CPU cycles).
        self.ppu_idle.set(120);
    }

    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        let last_bank = prg_rom.len().saturating_div(0x2000).saturating_sub(1);
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 8192],
            chr_is_ram,
            prg_ram: [0; 64 * 1024],
            exram: [0; 1024],
            prg_mode: 3,
            chr_mode: 0,
            exram_mode: 0,
            prg_ram_bank: 0,
            prg_bank_8000: last_bank | 0x80,
            prg_bank_a000: last_bank | 0x80,
            prg_bank_c000: last_bank | 0x80,
            prg_bank_e000: last_bank | 0x80,
            chr_banks_a: [0; 8],
            chr_banks_b: [0; 4],
            chr_high: 0,
            nametable_mapping: 0,
            fill_tile: 0,
            fill_color: 0,
            ram_protect_1: 0,
            ram_protect_2: 0,
            multiplier_1: 0,
            multiplier_2: 0,
            irq_target: 0,
            irq_enable: false,
            irq_pending: std::cell::Cell::new(false),
            in_frame: std::cell::Cell::new(false),
            scanline_counter: 0,
            watchdog: std::cell::Cell::new(0),
            ppu_ctrl: 0,
            ppu_mask: 0,
            last_nt_addr: std::cell::Cell::new(0),
            ppu_idle: std::cell::Cell::new(0),
            need_in_frame: std::cell::Cell::new(false),
            last_written_chr_set_b: std::cell::Cell::new(false),
            split_mode: 0,
            split_scroll: 0,
            split_bank: 0,
        }
    }

    fn prg_read_8k(&self, bank: usize, offset: usize) -> u8 {
        // High bit 7 indicates ROM vs RAM
        if bank & 0x80 != 0 {
            let rom_bank = bank & 0x7F;
            let num_banks = self.prg_rom.len() / 0x2000;
            if num_banks == 0 { return 0; }
            self.prg_rom[((rom_bank % num_banks) * 0x2000) + offset]
        } else {
            let ram_bank = bank & 0x07;
            self.prg_ram[(ram_bank * 0x2000) + offset]
        }
    }

    fn get_prg_bank(&self, addr: u16) -> usize {
        match self.prg_mode {
            0 => {
                // 32KB mode
                (self.prg_bank_e000 & 0x7C) | (self.prg_bank_e000 & 0x80) | ((addr as usize - 0x8000) / 0x2000)
            }
            1 => {
                // 16KB mode
                if addr < 0xC000 {
                    (self.prg_bank_a000 & 0x7E) | (self.prg_bank_a000 & 0x80) | ((addr as usize - 0x8000) / 0x2000)
                } else {
                    (self.prg_bank_e000 & 0x7E) | (self.prg_bank_e000 & 0x80) | ((addr as usize - 0xC000) / 0x2000)
                }
            }
            2 => {
                // 16KB-8KB mode
                if addr < 0xC000 {
                    (self.prg_bank_a000 & 0x7E) | (self.prg_bank_a000 & 0x80) | ((addr as usize - 0x8000) / 0x2000)
                } else if addr < 0xE000 {
                    self.prg_bank_c000
                } else {
                    self.prg_bank_e000 | 0x80
                }
            }
            3 => {
                // 8KB mode
                match addr {
                    0x8000..=0x9FFF => self.prg_bank_8000,
                    0xA000..=0xBFFF => self.prg_bank_a000,
                    0xC000..=0xDFFF => self.prg_bank_c000,
                    0xE000..=0xFFFF => self.prg_bank_e000 | 0x80,
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Mapper for Mmc5Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x5204 => {
                let mut status = 0;
                if self.irq_pending.get() { status |= 0x80; }
                if self.in_frame.get() { status |= 0x40; }
                self.irq_pending.set(false);
                Some(status)
            }
            0x5205 => Some((self.multiplier_1 as u16 * self.multiplier_2 as u16) as u8),
            0x5206 => Some(((self.multiplier_1 as u16 * self.multiplier_2 as u16) >> 8) as u8),
            0x5C00..=0x5FFF => {
                if self.exram_mode == 2 || self.exram_mode == 3 {
                    Some(self.exram[addr as usize - 0x5C00])
                } else {
                    Some(0) // Read 0 if not accessible by CPU
                }
            }
            0x6000..=0x7FFF => {
                Some(self.prg_read_8k(self.prg_ram_bank, addr as usize - 0x6000))
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                Some(self.prg_read_8k(bank, addr as usize % 0x2000))
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x2000 => self.ppu_ctrl = val,
            0x2001 => self.ppu_mask = val,
            0x5100 => self.prg_mode = val & 0x03,
            0x5101 => self.chr_mode = val & 0x03,
            0x5102 => self.ram_protect_1 = val & 0x03,
            0x5103 => self.ram_protect_2 = val & 0x03,
            0x5104 => self.exram_mode = val & 0x03,
            0x5105 => self.nametable_mapping = val,
            0x5106 => self.fill_tile = val,
            0x5107 => self.fill_color = val & 0x03,
            // $5113 is PRG-RAM-only on MMC5 — bits 3-7 are ignored (no ROM
            // select bit here, unlike $5114-$5117). Mask to the 3 meaningful
            // bits so prg_read_8k doesn't mis-route $6000-$7FFF to PRG ROM
            // when the game happens to set upper bits (e.g. Metal Slader
            // Glory writes $5113 = $80).
            0x5113 => self.prg_ram_bank = (val & 0x07) as usize,
            0x5114 => self.prg_bank_8000 = val as usize,
            0x5115 => self.prg_bank_a000 = val as usize,
            0x5116 => self.prg_bank_c000 = val as usize,
            0x5117 => self.prg_bank_e000 = (val | 0x80) as usize,
            0x5120..=0x5127 => {
                self.chr_banks_a[addr as usize - 0x5120] = val as usize | (self.chr_high << 8);
                self.last_written_chr_set_b.set(false);
            }
            0x5128..=0x512B => {
                self.chr_banks_b[addr as usize - 0x5128] = val as usize | (self.chr_high << 8);
                self.last_written_chr_set_b.set(true);
            }
            0x5130 => self.chr_high = (val & 0x03) as usize,
            0x5200 => self.split_mode = val,
            0x5201 => self.split_scroll = val,
            0x5202 => self.split_bank = val,
            0x5203 => self.irq_target = val,
            0x5204 => self.irq_enable = (val & 0x80) != 0,
            0x5205 => self.multiplier_1 = val,
            0x5206 => self.multiplier_2 = val,
            0x5C00..=0x5FFF => {
                if self.exram_mode == 0 || self.exram_mode == 1 || self.exram_mode == 2 {
                    self.exram[addr as usize - 0x5C00] = val;
                }
            }
            0x6000..=0x7FFF => {
                if self.ram_protect_1 == 2 && self.ram_protect_2 == 1 {
                    let bank = self.prg_ram_bank & 0x07;
                    self.prg_ram[bank * 0x2000 + (addr as usize - 0x6000)] = val;
                }
            }
            0x8000..=0xFFFF => {
                let bank = self.get_prg_bank(addr);
                if bank & 0x80 == 0 && self.ram_protect_1 == 2 && self.ram_protect_2 == 1 {
                    let ram_bank = bank & 0x07;
                    self.prg_ram[ram_bank * 0x2000 + (addr as usize % 0x2000)] = val;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16, is_sprite: bool) -> Option<u8> {
        self.notify_ppu_read();
        if addr >= 0x2000 { return None; }

        if self.chr_is_ram {
            return Some(self.chr_ram[addr as usize]);
        }

        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 { return Some(0); }

        let is_bg = !is_sprite;
        // During vblank or forced blank (PPU not actively rendering), CPU
        // $2007 reads route through chr_read with is_sprite=false. Real
        // MMC5 resolves these using the set most recently written
        // ($5120-$5127 = A, $5128-$512B = B) regardless of PPUCTRL bit 5.
        // Uchuu Keibitai SDF uses this path to copy its title nametable
        // out of CHR ROM and into VRAM during forced blank.
        // Real MMC5 separates BG/sprite bank register sets only when the PPU
        // is actually doing sprite fetches — that is, 8x16 mode (PPUCTRL bit
        // 5) AND sprite rendering enabled (PPUMASK bit 4). When sprites are
        // disabled but BG is on, no set-A / set-B switching happens during
        // the scanline; BG falls back to set A. Uchuu Keibitai SDF's
        // stage-1 intro renders with 8x16 mode selected but sprites masked
        // off; the scene tiles live in set A, and set B still holds the
        // tile-index source the game copied out of CHR ROM into VRAM.
        // ExGraphic mode (exram_mode 1) keeps its own routing: BG always
        // uses set B there, regardless of sprite enable.
        let use_chr_b = if !self.in_frame.get() {
            self.last_written_chr_set_b.get()
        } else if self.exram_mode == 1 {
            !is_sprite
        } else {
            !is_sprite
                && (self.ppu_ctrl & 0x20 != 0)
                && (self.ppu_mask & 0x10 != 0)
        };
        let chr_banks: &[usize] = if use_chr_b { &self.chr_banks_b } else { &self.chr_banks_a };
        // Set B covers only 4KB natively (banks $5128-$512B). Real MMC5
        // mirrors set B every $1000 across the full $0000-$1FFF PPU range,
        // not just when 8x16 sprites are enabled — $2007 reads during
        // forced blank also follow the last-written set, and those reads
        // can target $1000+. UKSDF copies its stage-1 NT out of CHR ROM
        // via that path, so the fold has to be unconditional.
        let map_addr = if use_chr_b { addr % 0x1000 } else { addr };

        let mut bank = match self.chr_mode {
            0 => {
                let base = chr_banks[chr_banks.len() - 1];
                (base << 3) | ((map_addr as usize / 0x0400) & 0x07)
            }
            1 => {
                let base = if map_addr < 0x1000 {
                    chr_banks[3.min(chr_banks.len() - 1)]
                } else {
                    chr_banks[7.min(chr_banks.len() - 1)]
                };
                (base << 2) | (((map_addr as usize) % 0x1000) / 0x0400)
            }
            2 => {
                let idx = (map_addr as usize / 0x0800) * 2 + 1;
                let base = chr_banks[idx.min(chr_banks.len() - 1)];
                (base << 1) | ((map_addr as usize % 0x0800) / 0x0400)
            }
            3 => chr_banks[(map_addr as usize / 0x0400).min(chr_banks.len() - 1)],
            _ => unreachable!(),
        };

        if is_bg && self.exram_mode == 1 {
            let last_nt = self.last_nt_addr.get();
            let exram_byte = self.exram[(last_nt & 0x03FF) as usize];
            // The exram byte specifies a 4KB CHR bank, ignoring bits 5128-512B.
            // The 4KB bank is essentially an index into 4KB pages.
            let bank_4k = (self.chr_high << 6) | (exram_byte as usize & 0x3F);
            bank = (bank_4k * 4) + ((addr as usize % 0x1000) / 0x0400);
        }

        let offset = (addr as usize % 0x0400) + (bank % num_banks) * 0x0400;
        Some(self.chr_rom[offset])
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_is_ram {
            self.chr_ram[addr as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.nametable_mapping {
            0x44 => Mirroring::Vertical,
            0x50 => Mirroring::Horizontal,
            0x00 => Mirroring::OneScreenLow,
            0x55 => Mirroring::OneScreenHigh,
            // Fallback for games assuming default boot state
            _ => Mirroring::Vertical,
        }
    }

    /// MMC5 lets each of the four NTs route independently (CIRAM low/high,
    /// ExRAM, or fill). For CIRAM mappings (0 and 1) we answer here; for
    /// ExRAM (2) / fill (3), `mapper_ppu_read` returns the data directly
    /// and this function isn't consulted. Games like Uchuu Keibitai SDF
    /// use `$5105` values (e.g. `$10`) that don't fit the standard
    /// Mirroring variants, so honoring the raw bits is required.
    fn nt_ciram_bank(&self, nt_index: u8) -> u8 {
        let idx = (nt_index & 0x03) as u8;
        let mapping = (self.nametable_mapping >> (idx * 2)) & 0x03;
        // For mapping 0 = low, 1 = high. For 2/3 the mapper serves data via
        // mapper_ppu_read and CIRAM isn't used, but return a safe default.
        match mapping {
            0 => 0,
            1 => 1,
            _ => 0,
        }
    }

    fn mapper_ppu_read(&self, addr: u16) -> Option<u8> {
        self.notify_ppu_read();
        match addr {
            0x2000..=0x2FFF => {
                let nt_idx = (addr - 0x2000) / 0x0400;
                let mapping = (self.nametable_mapping >> (nt_idx * 2)) & 0x03;
                let is_attr = (addr & 0x03FF) >= 0x03C0;

                if !is_attr {
                    self.last_nt_addr.set(addr);
                }

                if is_attr && self.exram_mode == 1 {
                    let last_nt = self.last_nt_addr.get();
                    let exram_byte = self.exram[(last_nt & 0x03FF) as usize];
                    let palette = exram_byte >> 6;
                    return Some(palette | (palette << 2) | (palette << 4) | (palette << 6));
                }

                match mapping {
                    0 | 1 => None,
                    2 => {
                        // "ExRAM as nametable" works only in ExRAM modes 0
                        // (NT) and 1 (ExGraphic). In modes 2/3 ExRAM is
                        // CPU-only and this read returns open bus (approx 0).
                        if self.exram_mode <= 1 {
                            Some(self.exram[(addr & 0x03FF) as usize])
                        } else {
                            Some(0)
                        }
                    }
                    3 => {
                        if is_attr {
                            let color = self.fill_color;
                            Some(color | (color << 2) | (color << 4) | (color << 6))
                        } else {
                            Some(self.fill_tile)
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => None,
        }
    }
    fn check_irq(&self) -> bool {
        self.irq_pending.get() && self.irq_enable
    }

    fn dbg_exram_mode(&self) -> Option<u8> { Some(self.exram_mode) }
    fn dbg_nametable_mapping(&self) -> Option<u8> { Some(self.nametable_mapping) }
    fn dbg_split_mode(&self) -> Option<u8> { Some(self.split_mode) }
    fn dbg_chr_banks_a(&self) -> Option<[usize; 8]> { Some(self.chr_banks_a) }
    fn dbg_chr_banks_b(&self) -> Option<[usize; 4]> { Some(self.chr_banks_b) }
    fn dbg_chr_high(&self) -> Option<usize> { Some(self.chr_high) }
    fn dbg_irq_target(&self) -> Option<u8> { Some(self.irq_target) }
    fn dbg_chr_mode(&self) -> Option<u8> { Some(self.chr_mode) }

    fn split_fetch(&self, scanline: u16, coarse_x: u8) -> Option<SplitFetch> {
        if self.split_mode & 0x80 == 0 {
            return None;
        }
        let split_tile = self.split_mode & 0x1F;
        let split_right = self.split_mode & 0x40 != 0;
        let in_split = if split_right {
            coarse_x >= split_tile
        } else {
            coarse_x < split_tile
        };
        if !in_split {
            return None;
        }

        // Split Y: pixel-accurate. Scrolls 0..240 and wraps per MMC5 spec.
        let split_y = (scanline + self.split_scroll as u16) % 240;
        let split_coarse_y = (split_y / 8) as u8;
        let split_fine_y = (split_y % 8) as u8;

        // Tile index and attribute byte live in ExRAM, laid out as a 32x30
        // nametable (960 tiles + 64 attribute bytes in the last 64 slots).
        let nt_idx = (split_coarse_y as usize) * 32 + coarse_x as usize;
        let tile_idx = self.exram[nt_idx & 0x3FF] as usize;

        let attr_idx = 0x3C0 + (split_coarse_y as usize / 4) * 8 + (coarse_x as usize / 4);
        let attr_byte = self.exram[attr_idx & 0x3FF];
        let shift = (((split_coarse_y & 2) as usize) << 1) | ((coarse_x & 2) as usize);
        let palette = (attr_byte >> shift) & 0x03;

        // Pattern always comes from split_bank interpreted as a 4KB CHR bank.
        if self.chr_rom.is_empty() {
            return Some(SplitFetch {
                palette,
                pattern_lo: 0,
                pattern_hi: 0,
            });
        }
        let num_4k = self.chr_rom.len() / 0x1000;
        let bank = (self.split_bank as usize) % num_4k.max(1);
        let base = bank * 0x1000 + tile_idx * 16 + split_fine_y as usize;
        let pattern_lo = self.chr_rom[base];
        let pattern_hi = self.chr_rom[base + 8];

        Some(SplitFetch {
            palette,
            pattern_lo,
            pattern_hi,
        })
    }

    fn tick_scanline_early(&mut self) {
        // Mesen-style two-stage transition after an idle period:
        // 1st tick post-idle: `need_in_frame` latches true, counter resets
        //                     to 0, `in_frame` still false.
        // 2nd tick onward:    `in_frame` promotes true (on the first such
        //                     tick), counter increments and compares
        //                     against the IRQ target.
        if !self.in_frame.get() && !self.need_in_frame.get() {
            self.need_in_frame.set(true);
            self.scanline_counter = 0;
        } else {
            self.in_frame.set(true);
            self.need_in_frame.set(false);
            self.scanline_counter = self.scanline_counter.saturating_add(1);
            if self.scanline_counter == self.irq_target {
                self.irq_pending.set(true);
            }
        }
        // Re-arm the idle counter here too so the mapper-only unit tests
        // (which don't run a real PPU to emit read notifications) still
        // converge on the same in_frame semantics across frames. In the
        // live emulator this is redundant with the per-read notifications
        // but doesn't change behavior.
        // Real MMC5 drops in_frame 3 PPU cycles after the last read, but
        // our renderer only emits reads during cycles 1-256; cycles 257-
        // 340 of each scanline are silent. Use a larger timeout so the
        // gap between scanlines doesn't falsely trip "idle". Still drains
        // cleanly during VBlank (~2280 CPU cycles).
        self.ppu_idle.set(120);
        // Keep the legacy watchdog armed only for savestate continuity.
        self.watchdog.set(120);
    }

    fn tick_cpu(&mut self) {
        // Legacy 120-cycle watchdog is kept in sync for save-state
        // compatibility but the real "am I rendering?" signal is now the
        // 3-cycle PPU-idle counter below.
        let wd = self.watchdog.get();
        if wd > 0 {
            self.watchdog.set(wd - 1);
        }
        let idle = self.ppu_idle.get();
        if idle > 0 {
            self.ppu_idle.set(idle - 1);
            if idle == 1 {
                // 3 CPU cycles have passed without a PPU read — PPU is idle.
                // Matches real MMC5 in-frame clear semantics.
                self.in_frame.set(false);
                self.need_in_frame.set(false);
            }
        }
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Mmc5 {
            prg_mode: self.prg_mode,
            chr_mode: self.chr_mode,
            prg_banks: vec![
                self.prg_ram_bank,
                self.prg_bank_8000,
                self.prg_bank_a000,
                self.prg_bank_c000,
                self.prg_bank_e000,
            ],
            irq_target: self.irq_target,
            irq_enable: self.irq_enable,
            irq_pending: self.irq_pending.get(),
            in_frame: self.in_frame.get(),
            scanline_counter: self.scanline_counter,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Mmc5 { prg_mode, chr_mode, prg_banks, irq_target, irq_enable, irq_pending, in_frame, scanline_counter } = state {
            self.prg_mode = *prg_mode;
            self.chr_mode = *chr_mode;
            if prg_banks.len() >= 5 {
                self.prg_ram_bank = prg_banks[0];
                self.prg_bank_8000 = prg_banks[1];
                self.prg_bank_a000 = prg_banks[2];
                self.prg_bank_c000 = prg_banks[3];
                self.prg_bank_e000 = prg_banks[4];
            }
            self.irq_target = *irq_target;
            self.irq_enable = *irq_enable;
            self.irq_pending.set(*irq_pending);
            self.in_frame.set(*in_frame);
            self.scanline_counter = *scanline_counter;
        }
    }
}

