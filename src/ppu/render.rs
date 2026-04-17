use super::{Ppu, Sprite0Debug};
use crate::cartridge::Cartridge;

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 240;
pub const SCANLINES_PER_FRAME: usize = 262; // 0-261 plus pre-render (-1)
pub const CYCLES_PER_SCANLINE: usize = 341; // PPU cycles per scanline
pub const VBLANK_SCANLINE: usize = 241;
pub const PRE_RENDER_SCANLINE: usize = 261;

/// Sprite data collected for one visible scanline.
struct SpriteRow {
    x: u8,
    lo: u8,          // low bit-plane byte for this tile row
    hi: u8,          // high bit-plane byte for this tile row
    palette: u8,     // sprite palette index (0-3)
    behind_bg: bool,
    flip_h: bool,
    is_sprite0: bool,
    // ── Debug only ──
    oam_y: u8,
    oam_tile: u8,
    oam_attr: u8,
    row_used: u16,
}

impl Ppu {
    /// Advance the PPU by one cycle. Called 3× per CPU cycle.
    pub fn tick(&mut self, cartridge: &mut Cartridge) {
        // ── Flag management at specific dots ────────────────────────────────
        if self.scanline == VBLANK_SCANLINE as i16 && self.cycle == 1 {
            self.status |= 0x80; // set VBlank
            if self.ctrl & 0x80 != 0 {
                self.nmi_triggered = true;
            }
        }
        if self.scanline == -1 && self.cycle == 1 {



            // Pre-render: clear VBlank, sprite-0 hit, sprite overflow
            self.status &= !0xE0;
        }

        // ── MMC3 IRQ counter ─────────────────────────────────────────────────
        if self.cycle == 260 && (self.scanline >= 0 && self.scanline < 240 || self.scanline == -1) && (self.mask & 0x18 != 0) {
             cartridge.tick_scanline();
        }

        // ── Advance cycle / scanline ─────────────────────────────────────────
        let rendering = (self.mask & 0x18) != 0;
        let visible = self.scanline >= 0 && self.scanline < SCREEN_HEIGHT as i16;
        let pre_render = self.scanline == -1;

        if self.cycle == 0 && visible {
            self.render_vram_addr = self.vram_addr;
            self.render_fine_x = self.fine_x;
        }
        if rendering && !self.render_was_enabled && visible {
            self.render_vram_addr = self.vram_addr;
            self.render_fine_x = self.fine_x;
        }
        self.render_was_enabled = rendering;

        if visible && self.cycle >= 1 && self.cycle <= 256 {
            self.render_pixel(self.scanline, self.cycle - 1, cartridge);
        }

        if self.cycle == 256 {
            if rendering && (visible || pre_render) {
                self.increment_coarse_y();
            }
        }

        // Dot 257: copy horizontal scroll bits from t to v (coarse X + NT X).
        if self.cycle == 257 && rendering && (visible || pre_render) {
            self.vram_addr = (self.vram_addr & !0x041F) | (self.temp_vram_addr & 0x041F);
        }

        // Pre-render only: dots 280-304 copy vertical scroll bits t -> v
        if pre_render && rendering && self.cycle >= 280 && self.cycle <= 304 {
            self.vram_addr = (self.vram_addr & !0x7BE0) | (self.temp_vram_addr & 0x7BE0);
        }

        self.cycle += 1;

        if self.cycle >= CYCLES_PER_SCANLINE as u16 {
            self.cycle = 0;
            self.scanline += 1;

            if self.scanline >= (SCANLINES_PER_FRAME - 1) as i16 {
                self.scanline = -1;
                self.frame_complete = true;
                self.odd_frame = !self.odd_frame;
            }
        }

        // Odd frame skip: skip cycle 340 of scanline -1 if rendering is enabled.
        if self.odd_frame && self.scanline == -1 && self.cycle == 340 && rendering {
            self.cycle = 0;
            self.scanline = 0;
        }
    }

    fn increment_render_coarse_x(&mut self) {
        if (self.render_vram_addr & 0x001F) == 31 {
            self.render_vram_addr &= !0x001F;
            self.render_vram_addr ^= 0x0400;
        } else {
            self.render_vram_addr += 1;
        }
    }

