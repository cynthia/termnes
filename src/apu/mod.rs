//! Minimal APU — enough for frame-counter IRQ, length-counter based timing
//! tests, and the $4015 status/enable protocol. No audio synthesis yet.

/// NES length counter lookup table. Indexed by the high 5 bits of the 4th
/// register of any length-counter channel (pulse1/2 $4003/$4007, triangle
/// $400B, noise $400F).
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20,  2, 40,  4, 80,  6, 160,  8, 60, 10, 14, 12, 26, 14,
    12,  16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Clone)]
pub struct Apu {
    cycle: u32,
    /// false = 4-step, true = 5-step
    mode: bool,
    irq_inhibit: bool,
    pub frame_interrupt: bool,

    // ── Channel enables (from $4015 low 4 bits) ─────────────────────────────
    enable_p1: bool,
    enable_p2: bool,
    enable_tri: bool,
    enable_noise: bool,

    // ── Length counters per channel ─────────────────────────────────────────
    len_p1: u8,
    len_p2: u8,
    len_tri: u8,
    len_noise: u8,

    // ── Length-counter halt flags (suppress half-frame decrement) ───────────
    // For pulse/noise this is bit 5 of the channel's first register; for
    // triangle it's bit 7 of $4008 (which also halts the linear counter).
    halt_p1: bool,
    halt_p2: bool,
    halt_tri: bool,
    halt_noise: bool,

    // ── Reset delay ($4017 writes take effect after 3-4 cycles) ───────────
    pending_reset_cycles: u8,
    pending_mode: bool,
    pending_irq_inhibit: bool,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            cycle: 0,
            mode: false,
            irq_inhibit: false,
            frame_interrupt: false,
            enable_p1: false,
            enable_p2: false,
            enable_tri: false,
            enable_noise: false,
            len_p1: 0,
            len_p2: 0,
            len_tri: 0,
            len_noise: 0,
            halt_p1: false,
            halt_p2: false,
            halt_tri: false,
            halt_noise: false,
            pending_reset_cycles: 0,
            pending_mode: false,
            pending_irq_inhibit: false,
        }
    }

    /// Advance the frame counter by `cpu_cycles` CPU cycles, firing quarter/
    /// half-frame events at the documented sequencer steps.
    pub fn tick(&mut self, cpu_cycles: u8) {
        for _ in 0..cpu_cycles {
            // Handle pending $4017 reset
            if self.pending_reset_cycles > 0 {
                self.pending_reset_cycles -= 1;
                if self.pending_reset_cycles == 0 {
                    self.mode = self.pending_mode;
                    self.irq_inhibit = self.pending_irq_inhibit;
                    self.cycle = 0;
                    if self.irq_inhibit {
                        self.frame_interrupt = false;
                    }
                    if self.mode {
                        self.clock_half_frame();
                    }
                }
            }

            self.cycle += 1;
            if !self.mode {
                // 4-step mode
                match self.cycle {
                    7457 => {} // quarter-frame only
                    14913 => self.clock_half_frame(),
                    22371 => {} // quarter-frame only
                    29828 => {
                        if !self.irq_inhibit {
                            self.frame_interrupt = true;
                        }
                    }
                    29829 => {
                        if !self.irq_inhibit {
                            self.frame_interrupt = true;
                        }
                        self.clock_half_frame();
                    }
                    29830 => {
                        if !self.irq_inhibit {
                            self.frame_interrupt = true;
                        }
                        self.cycle = 0;
                    }
                    _ => {}
                }
            } else {
                // 5-step mode — no IRQ ever; half-frame at 14913 and 37281.
                match self.cycle {
                    7457 => {}
                    14913 => self.clock_half_frame(),
                    22371 => {}
                    37281 => self.clock_half_frame(),
                    37282 => self.cycle = 0,
                    _ => {}
                }
            }
        }
    }

    /// Half-frame clock: decrement each non-halted, non-zero length counter.
    fn clock_half_frame(&mut self) {
        if !self.halt_p1    && self.len_p1    > 0 { self.len_p1    -= 1; }
        if !self.halt_p2    && self.len_p2    > 0 { self.len_p2    -= 1; }
        if !self.halt_tri   && self.len_tri   > 0 { self.len_tri   -= 1; }
        if !self.halt_noise && self.len_noise > 0 { self.len_noise -= 1; }
    }

    // ── Register dispatch ───────────────────────────────────────────────────

    /// Handles writes to $4000-$4013 — per-channel APU registers. Only the
    /// subset relevant to length counters is implemented; everything else is
    /// accepted and ignored (still required so the CPU can execute `STA` etc.
    /// to these addresses without the bus swallowing the write silently).
    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x4000 => self.halt_p1    = val & 0x20 != 0,
            0x4004 => self.halt_p2    = val & 0x20 != 0,
            0x4008 => self.halt_tri   = val & 0x80 != 0,
            0x400C => self.halt_noise = val & 0x20 != 0,

            // Length-counter load (high 5 bits). Only loads if the channel
            // is currently enabled.
            0x4003 => {
                if self.enable_p1 {
                    self.len_p1 = LENGTH_TABLE[(val >> 3) as usize];
                }
            }
            0x4007 => {
                if self.enable_p2 {
                    self.len_p2 = LENGTH_TABLE[(val >> 3) as usize];
                }
            }
            0x400B => {
                if self.enable_tri {
                    self.len_tri = LENGTH_TABLE[(val >> 3) as usize];
                }
            }
            0x400F => {
                if self.enable_noise {
                    self.len_noise = LENGTH_TABLE[(val >> 3) as usize];
                }
            }
            _ => {} // sweep, timer-lo, DMC, etc. — not needed for tests
        }
    }

    /// $4015 write: channel enable bits. Clearing a channel's bit forces its
    /// length counter to 0 immediately.
    pub fn write_status(&mut self, val: u8) {
        self.enable_p1    = val & 0x01 != 0;
        self.enable_p2    = val & 0x02 != 0;
        self.enable_tri   = val & 0x04 != 0;
        self.enable_noise = val & 0x08 != 0;
        if !self.enable_p1    { self.len_p1 = 0; }
        if !self.enable_p2    { self.len_p2 = 0; }
        if !self.enable_tri   { self.len_tri = 0; }
        if !self.enable_noise { self.len_noise = 0; }
    }

    /// $4015 read: bits 0-3 = length counter > 0 per channel; bit 6 = frame
    /// interrupt. Clears the frame-interrupt flag as a side effect.
    pub fn read_status(&mut self) -> u8 {
        let mut r = 0u8;
        if self.len_p1    > 0 { r |= 0x01; }
        if self.len_p2    > 0 { r |= 0x02; }
        if self.len_tri   > 0 { r |= 0x04; }
        if self.len_noise > 0 { r |= 0x08; }
        if self.frame_interrupt { r |= 0x40; }
        self.frame_interrupt = false;
        r
    }

    /// $4017 write: bit 7 = mode (0=4-step, 1=5-step), bit 6 = IRQ inhibit.
    /// Resets the frame-counter cycle. In 5-step mode a half-frame is
    /// immediately generated; in 4-step it is not.
    pub fn write_frame_counter(&mut self, val: u8, total_cycles: usize) {
        self.pending_mode = val & 0x80 != 0;
        self.pending_irq_inhibit = val & 0x40 != 0;
        // Delay of 3 or 4 cycles
        self.pending_reset_cycles = if total_cycles % 2 != 0 { 3 } else { 4 };
    }
}
