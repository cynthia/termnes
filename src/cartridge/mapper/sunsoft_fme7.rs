use crate::ppu::Mirroring;
use crate::savestate::MapperState;
use super::Mapper;

/// One of the three Sunsoft 5B tone channels. The 5B is a YM2149F variant —
/// each channel is a 12-bit-period square wave with a 4-bit linear volume and
/// a tone-enable bit from the mixer register (7). Envelope and noise aren't
/// implemented; NES music drivers seen in the wild (Gimmick!, Batman RoJ) use
/// only per-channel tone + volume.
#[derive(Clone, Copy, Default)]
struct Fme7Channel {
    period: u16,
    volume: u8,
    tone_disabled: bool,
    timer: u16,
    phase: bool,
}

impl Fme7Channel {
    /// Clocked at CPU/16 per the datasheet's tone divider. The channel flips
    /// its output whenever the timer hits 0; period 0 is treated as 1.
    fn clock(&mut self) {
        if self.timer == 0 {
            self.timer = self.period.max(1);
            self.phase = !self.phase;
        } else {
            self.timer -= 1;
        }
    }

    fn output(&self) -> u8 {
        if self.tone_disabled {
            return 0;
        }
        if self.phase { self.volume } else { 0 }
    }
}

/// Sunsoft FME-7 / Sunsoft 5B (Mapper 69)
/// Features 8KB PRG banking, 1KB CHR banking, a 16-bit IRQ timer, and 3
/// expansion-audio channels via a YM2149-style register file at $C000/$E000.
pub struct SunsoftFme7Mapper {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    chr_is_ram: bool,
    prg_ram: [u8; 8192],

    command: u8,
    chr_banks: [usize; 8],
    prg_banks: [usize; 4], // for $6000, $8000, $A000, $C000
    prg_ram_enable: bool,
    prg_ram_select: bool, // true if $6000 is RAM instead of ROM

    mirroring: Mirroring,

    irq_counter: u16,
    irq_enable: bool,
    irq_counter_enable: bool,
    irq_pending: bool,

    // 5B audio: the 16-entry register file, the last-selected register, the
    // three tone channels, and a /16 prescaler that drives the tone clock.
    audio_reg_select: u8,
    audio_regs: [u8; 16],
    audio_channels: [Fme7Channel; 3],
    audio_prescaler: u8,
}

impl SunsoftFme7Mapper {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr_is_ram = chr_rom.is_empty();
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 8192],
            chr_is_ram,
            prg_ram: [0; 8192],
            command: 0,
            chr_banks: [0; 8],
            prg_banks: [0, 0, 1, 2],
            prg_ram_enable: false,
            prg_ram_select: false,
            mirroring: Mirroring::Vertical,
            irq_counter: 0,
            irq_enable: false,
            irq_counter_enable: false,
            irq_pending: false,
            audio_reg_select: 0,
            audio_regs: [0; 16],
            audio_channels: [Fme7Channel::default(); 3],
            audio_prescaler: 0,
        }
    }

    fn write_audio_data(&mut self, val: u8) {
        let reg = (self.audio_reg_select & 0x0F) as usize;
        self.audio_regs[reg] = val;
        match reg {
            0 | 2 | 4 => {
                let ch = reg / 2;
                self.audio_channels[ch].period =
                    (self.audio_channels[ch].period & 0x0F00) | val as u16;
            }
            1 | 3 | 5 => {
                let ch = reg / 2;
                self.audio_channels[ch].period =
                    (self.audio_channels[ch].period & 0x00FF) | (((val & 0x0F) as u16) << 8);
            }
            7 => {
                for (i, ch) in self.audio_channels.iter_mut().enumerate() {
                    ch.tone_disabled = (val >> i) & 1 != 0;
                }
            }
            8 | 9 | 10 => {
                self.audio_channels[reg - 8].volume = val & 0x0F;
            }
            _ => {}
        }
    }

    fn prg_read_8k(&self, bank: usize, offset: usize) -> u8 {
        let num_banks = self.prg_rom.len() / 0x2000;
        if num_banks == 0 { return 0; }
        self.prg_rom[(bank % num_banks) * 0x2000 + offset]
    }
}

