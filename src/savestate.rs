//! Save state serialization for the NES emulator.

use serde::{Deserialize, Serialize};

const SAVE_STATE_MAGIC: &[u8; 8] = b"TNES_SAV";
const SAVE_STATE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct SaveState {
    pub magic: [u8; 8],
    pub version: u32,
    pub cpu: CpuState,
    pub ppu: PpuState,
    pub apu: ApuState,
    pub bus: BusState,
    pub joypad1: JoypadState,
    pub joypad2: JoypadState,
    pub mapper: MapperState,
}

#[derive(Serialize, Deserialize)]
pub struct CpuState {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub remaining_cycles: u8,
    pub total_cycles: u64,
}

#[derive(Serialize, Deserialize)]
pub struct PpuState {
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub oam_addr: u8,
    pub vram_addr: u16,
    pub temp_vram_addr: u16,
    pub fine_x: u8,
    pub write_latch: bool,
    pub data_buffer: u8,
    pub oam: Vec<u8>,
    pub vram: Vec<u8>,
    pub palette: Vec<u8>,
    pub chr_ram: Vec<u8>,
    pub scanline: i16,
    pub cycle: u16,
    pub nmi_triggered: bool,
    pub odd_frame: bool,
    pub last_write: u8,
}

#[derive(Serialize, Deserialize)]
pub struct ApuState {
    pub cycle: u32,
    pub mode: bool,
    pub irq_inhibit: bool,
    pub frame_interrupt: bool,
    pub pending_reset_cycles: u8,
    pub pending_mode: bool,
    pub pending_irq_inhibit: bool,
    pub even_cycle: bool,
    pub pulse1: PulseState,
    pub pulse2: PulseState,
    pub triangle: TriangleState,
    pub noise: NoiseState,
    pub dmc: DmcState,
}

#[derive(Serialize, Deserialize)]
pub struct PulseState {
    pub enabled: bool,
    pub is_channel_1: bool,
    pub duty: u8,
    pub duty_pos: u8,
    pub timer_period: u16,
    pub timer_counter: u16,
    pub length_counter: u8,
    pub length_halt: bool,
    pub envelope_start: bool,
    pub envelope_divider: u8,
    pub envelope_decay: u8,
    pub constant_volume: bool,
    pub volume: u8,
    pub sweep_enabled: bool,
    pub sweep_period: u8,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    pub sweep_divider: u8,
    pub sweep_reload: bool,
}

#[derive(Serialize, Deserialize)]
pub struct TriangleState {
    pub enabled: bool,
    pub timer_period: u16,
    pub timer_counter: u16,
    pub sequence_pos: u8,
    pub length_counter: u8,
    pub length_halt: bool,
    pub linear_counter: u8,
    pub linear_counter_reload: u8,
    pub linear_counter_reload_flag: bool,
}

#[derive(Serialize, Deserialize)]
pub struct NoiseState {
    pub enabled: bool,
    pub timer_period: u16,
    pub timer_counter: u16,
    pub mode: bool,
    pub shift_register: u16,
    pub length_counter: u8,
    pub length_halt: bool,
    pub envelope_start: bool,
    pub envelope_divider: u8,
    pub envelope_decay: u8,
    pub constant_volume: bool,
    pub volume: u8,
}

#[derive(Serialize, Deserialize)]
pub struct DmcState {
    pub output_level: u8,
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
}

#[derive(Serialize, Deserialize)]
pub struct BusState {
    pub cpu_ram: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub dma_page: u8,
    pub dma_active: bool,
    pub dma_addr: u8,
    pub total_cycles: usize,
}

#[derive(Serialize, Deserialize)]
pub struct JoypadState {
    pub strobe: bool,
    pub button_index: u8,
    pub button_state: u8,
}

#[derive(Serialize, Deserialize)]
pub enum MapperState {
    Nrom,
    Axrom {
        prg_bank: usize,
        chr_ram: Vec<u8>,
        mirroring: u8,
    },
    Mmc5 {
        prg_mode: u8,
        chr_mode: u8,
        prg_banks: Vec<usize>,
        irq_target: u8,
        irq_enable: bool,
        irq_pending: bool,
        in_frame: bool,
        scanline_counter: u8,
    },
    Vrc6 {
        prg_bank_16k: usize,
        prg_bank_8k: usize,
        chr_banks: Vec<usize>,
        mirroring: u8,
        audio_control: u8,
        pulse1_regs: Vec<u8>,
        pulse1_timer: u16,
        pulse1_step: u8,
        pulse2_regs: Vec<u8>,
        pulse2_timer: u16,
        pulse2_step: u8,
        saw_regs: Vec<u8>,
        saw_timer: u16,
        saw_step: u8,
        saw_accumulator: u8,
        irq_latch: u8,
        irq_counter: u8,
        irq_mode_cycle: bool,
        irq_enable: bool,
        irq_enable_after_ack: bool,
        irq_pending: bool,
        irq_prescaler: i16,
    },
    SunsoftFme7 {
        command: u8,
        chr_banks: Vec<usize>,
        prg_banks: Vec<usize>,
        prg_ram_enable: bool,
        prg_ram_select: bool,
        mirroring: u8,
        irq_counter: u16,
        irq_enable: bool,
        irq_counter_enable: bool,
        irq_pending: bool,
    },
    Unrom {
        bank_select: usize,
        chr_ram: Vec<u8>,
    },
    Cnrom {
        bank_select: usize,
    },
    Mmc1 {
        prg_ram: Vec<u8>,
        chr_ram: Vec<u8>,
        shift_register: u8,
        write_count: u8,
        control: u8,
        chr_bank_0: u8,
        chr_bank_1: u8,
        prg_bank: u8,
        mirroring: u8,
    },
    Mmc3 {
        prg_ram: Vec<u8>,
        chr_ram: Vec<u8>,
        registers: Vec<u8>,
        bank_select: u8,
        prg_mode: bool,
        chr_mode: bool,
        mirroring: u8,
        irq_latch: u8,
        irq_counter: u8,
        irq_enabled: bool,
        irq_reload: bool,
        irq_pending: bool,
    },
    Mmc2 {
        prg_bank: u8,
        chr_bank_0_l: u8,
        chr_bank_0_r: u8,
        chr_bank_1_l: u8,
        chr_bank_1_r: u8,
        latch_0: bool,
        latch_1: bool,
        mirroring: u8,
    },
}

impl SaveState {
    pub fn new(
        cpu: CpuState,
        ppu: PpuState,
        apu: ApuState,
        bus: BusState,
        joypad1: JoypadState,
        joypad2: JoypadState,
        mapper: MapperState,
    ) -> Self {
        Self {
            magic: *SAVE_STATE_MAGIC,
            version: SAVE_STATE_VERSION,
            cpu,
            ppu,
            apu,
            bus,
            joypad1,
            joypad2,
            mapper,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("Failed to serialize save state: {e}"))
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        let state: Self = bincode::deserialize(data)
            .map_err(|e| format!("Failed to deserialize save state: {e}"))?;
        if &state.magic != SAVE_STATE_MAGIC {
            return Err("Not a valid save state file".into());
        }
        if state.version != SAVE_STATE_VERSION {
            return Err(format!(
                "Save state version mismatch: expected {}, got {}",
                SAVE_STATE_VERSION, state.version
            ));
        }
        Ok(state)
    }
}
