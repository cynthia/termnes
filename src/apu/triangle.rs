use super::LENGTH_TABLE;
use crate::savestate::TriangleState;

const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

#[derive(Clone)]
pub struct Triangle {
    pub enabled: bool,

    // Timer (11-bit period, clocked every CPU cycle)
    timer_period: u16,
    timer_counter: u16,
    sequence_pos: u8,

    // Length counter
    pub length_counter: u8,
    length_halt: bool, // also controls linear counter reload behaviour

    // Linear counter
    linear_counter: u8,
    linear_counter_reload: u8,
    linear_counter_reload_flag: bool,
}

impl Triangle {
    pub fn new() -> Self {
        Self {
            enabled: false,
            timer_period: 0,
            timer_counter: 0,
            sequence_pos: 0,
            length_counter: 0,
            length_halt: false,
            linear_counter: 0,
            linear_counter_reload: 0,
            linear_counter_reload_flag: false,
        }
    }

    /// Write one of the triangle registers (reg 0, 2, 3 — reg 1 is unused).
    pub fn write_register(&mut self, reg: u8, val: u8) {
        match reg {
            0 => {
                // $4008
                self.length_halt = val & 0x80 != 0;
                self.linear_counter_reload = val & 0x7F;
            }
            2 => {
                // $400A
                self.timer_period = (self.timer_period & 0x700) | val as u16;
            }
            3 => {
                // $400B
                self.timer_period = (self.timer_period & 0xFF) | ((val as u16 & 7) << 8);
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.linear_counter_reload_flag = true;
            }
            _ => {}
        }
    }

    /// Clock the timer (called every CPU cycle).
    pub fn clock_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_period;
            // Only advance the sequencer when both counters are non-zero
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.sequence_pos = (self.sequence_pos + 1) % 32;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    /// Quarter-frame: clock the linear counter.
    pub fn clock_linear_counter(&mut self) {
        if self.linear_counter_reload_flag {
            self.linear_counter = self.linear_counter_reload;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.length_halt {
            self.linear_counter_reload_flag = false;
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
        if !self.enabled || self.length_counter == 0 || self.linear_counter == 0 {
            return 0;
        }
        TRIANGLE_SEQUENCE[self.sequence_pos as usize]
    }

    pub fn capture_state(&self) -> TriangleState {
        TriangleState {
            enabled: self.enabled, timer_period: self.timer_period,
            timer_counter: self.timer_counter, sequence_pos: self.sequence_pos,
            length_counter: self.length_counter, length_halt: self.length_halt,
            linear_counter: self.linear_counter,
            linear_counter_reload: self.linear_counter_reload,
            linear_counter_reload_flag: self.linear_counter_reload_flag,
        }
    }

    pub fn restore_state(&mut self, s: &TriangleState) {
        self.enabled = s.enabled; self.timer_period = s.timer_period;
        self.timer_counter = s.timer_counter; self.sequence_pos = s.sequence_pos;
        self.length_counter = s.length_counter; self.length_halt = s.length_halt;
        self.linear_counter = s.linear_counter;
        self.linear_counter_reload = s.linear_counter_reload;
        self.linear_counter_reload_flag = s.linear_counter_reload_flag;
    }
}
