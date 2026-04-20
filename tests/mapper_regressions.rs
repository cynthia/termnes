use termnes::bus::Bus;
use termnes::cartridge::mapper::{
    AxromMapper, Mapper, Mmc3Mapper, Mmc5Mapper, SunsoftFme7Mapper, Vrc6Mapper, Vrc6Variant,
};
use termnes::cartridge::Cartridge;

fn make_ines_rom(mapper_id: u8, prg_banks: u8, chr_banks: u8) -> Vec<u8> {
    let mut rom = Vec::new();
    rom.extend_from_slice(b"NES\x1A");
    rom.push(prg_banks);
    rom.push(chr_banks);
    rom.push(mapper_id << 4);
    rom.push(mapper_id & 0xF0);
    rom.extend_from_slice(&[0u8; 8]);
    rom.resize(
        16 + prg_banks as usize * 0x4000 + chr_banks as usize * 0x2000,
        0,
    );
    rom
}

#[test]
fn mmc5_registers_are_visible_on_cpu_bus() {
    let cart = Cartridge::from_ines(&make_ines_rom(5, 2, 1)).unwrap();
    let mut bus = Bus::new(cart);

    bus.cpu_write(0x5205, 13);
    bus.cpu_write(0x5206, 11);

    assert_eq!(bus.cpu_read(0x5205), 143);
    assert_eq!(bus.cpu_read(0x5206), 0);
}

#[test]
fn mmc5_keeps_c000_and_e000_prg_registers_separate() {
    let mut prg_rom = vec![0u8; 5 * 0x2000];
    for bank in 0..5 {
        prg_rom[bank * 0x2000] = bank as u8;
    }

    let mut mapper = Mmc5Mapper::new(prg_rom, vec![0; 0x2000]);
    mapper.cpu_write(0x5100, 3);
    mapper.cpu_write(0x5116, 0x82);
    mapper.cpu_write(0x5117, 4);

    assert_eq!(mapper.cpu_read(0xC000), Some(2));
    assert_eq!(mapper.cpu_read(0xE000), Some(4));
}

#[test]
fn mmc5_uses_chr_ram_when_chr_rom_is_absent() {
    let mut mapper = Mmc5Mapper::new(vec![0; 0x8000], Vec::new());
    mapper.chr_write(0x0123, 0x5A);
    assert_eq!(mapper.chr_read(0x0123, false), Some(0x5A));
}

/// Simulates Burai Fighter's pattern: two MMC3 IRQs per frame (top + bottom of
/// HUD) with the second IRQ armed from the first IRQ's handler. The key
/// property being tested: the second IRQ must fire exactly `latch2` scanlines
/// after the first one's reload, regardless of any intermediate $2007 palette
/// reads (which on real hardware don't clock the counter because of the
/// $3xxx→$2xxx mirror on nametable accesses).
#[test]
fn mmc3_second_irq_fires_at_expected_scanline_offset() {
    let mut mapper = Mmc3Mapper::new(vec![0u8; 0x8000], vec![0u8; 0x2000]);

    // Latch 1 = 10. First IRQ will fire 11 scanline ticks after the first
    // reload-trigger tick (1 reload + 10 decrements).
    mapper.cpu_write(0xC000, 10);
    mapper.cpu_write(0xC001, 0); // set reload flag
    mapper.cpu_write(0xE001, 0); // enable IRQ

    let mut first_irq_scanline = None;
    let mut second_irq_scanline = None;
    let mut second_irq_armed = false;

    // Simulate one frame: 241 scanline ticks (pre-render + 0..=239).
    for sl in 0..241i32 {
        mapper.tick_scanline();
        if mapper.check_irq() {
            if first_irq_scanline.is_none() {
                first_irq_scanline = Some(sl);
                // Arm IRQ 2 with latch 20. Ack the first IRQ.
                mapper.cpu_write(0xE000, 0); // disable+ack
                mapper.cpu_write(0xC000, 20);
                mapper.cpu_write(0xC001, 0); // reload flag
                mapper.cpu_write(0xE001, 0); // re-enable
                second_irq_armed = true;
            } else if second_irq_armed {
                second_irq_scanline = Some(sl);
                break;
            }
        }
    }

    let first = first_irq_scanline.expect("first IRQ should fire");
    let second = second_irq_scanline.expect("second IRQ should fire");
    let offset = second - first;
    // Expected: ~21 scanlines (1 reload tick + 20 decrements).
    assert_eq!(
        offset, 21,
        "second IRQ should fire 21 scanlines after the first"
    );
}

