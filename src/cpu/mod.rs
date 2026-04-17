pub mod addressing;
pub mod opcodes;

use bitflags::bitflags;
use crate::bus::Bus;
use crate::savestate::CpuState;
use addressing::AddressingMode;
use opcodes::{Opcode, decode};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CpuFlags: u8 {
        const CARRY     = 0b0000_0001;
        const ZERO      = 0b0000_0010;
        const IRQ_DIS   = 0b0000_0100;
        const DECIMAL   = 0b0000_1000;
        const BREAK     = 0b0001_0000;
        const UNUSED    = 0b0010_0000;
        const OVERFLOW  = 0b0100_0000;
        const NEGATIVE  = 0b1000_0000;
    }
}

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: CpuFlags,
    pub bus: Bus,
    remaining_cycles: u8,
    total_cycles: u64,
}

impl Cpu {
    pub fn new(bus: Bus) -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            status: CpuFlags::UNUSED | CpuFlags::IRQ_DIS,
            bus,
            remaining_cycles: 0,
            total_cycles: 0,
        }
    }

    /// Reads the reset vector ($FFFC-$FFFD) and initialises registers.
    pub fn reset(&mut self) {
        let lo = self.bus.cpu_read(0xFFFC) as u16;
        let hi = self.bus.cpu_read(0xFFFD) as u16;
        self.pc = (hi << 8) | lo;
        self.sp = 0xFD;
        self.status = CpuFlags::UNUSED | CpuFlags::IRQ_DIS;
        self.remaining_cycles = 8;
    }

    /// Fetches, decodes, and executes one instruction. Returns cycles consumed.
    pub fn step(&mut self) -> u8 {
        let start_cycles = self.bus.total_cycles;

        let opcode_byte = self.bus.cpu_read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        let info = decode(opcode_byte);
        let (addr, page_crossed) = self.resolve_addr(info.mode);
        let extra = self.execute(info.opcode, info.mode, addr, page_crossed, info.extra_cycle);

        let target_cycles = info.cycles + extra;
        let mut consumed = (self.bus.total_cycles - start_cycles) as u8;

        // Tick any remaining "idle" cycles for this instruction
        while consumed < target_cycles {
            self.bus.tick(1);
            consumed += 1;
        }

        self.total_cycles += consumed as u64;

        if self.bus.poll_nmi() {
            self.nmi();
        }
        if self.bus.poll_irq() && !self.status.contains(CpuFlags::IRQ_DIS) {
            self.irq();
        }
        consumed
    }

    /// Non-maskable interrupt: pushes PC and status, loads vector at $FFFA-$FFFB.
    /// Takes 7 cycles total: 2 internal + 3 stack writes (via push) + 2 vector reads.
    pub fn nmi(&mut self) {
        self.bus.tick(2);
        self.push_u16(self.pc);
        // BREAK clear, UNUSED set when pushed by NMI/IRQ
        self.push(self.status.bits() & !0x10 | 0x20);
        self.status.insert(CpuFlags::IRQ_DIS);
        let lo = self.bus.cpu_read(0xFFFA) as u16;
        let hi = self.bus.cpu_read(0xFFFB) as u16;
        self.pc = (hi << 8) | lo;
        self.total_cycles += 7;
    }

    /// Maskable interrupt. No-ops if IRQ_DIS is set.
    /// Takes 7 cycles total: 2 internal + 3 stack writes + 2 vector reads.
    pub fn irq(&mut self) {
        if self.status.contains(CpuFlags::IRQ_DIS) {
            return;
        }
        self.bus.tick(2);
        self.push_u16(self.pc);
        self.push(self.status.bits() & !0x10 | 0x20);
        self.status.insert(CpuFlags::IRQ_DIS);
        let lo = self.bus.cpu_read(0xFFFE) as u16;
        let hi = self.bus.cpu_read(0xFFFF) as u16;
        self.pc = (hi << 8) | lo;
        self.total_cycles += 7;
    }

    // ── Stack helpers ────────────────────────────────────────────────────────

    fn push(&mut self, val: u8) {
        self.bus.cpu_write(0x0100 | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.bus.cpu_read(0x0100 | self.sp as u16)
    }

    fn push_u16(&mut self, val: u16) {
        self.push((val >> 8) as u8);
        self.push(val as u8);
    }

    fn pull_u16(&mut self) -> u16 {
        let lo = self.pull() as u16;
        let hi = self.pull() as u16;
        (hi << 8) | lo
    }

    // ── Flag helpers ─────────────────────────────────────────────────────────

    fn set_zn(&mut self, val: u8) {
        self.status.set(CpuFlags::ZERO, val == 0);
        self.status.set(CpuFlags::NEGATIVE, val & 0x80 != 0);
    }

    // ── Address resolution ───────────────────────────────────────────────────

    /// Advances PC over operand bytes and returns (effective_address, page_crossed).
    fn resolve_addr(&mut self, mode: AddressingMode) -> (u16, bool) {
        match mode {
            AddressingMode::Implicit | AddressingMode::Accumulator => (0, false),

            AddressingMode::Immediate => {
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (addr, false)
            }

            AddressingMode::ZeroPage => {
                let addr = self.bus.cpu_read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                (addr, false)
            }

            AddressingMode::ZeroPageX => {
                let base = self.bus.cpu_read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                (base.wrapping_add(self.x) as u16, false)
            }

            AddressingMode::ZeroPageY => {
                let base = self.bus.cpu_read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                (base.wrapping_add(self.y) as u16, false)
            }

            AddressingMode::Relative => {
                let offset = self.bus.cpu_read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                // PC now points to the next instruction; branch target is pc + offset
                let target = self.pc.wrapping_add(offset as u16);
                let page_crossed = (self.pc & 0xFF00) != (target & 0xFF00);
                (target, page_crossed)
            }

            AddressingMode::Absolute => {
                let lo = self.bus.cpu_read(self.pc) as u16;
                let hi = self.bus.cpu_read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                ((hi << 8) | lo, false)
            }

            AddressingMode::AbsoluteX => {
                let lo = self.bus.cpu_read(self.pc) as u16;
                let hi = self.bus.cpu_read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.x as u16);
                (addr, (base & 0xFF00) != (addr & 0xFF00))
            }

            AddressingMode::AbsoluteY => {
                let lo = self.bus.cpu_read(self.pc) as u16;
                let hi = self.bus.cpu_read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                (addr, (base & 0xFF00) != (addr & 0xFF00))
            }

            // JMP indirect: page-boundary bug — if low byte of pointer is $FF,
            // the high byte wraps within the same page instead of crossing to the next.
            AddressingMode::Indirect => {
                let lo_ptr = self.bus.cpu_read(self.pc) as u16;
                let hi_ptr = self.bus.cpu_read(self.pc.wrapping_add(1)) as u16;
                self.pc = self.pc.wrapping_add(2);
                let ptr = (hi_ptr << 8) | lo_ptr;
                let lo = self.bus.cpu_read(ptr) as u16;
                let hi = self.bus.cpu_read((ptr & 0xFF00) | ((ptr + 1) & 0x00FF)) as u16;
                ((hi << 8) | lo, false)
            }

            // (zp,X): add X to zero-page base, read 16-bit address from zero page
            AddressingMode::IndirectX => {
                let base = self.bus.cpu_read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let ptr = base.wrapping_add(self.x) as u16;
                let lo = self.bus.cpu_read(ptr & 0x00FF) as u16;
                let hi = self.bus.cpu_read(ptr.wrapping_add(1) & 0x00FF) as u16;
                ((hi << 8) | lo, false)
            }

            // (zp),Y: read 16-bit base from zero page, add Y
            AddressingMode::IndirectY => {
                let ptr = self.bus.cpu_read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let lo = self.bus.cpu_read(ptr & 0x00FF) as u16;
                let hi = self.bus.cpu_read(ptr.wrapping_add(1) & 0x00FF) as u16;
                let base = (hi << 8) | lo;
                let addr = base.wrapping_add(self.y as u16);
                (addr, (base & 0xFF00) != (addr & 0xFF00))
            }
        }
    }

    // ── Instruction execution ────────────────────────────────────────────────

    /// Executes one decoded instruction. Returns extra cycles (page cross / branch).
    fn execute(
        &mut self,
        opcode: Opcode,
        mode: AddressingMode,
        addr: u16,
        page_crossed: bool,
        extra_cycle: bool,
    ) -> u8 {
        // For load-type instructions with extra_cycle=true, page cross adds 1 cycle.
        // Branch instructions handle their own cycle accounting and return early.
        let page_extra: u8 = if extra_cycle && page_crossed { 1 } else { 0 };

        match opcode {
            // ── Load/Store ───────────────────────────────────────────────────
            Opcode::LDA => {
                self.a = self.bus.cpu_read(addr);
                self.set_zn(self.a);
            }
            Opcode::LDX => {
                self.x = self.bus.cpu_read(addr);
                self.set_zn(self.x);
            }
            Opcode::LDY => {
                self.y = self.bus.cpu_read(addr);
                self.set_zn(self.y);
            }
            Opcode::STA => { self.bus.cpu_write(addr, self.a); }
            Opcode::STX => { self.bus.cpu_write(addr, self.x); }
            Opcode::STY => { self.bus.cpu_write(addr, self.y); }

            // ── Register transfers ───────────────────────────────────────────
            Opcode::TAX => { self.x = self.a; self.set_zn(self.x); }
            Opcode::TAY => { self.y = self.a; self.set_zn(self.y); }
            Opcode::TXA => { self.a = self.x; self.set_zn(self.a); }
            Opcode::TYA => { self.a = self.y; self.set_zn(self.a); }
            Opcode::TSX => { self.x = self.sp; self.set_zn(self.x); }
            Opcode::TXS => { self.sp = self.x; } // TXS does not affect flags

            // ── Stack ────────────────────────────────────────────────────────
            Opcode::PHA => { self.push(self.a); }
            // PHP always pushes with BREAK and UNUSED set
            Opcode::PHP => { self.push(self.status.bits() | 0x30); }
            Opcode::PLA => { self.a = self.pull(); self.set_zn(self.a); }
            // PLP restores all flags but forces BREAK clear, UNUSED set
            Opcode::PLP => {
                let val = self.pull();
                self.status = CpuFlags::from_bits_truncate((val & 0xEF) | 0x20);
            }

            // ── Arithmetic ───────────────────────────────────────────────────
            Opcode::ADC => {
                let val = self.bus.cpu_read(addr) as u16;
                let carry = (self.status & CpuFlags::CARRY).bits() as u16;
                let sum = self.a as u16 + val + carry;
                self.status.set(CpuFlags::CARRY, sum > 0xFF);
                // Overflow: same-sign operands produced opposite-sign result
                self.status.set(
                    CpuFlags::OVERFLOW,
                    (!(self.a as u16 ^ val) & (self.a as u16 ^ sum) & 0x80) != 0,
                );
                self.a = sum as u8;
                self.set_zn(self.a);
            }
            // SBC is ADC with the operand bitwise-inverted (~M = 255-M)
            Opcode::SBC => {
                let val = self.bus.cpu_read(addr) as u16 ^ 0x00FF;
                let carry = (self.status & CpuFlags::CARRY).bits() as u16;
                let sum = self.a as u16 + val + carry;
                self.status.set(CpuFlags::CARRY, sum > 0xFF);
                self.status.set(
                    CpuFlags::OVERFLOW,
                    (!(self.a as u16 ^ val) & (self.a as u16 ^ sum) & 0x80) != 0,
                );
                self.a = sum as u8;
                self.set_zn(self.a);
            }
            Opcode::INC => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val); // RMW: spurious write of original
                let result = val.wrapping_add(1);
                self.bus.cpu_write(addr, result);
                self.set_zn(result);
            }
            Opcode::DEC => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val); // RMW: spurious write of original
                let result = val.wrapping_sub(1);
                self.bus.cpu_write(addr, result);
                self.set_zn(result);
            }
            Opcode::INX => { self.x = self.x.wrapping_add(1); self.set_zn(self.x); }
            Opcode::INY => { self.y = self.y.wrapping_add(1); self.set_zn(self.y); }
            Opcode::DEX => { self.x = self.x.wrapping_sub(1); self.set_zn(self.x); }
            Opcode::DEY => { self.y = self.y.wrapping_sub(1); self.set_zn(self.y); }

            // ── Logic ────────────────────────────────────────────────────────
            Opcode::AND => { self.a &= self.bus.cpu_read(addr); self.set_zn(self.a); }
            Opcode::ORA => { self.a |= self.bus.cpu_read(addr); self.set_zn(self.a); }
            Opcode::EOR => { self.a ^= self.bus.cpu_read(addr); self.set_zn(self.a); }
            Opcode::BIT => {
                let val = self.bus.cpu_read(addr);
                self.status.set(CpuFlags::ZERO, (self.a & val) == 0);
                self.status.set(CpuFlags::OVERFLOW, val & 0x40 != 0);
                self.status.set(CpuFlags::NEGATIVE, val & 0x80 != 0);
            }

            // ── Shifts / Rotates ─────────────────────────────────────────────
            Opcode::ASL => match mode {
                AddressingMode::Accumulator => {
                    self.status.set(CpuFlags::CARRY, self.a & 0x80 != 0);
                    self.a <<= 1;
                    self.set_zn(self.a);
                }
                _ => {
                    let val = self.bus.cpu_read(addr);
                    self.bus.cpu_write(addr, val); // RMW spurious write
                    self.status.set(CpuFlags::CARRY, val & 0x80 != 0);
                    let result = val << 1;
                    self.bus.cpu_write(addr, result);
                    self.set_zn(result);
                }
            },
            Opcode::LSR => match mode {
                AddressingMode::Accumulator => {
                    self.status.set(CpuFlags::CARRY, self.a & 0x01 != 0);
                    self.a >>= 1;
                    self.set_zn(self.a);
                }
                _ => {
                    let val = self.bus.cpu_read(addr);
                    self.bus.cpu_write(addr, val); // RMW spurious write
                    self.status.set(CpuFlags::CARRY, val & 0x01 != 0);
                    let result = val >> 1;
                    self.bus.cpu_write(addr, result);
                    self.set_zn(result);
                }
            },
            Opcode::ROL => match mode {
                AddressingMode::Accumulator => {
                    let old_carry = (self.status & CpuFlags::CARRY).bits();
                    self.status.set(CpuFlags::CARRY, self.a & 0x80 != 0);
                    self.a = (self.a << 1) | old_carry;
                    self.set_zn(self.a);
                }
                _ => {
                    let val = self.bus.cpu_read(addr);
                    self.bus.cpu_write(addr, val); // RMW spurious write
                    let old_carry = (self.status & CpuFlags::CARRY).bits();
                    self.status.set(CpuFlags::CARRY, val & 0x80 != 0);
                    let result = (val << 1) | old_carry;
                    self.bus.cpu_write(addr, result);
                    self.set_zn(result);
                }
            },
            Opcode::ROR => match mode {
                AddressingMode::Accumulator => {
                    let old_carry = (self.status & CpuFlags::CARRY).bits();
                    self.status.set(CpuFlags::CARRY, self.a & 0x01 != 0);
                    self.a = (self.a >> 1) | (old_carry << 7);
                    self.set_zn(self.a);
                }
                _ => {
                    let val = self.bus.cpu_read(addr);
                    self.bus.cpu_write(addr, val); // RMW spurious write
                    let old_carry = (self.status & CpuFlags::CARRY).bits();
                    self.status.set(CpuFlags::CARRY, val & 0x01 != 0);
                    let result = (val >> 1) | (old_carry << 7);
                    self.bus.cpu_write(addr, result);
                    self.set_zn(result);
                }
            },

            // ── Compare ──────────────────────────────────────────────────────
            Opcode::CMP => {
                let val = self.bus.cpu_read(addr);
                self.status.set(CpuFlags::CARRY, self.a >= val);
                self.set_zn(self.a.wrapping_sub(val));
            }
            Opcode::CPX => {
                let val = self.bus.cpu_read(addr);
                self.status.set(CpuFlags::CARRY, self.x >= val);
                self.set_zn(self.x.wrapping_sub(val));
            }
            Opcode::CPY => {
                let val = self.bus.cpu_read(addr);
                self.status.set(CpuFlags::CARRY, self.y >= val);
                self.set_zn(self.y.wrapping_sub(val));
            }

            // ── Branches ─────────────────────────────────────────────────────
            // Base cost: 2 (not taken). +1 if taken, +1 more if page crossed.
            // Return early — branch cycles are independent of extra_cycle.
            Opcode::BCC => return if !self.status.contains(CpuFlags::CARRY)    { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BCS => return if  self.status.contains(CpuFlags::CARRY)    { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BEQ => return if  self.status.contains(CpuFlags::ZERO)     { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BNE => return if !self.status.contains(CpuFlags::ZERO)     { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BMI => return if  self.status.contains(CpuFlags::NEGATIVE) { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BPL => return if !self.status.contains(CpuFlags::NEGATIVE) { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BVC => return if !self.status.contains(CpuFlags::OVERFLOW) { self.branch(addr, page_crossed) } else { 0 },
            Opcode::BVS => return if  self.status.contains(CpuFlags::OVERFLOW) { self.branch(addr, page_crossed) } else { 0 },

            // ── Jumps / Calls ────────────────────────────────────────────────
            Opcode::JMP => { self.pc = addr; }
            Opcode::JSR => {
                // Push PC-1 (last byte of JSR instruction) so RTS can restore correctly
                let ret = self.pc.wrapping_sub(1);
                self.push_u16(ret);
                self.pc = addr;
            }
            Opcode::RTS => {
                let ret = self.pull_u16();
                self.pc = ret.wrapping_add(1);
            }
            Opcode::RTI => {
                // Pull status (BREAK cleared, UNUSED forced set), then PC
                let val = self.pull();
                self.status = CpuFlags::from_bits_truncate((val & 0xEF) | 0x20);
                self.pc = self.pull_u16();
            }

            // ── Unofficial / stable ──────────────────────────────────────────

            // LAX = LDA + LDX (same loaded value)
            Opcode::LAX => {
                let val = self.bus.cpu_read(addr);
                self.a = val;
                self.x = val;
                self.set_zn(val);
            }

            // SAX = write (A & X); no flags
            Opcode::SAX => {
                self.bus.cpu_write(addr, self.a & self.x);
            }

            // DCP = DEC then CMP (RMW, spurious write of original)
            Opcode::DCP => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val);
                let result = val.wrapping_sub(1);
                self.bus.cpu_write(addr, result);
                self.status.set(CpuFlags::CARRY, self.a >= result);
                self.set_zn(self.a.wrapping_sub(result));
            }

            // ISC = INC then SBC
            Opcode::ISC => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val);
                let inc = val.wrapping_add(1);
                self.bus.cpu_write(addr, inc);
                let m = (inc as u16) ^ 0x00FF;
                let carry = (self.status & CpuFlags::CARRY).bits() as u16;
                let sum = self.a as u16 + m + carry;
                self.status.set(CpuFlags::CARRY, sum > 0xFF);
                self.status.set(
                    CpuFlags::OVERFLOW,
                    (!(self.a as u16 ^ m) & (self.a as u16 ^ sum) & 0x80) != 0,
                );
                self.a = sum as u8;
                self.set_zn(self.a);
            }

            // SLO = ASL then ORA
            Opcode::SLO => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val);
                self.status.set(CpuFlags::CARRY, val & 0x80 != 0);
                let shifted = val << 1;
                self.bus.cpu_write(addr, shifted);
                self.a |= shifted;
                self.set_zn(self.a);
            }

            // RLA = ROL then AND
            Opcode::RLA => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val);
                let old_carry = (self.status & CpuFlags::CARRY).bits();
                self.status.set(CpuFlags::CARRY, val & 0x80 != 0);
                let rotated = (val << 1) | old_carry;
                self.bus.cpu_write(addr, rotated);
                self.a &= rotated;
                self.set_zn(self.a);
            }

            // SRE = LSR then EOR
            Opcode::SRE => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val);
                self.status.set(CpuFlags::CARRY, val & 0x01 != 0);
                let shifted = val >> 1;
                self.bus.cpu_write(addr, shifted);
                self.a ^= shifted;
                self.set_zn(self.a);
            }

            // RRA = ROR then ADC
            Opcode::RRA => {
                let val = self.bus.cpu_read(addr);
                self.bus.cpu_write(addr, val);
                let old_carry = (self.status & CpuFlags::CARRY).bits();
                self.status.set(CpuFlags::CARRY, val & 0x01 != 0);
                let rotated = (val >> 1) | (old_carry << 7);
                self.bus.cpu_write(addr, rotated);
                let m = rotated as u16;
                let carry = (self.status & CpuFlags::CARRY).bits() as u16;
                let sum = self.a as u16 + m + carry;
                self.status.set(CpuFlags::CARRY, sum > 0xFF);
                self.status.set(
                    CpuFlags::OVERFLOW,
                    (!(self.a as u16 ^ m) & (self.a as u16 ^ sum) & 0x80) != 0,
                );
                self.a = sum as u8;
                self.set_zn(self.a);
            }

            // ANC = AND # ; C <- bit 7 of result (i.e. same as N)
            Opcode::ANC => {
                self.a &= self.bus.cpu_read(addr);
                self.set_zn(self.a);
                self.status.set(CpuFlags::CARRY, self.a & 0x80 != 0);
            }

            // ALR = AND # then LSR A
            Opcode::ALR => {
                self.a &= self.bus.cpu_read(addr);
                self.status.set(CpuFlags::CARRY, self.a & 0x01 != 0);
                self.a >>= 1;
                self.set_zn(self.a);
            }

            // ARR = AND # then ROR A, with quirky C/V derived from bits 5/6
            // of the result:
            //   C = bit 6 of A (post-rotate)
            //   V = bit 6 XOR bit 5 of A
            Opcode::ARR => {
                self.a &= self.bus.cpu_read(addr);
                let old_carry = (self.status & CpuFlags::CARRY).bits();
                self.a = (self.a >> 1) | (old_carry << 7);
                self.set_zn(self.a);
                self.status.set(CpuFlags::CARRY, self.a & 0x40 != 0);
                let bit5 = (self.a >> 5) & 1;
                let bit6 = (self.a >> 6) & 1;
                self.status.set(CpuFlags::OVERFLOW, (bit5 ^ bit6) != 0);
            }

            // SBX = X <- (A & X) - # ; C <- !borrow
            Opcode::SBX => {
                let m = self.bus.cpu_read(addr);
                let lhs = self.a & self.x;
                self.status.set(CpuFlags::CARRY, lhs >= m);
                self.x = lhs.wrapping_sub(m);
                self.set_zn(self.x);
            }

            // SHY abs,X — writes Y & (H+1) where H is the high byte of the
            // *base* 16-bit operand (before adding X). On page cross, hardware
            // clobbers the target's high byte with the result value.
            Opcode::SHY => {
                let base_hi = if page_crossed {
                    (addr >> 8).wrapping_sub(1) as u8
                } else {
                    (addr >> 8) as u8
                };
                let value = self.y & base_hi.wrapping_add(1);
                let target = if page_crossed {
                    ((value as u16) << 8) | (addr & 0x00FF)
                } else {
                    addr
                };
                self.bus.cpu_write(target, value);
            }

            // SHX abs,Y — mirror of SHY for X.
            Opcode::SHX => {
                let base_hi = if page_crossed {
                    (addr >> 8).wrapping_sub(1) as u8
                } else {
                    (addr >> 8) as u8
                };
                let value = self.x & base_hi.wrapping_add(1);
                let target = if page_crossed {
                    ((value as u16) << 8) | (addr & 0x00FF)
                } else {
                    addr
                };
                self.bus.cpu_write(target, value);
            }

            // ── System ───────────────────────────────────────────────────────
            Opcode::BRK => {
                // BRK is 2 bytes (opcode + padding); skip the padding byte
                self.pc = self.pc.wrapping_add(1);
                self.push_u16(self.pc);
                self.push(self.status.bits() | 0x30); // BREAK + UNUSED set
                self.status.insert(CpuFlags::IRQ_DIS);
                let lo = self.bus.cpu_read(0xFFFE) as u16;
                let hi = self.bus.cpu_read(0xFFFF) as u16;
                self.pc = (hi << 8) | lo;
            }
            Opcode::NOP => {}
            Opcode::XXX => {} // undefined opcode — treat as a 2-cycle NOP

            // ── Flags ────────────────────────────────────────────────────────
            Opcode::SEC => { self.status.insert(CpuFlags::CARRY); }
            Opcode::CLC => { self.status.remove(CpuFlags::CARRY); }
            Opcode::SEI => { self.status.insert(CpuFlags::IRQ_DIS); }
            Opcode::CLI => { self.status.remove(CpuFlags::IRQ_DIS); }
            Opcode::SED => { self.status.insert(CpuFlags::DECIMAL); }
            Opcode::CLD => { self.status.remove(CpuFlags::DECIMAL); }
            Opcode::CLV => { self.status.remove(CpuFlags::OVERFLOW); }
        }

        page_extra
    }

    /// Takes the branch: sets PC and returns extra cycles (1 same-page, 2 cross-page).
    fn branch(&mut self, addr: u16, page_crossed: bool) -> u8 {
        self.pc = addr;
        if page_crossed { 2 } else { 1 }
    }

    pub fn capture_state(&self) -> CpuState {
        CpuState {
            a: self.a, x: self.x, y: self.y, sp: self.sp, pc: self.pc,
            status: self.status.bits(),
            remaining_cycles: self.remaining_cycles,
            total_cycles: self.total_cycles,
        }
    }

    pub fn restore_state(&mut self, s: &CpuState) {
        self.a = s.a; self.x = s.x; self.y = s.y; self.sp = s.sp; self.pc = s.pc;
        self.status = CpuFlags::from_bits_truncate(s.status);
        self.remaining_cycles = s.remaining_cycles;
        self.total_cycles = s.total_cycles;
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cartridge::Cartridge;

    /// Build a minimal CPU backed by a 32KB NROM ROM.
    /// `code` is placed at PRG offset 0 ($8000). Reset vector points to $8000.
    fn make_test_cpu(code: &[u8]) -> Cpu {
        let mut prg = vec![0u8; 0x8000]; // 32KB
        let len = code.len().min(0x7FFC);
        prg[..len].copy_from_slice(&code[..len]);
        prg[0x7FFC] = 0x00; // reset vector lo → $8000
        prg[0x7FFD] = 0x80; // reset vector hi

        let mut rom = vec![0u8; 16]; // iNES header
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 2; // 2 PRG banks = 32KB
        rom[5] = 0; // no CHR banks
        rom.extend_from_slice(&prg);

        let cart = Cartridge::from_ines(&rom).unwrap();
        let bus = Bus::new(cart);
        let mut cpu = Cpu::new(bus);
        cpu.reset();
        cpu
    }

    // ── LDA flags ────────────────────────────────────────────────────────────

    #[test]
    fn lda_immediate_sets_zero_flag() {
        let mut cpu = make_test_cpu(&[0xA9, 0x00]); // LDA #0
        cpu.step();
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn lda_immediate_sets_negative_flag() {
        let mut cpu = make_test_cpu(&[0xA9, 0x80]); // LDA #$80
        cpu.step();
        assert_eq!(cpu.a, 0x80);
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn lda_positive_clears_both_flags() {
        let mut cpu = make_test_cpu(&[0xA9, 0x42]); // LDA #$42
        cpu.step();
        assert_eq!(cpu.a, 0x42);
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
    }

    // ── ADC ──────────────────────────────────────────────────────────────────

    #[test]
    fn adc_with_carry_out() {
        // LDA #$FF; ADC #$01 → $00, carry set
        let mut cpu = make_test_cpu(&[0xA9, 0xFF, 0x69, 0x01]);
        cpu.step(); // LDA
        cpu.step(); // ADC
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        assert!(cpu.status.contains(CpuFlags::ZERO));
    }

    #[test]
    fn adc_sets_overflow_on_signed_overflow() {
        // LDA #$7F; ADC #$01 → $80 (positive + positive = negative)
        let mut cpu = make_test_cpu(&[0xA9, 0x7F, 0x69, 0x01]);
        cpu.step();
        cpu.step();
        assert_eq!(cpu.a, 0x80);
        assert!(cpu.status.contains(CpuFlags::OVERFLOW));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    // ── SBC ──────────────────────────────────────────────────────────────────

    #[test]
    fn sbc_basic_subtraction() {
        // LDA #$10; SEC; SBC #$01 → $0F, carry set (no borrow)
        let mut cpu = make_test_cpu(&[0xA9, 0x10, 0x38, 0xE9, 0x01]);
        cpu.step(); // LDA #$10
        cpu.step(); // SEC
        cpu.step(); // SBC #$01
        assert_eq!(cpu.a, 0x0F);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
    }

    // ── Stack ────────────────────────────────────────────────────────────────

    #[test]
    fn pha_pla_roundtrip() {
        // LDA #$42; PHA; LDA #$00; PLA
        let mut cpu = make_test_cpu(&[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68]);
        cpu.step(); // LDA #$42
        cpu.step(); // PHA
        let sp_after_push = cpu.sp;
        cpu.step(); // LDA #$00
        assert_eq!(cpu.a, 0x00);
        cpu.step(); // PLA
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cpu.sp, sp_after_push.wrapping_add(1));
    }

    // ── Branches ─────────────────────────────────────────────────────────────

    #[test]
    fn branch_not_taken_costs_two_cycles() {
        // LDA #$01 (Z=0); BEQ +$02 — not taken
        let mut cpu = make_test_cpu(&[0xA9, 0x01, 0xF0, 0x02]);
        cpu.step(); // LDA #1
        let cycles = cpu.step(); // BEQ not taken
        assert_eq!(cycles, 2);
        assert_eq!(cpu.pc, 0x8004);
    }

    #[test]
    fn branch_taken_same_page_costs_three_cycles() {
        // LDA #$00 (Z=1); BEQ +$02 — taken, same page
        let mut cpu = make_test_cpu(&[0xA9, 0x00, 0xF0, 0x02]);
        cpu.step(); // LDA #0 — sets Z
        let cycles = cpu.step(); // BEQ taken
        assert_eq!(cycles, 3);
        assert_eq!(cpu.pc, 0x8006); // $8004 + 2
    }

    #[test]
    fn branch_taken_page_cross_costs_four_cycles() {
        // BEQ with offset=-1 at $80FE: after reading offset PC=$8100,
        // target=$80FF — crosses from $8100 page to $80xx page.
        let mut code = vec![0xEA_u8; 0xFE]; // NOPs padding to $80FE
        code.push(0xF0); // BEQ
        code.push(0xFF); // offset -1
        let mut cpu = make_test_cpu(&code);
        cpu.pc = 0x80FE; // skip to BEQ directly
        cpu.status.insert(CpuFlags::ZERO); // ensure branch is taken
        let cycles = cpu.step();
        assert_eq!(cycles, 4);
        assert_eq!(cpu.pc, 0x80FF);
    }

    // ── JMP indirect page boundary bug ───────────────────────────────────────

    #[test]
    fn jmp_indirect_page_boundary_bug() {
        // JMP ($00FF): reads lo from $00FF, hi from $0000 (not $0100)
        let mut cpu = make_test_cpu(&[0x6C, 0xFF, 0x00]);
        cpu.bus.cpu_ram[0x00FF] = 0x34; // lo byte of target
        cpu.bus.cpu_ram[0x0000] = 0x12; // hi byte via page-wrap bug
        cpu.bus.cpu_ram[0x0100] = 0x56; // hi byte without bug — must NOT be used
        cpu.step();
        assert_eq!(cpu.pc, 0x1234, "JMP indirect must exhibit page boundary wrap");
    }

    // ── JSR / RTS ────────────────────────────────────────────────────────────

    #[test]
    fn jsr_rts_roundtrip() {
        // $8000: JSR $8006  ($8003: LDA #$AA  $8005: BRK  $8006: LDA #$42  $8008: RTS)
        let code = [
            0x20, 0x06, 0x80, // JSR $8006
            0xA9, 0xAA,       // LDA #$AA  (after return)
            0x00,             // BRK       (should not be reached)
            0xA9, 0x42,       // LDA #$42  (subroutine body)
            0x60,             // RTS
        ];
        let mut cpu = make_test_cpu(&code);
        cpu.step(); // JSR — jumps to $8006, pushes $8002
        assert_eq!(cpu.pc, 0x8006);
        cpu.step(); // LDA #$42 in subroutine
        assert_eq!(cpu.a, 0x42);
        cpu.step(); // RTS — pulls $8002, PC = $8003
        assert_eq!(cpu.pc, 0x8003);
        cpu.step(); // LDA #$AA back in caller
        assert_eq!(cpu.a, 0xAA);
    }
}
