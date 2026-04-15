use super::addressing::AddressingMode;

#[derive(Debug, Clone, Copy)]
pub enum Opcode {
    // Official 6502
    ADC, AND, ASL, BCC, BCS, BEQ, BIT, BMI,
    BNE, BPL, BRK, BVC, BVS, CLC, CLD, CLI,
    CLV, CMP, CPX, CPY, DEC, DEX, DEY, EOR,
    INC, INX, INY, JMP, JSR, LDA, LDX, LDY,
    LSR, NOP, ORA, PHA, PHP, PLA, PLP, ROL,
    ROR, RTI, RTS, SBC, SEC, SED, SEI, STA,
    STX, STY, TAX, TAY, TSX, TXA, TXS, TYA,

    // Unofficial but *stable* (documented & widely depended on)
    LAX,  // LDA + LDX combined
    SAX,  // write A & X
    DCP,  // DEC then CMP
    ISC,  // INC then SBC           (a.k.a. ISB)
    SLO,  // ASL then ORA
    RLA,  // ROL then AND
    SRE,  // LSR then EOR           (a.k.a. LSE)
    RRA,  // ROR then ADC
    ANC,  // AND # with carry := bit 7
    ALR,  // AND # then LSR A       (a.k.a. ASR)
    ARR,  // AND # then ROR A (with quirky V/C)
    SBX,  // X := (A & X) - #       (a.k.a. AXS)
    SHY,  // store Y & ((H+1) & 0xFF) at absolute,X  (a.k.a. SYA)
    SHX,  // store X & ((H+1) & 0xFF) at absolute,Y  (a.k.a. SXA)

    // Truly undefined / unstable (KIL/JAM, SHA/SHX/SHY/TAS/LAS/XAA). They
    // advance PC via their declared addressing mode and consume their cycles,
    // but have no behavior — sufficient for test ROMs, which don't depend on
    // their unstable results.
    XXX,
}

pub struct OpcodeInfo {
    pub opcode: Opcode,
    pub mode: AddressingMode,
    pub cycles: u8,
    pub bytes: u8,
    /// True if a page-boundary crossing adds 1 extra cycle.
    pub extra_cycle: bool,
}

macro_rules! op {
    ($opc:ident, $mode:ident, $cyc:expr, $bytes:expr, $extra:expr) => {
        OpcodeInfo {
            opcode: Opcode::$opc,
            mode: AddressingMode::$mode,
            cycles: $cyc,
            bytes: $bytes,
            extra_cycle: $extra,
        }
    };
}

