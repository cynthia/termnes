use nes_tui::apu::Apu;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// CPU cycles for one full NTSC 4-step frame sequence (~29830 CPU cycles).
/// The frame counter fires an IRQ at the end of step 3 (cycle ≈ 29828-29830).
const FOUR_STEP_FRAME_CYCLES: u32 = 29830;

/// CPU cycles for one full 5-step frame sequence (~37282 CPU cycles).
const FIVE_STEP_FRAME_CYCLES: u32 = 37282;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Tick the APU for `total` CPU cycles in increments of `step`, returning
/// true if the IRQ line is asserted after all ticks.
fn tick_for(apu: &mut Apu, total: u32, step: u8) -> bool {
    let mut remaining = total;
    while remaining > 0 {
        let s = (remaining.min(step as u32)) as u8;
        apu.tick(s);
        remaining -= s as u32;
    }
    apu.frame_interrupt
}

/// Tick one cycle at a time, returning the exact cycle on which IRQ fires,
/// or None if it never fires within `max_cycles`.
fn find_irq_cycle(apu: &mut Apu, max_cycles: u32) -> Option<u32> {
    for cycle in 1..=max_cycles {
        apu.tick(1);
        if apu.frame_interrupt {
            return Some(cycle);
        }
    }
    None
}

// ===========================================================================
// 1. Default state
// ===========================================================================

#[test]
fn default_state_no_interrupt() {
    let apu = Apu::new();
    assert!(!read_frame_interrupt(&apu), "frame_interrupt_flag should be false on init");
}

#[test]
fn default_status_reads_zero() {
    let mut apu = Apu::new();
    assert_eq!(apu.read_status(), 0x00, "status register should be 0 on init");
}

/// Helper: peek at the frame interrupt via read_status bit 6 without
/// consuming the flag (reads twice — if first read is 0 we know it was clear).
fn read_frame_interrupt(apu: &Apu) -> bool {
    // We can't read without mutating (read_status clears the flag), so we
    // clone to avoid side-effects in assertions that just want to observe.
    let mut copy = apu.clone();
    copy.read_status() & 0x40 != 0
}

// ===========================================================================
// 2. 4-step mode IRQ generation
// ===========================================================================

#[test]
fn four_step_mode_generates_irq() {
    let mut apu = Apu::new();

    // Write $00 to $4017: 4-step mode, IRQ enabled.
    apu.write_frame_counter(0x00);

    let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);
    assert!(irq, "4-step mode should generate an IRQ within one frame sequence");
}

#[test]
fn four_step_mode_sets_status_bit6() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);

    let status = apu.read_status();
    assert_ne!(
        status & 0x40, 0,
        "bit 6 of $4015 should be set after frame IRQ fires"
    );
}

#[test]
fn four_step_irq_fires_near_expected_cycle() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    let cycle = find_irq_cycle(&mut apu, 30_000)
        .expect("IRQ should fire within 30 000 cycles");

    // The exact cycle varies by implementation (29828–29830 is typical).
    assert!(
        (29826..=29832).contains(&cycle),
        "IRQ fired at cycle {cycle}, expected ~29828-29830"
    );
}

// ===========================================================================
// 3. IRQ inhibit (bit 6 of $4017)
// ===========================================================================

#[test]
fn irq_inhibit_prevents_irq() {
    let mut apu = Apu::new();

    // $40 = bit 6 set → IRQ inhibit, 4-step mode.
    apu.write_frame_counter(0x40);

    let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES * 2, 1);
    assert!(
        !irq,
        "no IRQ should fire when inhibit bit is set (4-step mode)"
    );
}

#[test]
fn irq_inhibit_keeps_status_clear() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x40);

    tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES * 2, 1);

    let status = apu.read_status();
    assert_eq!(
        status & 0x40, 0,
        "bit 6 of status should stay clear when IRQ is inhibited"
    );
}

// ===========================================================================
// 4. 5-step mode — never generates IRQs
// ===========================================================================

#[test]
fn five_step_mode_no_irq() {
    let mut apu = Apu::new();

    // $80 = bit 7 set → 5-step mode (IRQ enable is irrelevant in this mode).
    apu.write_frame_counter(0x80);

    let irq = tick_for(&mut apu, FIVE_STEP_FRAME_CYCLES * 2, 1);
    assert!(
        !irq,
        "5-step mode should never generate IRQs"
    );
}

#[test]
fn five_step_mode_status_bit6_stays_clear() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x80);

    tick_for(&mut apu, FIVE_STEP_FRAME_CYCLES * 2, 1);

    assert_eq!(
        apu.read_status() & 0x40, 0,
        "bit 6 should never be set in 5-step mode"
    );
}

