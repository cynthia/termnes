use nes_tui::input::{Joypad, JoypadButton};
use nes_tui::bus::Bus;
use nes_tui::cartridge::Cartridge;

#[test]
fn test_joypad_initial_state() {
    let pad = Joypad::new();
    assert!(!pad.strobe);
    assert_eq!(pad.button_index, 0);
    assert_eq!(pad.button_state, 0);
}

#[test]
fn test_strobe_resets_index() {
    let mut pad = Joypad::new();

    // Advance the index by reading a few times
    pad.read();
    pad.read();
    assert_eq!(pad.button_index, 2);

    // Strobe: write 1 then 0
    pad.write(1);
    assert_eq!(pad.button_index, 0, "strobe=1 should reset index");

    pad.write(0);
    assert!(!pad.strobe, "strobe should be off after writing 0");
    assert_eq!(pad.button_index, 0);
}

#[test]
fn test_read_all_buttons_none_pressed() {
    let mut pad = Joypad::new();
    // No buttons pressed — all reads should return 0
    for i in 0..8 {
        assert_eq!(pad.read(), 0, "read #{} should be 0 when no buttons pressed", i + 1);
    }
}

#[test]
fn test_read_button_a_pressed() {
    let mut pad = Joypad::new();
    pad.set_button(JoypadButton::A, true);

    // A is bit 0, read first
    assert_eq!(pad.read(), 1, "A should be pressed");
    assert_eq!(pad.read(), 0, "B should not be pressed");
    assert_eq!(pad.read(), 0, "Select should not be pressed");
    assert_eq!(pad.read(), 0, "Start should not be pressed");
    assert_eq!(pad.read(), 0, "Up should not be pressed");
    assert_eq!(pad.read(), 0, "Down should not be pressed");
    assert_eq!(pad.read(), 0, "Left should not be pressed");
    assert_eq!(pad.read(), 0, "Right should not be pressed");
}

#[test]
fn test_read_button_order() {
    let mut pad = Joypad::new();
    // Press all buttons
    pad.set_buttons(0xFF);

    // Read order: A, B, Select, Start, Up, Down, Left, Right
    for i in 0..8 {
        assert_eq!(pad.read(), 1, "button at index {} should be pressed", i);
    }
}

#[test]
fn test_read_specific_buttons() {
    let mut pad = Joypad::new();
    // Press Start (bit 3) and Right (bit 7)
    pad.set_button(JoypadButton::Start, true);
    pad.set_button(JoypadButton::Right, true);

    let expected = [0, 0, 0, 1, 0, 0, 0, 1]; // A B Sel Start Up Down Left Right
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(
            pad.read(), exp,
            "read #{} expected {} (Start+Right pattern)",
            i + 1, exp
        );
    }
}

#[test]
fn test_after_8_reads_returns_1() {
    let mut pad = Joypad::new();

    // Read through all 8 buttons
    for _ in 0..8 {
        pad.read();
    }

    // After 8 reads, should return 1
    assert_eq!(pad.read(), 1, "9th read should return 1");
    assert_eq!(pad.read(), 1, "10th read should return 1");
    assert_eq!(pad.read(), 1, "11th read should return 1");
}

#[test]
fn test_strobe_high_keeps_returning_button_a() {
    let mut pad = Joypad::new();
    pad.set_button(JoypadButton::A, true);

    // Set strobe high — index should stay at 0
    pad.write(1);

    // Multiple reads should all return A's state
    assert_eq!(pad.read(), 1, "strobe high: should keep returning A");
    assert_eq!(pad.read(), 1, "strobe high: should keep returning A");
    assert_eq!(pad.read(), 1, "strobe high: should keep returning A");
    assert_eq!(pad.button_index, 0, "index should not advance while strobe is high");
}

#[test]
fn test_strobe_high_a_not_pressed() {
    let mut pad = Joypad::new();
    pad.set_button(JoypadButton::Right, true); // only Right pressed

    pad.write(1);
    // With strobe high, reads always return bit 0 (A), which is 0
    assert_eq!(pad.read(), 0);
    assert_eq!(pad.read(), 0);
    assert_eq!(pad.read(), 0);
}

#[test]
fn test_set_button_toggle() {
    let mut pad = Joypad::new();

    pad.set_button(JoypadButton::B, true);
    assert_eq!(pad.button_state & JoypadButton::B as u8, JoypadButton::B as u8);

    pad.set_button(JoypadButton::B, false);
    assert_eq!(pad.button_state & JoypadButton::B as u8, 0);
}

#[test]
fn test_set_button_does_not_affect_others() {
    let mut pad = Joypad::new();

    pad.set_button(JoypadButton::A, true);
    pad.set_button(JoypadButton::Start, true);

    // Releasing A should not affect Start
    pad.set_button(JoypadButton::A, false);
    assert_eq!(pad.button_state & JoypadButton::A as u8, 0);
    assert_ne!(pad.button_state & JoypadButton::Start as u8, 0);
}

