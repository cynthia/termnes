/// Delta Modulation Channel — minimal implementation.
///
/// Only direct-load via $4011 is supported. Full DMA-based sample playback
/// requires the APU to issue reads on the CPU bus, which is architecturally
/// complex (the APU is owned by the Bus). Games that use DMC for percussion
/// will be silent on that channel, but the output level register still works
/// for volume tricks (e.g. some games write $4011 directly for crude PCM).
#[derive(Clone)]
pub struct Dmc {
    pub output_level: u8, // 7-bit (0–127)
}

impl Dmc {
    pub fn new() -> Self {
        Self { output_level: 0 }
    }

    pub fn write_register(&mut self, reg: u8, val: u8) {
        match reg {
            1 => {
                // $4011: direct load (bits 6–0)
                self.output_level = val & 0x7F;
            }
            0 | 2 | 3 => {} // TODO: IRQ, loop, rate, sample addr/length
            _ => {}
        }
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }
}
