use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::input::Joypad;
use crate::ppu::Ppu;
use crate::savestate::BusState;

pub struct Bus {
    pub cpu_ram: [u8; 2048],
    pub ppu: Ppu,
    pub apu: Apu,
    pub cartridge: Cartridge,
    pub joypad1: Joypad,
    pub joypad2: Joypad,
    prg_ram: [u8; 8192], // $6000-$7FFF battery-backed RAM
    dma_page: u8,
    dma_active: bool,
    dma_addr: u8,
    pub total_cycles: usize,
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
        self.tick(1);
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu_register_read(addr & 0x2007),
            0x4014 => 0, // write-only
            0x4015 => self.apu.read_status(),
            0x4016 => self.joypad1.read(),
            0x4017 => self.joypad2.read(),
            0x4000..=0x4013 => 0,
            0x4018..=0x5FFF => self.cartridge.cpu_read(addr).unwrap_or(0),
            0x6000..=0x7FFF => self
                .cartridge
                .cpu_read(addr)
                .unwrap_or(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => self.cartridge.cpu_read(addr).unwrap_or(0),
        }
    }

    /// CPU writes a byte to the address space.
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        self.tick(1);
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
            0x4017 => self.apu.write_frame_counter(val, self.total_cycles),
            0x4000..=0x4013 => self.apu.write_register(addr, val),
            0x4018..=0x5FFF => self.cartridge.cpu_write(addr, val),
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
                self.cartridge.cpu_write(addr, val);
            }
            0x8000..=0xFFFF => self.cartridge.cpu_write(addr, val),
        }
    }

    // ── PPU register bridge ──────────────────────────────────────────────────

    fn ppu_register_read(&mut self, addr: u16) -> u8 {
        self.ppu.cpu_read(addr, &self.cartridge)
    }

    fn ppu_register_write(&mut self, addr: u16, val: u8) {
        self.cartridge.cpu_write(addr, val);
        self.ppu.cpu_write(addr, val, &mut self.cartridge)
    }

    // ── Timing ───────────────────────────────────────────────────────────────

    /// Advances the PPU by 3 cycles for each CPU cycle and clocks the APU frame counter.
    pub fn tick(&mut self, cpu_cycles: u8) {
        for _ in 0..cpu_cycles {
            self.cartridge.tick_cpu();
            self.ppu.tick(&mut self.cartridge);
            self.ppu.tick(&mut self.cartridge);
            self.ppu.tick(&mut self.cartridge);
            let expansion_audio = self.cartridge.expansion_audio_sample();
            self.apu.set_expansion_audio_input(expansion_audio);
            self.apu.tick(1);

            if self.apu.dmc.dma_request {
                self.apu.dmc.dma_request = false;
                let addr = self.apu.dmc.current_address;
                let val = self.peek(addr);
                self.apu.dmc.load_sample(val);
                self.tick(3);
            }

            self.total_cycles = self.total_cycles.wrapping_add(1);
        }
    }

    // ── OAM DMA ──────────────────────────────────────────────────────────────

    /// Executes a pending OAM DMA transfer. Returns true if a transfer ran.
    /// The CPU is halted for 513 or 514 cycles (extra cycle on odd CPU cycle alignment)
    /// while PPU/APU/mapper continue to tick. Ticking is done internally, so callers
    /// must NOT re-tick external components based on this call.
    /// Attribute bytes (OAM index % 4 == 2) have bits 2-4 hardwired to 0 on
    /// real hardware; mask on write so readback matches hardware.
    pub fn do_dma(&mut self) -> bool {
        if !self.dma_active {
            return false;
        }
        // 1 dummy wait cycle, plus a second if we started on an odd CPU cycle.
        self.tick(1);
        if self.total_cycles % 2 == 1 {
            self.tick(1);
        }
        let base = (self.dma_page as u16) << 8;
        for i in 0u16..256 {
            // Internal access to avoid double-tick in cpu_read/write
            let val = match base + i {
                0x0000..=0x1FFF => self.cpu_ram[((base + i) & 0x07FF) as usize],
                0x6000..=0x7FFF => self
                    .cartridge
                    .cpu_read(base + i)
                    .unwrap_or(self.prg_ram[(base + i - 0x6000) as usize]),
                0x8000..=0xFFFF => self.cartridge.cpu_read(base + i).unwrap_or(0),
                _ => 0,
            };
            self.tick(1); // Read cycle
            let oam_idx = self.ppu.oam_addr.wrapping_add(i as u8) as usize;
            let masked = if oam_idx % 4 == 2 { val & 0xE3 } else { val };
            self.ppu.oam[oam_idx] = masked;
            self.tick(1); // Write cycle
        }
        self.dma_active = false;
        true
    }

    // ── Debug ────────────────────────────────────────────────────────────────

    /// Peek at a byte without side effects (for debug tracing).
    /// Avoids PPU/joypad register reads that would alter state.
    pub fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            0x4018..=0x5FFF => self.cartridge.cpu_read(addr).unwrap_or(0),
            0x6000..=0x7FFF => self
                .cartridge
                .cpu_read(addr)
                .unwrap_or(self.prg_ram[(addr - 0x6000) as usize]),
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
        self.apu.frame_interrupt || self.apu.dmc.irq_pending || self.cartridge.check_irq()
    }

    /// Load battery-backed RAM from a `.sav` file if it exists.
    pub fn load_battery_save(&mut self) {
        if !self.cartridge.has_battery {
            return;
        }
        if let Some(path) = self.cartridge.sav_path() {
            if path.exists() {
                match std::fs::read(&path) {
                    Ok(data) if data.len() == 8192 => {
                        self.prg_ram.copy_from_slice(&data);
                        eprintln!("Loaded save: {}", path.display());
                    }
                    Ok(data) => {
                        eprintln!(
                            "Save file wrong size ({}), ignoring: {}",
                            data.len(),
                            path.display()
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to load save: {}", e);
                    }
                }
            }
        }
    }

    /// Write battery-backed RAM to a `.sav` file.
    pub fn save_battery(&self) {
        if !self.cartridge.has_battery {
            return;
        }
        if let Some(path) = self.cartridge.sav_path() {
            match std::fs::write(&path, &self.prg_ram) {
                Ok(()) => eprintln!("Saved: {}", path.display()),
                Err(e) => eprintln!("Failed to save: {}", e),
            }
        }
    }

    pub fn capture_state(&self) -> BusState {
        BusState {
            cpu_ram: self.cpu_ram.to_vec(),
            prg_ram: self.prg_ram.to_vec(),
            dma_page: self.dma_page,
            dma_active: self.dma_active,
            dma_addr: self.dma_addr,
            total_cycles: self.total_cycles,
        }
    }

    pub fn restore_state(&mut self, s: &BusState) {
        if s.cpu_ram.len() == self.cpu_ram.len() {
            self.cpu_ram.copy_from_slice(&s.cpu_ram);
        }
        if s.prg_ram.len() == self.prg_ram.len() {
            self.prg_ram.copy_from_slice(&s.prg_ram);
        }
        self.dma_page = s.dma_page;
        self.dma_active = s.dma_active;
        self.dma_addr = s.dma_addr;
        self.total_cycles = s.total_cycles;
    }
}
