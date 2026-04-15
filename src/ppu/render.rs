use super::palette::NES_PALETTE;
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
        // MMC3 ticks its IRQ counter on PPU A12 transition (0 -> 1).
        // This usually happens at dot 260 of every visible scanline when BG
        // uses pattern table 0 ($0000) and sprites use pattern table 1 ($1000).
        if self.cycle == 260 && (self.scanline >= 0 && self.scanline < 240 || self.scanline == -1) && (self.mask & 0x18 != 0) {
             cartridge.tick_scanline();
        }

        // ── Advance cycle / scanline ─────────────────────────────────────────
        self.cycle += 1;

        let rendering = (self.mask & 0x18) != 0;
        let visible = self.scanline >= 0 && self.scanline < SCREEN_HEIGHT as i16;
        let pre_render = self.scanline == -1;

        // Render at dot 256 using the start-of-scanline `v` snapshot. Sprite-0
        // hit is surfaced here so the CPU can see it before the scanline ends.
        if self.cycle == 256 {
            if visible {
                self.render_scanline(self.scanline, cartridge);
            }
            if rendering && (visible || pre_render) {
                self.increment_coarse_y();
            }
        }

        // Dot 257: copy horizontal scroll bits from t to v (coarse X + NT X).
        if self.cycle == 257 && rendering && (visible || pre_render) {
            self.vram_addr = (self.vram_addr & !0x041F) | (self.temp_vram_addr & 0x041F);
        }

        // Pre-render only: dots 280-304 copy vertical scroll bits t -> v
        // (coarse Y + fine Y + NT Y).
        if pre_render && rendering && self.cycle >= 280 && self.cycle <= 304 {
            self.vram_addr = (self.vram_addr & !0x7BE0) | (self.temp_vram_addr & 0x7BE0);
        }

        if self.cycle >= CYCLES_PER_SCANLINE as u16 {
            self.cycle = 0;
            self.scanline += 1;

            // 262 scanlines per frame: pre-render (-1), visible (0-239),
            // post-render (240), VBlank (241-260). Wrap after scanline 260.
            if self.scanline >= (SCANLINES_PER_FRAME - 1) as i16 {
                self.scanline = -1;
                self.frame_complete = true;
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

    // ── Scanline renderer ────────────────────────────────────────────────────

    fn render_scanline(&mut self, scanline: i16, cartridge: &Cartridge) {
        let y = scanline as u16;
        let bg_enabled = self.mask & 0x08 != 0;
        let spr_enabled = self.mask & 0x10 != 0;

        let bg_pt_base: u16 = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0x0000 };
        let spr_pt_base: u16 = if self.ctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };

        // ── BG pixel buffer (33 tiles = 264 pixels; we slice 256 starting at fine_x)
        // Sourced from the snapshot of `v` at the start of this scanline; each tile
        // fetch increments a local v's coarse-X, wrapping the horizontal NT bit.
        let mut bg_buf: [(u8, u8); 264] = [(0, 0); 264];
        if bg_enabled {
            let mut v = self.vram_addr;
            for tile_i in 0..33usize {
                let nt_addr = 0x2000 | (v & 0x0FFF);
                let tile_idx = self.ppu_read(nt_addr, cartridge) as u16;

                let attr_addr = 0x23C0
                    | (v & 0x0C00)
                    | ((v >> 4) & 0x38)
                    | ((v >> 2) & 0x07);
                let attr = self.ppu_read(attr_addr, cartridge);
                let shift = ((v >> 4) & 4) | (v & 2);
                let palette = ((attr >> shift) & 0x03) as u8;

                let fine_y = (v >> 12) & 0x07;
                let pt_addr = bg_pt_base + tile_idx * 16 + fine_y;
                let lo = self.ppu_read(pt_addr, cartridge);
                let hi = self.ppu_read(pt_addr + 8, cartridge);

                for px in 0u8..8 {
                    let bit = 7 - px;
                    let color = (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1);
                    bg_buf[tile_i * 8 + px as usize] = (color, palette);
                }

                // Local coarse-X increment (31 → 0 toggles horizontal NT bit)
                if (v & 0x001F) == 31 {
                    v &= !0x001F;
                    v ^= 0x0400;
                } else {
                    v += 1;
                }
            }
        }

        // Collect up to 8 sprites visible on this scanline, then expand to pixels
        let sprites = self.collect_sprites(scanline, spr_pt_base, cartridge);

        // ── Debug: capture sprite-0 info if it's on this scanline ──
        let mut spr0_dbg: Option<Sprite0Debug> = None;
        if let Some(s0) = sprites.iter().find(|s| s.is_sprite0) {
            self.dbg_sprite0_collected += 1;
            spr0_dbg = Some(Sprite0Debug {
                scanline,
                oam_y: s0.oam_y,
                oam_tile: s0.oam_tile,
                oam_attr: s0.oam_attr,
                oam_x: s0.x,
                row_used: s0.row_used,
                lo: s0.lo,
                hi: s0.hi,
                mask_at_check: self.mask,
                bg_enabled,
                spr_enabled,
                had_opaque: false,
                first_opaque_x: 256,
                had_opaque_bg_at_opaque_spr: false,
                fired: false,
            });
        }

        // Per-pixel sprite buffer: (color, palette, behind_bg, is_sprite0).
        // Composited back-to-front: iterate from lowest priority (highest OAM
        // index) to highest (OAM index 0). Only OPAQUE pixels overwrite the
        // buffer — a higher-priority sprite's transparent pixel must never
        // erase a lower-priority sprite's opaque pixel.
        let mut spr_px: [(u8, u8, bool, bool); 256] = [(0, 0, false, false); 256];
        for spr in sprites.iter().rev() {
            for px in 0..8u16 {
                let sx = (spr.x as u16).wrapping_add(px);
                if sx >= 256 {
                    continue;
                }
                let bit = if spr.flip_h { px } else { 7 - px } as u8;
                let lo = (spr.lo >> bit) & 1;
                let hi = (spr.hi >> bit) & 1;
                let color = (hi << 1) | lo;

                if color != 0 {
                    spr_px[sx as usize] =
                        (color, spr.palette, spr.behind_bg, spr.is_sprite0);
                }

                // Debug: record sprite-0 opaque pixels regardless of whether
                // a higher-priority sprite ended up winning this pixel.
                if spr.is_sprite0 && color != 0 {
                    if let Some(dbg) = spr0_dbg.as_mut() {
                        if !dbg.had_opaque {
                            dbg.had_opaque = true;
                            dbg.first_opaque_x = sx;
                        }
                    }
                }
            }
        }

        let fx = self.fine_x as usize;
        for x in 0u16..256 {
            let (bg_color, bg_pal) = if bg_enabled {
                bg_buf[fx + x as usize]
            } else {
                (0, 0)
            };

            let (spr_color, spr_pal, spr_behind, is_spr0) = spr_px[x as usize];

            // Sprite-0 hit: opaque sprite-0 pixel overlaps opaque BG pixel
            // (x==255 is excluded per hardware spec)
            if is_spr0 && spr_enabled && bg_enabled && spr_color != 0 && bg_color != 0 && x < 255 {
                self.status |= 0x40;
                if let Some(dbg) = spr0_dbg.as_mut() {
                    dbg.had_opaque_bg_at_opaque_spr = true;
                    dbg.fired = true;
                }
            } else if is_spr0 && spr_color != 0 {
                if let Some(dbg) = spr0_dbg.as_mut() {
                    // Record that sprite 0 had an opaque pixel here, even if no hit.
                    if bg_color != 0 {
                        dbg.had_opaque_bg_at_opaque_spr = true;
                    }
                }
            }

            // Final pixel priority
            let (pal_base, final_color) = if spr_enabled
                && spr_color != 0
                && (!spr_behind || bg_color == 0)
            {
                // Sprite palettes at $3F10-$3F1F
                (0x3F10 + (spr_pal as u16) * 4, spr_color)
            } else if bg_enabled && bg_color != 0 {
                (0x3F00 + (bg_pal as u16) * 4, bg_color)
            } else {
                (0x3F00, 0u8) // backdrop / transparent
            };

            let pal_addr = if final_color == 0 {
                0x3F00u16
            } else {
                pal_base + final_color as u16
            };
            let entry = self.ppu_read(pal_addr, cartridge) & 0x3F;
            let (r, g, b) = NES_PALETTE[entry as usize];

            let i = (y as usize * SCREEN_WIDTH + x as usize) * 3;
            self.framebuffer[i] = r;
            self.framebuffer[i + 1] = g;
            self.framebuffer[i + 2] = b;
        }

        // ── Debug: commit sprite-0 record for this scanline ──
        if let Some(dbg) = spr0_dbg {
            if dbg.had_opaque {
                self.dbg_sprite0_opaque_scanlines += 1;
                if !dbg.had_opaque_bg_at_opaque_spr {
                    self.dbg_sprite0_opaque_but_bg_transparent += 1;
                }
            }
            if dbg.fired {
                self.dbg_sprite0_hits += 1;
                self.dbg_last_sprite0_hit = Some(dbg);
            }
            self.dbg_last_sprite0 = Some(dbg);
        }
    }

    /// Collect up to 8 sprites whose Y range covers `scanline`.
    fn collect_sprites(
        &self,
        scanline: i16,
        pt_base: u16,
        cartridge: &Cartridge,
    ) -> Vec<SpriteRow> {
        let mut sprites = Vec::with_capacity(8);
        let tall = self.ctrl & 0x20 != 0; // 8×16 mode
        let sprite_h = if tall { 16i16 } else { 8i16 };

        for i in 0..64usize {
            if sprites.len() >= 8 {
                break; // sprite overflow (flag not set for simplicity)
            }
            // OAM: [Y, tile, attr, X] — Y is stored as screen_y - 1
            let oy = self.oam[i * 4] as i16 + 1;
            if scanline < oy || scanline >= oy + sprite_h {
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
                lo: self.ppu_read(lo_addr, cartridge),
                hi: self.ppu_read(hi_addr, cartridge),
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
