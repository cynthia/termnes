use super::Mapper;
use crate::ppu::Mirroring;
use crate::savestate::MapperState;

/// VRC6 variant
#[derive(Clone, Copy, PartialEq)]
pub enum Vrc6Variant {
    Vrc6a, // Mapper 24
    Vrc6b, // Mapper 26
}

#[derive(Clone, Copy)]
struct Vrc6Pulse {
    reg0: u8,
    reg1: u8,
    reg2: u8,
    timer_counter: u16,
    step: u8,
}

impl Vrc6Pulse {
    fn new() -> Self {
        Self {
            reg0: 0,
            reg1: 0,
            reg2: 0,
            timer_counter: 0,
            step: 0,
        }
    }

    fn period(&self) -> u16 {
        (((self.reg2 & 0x0F) as u16) << 8) | self.reg1 as u16
    }

    fn enabled(&self) -> bool {
        self.reg2 & 0x80 != 0
    }

    fn gate(&self) -> bool {
        self.reg0 & 0x80 != 0
    }

    fn duty(&self) -> u8 {
        (self.reg0 >> 4) & 0x07
    }

    fn volume(&self) -> u8 {
        self.reg0 & 0x0F
    }

    fn clock(&mut self, period_shift: u8, halt: bool) {
        if halt || !self.enabled() {
            return;
        }

        if self.timer_counter == 0 {
            self.timer_counter = scaled_period(self.period(), period_shift);
            self.step = self.step.wrapping_add(1) & 0x0F;
        } else {
            self.timer_counter -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled() {
            return 0;
        }
        if self.gate() || self.step <= self.duty() {
            self.volume()
        } else {
            0
        }
    }
}

#[derive(Clone, Copy)]
struct Vrc6Saw {
    reg0: u8,
    reg1: u8,
    reg2: u8,
    timer_counter: u16,
    step: u8,
    accumulator: u8,
}

impl Vrc6Saw {
    fn new() -> Self {
        Self {
            reg0: 0,
            reg1: 0,
            reg2: 0,
            timer_counter: 0,
            step: 0,
            accumulator: 0,
        }
    }

    fn period(&self) -> u16 {
        (((self.reg2 & 0x0F) as u16) << 8) | self.reg1 as u16
    }

    fn enabled(&self) -> bool {
        self.reg2 & 0x80 != 0
    }

    fn rate(&self) -> u8 {
        self.reg0 & 0x3F
    }

    fn clock(&mut self, period_shift: u8, halt: bool) {
        if halt || !self.enabled() {
            return;
        }

        if self.timer_counter == 0 {
            self.timer_counter = scaled_period(self.period(), period_shift);
            self.step = (self.step + 1) % 14;
            if self.step == 0 {
                self.accumulator = 0;
            } else if self.step & 1 == 1 {
                self.accumulator = self.accumulator.wrapping_add(self.rate());
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled() {
            0
        } else {
            (self.accumulator >> 3) & 0x1F
        }
    }
}

fn scaled_period(period: u16, shift: u8) -> u16 {
    let shifted = match shift {
        4 => period >> 4,
        8 => period >> 8,
        _ => period,
    };
    shifted.max(1)
}

/// VRC6 (Mappers 24 and 26) — Konami's advanced mapper.
/// Features 16KB/8KB PRG banking, 1KB CHR banking, an IRQ timer, and 3 expansion-audio channels.
pub struct Vrc6Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    chr_is_ram: bool,
    prg_ram: [u8; 8192],
    variant: Vrc6Variant,

    prg_bank_16k: usize,
    prg_bank_8k: usize,
    chr_banks: [usize; 8],
    mirroring: Mirroring,

    audio_control: u8,
    pulse1: Vrc6Pulse,
    pulse2: Vrc6Pulse,
    saw: Vrc6Saw,

    irq_latch: u8,
    irq_counter: u8,
    irq_mode_cycle: bool,
    irq_enable: bool,
    irq_enable_after_ack: bool,
    irq_pending: bool,
    irq_prescaler: i16,
}

impl Vrc6Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, variant: Vrc6Variant) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 8192],
            chr_is_ram,
            prg_ram: [0; 8192],
            variant,
            prg_bank_16k: 0,
            prg_bank_8k: 0,
            chr_banks: [0; 8],
            mirroring: Mirroring::Vertical,
            audio_control: 0,
            pulse1: Vrc6Pulse::new(),
            pulse2: Vrc6Pulse::new(),
            saw: Vrc6Saw::new(),
            irq_latch: 0,
            irq_counter: 0,
            irq_mode_cycle: false,
            irq_enable: false,
            irq_enable_after_ack: false,
            irq_pending: false,
            irq_prescaler: 341,
        }
    }

    fn fix_addr(&self, addr: u16) -> u16 {
        if self.variant == Vrc6Variant::Vrc6b {
            let a0 = addr & 0x01;
            let a1 = (addr & 0x02) >> 1;
            (addr & 0xFFFC) | (a0 << 1) | a1
        } else {
            addr
        }
    }

    fn decode_reg_addr(&self, addr: u16) -> u16 {
        self.fix_addr(addr) & 0xF003
    }

    fn audio_period_shift(&self) -> u8 {
        match self.audio_control & 0x03 {
            0x01 => 4,
            0x02 | 0x03 => 8,
            _ => 0,
        }
    }

    fn audio_halt(&self) -> bool {
        self.audio_control & 0x04 != 0
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }
}

