//! NES APU — frame counter, five sound channels, mixing, and sample output.

use crate::savestate::ApuState;

mod dmc;
mod filters;
mod noise;
mod pulse;
mod triangle;

use dmc::Dmc;
use filters::{HighPassFilter, LowPassFilter};
use noise::Noise;
use pulse::Pulse;
use triangle::Triangle;

/// NES length counter lookup table. Indexed by the high 5 bits of the 4th
/// register of any length-counter channel (pulse1/2 $4003/$4007, triangle
/// $400B, noise $400F).
pub(crate) const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

/// NTSC CPU frequency in Hz.
const CPU_FREQ: f64 = 1_789_773.0;
/// Post-filter master gain. The NES pulse+TND non-linear mixer peaks around
/// 0.55 and average playback (one pulse + triangle) lands near 0.2, so the
/// emulator was output ing a signal well below unity. Multiplying by 1.8
/// brings a loud game's peak close to full scale while still leaving some
/// headroom for expansion audio; the final `clamp(-1, 1)` catches spikes.
const MASTER_GAIN: f32 = 1.8;

#[derive(Clone)]
pub struct Apu {
    // ── Frame counter ─────────────────────────────────────────────────────────
    cycle: u32,
    /// false = 4-step, true = 5-step
    mode: bool,
    irq_inhibit: bool,
    pub frame_interrupt: bool,

    pending_reset_cycles: u8,
    pending_mode: bool,
    pending_irq_inhibit: bool,

    // ── Channels ──────────────────────────────────────────────────────────────
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    pub dmc: Dmc,

    // ── Sample generation ─────────────────────────────────────────────────────
    even_cycle: bool,
    sample_rate: u32,
    sample_counter: f64,
    cycles_per_sample: f64,
    sample_accumulator: f32,
    sample_acc_count: u32,
    sample_buffer: Vec<f32>,
    expansion_audio_input: f32,

    // ── Output filters (NES hardware path) ────────────────────────────────────
    hp1: HighPassFilter,
    hp2: HighPassFilter,
    lp: LowPassFilter,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            cycle: 0,
            mode: false,
            irq_inhibit: true,
            frame_interrupt: false,

            pending_reset_cycles: 0,
            pending_mode: false,
            pending_irq_inhibit: true,

            pulse1: Pulse::new(true),
            pulse2: Pulse::new(false),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: Dmc::new(),

            even_cycle: false,
            sample_rate: 0,
            sample_counter: 0.0,
            cycles_per_sample: 0.0,
            sample_accumulator: 0.0,
            sample_acc_count: 0,
            sample_buffer: Vec::new(),
            expansion_audio_input: 0.0,