/// Decodes an opcode byte into its instruction metadata.
/// All 256 byte values are covered; truly undefined / unstable opcodes
/// (KIL, SHA/SHX/SHY, TAS, LAS, XAA) map to XXX but still carry the right
/// addressing mode + cycle count so PC advances correctly.
pub fn decode(byte: u8) -> OpcodeInfo {
    match byte {
        // ── 0x00 row ────────────────────────────────────────────────────────
        0x00 => op!(BRK, Implicit,     7, 1, false),
        0x01 => op!(ORA, IndirectX,    6, 2, false),
        0x02 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x03 => op!(SLO, IndirectX,    8, 2, false),
        0x04 => op!(NOP, ZeroPage,     3, 2, false),
        0x05 => op!(ORA, ZeroPage,     3, 2, false),
        0x06 => op!(ASL, ZeroPage,     5, 2, false),
        0x07 => op!(SLO, ZeroPage,     5, 2, false),
        0x08 => op!(PHP, Implicit,     3, 1, false),
        0x09 => op!(ORA, Immediate,    2, 2, false),
        0x0A => op!(ASL, Accumulator,  2, 1, false),
        0x0B => op!(ANC, Immediate,    2, 2, false),
        0x0C => op!(NOP, Absolute,     4, 3, false),
        0x0D => op!(ORA, Absolute,     4, 3, false),
        0x0E => op!(ASL, Absolute,     6, 3, false),
        0x0F => op!(SLO, Absolute,     6, 3, false),

        // ── 0x10 row ────────────────────────────────────────────────────────
        0x10 => op!(BPL, Relative,     2, 2, false),
        0x11 => op!(ORA, IndirectY,    5, 2, true),
        0x12 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x13 => op!(SLO, IndirectY,    8, 2, false),
        0x14 => op!(NOP, ZeroPageX,    4, 2, false),
        0x15 => op!(ORA, ZeroPageX,    4, 2, false),
        0x16 => op!(ASL, ZeroPageX,    6, 2, false),
        0x17 => op!(SLO, ZeroPageX,    6, 2, false),
        0x18 => op!(CLC, Implicit,     2, 1, false),
        0x19 => op!(ORA, AbsoluteY,    4, 3, true),
        0x1A => op!(NOP, Implicit,     2, 1, false),
        0x1B => op!(SLO, AbsoluteY,    7, 3, false),
        0x1C => op!(NOP, AbsoluteX,    4, 3, true),
        0x1D => op!(ORA, AbsoluteX,    4, 3, true),
        0x1E => op!(ASL, AbsoluteX,    7, 3, false),
        0x1F => op!(SLO, AbsoluteX,    7, 3, false),

        // ── 0x20 row ────────────────────────────────────────────────────────
        0x20 => op!(JSR, Absolute,     6, 3, false),
        0x21 => op!(AND, IndirectX,    6, 2, false),
        0x22 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x23 => op!(RLA, IndirectX,    8, 2, false),
        0x24 => op!(BIT, ZeroPage,     3, 2, false),
        0x25 => op!(AND, ZeroPage,     3, 2, false),
        0x26 => op!(ROL, ZeroPage,     5, 2, false),
        0x27 => op!(RLA, ZeroPage,     5, 2, false),
        0x28 => op!(PLP, Implicit,     4, 1, false),
        0x29 => op!(AND, Immediate,    2, 2, false),
        0x2A => op!(ROL, Accumulator,  2, 1, false),
        0x2B => op!(ANC, Immediate,    2, 2, false),
        0x2C => op!(BIT, Absolute,     4, 3, false),
        0x2D => op!(AND, Absolute,     4, 3, false),
        0x2E => op!(ROL, Absolute,     6, 3, false),
        0x2F => op!(RLA, Absolute,     6, 3, false),

        // ── 0x30 row ────────────────────────────────────────────────────────
        0x30 => op!(BMI, Relative,     2, 2, false),
        0x31 => op!(AND, IndirectY,    5, 2, true),
        0x32 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x33 => op!(RLA, IndirectY,    8, 2, false),
        0x34 => op!(NOP, ZeroPageX,    4, 2, false),
        0x35 => op!(AND, ZeroPageX,    4, 2, false),
        0x36 => op!(ROL, ZeroPageX,    6, 2, false),
        0x37 => op!(RLA, ZeroPageX,    6, 2, false),
        0x38 => op!(SEC, Implicit,     2, 1, false),
        0x39 => op!(AND, AbsoluteY,    4, 3, true),
        0x3A => op!(NOP, Implicit,     2, 1, false),
        0x3B => op!(RLA, AbsoluteY,    7, 3, false),
        0x3C => op!(NOP, AbsoluteX,    4, 3, true),
        0x3D => op!(AND, AbsoluteX,    4, 3, true),
        0x3E => op!(ROL, AbsoluteX,    7, 3, false),
        0x3F => op!(RLA, AbsoluteX,    7, 3, false),

        // ── 0x40 row ────────────────────────────────────────────────────────
        0x40 => op!(RTI, Implicit,     6, 1, false),
        0x41 => op!(EOR, IndirectX,    6, 2, false),
        0x42 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x43 => op!(SRE, IndirectX,    8, 2, false),
        0x44 => op!(NOP, ZeroPage,     3, 2, false),
        0x45 => op!(EOR, ZeroPage,     3, 2, false),
        0x46 => op!(LSR, ZeroPage,     5, 2, false),
        0x47 => op!(SRE, ZeroPage,     5, 2, false),
        0x48 => op!(PHA, Implicit,     3, 1, false),
        0x49 => op!(EOR, Immediate,    2, 2, false),
        0x4A => op!(LSR, Accumulator,  2, 1, false),
        0x4B => op!(ALR, Immediate,    2, 2, false),
        0x4C => op!(JMP, Absolute,     3, 3, false),
        0x4D => op!(EOR, Absolute,     4, 3, false),
        0x4E => op!(LSR, Absolute,     6, 3, false),
        0x4F => op!(SRE, Absolute,     6, 3, false),

        // ── 0x50 row ────────────────────────────────────────────────────────
        0x50 => op!(BVC, Relative,     2, 2, false),
        0x51 => op!(EOR, IndirectY,    5, 2, true),
        0x52 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x53 => op!(SRE, IndirectY,    8, 2, false),
        0x54 => op!(NOP, ZeroPageX,    4, 2, false),
        0x55 => op!(EOR, ZeroPageX,    4, 2, false),
        0x56 => op!(LSR, ZeroPageX,    6, 2, false),
        0x57 => op!(SRE, ZeroPageX,    6, 2, false),
        0x58 => op!(CLI, Implicit,     2, 1, false),
        0x59 => op!(EOR, AbsoluteY,    4, 3, true),
        0x5A => op!(NOP, Implicit,     2, 1, false),
        0x5B => op!(SRE, AbsoluteY,    7, 3, false),
        0x5C => op!(NOP, AbsoluteX,    4, 3, true),
        0x5D => op!(EOR, AbsoluteX,    4, 3, true),
        0x5E => op!(LSR, AbsoluteX,    7, 3, false),
        0x5F => op!(SRE, AbsoluteX,    7, 3, false),

        // ── 0x60 row ────────────────────────────────────────────────────────
        0x60 => op!(RTS, Implicit,     6, 1, false),
        0x61 => op!(ADC, IndirectX,    6, 2, false),
        0x62 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x63 => op!(RRA, IndirectX,    8, 2, false),
        0x64 => op!(NOP, ZeroPage,     3, 2, false),
        0x65 => op!(ADC, ZeroPage,     3, 2, false),
        0x66 => op!(ROR, ZeroPage,     5, 2, false),
        0x67 => op!(RRA, ZeroPage,     5, 2, false),
        0x68 => op!(PLA, Implicit,     4, 1, false),
        0x69 => op!(ADC, Immediate,    2, 2, false),
        0x6A => op!(ROR, Accumulator,  2, 1, false),
        0x6B => op!(ARR, Immediate,    2, 2, false),
        0x6C => op!(JMP, Indirect,     5, 3, false),
        0x6D => op!(ADC, Absolute,     4, 3, false),
        0x6E => op!(ROR, Absolute,     6, 3, false),
        0x6F => op!(RRA, Absolute,     6, 3, false),

        // ── 0x70 row ────────────────────────────────────────────────────────
        0x70 => op!(BVS, Relative,     2, 2, false),
        0x71 => op!(ADC, IndirectY,    5, 2, true),
        0x72 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x73 => op!(RRA, IndirectY,    8, 2, false),
        0x74 => op!(NOP, ZeroPageX,    4, 2, false),
        0x75 => op!(ADC, ZeroPageX,    4, 2, false),
        0x76 => op!(ROR, ZeroPageX,    6, 2, false),
        0x77 => op!(RRA, ZeroPageX,    6, 2, false),
        0x78 => op!(SEI, Implicit,     2, 1, false),
        0x79 => op!(ADC, AbsoluteY,    4, 3, true),
        0x7A => op!(NOP, Implicit,     2, 1, false),
        0x7B => op!(RRA, AbsoluteY,    7, 3, false),
        0x7C => op!(NOP, AbsoluteX,    4, 3, true),
        0x7D => op!(ADC, AbsoluteX,    4, 3, true),
        0x7E => op!(ROR, AbsoluteX,    7, 3, false),
        0x7F => op!(RRA, AbsoluteX,    7, 3, false),

        // ── 0x80 row ────────────────────────────────────────────────────────
        0x80 => op!(NOP, Immediate,    2, 2, false),
        0x81 => op!(STA, IndirectX,    6, 2, false),
        0x82 => op!(NOP, Immediate,    2, 2, false),
        0x83 => op!(SAX, IndirectX,    6, 2, false),
        0x84 => op!(STY, ZeroPage,     3, 2, false),
        0x85 => op!(STA, ZeroPage,     3, 2, false),
        0x86 => op!(STX, ZeroPage,     3, 2, false),
        0x87 => op!(SAX, ZeroPage,     3, 2, false),
        0x88 => op!(DEY, Implicit,     2, 1, false),
        0x89 => op!(NOP, Immediate,    2, 2, false),
        0x8A => op!(TXA, Implicit,     2, 1, false),
        0x8B => op!(XXX, Immediate,    2, 2, false),  // XAA (unstable)
        0x8C => op!(STY, Absolute,     4, 3, false),
        0x8D => op!(STA, Absolute,     4, 3, false),
        0x8E => op!(STX, Absolute,     4, 3, false),
        0x8F => op!(SAX, Absolute,     4, 3, false),

        // ── 0x90 row ────────────────────────────────────────────────────────
        0x90 => op!(BCC, Relative,     2, 2, false),
        0x91 => op!(STA, IndirectY,    6, 2, false),
        0x92 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0x93 => op!(XXX, IndirectY,    6, 2, false),  // SHA (unstable)
        0x94 => op!(STY, ZeroPageX,    4, 2, false),
        0x95 => op!(STA, ZeroPageX,    4, 2, false),
        0x96 => op!(STX, ZeroPageY,    4, 2, false),
        0x97 => op!(SAX, ZeroPageY,    4, 2, false),
        0x98 => op!(TYA, Implicit,     2, 1, false),
        0x99 => op!(STA, AbsoluteY,    5, 3, false),
        0x9A => op!(TXS, Implicit,     2, 1, false),
        0x9B => op!(XXX, AbsoluteY,    5, 3, false),  // TAS (unstable)
        0x9C => op!(SHY, AbsoluteX,    5, 3, false),
        0x9D => op!(STA, AbsoluteX,    5, 3, false),
        0x9E => op!(SHX, AbsoluteY,    5, 3, false),
        0x9F => op!(XXX, AbsoluteY,    5, 3, false),  // SHA (unstable)

        // ── 0xA0 row ────────────────────────────────────────────────────────
        0xA0 => op!(LDY, Immediate,    2, 2, false),
        0xA1 => op!(LDA, IndirectX,    6, 2, false),
        0xA2 => op!(LDX, Immediate,    2, 2, false),
        0xA3 => op!(LAX, IndirectX,    6, 2, false),
        0xA4 => op!(LDY, ZeroPage,     3, 2, false),
        0xA5 => op!(LDA, ZeroPage,     3, 2, false),
        0xA6 => op!(LDX, ZeroPage,     3, 2, false),
        0xA7 => op!(LAX, ZeroPage,     3, 2, false),
        0xA8 => op!(TAY, Implicit,     2, 1, false),
        0xA9 => op!(LDA, Immediate,    2, 2, false),
        0xAA => op!(TAX, Implicit,     2, 1, false),
        0xAB => op!(LAX, Immediate,    2, 2, false),  // unstable, treat as LAX
        0xAC => op!(LDY, Absolute,     4, 3, false),
        0xAD => op!(LDA, Absolute,     4, 3, false),
        0xAE => op!(LDX, Absolute,     4, 3, false),
        0xAF => op!(LAX, Absolute,     4, 3, false),

        // ── 0xB0 row ────────────────────────────────────────────────────────
        0xB0 => op!(BCS, Relative,     2, 2, false),
        0xB1 => op!(LDA, IndirectY,    5, 2, true),
        0xB2 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0xB3 => op!(LAX, IndirectY,    5, 2, true),
        0xB4 => op!(LDY, ZeroPageX,    4, 2, false),
        0xB5 => op!(LDA, ZeroPageX,    4, 2, false),
        0xB6 => op!(LDX, ZeroPageY,    4, 2, false),
        0xB7 => op!(LAX, ZeroPageY,    4, 2, false),
        0xB8 => op!(CLV, Implicit,     2, 1, false),
        0xB9 => op!(LDA, AbsoluteY,    4, 3, true),
        0xBA => op!(TSX, Implicit,     2, 1, false),
        0xBB => op!(XXX, AbsoluteY,    4, 3, true),   // LAS (unstable)
        0xBC => op!(LDY, AbsoluteX,    4, 3, true),
        0xBD => op!(LDA, AbsoluteX,    4, 3, true),
        0xBE => op!(LDX, AbsoluteY,    4, 3, true),
        0xBF => op!(LAX, AbsoluteY,    4, 3, true),

        // ── 0xC0 row ────────────────────────────────────────────────────────
        0xC0 => op!(CPY, Immediate,    2, 2, false),
        0xC1 => op!(CMP, IndirectX,    6, 2, false),
        0xC2 => op!(NOP, Immediate,    2, 2, false),
        0xC3 => op!(DCP, IndirectX,    8, 2, false),
        0xC4 => op!(CPY, ZeroPage,     3, 2, false),
        0xC5 => op!(CMP, ZeroPage,     3, 2, false),
        0xC6 => op!(DEC, ZeroPage,     5, 2, false),
        0xC7 => op!(DCP, ZeroPage,     5, 2, false),
        0xC8 => op!(INY, Implicit,     2, 1, false),
        0xC9 => op!(CMP, Immediate,    2, 2, false),
        0xCA => op!(DEX, Implicit,     2, 1, false),
        0xCB => op!(SBX, Immediate,    2, 2, false),
        0xCC => op!(CPY, Absolute,     4, 3, false),
        0xCD => op!(CMP, Absolute,     4, 3, false),
        0xCE => op!(DEC, Absolute,     6, 3, false),
        0xCF => op!(DCP, Absolute,     6, 3, false),

        // ── 0xD0 row ────────────────────────────────────────────────────────
        0xD0 => op!(BNE, Relative,     2, 2, false),
        0xD1 => op!(CMP, IndirectY,    5, 2, true),
        0xD2 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0xD3 => op!(DCP, IndirectY,    8, 2, false),
        0xD4 => op!(NOP, ZeroPageX,    4, 2, false),
        0xD5 => op!(CMP, ZeroPageX,    4, 2, false),
        0xD6 => op!(DEC, ZeroPageX,    6, 2, false),
        0xD7 => op!(DCP, ZeroPageX,    6, 2, false),
        0xD8 => op!(CLD, Implicit,     2, 1, false),
        0xD9 => op!(CMP, AbsoluteY,    4, 3, true),
        0xDA => op!(NOP, Implicit,     2, 1, false),
        0xDB => op!(DCP, AbsoluteY,    7, 3, false),
        0xDC => op!(NOP, AbsoluteX,    4, 3, true),
        0xDD => op!(CMP, AbsoluteX,    4, 3, true),
        0xDE => op!(DEC, AbsoluteX,    7, 3, false),
        0xDF => op!(DCP, AbsoluteX,    7, 3, false),

        // ── 0xE0 row ────────────────────────────────────────────────────────
        0xE0 => op!(CPX, Immediate,    2, 2, false),
        0xE1 => op!(SBC, IndirectX,    6, 2, false),
        0xE2 => op!(NOP, Immediate,    2, 2, false),
        0xE3 => op!(ISC, IndirectX,    8, 2, false),
        0xE4 => op!(CPX, ZeroPage,     3, 2, false),
        0xE5 => op!(SBC, ZeroPage,     3, 2, false),
        0xE6 => op!(INC, ZeroPage,     5, 2, false),
        0xE7 => op!(ISC, ZeroPage,     5, 2, false),
        0xE8 => op!(INX, Implicit,     2, 1, false),
        0xE9 => op!(SBC, Immediate,    2, 2, false),
        0xEA => op!(NOP, Implicit,     2, 1, false),
        0xEB => op!(SBC, Immediate,    2, 2, false),  // USBC = SBC # (alias)
        0xEC => op!(CPX, Absolute,     4, 3, false),
        0xED => op!(SBC, Absolute,     4, 3, false),
        0xEE => op!(INC, Absolute,     6, 3, false),
        0xEF => op!(ISC, Absolute,     6, 3, false),

        // ── 0xF0 row ────────────────────────────────────────────────────────
        0xF0 => op!(BEQ, Relative,     2, 2, false),
        0xF1 => op!(SBC, IndirectY,    5, 2, true),
        0xF2 => op!(XXX, Implicit,     2, 1, false),  // KIL
        0xF3 => op!(ISC, IndirectY,    8, 2, false),
        0xF4 => op!(NOP, ZeroPageX,    4, 2, false),
        0xF5 => op!(SBC, ZeroPageX,    4, 2, false),
        0xF6 => op!(INC, ZeroPageX,    6, 2, false),
        0xF7 => op!(ISC, ZeroPageX,    6, 2, false),
        0xF8 => op!(SED, Implicit,     2, 1, false),
        0xF9 => op!(SBC, AbsoluteY,    4, 3, true),
        0xFA => op!(NOP, Implicit,     2, 1, false),
        0xFB => op!(ISC, AbsoluteY,    7, 3, false),
        0xFC => op!(NOP, AbsoluteX,    4, 3, true),
        0xFD => op!(SBC, AbsoluteX,    4, 3, true),
        0xFE => op!(INC, AbsoluteX,    7, 3, false),
        0xFF => op!(ISC, AbsoluteX,    7, 3, false),
    }
}
