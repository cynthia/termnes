use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::input::Joypad;
use crate::ppu::Ppu;

pub struct Bus {
    pub cpu_ram: [u8; 2048],
    pub ppu: Ppu,
    pub apu: Apu,
    pub cartridge: Cartridge,
    pub joypad1: Joypad,
    pub joypad2: Joypad,
    prg_ram: [u8; 8192],   // $6000-$7FFF battery-backed RAM
    dma_page: u8,
    dma_active: bool,
    dma_addr: u8,
    total_cycles: usize,
}

impl Bus {
    pub fn new(cartridge: Cartridge) -> Self {
        let mirroring = cartridge.mirroring;
        Self {
            cpu_ram: [0; 2048],
            ppu: Ppu::new(mirroring),
            apu: Apu::new(),
            cartridge,
            joypad1: Joypad::new(),
            joypad2: Joypad::new(),
            prg_ram: [0; 8192],
            dma_page: 0,
            dma_active: false,
            dma_addr: 0,
            total_cycles: 0,
        }
    }

    /// CPU reads a byte from the address space.
    pub fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu_register_read(addr & 0x2007),
            0x4014 => 0, // write-only
            0x4015 => self.apu.read_status(),
            0x4016 => self.joypad1.read(),
            0x4017 => self.joypad2.read(),
            0x4000..=0x4013 | 0x4018..=0x5FFF => 0, // APU/expansion stubs
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => self.cartridge.cpu_read(addr).unwrap_or(0),
        }
    }

    /// CPU writes a byte to the address space.
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize] = val,
            0x2000..=0x3FFF => self.ppu_register_write(addr & 0x2007, val),
            0x4014 => {
                self.dma_page = val;
                self.dma_active = true;
                self.dma_addr = 0;
            }
            0x4015 => self.apu.write_status(val),
            0x4016 => {
                self.joypad1.write(val);
                self.joypad2.write(val);
            }
            0x4017 => self.apu.write_frame_counter(val),
            0x4000..=0x4013 => self.apu.write_register(addr, val),
            0x4018..=0x5FFF => {} // expansion ROM stubs
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = val,
            0x8000..=0xFFFF => self.cartridge.cpu_write(addr, val),
        }
    }

    // ── PPU register bridge ──────────────────────────────────────────────────

    fn ppu_register_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x2002 => self.ppu.read_status(),
            0x2004 => self.ppu.read_oam_data(),
            0x2007 => self.ppu.read_data(&self.cartridge),
            _ => 0, // write-only registers return 0
        }
    }

    fn ppu_register_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x2000 => self.ppu.write_ctrl(val),
            0x2001 => self.ppu.write_mask(val),
            0x2003 => self.ppu.write_oam_addr(val),
            0x2004 => self.ppu.write_oam_data(val),
            0x2005 => self.ppu.write_scroll(val),
            0x2006 => self.ppu.write_addr(val),
            0x2007 => self.ppu.write_data(val, &mut self.cartridge),
            _ => {}
        }
    }

    // ── Timing ───────────────────────────────────────────────────────────────

    /// Advances the PPU by 3 cycles for each CPU cycle and clocks the APU frame counter.
    pub fn tick(&mut self, cpu_cycles: u8) {
        for _ in 0..(cpu_cycles as usize * 3) {
            self.ppu.tick(&mut self.cartridge);
        }
        self.apu.tick(cpu_cycles);
        self.total_cycles = self.total_cycles.wrapping_add(cpu_cycles as usize);
    }

    // ── OAM DMA ──────────────────────────────────────────────────────────────

    /// Executes a pending OAM DMA transfer. Returns cycles consumed (0 if none pending).
    /// Attribute bytes (OAM index % 4 == 2) have bits 2-4 hardwired to 0 on
    /// real hardware; mask on write so readback matches hardware.
    pub fn do_dma(&mut self) -> u16 {
        if !self.dma_active {
            return 0;
        }
        let base = (self.dma_page as u16) << 8;
        for i in 0u16..256 {
            let val = self.cpu_read(base + i);
            let oam_idx = self.ppu.oam_addr.wrapping_add(i as u8) as usize;
            let masked = if oam_idx % 4 == 2 { val & 0xE3 } else { val };
            self.ppu.oam[oam_idx] = masked;
        }
        self.dma_active = false;
        513
    }

    // ── Debug ────────────────────────────────────────────────────────────────

    /// Peek at a byte without side effects (for debug tracing).
    /// Avoids PPU/joypad register reads that would alter state.
    pub fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => self.cartridge.cpu_read(addr).unwrap_or(0),
            _ => 0,
        }
    }

    // ── Interrupt polling ────────────────────────────────────────────────────

    /// Returns true (and clears the flag) if the PPU wants to fire an NMI.
    pub fn poll_nmi(&mut self) -> bool {
        if self.ppu.nmi_triggered {
            self.ppu.nmi_triggered = false;
            true
        } else {
            false
        }
    }

    /// Returns true if the APU IRQ line is currently asserted.
    pub fn poll_irq(&self) -> bool {
        self.apu.frame_interrupt || self.cartridge.check_irq()
    }
}