#[test]
fn mmc5_5113_ignores_upper_bits_for_prg_ram_select() {
    // On MMC5 $5113 is PRG-RAM-only — bits 3-7 are ignored (unlike
    // $5114-$5117, where bit 7 picks ROM vs RAM). Metal Slader Glory writes
    // $5113 = $80 and expects PRG-RAM bank 0; an emulator that treats bit 7
    // as a ROM selector would return PRG-ROM data at $6000-$7FFF and
    // corrupt game state.
    let mut prg_rom = vec![0xFFu8; 0x40000]; // 256KB — clearly not zero
    prg_rom[0] = 0xFF;
    let mut mapper = Mmc5Mapper::new(prg_rom, vec![0u8; 0x2000]);

    // Unlock PRG RAM writes ($5102 = 2, $5103 = 1) then write a known byte.
    mapper.cpu_write(0x5102, 2);
    mapper.cpu_write(0x5103, 1);
    mapper.cpu_write(0x6000, 0x5A);

    // Now write $5113 with upper bits set. Lower 3 bits = 0, so bank 0.
    mapper.cpu_write(0x5113, 0x80);

    assert_eq!(
        mapper.cpu_read(0x6000),
        Some(0x5A),
        "$5113 = $80 should still address PRG-RAM bank 0, not PRG-ROM"
    );
}

#[test]
fn mmc5_chr_read_outside_frame_uses_last_written_bank_set() {
    // On real MMC5, $2007 reads during vblank / forced blank resolve the
    // CHR bank using whichever bank register set ($5120-$5127 = A or
    // $5128-$512B = B) was most recently written — not PPUCTRL bit 5.
    // Uchuu Keibitai SDF relies on this to copy its title nametable out
    // of CHR ROM via $2007 during forced blank.
    let mut chr = vec![0u8; 0x4000]; // 16KB = 16 × 1KB banks
    // Put distinct sentinel bytes at offset 0 of 1KB banks 0 and 4 so we
    // can tell which set served a given read.
    chr[0x0000] = 0xA1; // bank 0 (set A's default target at $5120)
    chr[0x1000] = 0xB1; // bank 4 (set B's default target at $5128)

    let mut mapper = Mmc5Mapper::new(vec![0u8; 0x8000], chr);
    mapper.cpu_write(0x5101, 3); // chr_mode 3 (1KB banks)
    // PPUCTRL = 0 (8x8 sprites, so non-in-frame resolution uses the
    // "last written" rule, not PPUCTRL bit 5).
    mapper.cpu_write(0x2000, 0x00);

    // Program both sets so we can tell which one gets used.
    mapper.cpu_write(0x5120, 0); // set A bank 0 at $0000
    mapper.cpu_write(0x5128, 4); // set B bank 4 at $0000 (set B is only 4KB = 4 × 1KB)

    // Last write was to set B. With !in_frame (default on a fresh mapper),
    // chr_read($0000) should pick up set B's bank 4 → 0xB1.
    assert_eq!(
        mapper.chr_read(0x0000, false),
        Some(0xB1),
        "after $5128 write, out-of-frame read should use set B"
    );

    // Write set A last; out-of-frame read flips to set A's bank 0 → 0xA1.
    mapper.cpu_write(0x5120, 0);
    assert_eq!(
        mapper.chr_read(0x0000, false),
        Some(0xA1),
        "after $5120 write, out-of-frame read should use set A"
    );
}