            // Nesdev documents three filters (90 Hz HP, 440 Hz HP, 14 kHz
            // LP) as matching a stock Famicom's RF-modulated output. The
            // 440 Hz HP in particular is an artifact of that output stage
            // and rolls off everything below mid-band by ~17 dB at 60 Hz,
            // which makes bass basically disappear. Keeping the DC blocker
            // HP at 90 Hz and dropping the second HP to a very gentle
            // 37 Hz (just enough to suppress slow envelope drift) matches
            // what most modern emulators do by default and preserves the
            // bass that composers expect to hear.
            hp1: HighPassFilter::new(90.0, 44100.0),
            hp2: HighPassFilter::new(37.0, 44100.0),
            lp: LowPassFilter::new(14000.0, 44100.0),
        }
    }

    /// Enable audio sample generation at the given sample rate (e.g. 44100).
    /// Call once before starting emulation; samples are buffered internally
    /// and drained with [`drain_samples`].
    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate;
        self.cycles_per_sample = CPU_FREQ / sample_rate as f64;
        let sr = sample_rate as f32;
        self.hp1 = HighPassFilter::new(90.0, sr);
        self.hp2 = HighPassFilter::new(37.0, sr);
        self.lp = LowPassFilter::new(14000.0, sr);
    }

    /// Take all buffered audio samples, leaving the internal buffer empty.
    pub fn drain_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.sample_buffer)
    }

    pub fn set_expansion_audio_input(&mut self, sample: f32) {
        self.expansion_audio_input = sample;
    }

    // ── Tick ──────────────────────────────────────────────────────────────────

    /// Advance the APU by `cpu_cycles` CPU cycles.
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

            // Clock channel timers ──────────────────────────────────────────
            // Triangle timer runs at CPU rate; pulse & noise at half-rate.
            self.triangle.clock_timer();
            self.dmc.clock_timer();
            self.even_cycle = !self.even_cycle;
            if self.even_cycle {
                self.pulse1.clock_timer();
                self.pulse2.clock_timer();
                self.noise.clock_timer();
            }

            // Frame counter sequencer ──────────────────────────────────────
            self.cycle += 1;
            if !self.mode {
                // 4-step mode
                match self.cycle {
                    7457 => self.clock_quarter_frame(),
                    14913 => self.clock_half_frame(),
                    22371 => self.clock_quarter_frame(),
                    29828..=29832 => {
                        if !self.irq_inhibit {
                            self.frame_interrupt = true;
                        }
                        if self.cycle >= 29829 && self.cycle <= 29830 {
                            self.clock_half_frame();
                        }
                        if self.cycle >= 29830 {
                            self.cycle = 0;
                        }
                    }
                    _ => {}
                }
            } else {
                // 5-step mode — no IRQ; half-frame at 14913 and 37281.
                match self.cycle {
                    7457 => self.clock_quarter_frame(),
                    14913 => self.clock_half_frame(),
                    22371 => self.clock_quarter_frame(),
                    37281 => self.clock_half_frame(),
                    37282 => self.cycle = 0,
                    _ => {}
                }
            }

            // Audio sample generation (box-filter averaging) ──────────────
            if self.sample_rate > 0 {
                self.sample_accumulator += self.mix();
                self.sample_acc_count += 1;
                self.sample_counter += 1.0;
                if self.sample_counter >= self.cycles_per_sample {
                    self.sample_counter -= self.cycles_per_sample;
                    let avg = self.sample_accumulator / self.sample_acc_count as f32;
                    self.sample_accumulator = 0.0;
                    self.sample_acc_count = 0;
                    let filtered = self.lp.apply(self.hp2.apply(self.hp1.apply(avg)));
                    // Master gain: the non-linear 2A03 mixer peaks around
                    // 0.55 with both pulses + triangle + noise + DMC at
                    // max, which leaves a lot of headroom on the [-1, 1]
                    // cpal output. Boost so typical playback sits closer
                    // to unity, then soft-clip to avoid popping on peaks.
                    let boosted = filtered * MASTER_GAIN;
                    self.sample_buffer.push(boosted.clamp(-1.0, 1.0));
                }
            }
        }
    }

    // ── Frame-counter clocking events ─────────────────────────────────────────

    /// Quarter-frame: clock envelopes and triangle linear counter.
    fn clock_quarter_frame(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear_counter();
        self.noise.clock_envelope();
    }

    /// Half-frame: quarter-frame events + length counters + sweep units.
    fn clock_half_frame(&mut self) {
        self.clock_quarter_frame();
        self.pulse1.clock_length_counter();
        self.pulse2.clock_length_counter();
        self.triangle.clock_length_counter();
        self.noise.clock_length_counter();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    // ── Mixing ────────────────────────────────────────────────────────────────

    /// Non-linear NES mixer (lookup-table approximation from nesdev wiki).
    fn mix(&self) -> f32 {
        let p1 = self.pulse1.output() as f32;
        let p2 = self.pulse2.output() as f32;
        let tri = self.triangle.output() as f32;
        let noi = self.noise.output() as f32;
        let dmc = self.dmc.output() as f32;

        let pulse_out = if p1 + p2 > 0.0 {
            95.88 / (8128.0 / (p1 + p2) + 100.0)
        } else {
            0.0
        };

        let tnd_sum = tri / 8227.0 + noi / 12241.0 + dmc / 22638.0;
        let tnd_out = if tnd_sum > 0.0 {
            159.79 / (1.0 / tnd_sum + 100.0)
        } else {
            0.0
        };

        pulse_out + tnd_out + self.expansion_audio_input
    }

    // ── Register dispatch ─────────────────────────────────────────────────────

    /// Handles writes to $4000–$4013 (per-channel APU registers).
    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write_register((addr - 0x4000) as u8, val),
            0x4004..=0x4007 => self.pulse2.write_register((addr - 0x4004) as u8, val),
            0x4008..=0x400B => self.triangle.write_register((addr - 0x4008) as u8, val),
            0x400C..=0x400F => self.noise.write_register((addr - 0x400C) as u8, val),
            0x4010..=0x4013 => self.dmc.write_register((addr - 0x4010) as u8, val),
            _ => {}
        }
    }

    /// $4015 write: channel enable bits. Clearing a channel's bit zeroes its
    /// length counter immediately.
    pub fn write_status(&mut self, val: u8) {
        self.pulse1.set_enabled(val & 0x01 != 0);
        self.pulse2.set_enabled(val & 0x02 != 0);
        self.triangle.set_enabled(val & 0x04 != 0);
        self.noise.set_enabled(val & 0x08 != 0);
        self.dmc.write_status(val);
    }

    /// $4015 read: bits 0–3 = length counter > 0 per channel; bit 6 = frame
    /// interrupt. Clears the frame-interrupt flag as a side effect.
    pub fn read_status(&mut self) -> u8 {
        let mut r = 0u8;
        if self.pulse1.length_counter > 0 {
            r |= 0x01;
        }
        if self.pulse2.length_counter > 0 {
            r |= 0x02;
        }
        if self.triangle.length_counter > 0 {
            r |= 0x04;
        }
        if self.noise.length_counter > 0 {
            r |= 0x08;
        }
        if self.dmc.current_length > 0 {
            r |= 0x10;
        }
        if self.frame_interrupt {
            r |= 0x40;
        }
        if self.dmc.irq_pending {
            r |= 0x80;
        }
        self.frame_interrupt = false;
        r
    }

    /// $4017 write: bit 7 = mode (0=4-step, 1=5-step), bit 6 = IRQ inhibit.
    /// Resets the frame-counter cycle. In 5-step mode a half-frame is
    /// immediately generated; in 4-step it is not.
    pub fn write_frame_counter(&mut self, val: u8, total_cycles: usize) {
        self.pending_mode = val & 0x80 != 0;
        self.pending_irq_inhibit = val & 0x40 != 0;
        // Delay of 3 or 4 cycles depending on alignment
        self.pending_reset_cycles = if total_cycles % 2 != 0 { 3 } else { 4 };
    }

    pub fn capture_state(&self) -> ApuState {
        ApuState {
            cycle: self.cycle,
            mode: self.mode,
            irq_inhibit: self.irq_inhibit,
            frame_interrupt: self.frame_interrupt,
            pending_reset_cycles: self.pending_reset_cycles,
            pending_mode: self.pending_mode,
            pending_irq_inhibit: self.pending_irq_inhibit,
            even_cycle: self.even_cycle,
            pulse1: self.pulse1.capture_state(),
            pulse2: self.pulse2.capture_state(),
            triangle: self.triangle.capture_state(),
            noise: self.noise.capture_state(),
            dmc: self.dmc.capture_state(),
        }
    }

    pub fn restore_state(&mut self, s: &ApuState) {
        self.cycle = s.cycle;
        self.mode = s.mode;
        self.irq_inhibit = s.irq_inhibit;
        self.frame_interrupt = s.frame_interrupt;
        self.pending_reset_cycles = s.pending_reset_cycles;
        self.pending_mode = s.pending_mode;
        self.pending_irq_inhibit = s.pending_irq_inhibit;
        self.even_cycle = s.even_cycle;
        self.pulse1.restore_state(&s.pulse1);
        self.pulse2.restore_state(&s.pulse2);
        self.triangle.restore_state(&s.triangle);
        self.noise.restore_state(&s.noise);
        self.dmc.restore_state(&s.dmc);
    }
}