impl Mapper for Vrc6Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[addr as usize - 0x6000]),
            0x8000..=0xBFFF => {
                let num_banks = self.prg_rom.len() / 0x4000;
                if num_banks == 0 {
                    return None;
                }
                let offset = (addr as usize - 0x8000) + (self.prg_bank_16k % num_banks) * 0x4000;
                Some(self.prg_rom[offset])
            }
            0xC000..=0xDFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 {
                    return None;
                }
                let offset = (addr as usize - 0xC000) + (self.prg_bank_8k % num_banks) * 0x2000;
                Some(self.prg_rom[offset])
            }
            0xE000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 {
                    return None;
                }
                let last_bank = num_banks.saturating_sub(1);
                let offset = (addr as usize - 0xE000) + last_bank * 0x2000;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        let fixed_addr = self.decode_reg_addr(addr);

        match fixed_addr {
            0x6000..=0x7FFF => self.prg_ram[addr as usize - 0x6000] = val,
            0x8000..=0x8003 => self.prg_bank_16k = (val & 0x0F) as usize,
            0x9000 => self.pulse1.reg0 = val,
            0x9001 => self.pulse1.reg1 = val,
            0x9002 => self.pulse1.reg2 = val,
            0x9003 => self.audio_control = val,
            0xA000 => self.pulse2.reg0 = val,
            0xA001 => self.pulse2.reg1 = val,
            0xA002 => self.pulse2.reg2 = val,
            0xB000 => self.saw.reg0 = val,
            0xB001 => self.saw.reg1 = val,
            0xB002 => self.saw.reg2 = val,
            0xB003 => {
                self.mirroring = match (val >> 2) & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::OneScreenLow,
                    3 => Mirroring::OneScreenHigh,
                    _ => Mirroring::Vertical,
                };
            }
            0xC000..=0xC003 => self.prg_bank_8k = (val & 0x1F) as usize,
            0xD000..=0xD003 => match fixed_addr {
                0xD000 => self.chr_banks[0] = val as usize,
                0xD001 => self.chr_banks[1] = val as usize,
                0xD002 => self.chr_banks[2] = val as usize,
                0xD003 => self.chr_banks[3] = val as usize,
                _ => {}
            },
            0xE000..=0xE003 => match fixed_addr {
                0xE000 => self.chr_banks[4] = val as usize,
                0xE001 => self.chr_banks[5] = val as usize,
                0xE002 => self.chr_banks[6] = val as usize,
                0xE003 => self.chr_banks[7] = val as usize,
                _ => {}
            },
            0xF000 => self.irq_latch = val,
            0xF001 => {
                self.irq_enable_after_ack = (val & 0x01) != 0;
                self.irq_enable = (val & 0x02) != 0;
                self.irq_mode_cycle = (val & 0x04) != 0;
                self.irq_prescaler = 341;
                if val & 0x02 != 0 {
                    self.irq_counter = self.irq_latch;
                }
                self.irq_pending = false;
            }
            0xF002 => {
                self.irq_enable = self.irq_enable_after_ack;
                self.irq_pending = false;
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x2000 {
            return None;
        }
        if self.chr_is_ram {
            return Some(self.chr_ram[addr as usize]);
        }
        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 {
            return Some(0);
        }

        let bank = self.chr_banks[(addr / 0x0400) as usize];
        let offset = (addr as usize % 0x0400) + (bank % num_banks) * 0x0400;
        Some(self.chr_rom[offset])
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_is_ram {
            self.chr_ram[addr as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn tick_cpu(&mut self) {
        let period_shift = self.audio_period_shift();
        let halt = self.audio_halt();
        self.pulse1.clock(period_shift, halt);
        self.pulse2.clock(period_shift, halt);
        self.saw.clock(period_shift, halt);

        if !self.irq_enable {
            return;
        }

        if self.irq_mode_cycle {
            self.clock_irq_counter();
        } else {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        }
    }

    fn expansion_audio_sample(&self) -> f32 {
        let pulse_mix = (self.pulse1.output() as f32 + self.pulse2.output() as f32) / 30.0;
        let saw_mix = self.saw.output() as f32 / 31.0;
        (pulse_mix + saw_mix) * 0.12
    }

    fn check_irq(&self) -> bool {
        self.irq_pending
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::Vrc6 {
            prg_bank_16k: self.prg_bank_16k,
            prg_bank_8k: self.prg_bank_8k,
            chr_banks: self.chr_banks.to_vec(),
            mirroring: match self.mirroring {
                Mirroring::Vertical => 0,
                Mirroring::Horizontal => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
            audio_control: self.audio_control,
            pulse1_regs: vec![self.pulse1.reg0, self.pulse1.reg1, self.pulse1.reg2],
            pulse1_timer: self.pulse1.timer_counter,
            pulse1_step: self.pulse1.step,
            pulse2_regs: vec![self.pulse2.reg0, self.pulse2.reg1, self.pulse2.reg2],
            pulse2_timer: self.pulse2.timer_counter,
            pulse2_step: self.pulse2.step,
            saw_regs: vec![self.saw.reg0, self.saw.reg1, self.saw.reg2],
            saw_timer: self.saw.timer_counter,
            saw_step: self.saw.step,
            saw_accumulator: self.saw.accumulator,
            irq_latch: self.irq_latch,
            irq_counter: self.irq_counter,
            irq_mode_cycle: self.irq_mode_cycle,
            irq_enable: self.irq_enable,
            irq_enable_after_ack: self.irq_enable_after_ack,
            irq_pending: self.irq_pending,
            irq_prescaler: self.irq_prescaler,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::Vrc6 {
            prg_bank_16k,
            prg_bank_8k,
            chr_banks,
            mirroring,
            audio_control,
            pulse1_regs,
            pulse1_timer,
            pulse1_step,
            pulse2_regs,
            pulse2_timer,
            pulse2_step,
            saw_regs,
            saw_timer,
            saw_step,
            saw_accumulator,
            irq_latch,
            irq_counter,
            irq_mode_cycle,
            irq_enable,
            irq_enable_after_ack,
            irq_pending,
            irq_prescaler,
        } = state
        {
            self.prg_bank_16k = *prg_bank_16k;
            self.prg_bank_8k = *prg_bank_8k;
            if chr_banks.len() == 8 {
                self.chr_banks.copy_from_slice(chr_banks);
            }
            self.mirroring = match mirroring {
                0 => Mirroring::Vertical,
                1 => Mirroring::Horizontal,
                2 => Mirroring::OneScreenLow,
                3 => Mirroring::OneScreenHigh,
                _ => Mirroring::Vertical,
            };
            self.audio_control = *audio_control;
            if pulse1_regs.len() == 3 {
                self.pulse1.reg0 = pulse1_regs[0];
                self.pulse1.reg1 = pulse1_regs[1];
                self.pulse1.reg2 = pulse1_regs[2];
            }
            self.pulse1.timer_counter = *pulse1_timer;
            self.pulse1.step = *pulse1_step;
            if pulse2_regs.len() == 3 {
                self.pulse2.reg0 = pulse2_regs[0];
                self.pulse2.reg1 = pulse2_regs[1];
                self.pulse2.reg2 = pulse2_regs[2];
            }
            self.pulse2.timer_counter = *pulse2_timer;
            self.pulse2.step = *pulse2_step;
            if saw_regs.len() == 3 {
                self.saw.reg0 = saw_regs[0];
                self.saw.reg1 = saw_regs[1];
                self.saw.reg2 = saw_regs[2];
            }
            self.saw.timer_counter = *saw_timer;
            self.saw.step = *saw_step;
            self.saw.accumulator = *saw_accumulator;
            self.irq_latch = *irq_latch;
            self.irq_counter = *irq_counter;
            self.irq_mode_cycle = *irq_mode_cycle;
            self.irq_enable = *irq_enable;
            self.irq_enable_after_ack = *irq_enable_after_ack;
            self.irq_pending = *irq_pending;
            self.irq_prescaler = *irq_prescaler;
        }
    }
}
