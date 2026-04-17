pub mod apu;
pub mod audio;
pub mod bus;
pub mod cartridge;
pub mod cpu;
pub mod input;
pub mod ppu;
pub mod remote_audio;
pub mod renderer;
pub mod savestate;

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
                    self.cpu.bus.cartridge.tick_cpu();
                    let expansion_audio = self.cpu.bus.cartridge.expansion_audio_sample();
                    self.cpu.bus.apu.set_expansion_audio_input(expansion_audio);
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

    /// Capture the full emulator state for save/load.
    pub fn save_state(&self) -> savestate::SaveState {
        savestate::SaveState::new(
            self.cpu.capture_state(),
            self.cpu.bus.ppu.capture_state(),
            self.cpu.bus.apu.capture_state(),
            self.cpu.bus.capture_state(),
            self.cpu.bus.joypad1.capture_state(),
            self.cpu.bus.joypad2.capture_state(),
            self.cpu.bus.cartridge.save_mapper_state(),
        )
    }

    /// Restore emulator state from a save state.
    pub fn load_state(&mut self, state: &savestate::SaveState) {
        self.cpu.restore_state(&state.cpu);
        self.cpu.bus.ppu.restore_state(&state.ppu);
        self.cpu.bus.apu.restore_state(&state.apu);
        self.cpu.bus.restore_state(&state.bus);
        self.cpu.bus.joypad1.restore_state(&state.joypad1);
        self.cpu.bus.joypad2.restore_state(&state.joypad2);
        self.cpu.bus.cartridge.load_mapper_state(&state.mapper);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_rom() -> Vec<u8> {
        let mut rom = Vec::new();
        rom.extend_from_slice(b"NES\x1A");
        rom.push(1); // 1 PRG bank
        rom.push(0); // 0 CHR banks
        rom.push(0); // mapper 0, horizontal mirroring
        rom.push(0);
        rom.extend_from_slice(&[0u8; 8]);
        // PRG: fill with NOPs (0xEA), set reset vector to $8000
        let mut prg = vec![0xEA; 0x4000];
        prg[0x3FFC] = 0x00; // reset vector low
        prg[0x3FFD] = 0x80; // reset vector high
        rom.extend_from_slice(&prg);
        rom
    }

    #[test]
    fn save_state_round_trip() {
        let mut nes = Nes::from_ines_bytes(&make_test_rom()).unwrap();

        // Run a few frames to get into an interesting state
        nes.run_frames(5);

        // Capture state
        let state = nes.save_state();
        let bytes = state.to_bytes().unwrap();

        // Modify emulator state
        nes.run_frames(10);
        let pc_after = nes.cpu.pc;

        // Restore from bytes
        let restored = savestate::SaveState::from_bytes(&bytes).unwrap();
        nes.load_state(&restored);

        // PC should be back to the saved state, not the post-10-frame state
        assert_ne!(nes.cpu.pc, pc_after);
    }
}
