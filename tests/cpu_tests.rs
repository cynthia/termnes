use nes_tui::cartridge::Cartridge;
use nes_tui::bus::Bus;
use nes_tui::cpu::{Cpu, CpuFlags};

/// Helper: builds a minimal iNES ROM (mapper 2, 2 PRG banks = 32KB)
/// with the given bytes patched into the last bank at offset relative to $C000.
fn make_test_rom(patches: &[(u16, u8)]) -> Vec<u8> {
    let prg_banks = 2u8;
    let mut rom = Vec::new();

    // iNES header (16 bytes)
    rom.extend_from_slice(b"NES\x1A");
    rom.push(prg_banks);  // PRG-ROM banks (16KB each)
    rom.push(0);           // CHR-ROM banks (0 = CHR-RAM)
    rom.push(0x20);        // flags6: mapper low nibble = 2
    rom.push(0x00);        // flags7: mapper high nibble = 0
    rom.extend_from_slice(&[0u8; 8]); // rest of header

    // PRG-ROM: 2 banks x 16KB = 32KB, initialized to 0
    let prg_size = prg_banks as usize * 0x4000;
    rom.resize(16 + prg_size, 0x00);

    // Patches are applied relative to CPU address space.
    // Bank 0 maps to $8000-$BFFF (ROM offset 16..16+0x4000)
    // Bank 1 (last) maps to $C000-$FFFF (ROM offset 16+0x4000..16+0x8000)
    for &(addr, val) in patches {
        let rom_offset = match addr {
            0x8000..=0xBFFF => 16 + (addr as usize - 0x8000),
            0xC000..=0xFFFF => 16 + 0x4000 + (addr as usize - 0xC000),
            _ => panic!("patch address {:#06X} out of PRG range", addr),
        };
        rom[rom_offset] = val;
    }

    rom
}

/// Builds a Cpu with the given ROM patches applied.
fn make_cpu(patches: &[(u16, u8)]) -> Cpu {
    let rom_data = make_test_rom(patches);
    let cart = Cartridge::from_ines(&rom_data).expect("valid test ROM");
    let bus = Bus::new(cart);
    Cpu::new(bus)
}

#[test]
fn test_cpu_reset_vector() {
    // Set reset vector ($FFFC/$FFFD) to point to $C000
    let mut cpu = make_cpu(&[
        (0xFFFC, 0x00), // low byte
        (0xFFFD, 0xC0), // high byte
    ]);

    cpu.reset();
    assert_eq!(cpu.pc, 0xC000, "PC should be loaded from reset vector");
}

#[test]
fn test_cpu_reset_stack_pointer() {
    let mut cpu = make_cpu(&[
        (0xFFFC, 0x00),
        (0xFFFD, 0xC0),
    ]);

    cpu.sp = 0xFF; // clobber SP
    cpu.reset();
    assert_eq!(cpu.sp, 0xFD, "SP should be set to 0xFD after reset");
}

#[test]
fn test_cpu_reset_flags() {
    let mut cpu = make_cpu(&[
        (0xFFFC, 0x00),
        (0xFFFD, 0xC0),
    ]);

    cpu.reset();
    // After reset: UNUSED set, IRQ_DIS set, rest clear
    assert_eq!(
        cpu.status.bits(),
        0b0010_0100,
        "status should be UNUSED | IRQ_DIS after reset"
    );
}

// ===========================================================================
// IRQ regression: level-sensitive delivery — IRQ must not be lost when
// it fires while the I flag is set (edge-triggered bug, fixed in phase 1)
// ===========================================================================

#[test]
fn irq_not_lost_while_i_flag_set() {
    // Memory layout:
    //   $C000: NOP  ($EA)          — executes with I flag set, IRQ blocked
    //   $C001: CLI  ($58)          — clears I flag; IRQ fires at end of step
    //   IRQ handler at $C010:
    //   $C010: LDA $4015 ($AD $15 $40) — reads APU status, clears frame_interrupt
    //   $C013: RTI ($40)
    let mut cpu = make_cpu(&[
        // Reset vector → $C000
        (0xFFFC, 0x00),
        (0xFFFD, 0xC0),
        // IRQ vector → $C010
        (0xFFFE, 0x10),
        (0xFFFF, 0xC0),
        // Code
        (0xC000, 0xEA), // NOP
        (0xC001, 0x58), // CLI
        // IRQ handler
        (0xC010, 0xAD), // LDA $4015
        (0xC011, 0x15),
        (0xC012, 0x40),
        (0xC013, 0x40), // RTI
    ]);

    cpu.reset();
    assert_eq!(cpu.pc, 0xC000);
    assert!(
        cpu.status.contains(CpuFlags::IRQ_DIS),
        "I flag should be set after reset"
    );

    // Simulate APU frame interrupt (level-sensitive line asserted)
    cpu.bus.apu.frame_interrupt = true;

    // Step 1: NOP — I flag is set, so IRQ must NOT be taken
    cpu.step();
    assert_eq!(
        cpu.pc, 0xC001,
        "PC should advance past NOP; IRQ blocked by I flag"
    );
    assert!(
        cpu.bus.poll_irq(),
        "frame_interrupt must remain asserted (level-sensitive, not consumed)"
    );

    // Step 2: CLI — clears I flag, then poll_irq() fires at end of step()
    cpu.step();
    assert_eq!(
        cpu.pc, 0xC010,
        "IRQ should fire immediately after CLI clears I flag"
    );
    assert!(
        cpu.bus.poll_irq(),
        "frame_interrupt still asserted until $4015 is read"
    );
    assert!(
        cpu.status.contains(CpuFlags::IRQ_DIS),
        "IRQ handler should have set I flag"
    );

    // Step 3: LDA $4015 in IRQ handler — clears frame_interrupt
    cpu.step();
    assert!(
        !cpu.bus.poll_irq(),
        "frame_interrupt should be cleared after reading $4015"
    );
}