#[test]
fn mmc5_per_nt_ciram_mapping_honors_5105_bits() {
    // $5105 = $10 (as used by Uchuu Keibitai SDF's title) gives:
    //   NT0=CIRAM-low, NT1=CIRAM-low, NT2=CIRAM-high, NT3=CIRAM-low.
    // Our older code bucketed this into the four-variant Mirroring enum
    // and landed on Vertical, which maps NT1->high (wrong).
    let mut mapper = Mmc5Mapper::new(vec![0u8; 0x8000], vec![0u8; 0x2000]);
    mapper.cpu_write(0x5105, 0x10);
    assert_eq!(mapper.nt_ciram_bank(0), 0);
    assert_eq!(mapper.nt_ciram_bank(1), 0, "NT1 should share CIRAM-low, not map to high");
    assert_eq!(mapper.nt_ciram_bank(2), 1);
    assert_eq!(mapper.nt_ciram_bank(3), 0, "NT3 should share CIRAM-low");

    // Sanity: the four conventional values still work.
    mapper.cpu_write(0x5105, 0x50); // horizontal
    assert_eq!(
        (0..4).map(|i| mapper.nt_ciram_bank(i)).collect::<Vec<_>>(),
        vec![0, 0, 1, 1]
    );
    mapper.cpu_write(0x5105, 0x44); // vertical
    assert_eq!(
        (0..4).map(|i| mapper.nt_ciram_bank(i)).collect::<Vec<_>>(),
        vec![0, 1, 0, 1]
    );
    mapper.cpu_write(0x5105, 0x00); // one-screen low
    assert!((0..4).all(|i| mapper.nt_ciram_bank(i) == 0));
    mapper.cpu_write(0x5105, 0x55); // one-screen high
    assert!((0..4).all(|i| mapper.nt_ciram_bank(i) == 1));
}

#[test]
fn mmc5_vertical_split_overrides_bg_in_split_region() {
    // Uchuu Keibitai SDF is the only commercial NES game that uses MMC5's
    // vertical split screen ($5200-$5202). Without it, the title screen
    // renders with wrong CHR. This test exercises the mapper-level piece:
    // given a split configured for "left side of column 16", a tile at
    // coarse_x=5 should return the split fetch data, and one at coarse_x=20
    // should not.
    let mut chr = vec![0u8; 0x2000]; // 8KB CHR = 2 x 4KB banks
    // 4KB bank 1 starts at 0x1000. Tile 0 in that bank: lo plane at
    // [0x1000..0x1008], hi plane at [0x1008..0x1010]. Mark each plane
    // distinctly so we can verify both pattern_lo and pattern_hi came from
    // this bank and not bank 0.
    for i in 0..8 {
        chr[0x1000 + i] = 0xAA;
        chr[0x1008 + i] = 0x55;
    }
    let mut mapper = Mmc5Mapper::new(vec![0u8; 0x8000], chr);

    // Put a recognizable tile index at ExRAM offset 0 so split's NT read lands on it.
    // ExRAM is CPU-writable in modes 0, 1, 2 — use mode 2 for simplicity, then
    // switch back to 0 if needed. The split path reads ExRAM directly regardless.
    mapper.cpu_write(0x5104, 2); // exram mode 2 (CPU R/W)
    mapper.cpu_write(0x5C00, 0); // ExRAM[0] = tile 0 (the one we pre-filled in bank 1)
    // Palette 0 at attribute offset 0x3C0 (top-left 4x4 tiles).
    mapper.cpu_write(0x5C00 + 0x3C0, 0x00);

    // Split config: enabled, LEFT side, split tile = 16. CHR bank = 1 (4KB).
    mapper.cpu_write(0x5200, 0x80 | 16);
    mapper.cpu_write(0x5201, 0); // Y scroll 0
    mapper.cpu_write(0x5202, 1); // 4KB bank 1

    // Inside the split region (coarse_x < 16): expect a SplitFetch.
    let inside = mapper.split_fetch(0, 5).expect("coarse_x 5 is in split");
    assert_eq!(inside.pattern_lo, 0xAA, "pattern_lo should come from bank 1");
    assert_eq!(inside.pattern_hi, 0x55, "pattern_hi should come from bank 1");

    // Outside: no override.
    assert!(
        mapper.split_fetch(0, 20).is_none(),
        "coarse_x 20 should fall outside a left-side split at col 16"
    );

    // Flipping bit 6 inverts the side.
    mapper.cpu_write(0x5200, 0x80 | 0x40 | 16);
    assert!(mapper.split_fetch(0, 5).is_none(), "after flip, col 5 is outside");
    assert!(
        mapper.split_fetch(0, 20).is_some(),
        "after flip, col 20 should be inside"
    );

    // Disabling the split makes it a no-op.
    mapper.cpu_write(0x5200, 0);
    assert!(mapper.split_fetch(0, 5).is_none());
    assert!(mapper.split_fetch(0, 20).is_none());
}