#[test]
fn test_full_strobe_cycle() {
    let mut pad = Joypad::new();
    pad.set_button(JoypadButton::Up, true);
    pad.set_button(JoypadButton::B, true);

    // NES strobe protocol: write 1, then write 0, then read 8 times
    pad.write(1);
    pad.write(0);

    let expected = [0, 1, 0, 0, 1, 0, 0, 0]; // A=0 B=1 Sel=0 Start=0 Up=1 Down=0 Left=0 Right=0
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(pad.read(), exp, "strobe cycle read #{}", i + 1);
    }
}

#[test]
fn test_joypad_button_values() {
    // Verify the bit positions match the NES spec read order
    assert_eq!(JoypadButton::A as u8,      0b0000_0001); // bit 0 - read 1st
    assert_eq!(JoypadButton::B as u8,      0b0000_0010); // bit 1 - read 2nd
    assert_eq!(JoypadButton::Select as u8, 0b0000_0100); // bit 2 - read 3rd
    assert_eq!(JoypadButton::Start as u8,  0b0000_1000); // bit 3 - read 4th
    assert_eq!(JoypadButton::Up as u8,     0b0001_0000); // bit 4 - read 5th
    assert_eq!(JoypadButton::Down as u8,   0b0010_0000); // bit 5 - read 6th
    assert_eq!(JoypadButton::Left as u8,   0b0100_0000); // bit 6 - read 7th
    assert_eq!(JoypadButton::Right as u8,  0b1000_0000); // bit 7 - read 8th
}

// ===========================================================================
// Controller regression: shift register and strobe behaviour
// (bug fixed in phase 1 — button_index was only reset on strobe=1,
// causing index to stick at 8+ and all subsequent reads to return 1)
// ===========================================================================

/// Helper: build a minimal Bus for controller integration tests.
fn make_test_bus() -> Bus {
    let mut rom = vec![0u8; 16]; // iNES header
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2; // 2 PRG banks = 32KB
    rom[5] = 0;
    rom.resize(16 + 0x8000, 0x00); // 32KB PRG filled with 0
    let cart = Cartridge::from_ines(&rom).unwrap();
    Bus::new(cart)
}

#[test]
fn controller_full_read_cycle_through_bus() {
    let mut bus = make_test_bus();

    // Press A + Start
    bus.joypad1.set_button(JoypadButton::A, true);
    bus.joypad1.set_button(JoypadButton::Start, true);

    // Strobe: write 1 then 0 to $4016
    bus.cpu_write(0x4016, 1);
    bus.cpu_write(0x4016, 0);

    // Read $4016 eight times — expected: A B Sel Start Up Down Left Right
    let expected = [1, 0, 0, 1, 0, 0, 0, 0];
    for (i, &exp) in expected.iter().enumerate() {
        assert_eq!(
            bus.cpu_read(0x4016) & 1,
            exp,
            "read #{} (button index {i}) mismatch",
            i + 1,
        );
    }

    // After 8 reads, subsequent reads must return 1
    for i in 0..4 {
        assert_eq!(
            bus.cpu_read(0x4016) & 1,
            1,
            "read #{} after shift register exhausted should return 1",
            9 + i,
        );
    }

    // Change button state WITHOUT re-strobing (release A, press B)
    bus.joypad1.set_button(JoypadButton::A, false);
    bus.joypad1.set_button(JoypadButton::B, true);

    // Without re-strobing, index is still at 8+ — reads still return 1
    assert_eq!(
        bus.cpu_read(0x4016) & 1,
        1,
        "without re-strobe, reads should still return 1 (index past 8)"
    );

    // Now strobe again to latch the new state
    bus.cpu_write(0x4016, 1);
    bus.cpu_write(0x4016, 0);

    // New state: A=0 B=1 Sel=0 Start=1 Up=0 Down=0 Left=0 Right=0
    let expected_new = [0, 1, 0, 1, 0, 0, 0, 0];
    for (i, &exp) in expected_new.iter().enumerate() {
        assert_eq!(
            bus.cpu_read(0x4016) & 1,
            exp,
            "after re-strobe, read #{} should reflect new button state",
            i + 1,
        );
    }
}

#[test]
fn controller_write_zero_resets_index() {
    // Regression: previously only write(1) reset the index.
    // The fix ensures ANY write to $4016 resets button_index to 0.
    let mut pad = Joypad::new();
    pad.set_button(JoypadButton::A, true);
    pad.set_button(JoypadButton::Start, true);

    // Strobe and read all 8 buttons
    pad.write(1);
    pad.write(0);
    for _ in 0..8 {
        pad.read();
    }
    assert!(pad.button_index >= 8, "index should be 8 after full read");

    // Write 0 alone (without write(1) first) must reset the index
    pad.write(0);
    assert_eq!(
        pad.button_index, 0,
        "write(0) should reset button_index to 0"
    );

    // Verify we can read the buttons again correctly
    assert_eq!(pad.read(), 1, "A should be pressed after index reset");
    assert_eq!(pad.read(), 0, "B should not be pressed");
    assert_eq!(pad.read(), 0, "Select should not be pressed");
    assert_eq!(pad.read(), 1, "Start should be pressed");
}
