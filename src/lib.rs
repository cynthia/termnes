pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod cpu;
pub mod input;
pub mod ppu;
pub mod renderer;

use std::path::Path;

/// Headless emulator facade for integration tests, benchmarks, and tools.
/// Owns a `Cpu` (which owns the `Bus`) and drives them together.
pub struct Nes {
    pub cpu: cpu::Cpu,
}

impl Nes {
    /// Build from iNES bytes and run the power-on reset sequence.
    pub fn from_ines_bytes(bytes: &[u8]) -> Result<Self, String> {
        let cart = cartridge::Cartridge::from_ines(bytes)?;
        let bus = bus::Bus::new(cart);
        let mut cpu = cpu::Cpu::new(bus);
        cpu.reset();
        Ok(Self { cpu })
    }

    /// Convenience: load an iNES file from disk.
    pub fn from_ines_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_ines_bytes(&bytes)
    }

    /// Drive CPU + PPU + APU until the PPU signals a completed frame.
    /// Handles OAM DMA (513 CPU cycles halt CPU but keep PPU/APU running).
    pub fn step_frame(&mut self) {
        while !self.cpu.bus.ppu.frame_complete {
            let dma_cycles = self.cpu.bus.do_dma();
            if dma_cycles > 0 {
                for _ in 0..dma_cycles as u32 {
                    for _ in 0..3 {
                        self.cpu.bus.ppu.tick(&mut self.cpu.bus.cartridge);
                    }
                    self.cpu.bus.apu.tick(1);
                }
                if self.cpu.bus.poll_nmi() {
                    self.cpu.nmi();
                }
                if self.cpu.bus.poll_irq() {
                    self.cpu.irq();
                }
                continue;
            }
            self.cpu.step();
        }
        self.cpu.bus.ppu.frame_complete = false;
    }

    /// Step individual CPU instructions — useful for PC-driven tests like
    /// nestest. Does NOT handle OAM DMA halt, so avoid in games that trigger it.
    pub fn step_instruction(&mut self) {
        self.cpu.step();
    }

    /// Step `frames` frames, then stop. Returns the number of frames executed
    /// (always equal to `frames` — exists as a symmetric counterpart to
    /// `run_until`).
    pub fn run_frames(&mut self, frames: u64) -> u64 {
        for _ in 0..frames {
            self.step_frame();
        }
        frames
    }

    /// Run frames until `cond` returns true or `max_frames` is reached.
    /// Returns the number of frames actually executed.
    pub fn run_until<F: FnMut(&Self) -> bool>(&mut self, mut cond: F, max_frames: u64) -> u64 {
        for i in 0..max_frames {
            if cond(self) {
                return i;
            }
            self.step_frame();
        }
        max_frames
    }

    /// Side-effect-free read into the CPU bus. Safe for test observers to
    /// poll memory-mapped status locations without disturbing emulator state.
    pub fn peek(&self, addr: u16) -> u8 {
        self.cpu.bus.peek(addr)
    }
}