#[test]
fn mmc5_bg_fetch_uses_set_a_when_sprite_rendering_disabled() {
    // MMC5 separates BG/sprite bank register sets only when sprite rendering
    // is actually happening — nesdev wiki: "Only when Z is set and at least
    // one E bit is set does the MMC5 draw 8x16 sprites from eight independent
    // banks." The implication is that the "sprite bank set kicks in" part
    // requires PPUMASK bit 4 (show sprites). When sprites are masked off but
    // 8x16 is still selected in PPUCTRL, the PPU doesn't switch banks between
    // BG and sprite phases, so BG fetches stay on set A.
    //
    // Uchuu Keibitai SDF's stage-1 intro runs with PPUCTRL=$B0 (8x16 + NMI)
    // and PPUMASK=$1A (BG on, sprites off). The scene tiles live in set A;
    // set B holds the tile-index source the game already copied out of CHR
    // ROM into NT0 via $2007. With the naive rule, BG would erroneously
    // fetch from set B and render a mosaic of the tile-index bytes.
    let mut chr = vec![0u8; 0x4000]; // 16KB = 16 × 1KB banks
    chr[0x0000] = 0xA1; // bank 0 offset 0 (set A default)
    chr[0x1000] = 0xB1; // bank 4 offset 0 (set B default)
    let mut mapper = Mmc5Mapper::new(vec![0u8; 0x8000], chr);
    mapper.cpu_write(0x5101, 3); // chr_mode 3 (1KB banks)
    mapper.cpu_write(0x5120, 0); // set A[0] = bank 0 → tile there = 0xA1
    mapper.cpu_write(0x5128, 4); // set B[0] = bank 4 → tile there = 0xB1

    // Promote to in-frame by calling tick_scanline_early twice (mirrors the
    // Mesen two-stage need_in_frame → in_frame transition). CHR reads between
    // ticks keep ppu_idle armed so tick_cpu doesn't drop in_frame.
    let prime_in_frame = |m: &mut Mmc5Mapper| {
        m.tick_scanline_early();
        let _ = m.chr_read(0x0000, false);
        m.tick_scanline_early();
        let _ = m.chr_read(0x0000, false);
    };

    // 8x16 sprites ON (PPUCTRL bit 5), sprite rendering ON (PPUMASK bit 4):
    // BG should use set B.
    mapper.cpu_write(0x2000, 0x20);
    mapper.cpu_write(0x2001, 0x18); // bg+sprites on
    prime_in_frame(&mut mapper);
    assert_eq!(
        mapper.chr_read(0x0000, false),
        Some(0xB1),
        "BG fetch with 8x16 + sprites enabled should use set B"
    );

    // Same PPUCTRL, but PPUMASK bit 4 cleared (sprite rendering off). BG
    // reverts to set A because the sprite-bank-switching feature isn't
    // engaged for this scanline.
    mapper.cpu_write(0x2001, 0x08); // bg on, sprites off
    prime_in_frame(&mut mapper);
    assert_eq!(
        mapper.chr_read(0x0000, false),
        Some(0xA1),
        "with sprites masked off, 8x16 BG fetch should fall back to set A"
    );
}

