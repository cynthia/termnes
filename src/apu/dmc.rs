use crate::savestate::DmcState;

const DMC_PERIOD_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54
];

#[derive(Clone)]
pub struct Dmc {
    pub output_level: u8, // 7-bit (0–127)
    pub irq_enable: bool,
    pub loop_flag: bool,
    pub rate_index: u8,
    pub sample_address: u16,
    pub sample_length: u16,

    pub current_address: u16,
    pub current_length: u16,
    
    pub shift_register: u8,
    pub bits_remaining: u8,
    
    pub timer: u16,
    pub sample_buffer: Option<u8>,
    pub irq_pending: bool,
    pub silence: bool,

    // When true, signals the Bus to perform a DMA read.
    pub dma_request: bool,
}

impl Dmc {
    pub fn new() -> Self {
        Self {
            output_level: 0,
            irq_enable: false,
            loop_flag: false,
            rate_index: 0,
            sample_address: 0xC000,
            sample_length: 1,

            current_address: 0xC000,
            current_length: 0,

            shift_register: 0,
            bits_remaining: 8,

            timer: DMC_PERIOD_TABLE[0],
            sample_buffer: None,
            irq_pending: false,
            silence: true,
            
            dma_request: false,
        }
    }

    pub fn write_register(&mut self, reg: u8, val: u8) {
        match reg {
            0 => {
                // $4010: IL--.RRRR
                self.irq_enable = (val & 0x80) != 0;
                self.loop_flag = (val & 0x40) != 0;
                self.rate_index = val & 0x0F;
                self.timer = DMC_PERIOD_TABLE[self.rate_index as usize];
                if !self.irq_enable {
                    self.irq_pending = false;
                }
            }
            1 => {
                // $4011: -DDD.DDDD (direct load)
                self.output_level = val & 0x7F;
            }
            2 => {
                // $4012: AAAA.AAAA (Sample address)
                self.sample_address = 0xC000 + (val as u16 * 64);
            }
            3 => {
                // $4013: LLLL.LLLL (Sample length)
                self.sample_length = (val as u16 * 16) + 1;
            }
            _ => {}
        }
    }

    pub fn write_status(&mut self, val: u8) {
        // Writing to $4015 bit 4 enables/disables DMC
        self.irq_pending = false;
        if val & 0x10 == 0 {
            self.current_length = 0;
        } else {
            if self.current_length == 0 {
                self.current_address = self.sample_address;
                self.current_length = self.sample_length;
                self.check_dma();
            }
        }
    }

    pub fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = DMC_PERIOD_TABLE[self.rate_index as usize];
            self.clock_output();
        } else {
            self.timer -= 1;
        }
    }

    fn clock_output(&mut self) {
        if !self.silence {
            if self.shift_register & 1 != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else {
                if self.output_level >= 2 {
                    self.output_level -= 2;
                }
            }
            self.shift_register >>= 1;
        }

        self.bits_remaining -= 1;
        if self.bits_remaining == 0 {
            self.bits_remaining = 8;
            if let Some(byte) = self.sample_buffer {
                self.silence = false;
                self.shift_register = byte;
                self.sample_buffer = None;
                self.check_dma();
            } else {
                self.silence = true;
            }
        }
    }

    pub fn check_dma(&mut self) {
        if self.sample_buffer.is_none() && self.current_length > 0 {
            self.dma_request = true;
        }
    }

    pub fn load_sample(&mut self, byte: u8) {
        self.dma_request = false;
        self.sample_buffer = Some(byte);
        self.current_address = self.current_address.wrapping_add(1);
        if self.current_address == 0 {
            self.current_address = 0x8000;
        }
        self.current_length -= 1;
        if self.current_length == 0 {
            if self.loop_flag {
                self.current_address = self.sample_address;
                self.current_length = self.sample_length;
                self.check_dma();
            } else if self.irq_enable {
                self.irq_pending = true;
            }
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }

    pub fn capture_state(&self) -> DmcState {
        DmcState {
            output_level: self.output_level,
            irq_enable: self.irq_enable,
            loop_flag: self.loop_flag,
            rate_index: self.rate_index,
            sample_address: self.sample_address,
            sample_length: self.sample_length,
            current_address: self.current_address,
            current_length: self.current_length,
            shift_register: self.shift_register,
            bits_remaining: self.bits_remaining,
            timer: self.timer,
            sample_buffer: self.sample_buffer,
            irq_pending: self.irq_pending,
            silence: self.silence,
        }
    }

    pub fn restore_state(&mut self, s: &DmcState) {
        self.output_level = s.output_level;
        self.irq_enable = s.irq_enable;
        self.loop_flag = s.loop_flag;
        self.rate_index = s.rate_index;
        self.sample_address = s.sample_address;
        self.sample_length = s.sample_length;
        self.current_address = s.current_address;
        self.current_length = s.current_length;
        self.shift_register = s.shift_register;
        self.bits_remaining = s.bits_remaining;
        self.timer = s.timer;
        self.sample_buffer = s.sample_buffer;
        self.irq_pending = s.irq_pending;
        self.silence = s.silence;
    }
}