    fn bg_pixel(&self, x: u16, cartridge: &Cartridge) -> (u8, u8) {
        if self.mask & 0x08 == 0 || (x < 8 && self.mask & 0x02 == 0) {
            return (0, 0);
        }

        let nt_addr = 0x2000 | (self.render_vram_addr & 0x0FFF);
        let tile_idx = self.ppu_read(nt_addr, cartridge, false) as u16;

        let attr_addr = 0x23C0
            | (self.render_vram_addr & 0x0C00)
            | ((self.render_vram_addr >> 4) & 0x38)
            | ((self.render_vram_addr >> 2) & 0x07);
        let attr = self.ppu_read(attr_addr, cartridge, false);
        let shift = ((self.render_vram_addr >> 4) & 4) | (self.render_vram_addr & 2);
        let palette = ((attr >> shift) & 0x03) as u8;

        let fine_y = (self.render_vram_addr >> 12) & 0x07;
        let bg_pt_base: u16 = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0x0000 };
        let pt_addr = bg_pt_base + tile_idx * 16 + fine_y;
        let lo = self.ppu_read(pt_addr, cartridge, false);
        let hi = self.ppu_read(pt_addr + 8, cartridge, false);
        let bit = 7 - self.render_fine_x;
        let color = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
        (color, palette)
    }

    fn sprite_pixel(&mut self, scanline: i16, x: u16, cartridge: &Cartridge) -> (u8, u8, bool, bool, Option<Sprite0Debug>) {
        if self.mask & 0x10 == 0 || (x < 8 && self.mask & 0x04 == 0) {
            return (0, 0, false, false, None);
        }

        let spr_pt_base: u16 = if self.ctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
        let sprites = self.collect_sprites(scanline, spr_pt_base, cartridge);
        let mut debug = None;

        for spr in sprites.iter() {
            let sx = x as i16 - spr.x as i16;
            if !(0..8).contains(&sx) {
                continue;
            }

            let bit = if spr.flip_h { sx as u8 } else { 7 - sx as u8 };
            let lo = (spr.lo >> bit) & 1;
            let hi = (spr.hi >> bit) & 1;
            let color = (hi << 1) | lo;
            if color == 0 {
                continue;
            }

            if spr.is_sprite0 {
                debug = Some(Sprite0Debug {
                    scanline,
                    oam_y: spr.oam_y,
                    oam_tile: spr.oam_tile,
                    oam_attr: spr.oam_attr,
                    oam_x: spr.x,
                    row_used: spr.row_used,
                    lo: spr.lo,
                    hi: spr.hi,
                    mask_at_check: self.mask,
                    bg_enabled: self.mask & 0x08 != 0,
                    spr_enabled: self.mask & 0x10 != 0,
                    had_opaque: true,
                    first_opaque_x: x,
                    had_opaque_bg_at_opaque_spr: false,
                    fired: false,
                });
            }

            return (color, spr.palette, spr.behind_bg, spr.is_sprite0, debug);
        }

        (0, 0, false, false, None)
    }

    fn render_pixel(&mut self, scanline: i16, x: u16, cartridge: &Cartridge) {
        let y = scanline as u16;
        let (bg_color, bg_pal) = self.bg_pixel(x, cartridge);
        let (spr_color, spr_pal, spr_behind, is_spr0, mut spr0_dbg) =
            self.sprite_pixel(scanline, x, cartridge);

        if let Some(dbg) = spr0_dbg.as_mut() {
            self.dbg_sprite0_collected += 1;
            self.dbg_sprite0_opaque_scanlines += 1;
            if bg_color != 0 {
                dbg.had_opaque_bg_at_opaque_spr = true;
            } else {
                self.dbg_sprite0_opaque_but_bg_transparent += 1;
            }
        }

        if is_spr0 && spr_color != 0 && bg_color != 0 && x < 255 {
            self.status |= 0x40;
            if let Some(dbg) = spr0_dbg.as_mut() {
                dbg.had_opaque_bg_at_opaque_spr = true;
                dbg.fired = true;
                self.dbg_sprite0_hits += 1;
                self.dbg_last_sprite0_hit = Some(*dbg);
            }
        }

        if let Some(dbg) = spr0_dbg {
            self.dbg_last_sprite0 = Some(dbg);
        }

        let (pal_base, final_color) = if spr_color != 0 && (!spr_behind || bg_color == 0) {
            (0x3F10 + (spr_pal as u16) * 4, spr_color)
        } else if bg_color != 0 {
            (0x3F00 + (bg_pal as u16) * 4, bg_color)
        } else {
            (0x3F00, 0u8)
        };

        let pal_addr = if final_color == 0 {
            0x3F00u16
        } else {
            pal_base + final_color as u16
        };
        let entry = self.ppu_read(pal_addr, cartridge, false) & 0x3F;
        let (mut r, mut g, mut b) = crate::ppu::palette::NES_PALETTE[entry as usize];
        if self.mask & 0xE0 != 0 {
            let emp_r = self.mask & 0x20 != 0;
            let emp_g = self.mask & 0x40 != 0;
            let emp_b = self.mask & 0x80 != 0;

            if !emp_r { r = r.saturating_sub(r / 4); }
            if !emp_g { g = g.saturating_sub(g / 4); }
            if !emp_b { b = b.saturating_sub(b / 4); }
        }

        let i = (y as usize * SCREEN_WIDTH + x as usize) * 3;
        self.framebuffer[i] = r;
        self.framebuffer[i + 1] = g;
        self.framebuffer[i + 2] = b;

        if self.mask & 0x18 != 0 {
            self.render_fine_x += 1;
            if self.render_fine_x >= 8 {
                self.render_fine_x = 0;
                self.increment_render_coarse_x();
            }
        }
    }


    /// Increment the coarse-Y component of `v`, advancing fine Y first and
    /// wrapping coarse Y at 29 (toggles vertical nametable bit) / 31 (no toggle).
    fn increment_coarse_y(&mut self) {
        if (self.vram_addr & 0x7000) != 0x7000 {
            // Fine Y < 7: just increment fine Y.
            self.vram_addr = self.vram_addr.wrapping_add(0x1000);
        } else {
            // Fine Y = 7: reset fine Y, advance coarse Y.
            self.vram_addr &= !0x7000;
            let mut coarse_y = (self.vram_addr & 0x03E0) >> 5;
            if coarse_y == 29 {
                coarse_y = 0;
                self.vram_addr ^= 0x0800; // toggle vertical nametable bit
            } else if coarse_y == 31 {
                coarse_y = 0; // 30/31 are the attribute-table region; no NT toggle
            } else {
                coarse_y += 1;
            }
            self.vram_addr = (self.vram_addr & !0x03E0) | (coarse_y << 5);
        }
    }

    /// Collect up to 8 sprites whose Y range covers `scanline`.
    fn collect_sprites(
        &mut self,
        scanline: i16,
        pt_base: u16,
        cartridge: &Cartridge,
    ) -> Vec<SpriteRow> {
        let mut sprites = Vec::with_capacity(8);
        let tall = self.ctrl & 0x20 != 0; // 8×16 mode
        let sprite_h = if tall { 16i16 } else { 8i16 };

        let mut sprite_count = 0;
        for i in 0..64usize {
            // OAM: [Y, tile, attr, X] — Y is stored as screen_y - 1
            let oy = self.oam[i * 4] as i16 + 1;
            if scanline < oy || scanline >= oy + sprite_h {
                continue;
            }

            sprite_count += 1;
            if sprite_count > 8 {
                self.status |= 0x20;
                continue;
            }

            let tile = self.oam[i * 4 + 1];
            let attr = self.oam[i * 4 + 2];
            let ox = self.oam[i * 4 + 3];

            let flip_v = attr & 0x80 != 0;
            let flip_h = attr & 0x40 != 0;
            let behind_bg = attr & 0x20 != 0;
            let palette = attr & 0x03;

            let mut row = (scanline - oy) as u16;
            if flip_v {
                row = (sprite_h as u16 - 1) - row;
            }

            let (lo_addr, hi_addr) = if tall {
                // 8×16: tile bit 0 selects CHR bank; top half = base tile, bottom = base+1
                let bank: u16 = if tile & 0x01 != 0 { 0x1000 } else { 0x0000 };
                let base = (tile & 0xFE) as u16;
                let (t, r) = if row < 8 { (base, row) } else { (base + 1, row - 8) };
                let addr = bank + t * 16 + r;
                (addr, addr + 8)
            } else {
                let addr = pt_base + (tile as u16) * 16 + row;
                (addr, addr + 8)
            };

            sprites.push(SpriteRow {
                x: ox,
                lo: self.ppu_read(lo_addr, cartridge, true),
                hi: self.ppu_read(hi_addr, cartridge, true),
                palette,
                behind_bg,
                flip_h,
                is_sprite0: i == 0,
                oam_y: self.oam[i * 4],
                oam_tile: tile,
                oam_attr: attr,
                row_used: row,
            });
        }

        sprites
    }
}