#[test]
fn mmc5_scanline_irq_fires_every_frame() {
    // MMC5 distinguishes "in-frame" from "between frames" via a watchdog
    // that should drain during VBlank so the counter resets at each frame's
    // first tick. Without that reset, the scanline IRQ would fire only once
    // in the emulator's lifetime (regression seen in Castlevania III, where
    // the HUD/playfield CHR-bank split depends on a per-frame IRQ).
    let mut mapper = Mmc5Mapper::new(vec![0; 0x8000], vec![0; 0x2000]);

    // Enable IRQ, target scanline 10.
    mapper.cpu_write(0x5203, 10);
    mapper.cpu_write(0x5204, 0x80);

    let run_frame = |m: &mut Mmc5Mapper| -> bool {
        let mut fired = false;
        // 240 visible scanlines, each ~114 CPU cycles apart. In each CPU
        // cycle of a rendering scanline, nudge the mapper's PPU-idle
        // counter by issuing a CHR read (simulating the real PPU's bus
        // activity that the MMC5 watches to stay "in-frame").
        for _ in 0..240 {
            m.tick_scanline_early();
            if m.check_irq() {
                fired = true;
                // Ack by reading $5204, like the CPU's IRQ handler would.
                let _ = m.cpu_read(0x5204);
            }
            for _ in 0..114 {
                let _ = m.chr_read(0x0000, false);
                m.tick_cpu();
            }
        }
        // VBlank: no PPU reads. PPU-idle counter drains in 3 cycles,
        // clearing in_frame so the next frame's first tick resets the
        // scanline counter.
        for _ in 0..22 * 114 {
            m.tick_cpu();
        }
        fired
    };

    assert!(run_frame(&mut mapper), "IRQ should fire on frame 1");
    assert!(run_frame(&mut mapper), "IRQ should fire again on frame 2");
    assert!(run_frame(&mut mapper), "IRQ should fire again on frame 3");
}

#[test]
fn mmc5_audio_pulse_produces_nonzero_sample_after_enable() {
    // MMC5 pulse1 mirrors the 2A03 layout at $5000-$5003. After programming
    // a period > 8, enabling the channel via $5015, and letting the timer
    // step through a duty cycle, the mapper should emit a nonzero sample.
    let mut mapper = Mmc5Mapper::new(vec![0u8; 0x8000], vec![0u8; 0x2000]);
    // Enable pulse1 first — length counter is gated on the channel being
    // enabled at the time $5003 is written, same as the 2A03.
    mapper.cpu_write(0x5015, 0x01);
    // Pulse1 control: constant volume, max level, 50% duty.
    mapper.cpu_write(0x5000, 0b1011_1111);
    // Period low / high = 0x100 (above the 8-tick mute threshold).
    mapper.cpu_write(0x5002, 0x00);
    mapper.cpu_write(0x5003, 0x01);

    let mut saw_high = false;
    for _ in 0..4096 {
        mapper.tick_cpu();
        if mapper.expansion_audio_sample() > 0.0 {
            saw_high = true;
            break;
        }
    }
    assert!(saw_high, "pulse1 should produce a nonzero sample after enable");
}