impl Mapper for SunsoftFme7Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select {
                    if self.prg_ram_enable {
                        Some(self.prg_ram[addr as usize - 0x6000])
                    } else {
                        Some(0)
                    }
                } else {
                    Some(self.prg_read_8k(self.prg_banks[0], addr as usize - 0x6000))
                }
            }
            0x8000..=0x9FFF => Some(self.prg_read_8k(self.prg_banks[1], addr as usize - 0x8000)),
            0xA000..=0xBFFF => Some(self.prg_read_8k(self.prg_banks[2], addr as usize - 0xA000)),
            0xC000..=0xDFFF => Some(self.prg_read_8k(self.prg_banks[3], addr as usize - 0xC000)),
            0xE000..=0xFFFF => {
                let num_banks = self.prg_rom.len() / 0x2000;
                if num_banks == 0 { return None; }
                let last_bank = num_banks.saturating_sub(1);
                Some(self.prg_read_8k(last_bank, addr as usize - 0xE000))
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enable {
                    self.prg_ram[addr as usize - 0x6000] = val;
                }
            }
            0x8000..=0x9FFF => {
                self.command = val & 0x0F;
            }
            0xA000..=0xBFFF => {
                match self.command {
                    0x00..=0x07 => self.chr_banks[self.command as usize] = val as usize,
                    0x08 => {
                        self.prg_ram_enable = (val & 0x80) != 0;
                        self.prg_ram_select = (val & 0x40) != 0;
                        self.prg_banks[0] = (val & 0x3F) as usize;
                    }
                    0x09 => self.prg_banks[1] = (val & 0x3F) as usize,
                    0x0A => self.prg_banks[2] = (val & 0x3F) as usize,
                    0x0B => self.prg_banks[3] = (val & 0x3F) as usize,
                    0x0C => {
                        self.mirroring = match val & 0x03 {
                            0 => Mirroring::Vertical,
                            1 => Mirroring::Horizontal,
                            2 => Mirroring::OneScreenLow,
                            3 => Mirroring::OneScreenHigh,
                            _ => Mirroring::Vertical,
                        };
                    }
                    0x0D => {
                        self.irq_counter_enable = (val & 0x80) != 0;
                        self.irq_enable = (val & 0x01) != 0;
                        self.irq_pending = false;
                    }
                    0x0E => {
                        self.irq_counter = (self.irq_counter & 0xFF00) | (val as u16);
                    }
                    0x0F => {
                        self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8);
                    }
                    _ => {}
                }
            }
            0xC000..=0xDFFF => self.audio_reg_select = val & 0x0F,
            0xE000..=0xFFFF => self.write_audio_data(val),
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16, _is_sprite: bool) -> Option<u8> {
        if addr >= 0x2000 { return None; }
        if self.chr_is_ram {
            return Some(self.chr_ram[addr as usize]);
        }
        let num_banks = self.chr_rom.len() / 0x0400;
        if num_banks == 0 { return Some(0); }

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
        // Tone generators clock at CPU / 16 per the 5B spec.
        self.audio_prescaler = self.audio_prescaler.wrapping_add(1);
        if self.audio_prescaler & 0x0F == 0 {
            for ch in &mut self.audio_channels {
                ch.clock();
            }
        }

        if !self.irq_counter_enable {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_counter = 0xFFFF;
            if self.irq_enable {
                self.irq_pending = true;
            }
        } else {
            self.irq_counter -= 1;
        }
    }

    fn expansion_audio_sample(&self) -> f32 {
        let sum: u32 = self
            .audio_channels
            .iter()
            .map(|c| c.output() as u32)
            .sum();
        // Three 4-bit channels → max sum = 45. Scale to ~0.12 peak, matching
        // the VRC6 mixer amplitude.
        (sum as f32 / 45.0) * 0.12
    }

    fn check_irq(&self) -> bool {
        self.irq_pending
    }

    fn save_mapper_state(&self) -> MapperState {
        MapperState::SunsoftFme7 {
            command: self.command,
            chr_banks: self.chr_banks.to_vec(),
            prg_banks: self.prg_banks.to_vec(),
            prg_ram_enable: self.prg_ram_enable,
            prg_ram_select: self.prg_ram_select,
            mirroring: match self.mirroring {
                Mirroring::Vertical => 0,
                Mirroring::Horizontal => 1,
                Mirroring::OneScreenLow => 2,
                Mirroring::OneScreenHigh => 3,
            },
            irq_counter: self.irq_counter,
            irq_enable: self.irq_enable,
            irq_counter_enable: self.irq_counter_enable,
            irq_pending: self.irq_pending,
        }
    }

    fn load_mapper_state(&mut self, state: &MapperState) {
        if let MapperState::SunsoftFme7 {
            command, chr_banks, prg_banks, prg_ram_enable, prg_ram_select, mirroring,
            irq_counter, irq_enable, irq_counter_enable, irq_pending
        } = state {
            self.command = *command;
            if chr_banks.len() == 8 {
                self.chr_banks.copy_from_slice(chr_banks);
            }
            if prg_banks.len() == 4 {
                self.prg_banks.copy_from_slice(prg_banks);
            }
            self.prg_ram_enable = *prg_ram_enable;
            self.prg_ram_select = *prg_ram_select;
            self.mirroring = match mirroring {
                0 => Mirroring::Vertical,
                1 => Mirroring::Horizontal,
                2 => Mirroring::OneScreenLow,
                3 => Mirroring::OneScreenHigh,
                _ => Mirroring::Vertical,
            };
            self.irq_counter = *irq_counter;
            self.irq_enable = *irq_enable;
            self.irq_counter_enable = *irq_counter_enable;
            self.irq_pending = *irq_pending;
        }
    }
}
