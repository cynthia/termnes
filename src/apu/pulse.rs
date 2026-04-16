use super::LENGTH_TABLE;

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 75% (inverted 25%)
];

#[derive(Clone)]
pub struct Pulse {
    pub enabled: bool,
    is_channel_1: bool,

    // Duty cycle
    duty: u8,
    duty_pos: u8,

    // Timer (11-bit period, clocked every 2 CPU cycles)
    timer_period: u16,
    timer_counter: u16,

    // Length counter
    pub length_counter: u8,
    length_halt: bool,

    // Envelope
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    constant_volume: bool,
    volume: u8,

    // Sweep unit
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_divider: u8,
    sweep_reload: bool,
}

impl Pulse {
    pub fn new(is_channel_1: bool) -> Self {
        Self {
            enabled: false,
            is_channel_1,
            duty: 0,
            duty_pos: 0,
            timer_period: 0,
            timer_counter: 0,
            length_counter: 0,
            length_halt: false,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            constant_volume: false,
            volume: 0,
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_divider: 0,
            sweep_reload: false,
        }
    }

    /// Write one of the four pulse registers (reg 0–3).
    pub fn write_register(&mut self, reg: u8, val: u8) {
        match reg {
            0 => {
                self.duty = (val >> 6) & 3;
                self.length_halt = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.volume = val & 0x0F;
            }
            1 => {
                self.sweep_enabled = val & 0x80 != 0;
                self.sweep_period = (val >> 4) & 7;
                self.sweep_negate = val & 0x08 != 0;
                self.sweep_shift = val & 7;
                self.sweep_reload = true;
            }
            2 => {
                self.timer_period = (self.timer_period & 0x700) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0xFF) | ((val as u16 & 7) << 8);
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.duty_pos = 0;
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    /// Clock the timer (called every 2 CPU cycles — APU half-rate).
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            self.duty_pos = (self.duty_pos + 1) % 8;
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

    /// Compute the sweep target period.
    fn sweep_target_period(&self) -> u16 {
        let shift_amount = self.timer_period >> self.sweep_shift;
        if self.sweep_negate {
            if self.is_channel_1 {
                // Pulse 1: one's complement (subtract, then subtract 1 more)
                self.timer_period.wrapping_sub(shift_amount).wrapping_sub(1)
            } else {
                // Pulse 2: two's complement
                self.timer_period.wrapping_sub(shift_amount)
            }
        } else {
            self.timer_period.wrapping_add(shift_amount)
        }
    }

    /// Half-frame: clock the sweep unit.
    pub fn clock_sweep(&mut self) {
        let target = self.sweep_target_period();

        if self.sweep_divider == 0 && self.sweep_enabled && self.sweep_shift > 0
            && self.timer_period >= 8
            && target <= 0x7FF
        {
            self.timer_period = target;
        }

        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
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
        // Mute when period is too low or sweep would overflow
        if self.timer_period < 8 || self.sweep_target_period() > 0x7FF {
            return 0;
        }
        if DUTY_TABLE[self.duty as usize][self.duty_pos as usize] == 0 {
            return 0;
        }
        if self.constant_volume {
            self.volume
        } else {
            self.envelope_decay
        }
    }
}
