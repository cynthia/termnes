pub mod palette;
pub mod render;

use crate::cartridge::Cartridge;
use crate::savestate::PpuState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    OneScreenLow,
    OneScreenHigh,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Sprite0Debug {
    pub scanline: i16,
    pub oam_y: u8,
    pub oam_tile: u8,
    pub oam_attr: u8,
    pub oam_x: u8,
    pub row_used: u16,
    pub lo: u8,
    pub hi: u8,
    pub mask_at_check: u8,
    pub bg_enabled: bool,
    pub spr_enabled: bool,
    /// Whether any opaque sprite-0 pixel was written to spr_px this scanline.
    pub had_opaque: bool,
    /// First x (0-255) where sprite-0 had an opaque pixel this scanline, or 256.
    pub first_opaque_x: u16,
    /// Whether any opaque BG pixel overlapped on this scanline.
    pub had_opaque_bg_at_opaque_spr: bool,
    /// Whether the hit fired on this scanline.
    pub fired: bool,
}

pub struct Ppu {
    pub ctrl: u8,            // $2000
    pub mask: u8,            // $2001
    pub status: u8,          // $2002
    pub oam_addr: u8,        // $2003
    pub vram_addr: u16,      // v — current rendering VRAM address
    pub temp_vram_addr: u16, // t — temporary VRAM address (latched by $2000/$2005/$2006)
    pub fine_x: u8,          // x — fine X scroll (0-7)
    pub write_latch: bool,   // toggle for $2005/$2006
    pub data_buffer: u8,     // $2007 read buffer
    pub oam: [u8; 256],      // sprite OAM
    pub vram: [u8; 2048],    // 2 nametables
    pub palette: [u8; 32],   // palette RAM
    pub chr_ram: [u8; 8192], // for UNROM (VRAM, not VROM)
    pub scanline: i16,
    pub cycle: u16,
    pub nmi_triggered: bool,
    pub frame_complete: bool,
    pub framebuffer: [u8; 256 * 240 * 3], // RGB framebuffer
    pub mirroring: Mirroring,

    // ── Debug instrumentation for sprite-0-hit analysis ─────────────────────
    pub dbg_sprite0_collected: u64,
    pub dbg_sprite0_opaque_scanlines: u64,
    pub dbg_sprite0_hits: u64,
    /// Scanlines where sprite-0 had an opaque pixel but the BG at that same pixel was transparent.
    pub dbg_sprite0_opaque_but_bg_transparent: u64,
    /// Counts of PPUSCROLL writes during "visible" rendering (SL 0-239) vs during VBlank/pre-render.
    pub dbg_scroll_writes_visible: u64,
    pub dbg_scroll_writes_vblank: u64,
    pub dbg_scroll_writes_visible_rendering_on: u64,
    pub dbg_scroll_writes_visible_rendering_off: u64,
    pub dbg_mask_writes_visible: u64,
    pub dbg_ctrl_writes_visible: u64,
    pub dbg_addr_writes_visible: u64,
    pub dbg_last_sprite0: Option<Sprite0Debug>,
    pub dbg_last_sprite0_hit: Option<Sprite0Debug>,
    pub odd_frame: bool,
    pub last_write: u8,
    pub render_vram_addr: u16,
    pub render_fine_x: u8,
    pub render_was_enabled: bool,
}

impl Ppu {
    pub fn new(mirroring: Mirroring) -> Self {
        Self {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            vram_addr: 0,
            temp_vram_addr: 0,
            fine_x: 0,
            write_latch: false,
            data_buffer: 0,
            oam: [0; 256],
            vram: [0; 2048],
            palette: [0; 32],
            chr_ram: [0; 8192],
            scanline: -1,
            cycle: 0,
            nmi_triggered: false,
            frame_complete: false,
            framebuffer: [0; 256 * 240 * 3],
            mirroring,
            dbg_sprite0_collected: 0,
            dbg_sprite0_opaque_scanlines: 0,
            dbg_sprite0_hits: 0,
            dbg_sprite0_opaque_but_bg_transparent: 0,
            dbg_scroll_writes_visible: 0,
            dbg_scroll_writes_vblank: 0,
            dbg_scroll_writes_visible_rendering_on: 0,
            dbg_scroll_writes_visible_rendering_off: 0,
            dbg_mask_writes_visible: 0,
            dbg_ctrl_writes_visible: 0,
            dbg_addr_writes_visible: 0,
            dbg_last_sprite0: None,
            dbg_last_sprite0_hit: None,
            odd_frame: false,
            last_write: 0,
            render_vram_addr: 0,
            render_fine_x: 0,
            render_was_enabled: false,
        }
    }

