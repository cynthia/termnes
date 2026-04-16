use super::LENGTH_TABLE;
use crate::savestate::NoiseState;

/// NTSC noise timer period lookup table.
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

#[derive(Clone)]
pub struct Noise {
    pub enabled: bool,

    // Timer (clocked every 2 CPU cycles — APU half-rate)
    timer_period: u16,
    timer_counter: u16,

    // LFSR (15-bit shift register)
    mode: bool, // false = long mode (tap bit 1), true = short mode (tap bit 6)
    shift_register: u16,

    // Length counter
    pub length_counter: u8,
    length_halt: bool,

    // Envelope
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    constant_volume: bool,
    volume: u8,
}

impl Noise {
    pub fn new() -> Self {
        Self {
            enabled: false,
            timer_period: 0,
            timer_counter: 0,
            mode: false,
            shift_register: 1, // must be non-zero
            length_counter: 0,
            length_halt: false,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            constant_volume: false,
            volume: 0,
        }
    }

    /// Write one of the noise registers (reg 0, 2, 3 — reg 1 is unused).
    pub fn write_register(&mut self, reg: u8, val: u8) {
        match reg {
            0 => {
                // $400C
                self.length_halt = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.volume = val & 0x0F;
            }
            2 => {
                // $400E
                self.mode = val & 0x80 != 0;
                self.timer_period = NOISE_PERIOD_TABLE[(val & 0x0F) as usize];
            }
            3 => {
                // $400F
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    /// Clock the timer (called every 2 CPU cycles — APU half-rate).
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            let feedback_bit = if self.mode { 6 } else { 1 };
            let feedback =
                (self.shift_register & 1) ^ ((self.shift_register >> feedback_bit) & 1);
            self.shift_register >>= 1;
            self.shift_register |= feedback << 14;
        } else {
            self.timer_counter -= 1;
        }
    }

    /// Quarter-frame: clock the envelope generator.
    pub fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.volume;
        } else if self.envelope_divider == 0 {
            self.envelope_divider = self.volume;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.length_halt {
                self.envelope_decay = 15; // loop
            }
        } else {
            self.envelope_divider -= 1;
        }
    }

    /// Half-frame: clock the length counter.
    pub fn clock_length_counter(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    /// $4015 channel-enable bit.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    /// Current output value (0–15).
    pub fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 {
            return 0;
        }
        // Output is 0 when LFSR bit 0 is set
        if self.shift_register & 1 != 0 {
            return 0;
        }
        if self.constant_volume {
            self.volume
        } else {
            self.envelope_decay
        }
    }

    pub fn capture_state(&self) -> NoiseState {
        NoiseState {
            enabled: self.enabled, timer_period: self.timer_period,
            timer_counter: self.timer_counter, mode: self.mode,
            shift_register: self.shift_register, length_counter: self.length_counter,
            length_halt: self.length_halt, envelope_start: self.envelope_start,
            envelope_divider: self.envelope_divider, envelope_decay: self.envelope_decay,
            constant_volume: self.constant_volume, volume: self.volume,
        }
    }

    pub fn restore_state(&mut self, s: &NoiseState) {
        self.enabled = s.enabled; self.timer_period = s.timer_period;
        self.timer_counter = s.timer_counter; self.mode = s.mode;
        self.shift_register = s.shift_register; self.length_counter = s.length_counter;
        self.length_halt = s.length_halt; self.envelope_start = s.envelope_start;
        self.envelope_divider = s.envelope_divider; self.envelope_decay = s.envelope_decay;
        self.constant_volume = s.constant_volume; self.volume = s.volume;
    }
}