#[test]
fn mmc5_audio_pcm_register_sets_output_directly() {
    // $5011 is a raw 8-bit DAC when $5010 bit 0 = 0 (write mode, the default).
    let mut mapper = Mmc5Mapper::new(vec![0u8; 0x8000], vec![0u8; 0x2000]);
    assert_eq!(mapper.expansion_audio_sample(), 0.0, "silent at boot");
    mapper.cpu_write(0x5011, 200);
    assert!(mapper.expansion_audio_sample() > 0.05, "PCM write should lift output");
    // Switching to read mode (bit 0 = 1) makes $5011 writes a no-op.
    mapper.cpu_write(0x5010, 0x01);
    mapper.cpu_write(0x5011, 0);
    assert!(
        mapper.expansion_audio_sample() > 0.05,
        "write-mode disabled: $5011 write should not change the held PCM value"
    );
}

#[test]
fn vrc6_uses_chr_ram_when_chr_rom_is_absent() {
    let mut mapper = Vrc6Mapper::new(vec![0; 0x10000], Vec::new(), Vrc6Variant::Vrc6a);
    mapper.chr_write(0x0456, 0xA5);
    assert_eq!(mapper.chr_read(0x0456, false), Some(0xA5));
}

#[test]
fn vrc6_pulse_channel_generates_audio_after_register_writes() {
    let mut mapper = Vrc6Mapper::new(vec![0; 0x10000], vec![0; 0x2000], Vrc6Variant::Vrc6a);
    mapper.cpu_write(0x9000, 0x8F);
    mapper.cpu_write(0x9001, 0x10);
    mapper.cpu_write(0x9002, 0x80);

    for _ in 0..256 {
        mapper.tick_cpu();
    }

    assert!(
        mapper.expansion_audio_sample() > 0.0,
        "VRC6 pulse output should be audible after programming the channel"
    );
}

#[test]
fn vrc6_audio_reaches_apu_mixer_through_bus_clock() {
    let cart = Cartridge::from_ines(&make_ines_rom(24, 2, 1)).unwrap();
    let mut bus = Bus::new(cart);
    bus.apu.set_sample_rate(44_100);

    bus.cpu_write(0x9000, 0x8F);
    bus.cpu_write(0x9001, 0x10);
    bus.cpu_write(0x9002, 0x80);

    for _ in 0..4_000 {
        bus.tick(1);
    }

    let samples = bus.apu.drain_samples();
    assert!(
        samples.iter().any(|sample| sample.abs() > 0.0001),
        "mixed audio should include the VRC6 expansion channel"
    );
}

#[test]
fn vrc6_accepts_mirrored_chr_register_addresses() {
    let mut prg_rom = vec![0u8; 0x10000];
    let mut chr_rom = vec![0u8; 8 * 0x0400];
    for bank in 0..8 {
        chr_rom[bank * 0x0400] = bank as u8;
    }

    let mut mapper = Vrc6Mapper::new(std::mem::take(&mut prg_rom), chr_rom, Vrc6Variant::Vrc6a);
    mapper.cpu_write(0xD800, 3);

    assert_eq!(
        mapper.chr_read(0x0000, false),
        Some(3),
        "mirrored VRC6 CHR register writes should update the bank"
    );
}

#[test]
fn vrc6_scanline_irq_is_clocked_from_cpu_cycles() {
    let mut mapper = Vrc6Mapper::new(vec![0; 0x10000], vec![0; 0x2000], Vrc6Variant::Vrc6a);
    mapper.cpu_write(0xF000, 0xFF);
    mapper.cpu_write(0xF001, 0x02);

    for _ in 0..113 {
        mapper.tick_cpu();
    }
    assert!(
        !mapper.check_irq(),
        "scanline-mode IRQ should not fire before roughly one scanline worth of CPU cycles"
    );

    mapper.tick_cpu();
    assert!(
        mapper.check_irq(),
        "scanline-mode IRQ should fire after the prescaler elapses"
    );
}