    // ── PPU internal memory access ───────────────────────────────────────────

    /// Reads from PPU address space: pattern tables, nametables, palettes.
    pub fn ppu_read(&self, addr: u16, cartridge: &Cartridge, is_sprite: bool) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => cartridge.chr_read(addr, is_sprite),
            0x2000..=0x3EFF => {
                let mirrored = self.mirror_nametable(addr, cartridge);
                self.vram[mirrored]
            }
            0x3F00..=0x3FFF => {
                let mut idx = (addr & 0x1F) as usize;
                // $3F10/$3F14/$3F18/$3F1C mirror $3F00/$3F04/$3F08/$3F0C
                if idx >= 0x10 && idx & 0x03 == 0 {
                    idx &= 0x0F;
                }
                self.palette[idx]
            }
            _ => 0,
        }
    }

    /// Writes to PPU address space.
    pub fn ppu_write(&mut self, addr: u16, val: u8, cartridge: &mut Cartridge) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => cartridge.chr_write(addr, val),
            0x2000..=0x3EFF => {
                let mirrored = self.mirror_nametable(addr, cartridge);
                self.vram[mirrored] = val;
            }
            0x3F00..=0x3FFF => {
                let mut idx = (addr & 0x1F) as usize;
                if idx >= 0x10 && idx & 0x03 == 0 {
                    idx &= 0x0F;
                }
                self.palette[idx] = val;
            }
            _ => {}
        }
    }

    /// Maps a nametable address ($2000-$3EFF) to a VRAM index using the
    /// cartridge mirroring mode. Returns index into `self.vram`.
    fn mirror_nametable(&self, addr: u16, cartridge: &Cartridge) -> usize {
        let addr = (addr - 0x2000) & 0x0FFF;
        let table = (addr / 0x0400) as usize; // 0-3
        let offset = (addr % 0x0400) as usize;
        let mapped = match cartridge.mirroring() {
            Mirroring::Horizontal => [0, 0, 1, 1][table],
            Mirroring::Vertical   => [0, 1, 0, 1][table],
            Mirroring::OneScreenLow => [0, 0, 0, 0][table],
            Mirroring::OneScreenHigh => [1, 1, 1, 1][table],
        };
        mapped * 0x0400 + offset
    }

    // ── CPU-facing register reads ($2002, $2004, $2007) ──────────────────────

    /// $2002 STATUS — returns status byte, clears VBlank flag and write latch.
    pub fn read_status(&mut self) -> u8 {
        let result = (self.status & 0xE0) | (self.last_write & 0x1F);
        self.status &= !0x80; // clear VBlank flag
        self.write_latch = false;
        result
    }

    /// $2002 STATUS — returns status byte, clears VBlank flag and write latch.

    pub fn read_oam_data(&self) -> u8 {
        self.oam[self.oam_addr as usize]
    }

    /// $2007 DATA — buffered VRAM read; palette reads are immediate.
    pub fn read_data(&mut self, cartridge: &Cartridge) -> u8 {
        let addr = self.vram_addr & 0x3FFF;
        let inc = if self.ctrl & 0x04 != 0 { 32 } else { 1 };
        self.vram_addr = self.vram_addr.wrapping_add(inc);

        if addr >= 0x3F00 {
            // Palette: immediate return; buffer gets the nametable mirror below
            self.data_buffer = self.ppu_read(addr - 0x1000, cartridge, false);
            self.ppu_read(addr, cartridge, false)
        } else {
            let result = self.data_buffer;
            self.data_buffer = self.ppu_read(addr, cartridge, false);
            result
        }
    }

    // ── CPU-facing register writes ───────────────────────────────────────────

    /// $2000 CTRL
    pub fn write_ctrl(&mut self, val: u8) {
        self.last_write = val;
        if self.scanline >= 0 && self.scanline < 240 {
            self.dbg_ctrl_writes_visible += 1;
        }
        let old_nmi_enabled = (self.ctrl & 0x80) != 0;
        self.ctrl = val;
        let new_nmi_enabled = (val & 0x80) != 0;

        // If NMI is newly enabled and we are currently in VBlank, trigger an NMI immediately.
        // This is critical for games that wait for VBlank before enabling NMIs.
        if !old_nmi_enabled && new_nmi_enabled && (self.status & 0x80 != 0) {
            self.nmi_triggered = true;
        }

        // Update nametable select bits in temp_vram_addr
        self.temp_vram_addr = (self.temp_vram_addr & 0xF3FF) | ((val as u16 & 0x03) << 10);
    }

    /// $2001 MASK
    pub fn write_mask(&mut self, val: u8) {
        self.last_write = val;
        if self.scanline >= 0 && self.scanline < 240 {
            self.dbg_mask_writes_visible += 1;
        }
        self.mask = val;
    }

    /// $2003 OAM ADDR
    pub fn write_oam_addr(&mut self, val: u8) {
        self.last_write = val;
        self.oam_addr = val;
    }

    /// $2004 OAM DATA — writes byte to OAM, increments OAM address.
    /// Attribute bytes (OAM index % 4 == 2) have bits 2-4 hardwired to 0 on
    /// real hardware, so mask them out on write.
    pub fn write_oam_data(&mut self, val: u8) {
        self.last_write = val;
        let idx = self.oam_addr as usize;
        let masked = if idx % 4 == 2 { val & 0xE3 } else { val };
        self.oam[idx] = masked;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    /// $2005 SCROLL — first write = X (coarse X + fine X), second write = Y
    /// (coarse Y + fine Y). Values are latched into `t` (not v) so mid-frame
    /// writes only affect rendering after the next horizontal/vertical copy.
    pub fn write_scroll(&mut self, val: u8) {
        self.last_write = val;
        if self.scanline >= 0 && self.scanline < 240 {
            self.dbg_scroll_writes_visible += 1;
            if self.mask & 0x18 != 0 {
                self.dbg_scroll_writes_visible_rendering_on += 1;
            } else {
                self.dbg_scroll_writes_visible_rendering_off += 1;
            }
        } else {
            self.dbg_scroll_writes_vblank += 1;
        }
        if !self.write_latch {
            // t: ....... ...ABCDE <- d: ABCDE... ;  x: <- d: .....FGH
            self.fine_x = val & 0x07;
            if self.scanline >= 0 && self.scanline < 240 && (self.mask & 0x18) != 0 {
                self.render_fine_x = self.fine_x;
            }
            self.temp_vram_addr = (self.temp_vram_addr & 0xFFE0) | (val as u16 >> 3);
            self.write_latch = true;
        } else {
            // t: FGH..AB CDE..... <- d: ABCDEFGH
            self.temp_vram_addr = (self.temp_vram_addr & 0x8FFF) | ((val as u16 & 0x07) << 12);
            self.temp_vram_addr = (self.temp_vram_addr & 0xFC1F) | ((val as u16 & 0xF8) << 2);
            self.write_latch = false;
        }
    }

    /// $2006 ADDR — first write = high byte, second write = low byte.
    pub fn write_addr(&mut self, val: u8) {
        self.last_write = val;
        if self.scanline >= 0 && self.scanline < 240 {
            self.dbg_addr_writes_visible += 1;
        }
        if !self.write_latch {
            self.temp_vram_addr = (self.temp_vram_addr & 0x00FF) | ((val as u16 & 0x3F) << 8);
            self.write_latch = true;
        } else {
            self.temp_vram_addr = (self.temp_vram_addr & 0xFF00) | val as u16;
            self.vram_addr = self.temp_vram_addr;
            self.write_latch = false;
        }
    }

    /// $2007 DATA — writes to PPU memory at current VRAM address, then increments.
    pub fn write_data(&mut self, val: u8, cartridge: &mut Cartridge) {
        self.last_write = val;
        let addr = self.vram_addr & 0x3FFF;

        self.ppu_write(addr, val, cartridge);
        let inc = if self.ctrl & 0x04 != 0 { 32 } else { 1 };
        self.vram_addr = self.vram_addr.wrapping_add(inc);
    }

    // ── Bus-facing dispatch ($2000-$2007) ────────────────────────────────────

    /// Called by Bus for CPU reads from $2000-$2007 (mirrored).
    pub fn cpu_read(&mut self, addr: u16, cartridge: &Cartridge) -> u8 {
        match addr & 0x0007 {
            2 => self.read_status(),
            4 => self.read_oam_data(),
            7 => self.read_data(cartridge),
            _ => 0, // write-only registers return open bus (approximate as 0)
        }
    }

    /// Called by Bus for CPU writes to $2000-$2007 (mirrored).
    pub fn cpu_write(&mut self, addr: u16, val: u8, cartridge: &mut Cartridge) {
        match addr & 0x0007 {
            0 => self.write_ctrl(val),
            1 => self.write_mask(val),
            3 => self.write_oam_addr(val),
            4 => self.write_oam_data(val),
            5 => self.write_scroll(val),
            6 => self.write_addr(val),
            7 => self.write_data(val, cartridge),
            _ => {}
        }
    }

    pub fn capture_state(&self) -> PpuState {
        PpuState {
            ctrl: self.ctrl, mask: self.mask, status: self.status,
            oam_addr: self.oam_addr, vram_addr: self.vram_addr,
            temp_vram_addr: self.temp_vram_addr, fine_x: self.fine_x,
            write_latch: self.write_latch, data_buffer: self.data_buffer,
            oam: self.oam.to_vec(), vram: self.vram.to_vec(),
            palette: self.palette.to_vec(), chr_ram: self.chr_ram.to_vec(),
            scanline: self.scanline, cycle: self.cycle,
            nmi_triggered: self.nmi_triggered, odd_frame: self.odd_frame,
            last_write: self.last_write,
        }
    }

    pub fn restore_state(&mut self, s: &PpuState) {
        self.ctrl = s.ctrl; self.mask = s.mask; self.status = s.status;
        self.oam_addr = s.oam_addr; self.vram_addr = s.vram_addr;
        self.temp_vram_addr = s.temp_vram_addr; self.fine_x = s.fine_x;
        self.write_latch = s.write_latch; self.data_buffer = s.data_buffer;
        if s.oam.len() == self.oam.len() { self.oam.copy_from_slice(&s.oam); }
        if s.vram.len() == self.vram.len() { self.vram.copy_from_slice(&s.vram); }
        if s.palette.len() == self.palette.len() { self.palette.copy_from_slice(&s.palette); }
        if s.chr_ram.len() == self.chr_ram.len() { self.chr_ram.copy_from_slice(&s.chr_ram); }
        self.scanline = s.scanline; self.cycle = s.cycle;
        self.nmi_triggered = s.nmi_triggered; self.odd_frame = s.odd_frame;
        self.last_write = s.last_write;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;

    fn make_cart() -> Cartridge {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1); // 1 PRG bank
        rom.push(1); // 1 CHR bank
        rom.push(0x00);
        rom.push(0x00);
        rom.extend_from_slice(&[0u8; 8]);
        rom.extend(vec![0u8; 0x4000]);
        rom.extend(vec![0u8; 0x2000]);
        Cartridge::from_ines(&rom).unwrap()
    }

    // ── Nametable mirroring ───────────────────────────────────────────────────

    #[test]
    fn horizontal_mirroring() {
        let ppu = Ppu::new(Mirroring::Horizontal);
        let cart = make_cart();
        // NT0 ($2000) and NT1 ($2400) share physical bank 0
        assert_eq!(ppu.mirror_nametable(0x2000, &cart), ppu.mirror_nametable(0x2400, &cart));
        // NT2 ($2800) and NT3 ($2C00) share physical bank 1
        assert_eq!(ppu.mirror_nametable(0x2800, &cart), ppu.mirror_nametable(0x2C00, &cart));
        // NT0 and NT2 are different physical banks
        assert_ne!(ppu.mirror_nametable(0x2000, &cart), ppu.mirror_nametable(0x2800, &cart));
    }

    #[test]
    fn vertical_mirroring() {
        // Set cart mirroring to vertical
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1); rom.push(1);
        rom.push(0x01); // Vertical
        rom.push(0x00);
        rom.extend_from_slice(&[0u8; 8]);
        rom.extend(vec![0u8; 0x4000]);
        rom.extend(vec![0u8; 0x2000]);
        let cart = Cartridge::from_ines(&rom).unwrap();

        let ppu = Ppu::new(Mirroring::Vertical);
        // NT0 ($2000) and NT2 ($2800) share physical bank 0
        assert_eq!(ppu.mirror_nametable(0x2000, &cart), ppu.mirror_nametable(0x2800, &cart));
        // NT1 ($2400) and NT3 ($2C00) share physical bank 1
        assert_eq!(ppu.mirror_nametable(0x2400, &cart), ppu.mirror_nametable(0x2C00, &cart));
        // NT0 and NT1 are different physical banks
        assert_ne!(ppu.mirror_nametable(0x2000, &cart), ppu.mirror_nametable(0x2400, &cart));
    }

    #[test]
    fn nametable_offset_preserved() {
        let ppu = Ppu::new(Mirroring::Horizontal);
        let cart = make_cart();
        let base = ppu.mirror_nametable(0x2000, &cart);
        assert_eq!(ppu.mirror_nametable(0x23FF, &cart), base + 0x3FF);
    }

    #[test]
    fn mirror_range_3000_maps_to_2000() {
        let ppu = Ppu::new(Mirroring::Vertical);
        let cart = make_cart();
        // $3000 should map the same as $2000 (it's in $3000-$3EFF mirror range)
        // ppu_read masks with 0x2FFF before calling mirror_nametable
        assert_eq!(ppu.mirror_nametable(0x3000 & 0x2FFF, &cart), ppu.mirror_nametable(0x2000, &cart));
        assert_eq!(ppu.mirror_nametable(0x3400 & 0x2FFF, &cart), ppu.mirror_nametable(0x2400, &cart));
    }

    // ── Palette mirroring ─────────────────────────────────────────────────────

    #[test]
    fn palette_transparent_mirrors() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        let cart = make_cart();
        ppu.palette[0x00] = 0x3F;
        assert_eq!(ppu.ppu_read(0x3F00, &cart, false), 0x3F);
        assert_eq!(ppu.ppu_read(0x3F10, &cart, false), 0x3F); // mirrors $3F00
        ppu.palette[0x04] = 0x2A;
        assert_eq!(ppu.ppu_read(0x3F04, &cart, false), 0x2A);
        assert_eq!(ppu.ppu_read(0x3F14, &cart, false), 0x2A); // mirrors $3F04
    }

    #[test]
    fn palette_non_transparent_independent() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        let cart = make_cart();
        ppu.palette[0x01] = 0x11;
        ppu.palette[0x11] = 0x22;
        assert_eq!(ppu.ppu_read(0x3F01, &cart, false), 0x11);
        assert_eq!(ppu.ppu_read(0x3F11, &cart, false), 0x22);
    }

    #[test]
    fn palette_write_read_roundtrip() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        let mut cart = make_cart();
        ppu.ppu_write(0x3F05, 0x17, &mut cart);
        assert_eq!(ppu.ppu_read(0x3F05, &cart, false), 0x17);
    }

    // ── Register semantics ────────────────────────────────────────────────────

    #[test]
    fn ppustatus_read_clears_vblank_and_latch() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.status = 0xFF;
        ppu.write_latch = true;
        let val = ppu.read_status();
        assert_eq!(val & 0x80, 0x80);
        assert_eq!(ppu.status & 0x80, 0);
        assert!(!ppu.write_latch);
    }

    #[test]
    fn ppuscroll_double_write_latches_t_and_fine_x() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        // First write (X): 0x40 = coarse X 8, fine X 0
        ppu.write_scroll(0x40);
        // Second write (Y): 0x20 = coarse Y 4, fine Y 0
        ppu.write_scroll(0x20);
        assert_eq!(ppu.fine_x, 0);
        assert_eq!(ppu.temp_vram_addr & 0x001F, 8, "coarse X in t");
        assert_eq!((ppu.temp_vram_addr >> 5) & 0x1F, 4, "coarse Y in t");
        assert_eq!((ppu.temp_vram_addr >> 12) & 0x07, 0, "fine Y in t");
        assert!(!ppu.write_latch);

        // Non-zero fine X / fine Y case
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.write_scroll(0x7D); // X: coarse=15, fine=5
        ppu.write_scroll(0xB3); // Y: coarse=22, fine=3
        assert_eq!(ppu.fine_x, 5);
        assert_eq!(ppu.temp_vram_addr & 0x001F, 15);
        assert_eq!((ppu.temp_vram_addr >> 5) & 0x1F, 22);
        assert_eq!((ppu.temp_vram_addr >> 12) & 0x07, 3);
    }

    #[test]
    fn ppuaddr_double_write_sets_vram_addr() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.write_addr(0x21);
        ppu.write_addr(0x00);
        assert_eq!(ppu.vram_addr, 0x2100);
    }

    #[test]
    fn ppudata_write_increments_addr() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        let mut cart = make_cart();
        ppu.vram_addr = 0x2100;
        ppu.write_data(0xAB, &mut cart);
        assert_eq!(ppu.vram_addr, 0x2101);
    }

    #[test]
    fn ppudata_write_increment_by_32_when_ctrl_bit2() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        let mut cart = make_cart();
        ppu.ctrl = 0x04;
        ppu.vram_addr = 0x2000;
        ppu.write_data(0x55, &mut cart);
        assert_eq!(ppu.vram_addr, 0x2020);
    }
}