// ===========================================================================
// 5. $4015 read clears frame interrupt flag
// ===========================================================================

#[test]
fn read_status_clears_frame_interrupt() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // Drive to an IRQ.
    tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);

    let first = apu.read_status();
    assert_ne!(first & 0x40, 0, "first read should see bit 6 set");

    let second = apu.read_status();
    assert_eq!(second & 0x40, 0, "second read should see bit 6 cleared");
}

#[test]
fn read_status_clears_only_frame_interrupt() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);

    // First read consumes the flag.
    let _ = apu.read_status();

    // Third, fourth, … reads should all stay zero for bit 6.
    for _ in 0..5 {
        assert_eq!(
            apu.read_status() & 0x40, 0,
            "repeated reads should keep bit 6 clear"
        );
    }
}

// ===========================================================================
// 6. Frame counter reset on $4017 write
// ===========================================================================

#[test]
fn write_frame_counter_resets_cycle_counter() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // Advance most of the way through a frame (but not quite).
    let partial = FOUR_STEP_FRAME_CYCLES - 1000;
    let irq = tick_for(&mut apu, partial, 1);
    assert!(!irq, "IRQ should not have fired yet");

    // Re-write $4017 — this should reset the sequencer.
    apu.write_frame_counter(0x00);

    // Tick another 1000 cycles — without the reset we would have crossed the
    // threshold, but with the reset we're back near the start.
    let irq = tick_for(&mut apu, 1000, 1);
    assert!(
        !irq,
        "IRQ should NOT fire because $4017 write reset the cycle counter"
    );
}

#[test]
fn write_frame_counter_mid_sequence_allows_full_new_frame() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // Advance partway.
    tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES / 2, 1);

    // Reset.
    apu.write_frame_counter(0x00);

    // Now a full frame from the reset point should trigger the IRQ.
    let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);
    assert!(irq, "a full frame after reset should produce an IRQ");
}

// ===========================================================================
// 7. Multiple frames — IRQ is periodic
// ===========================================================================

#[test]
fn irq_fires_on_successive_frames() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    for frame in 1..=3 {
        // Clear any pending flag so we can detect the *new* IRQ.
        let _ = apu.read_status();

        let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);
        assert!(
            irq,
            "IRQ should fire on frame {frame}"
        );

        let status = apu.read_status();
        assert_ne!(
            status & 0x40, 0,
            "bit 6 should be set after frame {frame}"
        );
    }
}

#[test]
fn irq_fires_with_multi_cycle_ticks() {
    // Ensure the implementation handles tick(n) where n > 1 correctly.
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // Use larger tick steps (as a real CPU would — instructions are 2-7 cycles).
    let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 4);
    assert!(irq, "IRQ should fire even with multi-cycle tick steps");
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn switching_from_five_step_to_four_step_enables_irq() {
    let mut apu = Apu::new();

    // Start in 5-step mode (no IRQs).
    apu.write_frame_counter(0x80);
    tick_for(&mut apu, FIVE_STEP_FRAME_CYCLES, 1);
    assert!(!tick_for(&mut apu, 1, 1), "sanity: no IRQ in 5-step mode");

    // Switch to 4-step mode.
    apu.write_frame_counter(0x00);
    let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);
    assert!(irq, "IRQ should fire after switching to 4-step mode");
}

#[test]
fn enabling_inhibit_mid_frame_suppresses_pending_irq() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // Advance close to (but not past) the IRQ point.
    let partial = FOUR_STEP_FRAME_CYCLES - 500;
    tick_for(&mut apu, partial, 1);

    // Now set inhibit — should also clear any pending frame interrupt flag.
    apu.write_frame_counter(0x40);

    // Tick past where the old IRQ point would have been.
    let irq = tick_for(&mut apu, 2000, 1);
    assert!(
        !irq,
        "enabling inhibit should suppress IRQ even when set mid-frame"
    );

    assert_eq!(
        apu.read_status() & 0x40, 0,
        "bit 6 should be clear after inhibit is enabled"
    );
}

#[test]
fn zero_cycle_tick_is_noop() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // Ticking 0 cycles should not advance state or fire IRQ.
    apu.tick(0);
    assert!(!apu.frame_interrupt, "tick(0) should not fire an IRQ");
    assert_eq!(apu.read_status(), 0x00);
}

#[test]
fn write_status_does_not_affect_frame_counter() {
    let mut apu = Apu::new();
    apu.write_frame_counter(0x00);

    // write_status ($4015) controls channel enables — it should not touch the
    // frame counter or inhibit flag.
    apu.write_status(0xFF);

    let irq = tick_for(&mut apu, FOUR_STEP_FRAME_CYCLES, 1);
    assert!(irq, "write_status should not interfere with frame counter IRQ");
}