#[test]
fn sunsoft_fme7_uses_chr_ram_when_chr_rom_is_absent() {
    let mut mapper = SunsoftFme7Mapper::new(vec![0; 0x10000], Vec::new());
    mapper.chr_write(0x0789, 0x3C);
    assert_eq!(mapper.chr_read(0x0789, false), Some(0x3C));
}

#[test]
fn sunsoft_fme7_reads_rom_at_6000_without_ram_enable() {
    let mut prg_rom = vec![0u8; 8 * 0x2000];
    for bank in 0..8 {
        prg_rom[bank * 0x2000] = bank as u8;
    }

    let mut mapper = SunsoftFme7Mapper::new(prg_rom, vec![0; 0x2000]);
    mapper.cpu_write(0x8000, 0x08);
    mapper.cpu_write(0xA000, 0x03);

    assert_eq!(
        mapper.cpu_read(0x6000),
        Some(3),
        "ROM at $6000 should be readable even when the RAM-enable bit is clear"
    );
}

#[test]
fn sunsoft_fme7_rom_at_6000_is_not_writable() {
    let prg_rom = vec![0u8; 8 * 0x2000];
    let mut mapper = SunsoftFme7Mapper::new(prg_rom, vec![0; 0x2000]);
    mapper.cpu_write(0x8000, 0x08);
    mapper.cpu_write(0xA000, 0x00);

    mapper.cpu_write(0x6000, 0xAB);
    assert_eq!(
        mapper.cpu_read(0x6000),
        Some(0),
        "writes to $6000 should be ignored when the region is mapped to ROM"
    );
}

#[test]
fn sunsoft_fme7_irq_counter_is_clocked_per_cpu_cycle() {
    let mut mapper = SunsoftFme7Mapper::new(vec![0; 0x10000], vec![0; 0x2000]);
    mapper.cpu_write(0x8000, 0x0E);
    mapper.cpu_write(0xA000, 0x05);
    mapper.cpu_write(0x8000, 0x0F);
    mapper.cpu_write(0xA000, 0x00);
    mapper.cpu_write(0x8000, 0x0D);
    mapper.cpu_write(0xA000, 0x81);

    for _ in 0..5 {
        mapper.tick_cpu();
    }
    assert!(
        !mapper.check_irq(),
        "IRQ should not fire before the 16-bit counter underflows"
    );

    mapper.tick_cpu();
    assert!(
        mapper.check_irq(),
        "IRQ should fire on the cycle that decrements $0000 to $FFFF"
    );
}

#[test]
fn axrom_accepts_the_high_prg_bank_bit() {
    let mut prg_rom = vec![0u8; 16 * 0x8000];
    for bank in 0..16 {
        prg_rom[bank * 0x8000] = bank as u8;
    }

    let mut mapper = AxromMapper::new(prg_rom);
    mapper.cpu_write(0x8000, 0x08);

    assert_eq!(mapper.cpu_read(0x8000), Some(8));
}

#[test]
fn axrom_ignores_bit_3_on_standard_boards() {
    let mut prg_rom = vec![0u8; 8 * 0x8000];
    for bank in 0..8 {
        prg_rom[bank * 0x8000] = bank as u8;
    }

    let mut mapper = AxromMapper::new(prg_rom);
    mapper.cpu_write(0x8000, 0x08);

    assert_eq!(
        mapper.cpu_read(0x8000),
        Some(0),
        "standard AxROM should ignore bit 3 and use only the low 3 bank bits"
    );
}

#[test]
fn axrom_d4_selects_the_upper_nametable_page() {
    let mut mapper = AxromMapper::new(vec![0; 8 * 0x8000]);

    mapper.cpu_write(0x8000, 0x00);
    assert!(matches!(mapper.mirroring(), termnes::ppu::Mirroring::OneScreenLow));

    mapper.cpu_write(0x8000, 0x10);
    assert!(matches!(
        mapper.mirroring(),
        termnes::ppu::Mirroring::OneScreenHigh
    ));
}
