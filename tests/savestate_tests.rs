use termnes::Nes;
use termnes::savestate::SaveState;

fn make_nrom_rom() -> Vec<u8> {
    let mut rom = Vec::new();
    rom.extend_from_slice(b"NES\x1A");
    rom.push(1);
    rom.push(0);
    rom.push(0x00); // mapper 0
    rom.push(0x00);
    rom.extend_from_slice(&[0u8; 8]);
    let mut prg = vec![0xEA; 0x4000]; // NOPs
    prg[0x3FFC] = 0x00;
    prg[0x3FFD] = 0x80;
    rom.extend_from_slice(&prg);
    rom
}

fn make_unrom_rom() -> Vec<u8> {
    let mut rom = Vec::new();
    rom.extend_from_slice(b"NES\x1A");
    rom.push(2);
    rom.push(0);
    rom.push(0x20); // mapper 2
    rom.push(0x00);
    rom.extend_from_slice(&[0u8; 8]);
    let mut prg = vec![0xEA; 2 * 0x4000];
    prg[0x7FFC] = 0x00;
    prg[0x7FFD] = 0xC0;
    rom.extend_from_slice(&prg);
    rom
}

#[test]
fn nrom_save_state_round_trip() {
    let mut nes = Nes::from_ines_bytes(&make_nrom_rom()).unwrap();
    nes.run_frames(3);

    let state = nes.save_state();
    let bytes = state.to_bytes().unwrap();
    let restored = SaveState::from_bytes(&bytes).unwrap();

    let saved_pc = nes.cpu.pc;
    nes.run_frames(5);
    assert_ne!(nes.cpu.pc, saved_pc);

    nes.load_state(&restored);
    assert_eq!(nes.cpu.pc, saved_pc);
}

#[test]
fn unrom_save_state_round_trip() {
    let mut nes = Nes::from_ines_bytes(&make_unrom_rom()).unwrap();
    nes.run_frames(3);

    let state = nes.save_state();
    let bytes = state.to_bytes().unwrap();

    let saved_pc = nes.cpu.pc;
    nes.run_frames(5);

    let restored = SaveState::from_bytes(&bytes).unwrap();
    nes.load_state(&restored);
    assert_eq!(nes.cpu.pc, saved_pc);
}

#[test]
fn save_state_rejects_bad_magic() {
    let mut data = vec![0u8; 256];
    data[0..8].copy_from_slice(b"NOTVALID");
    assert!(SaveState::from_bytes(&data).is_err());
}

#[test]
fn save_state_rejects_truncated_data() {
    assert!(SaveState::from_bytes(&[]).is_err());
    assert!(SaveState::from_bytes(&[0; 16]).is_err());
}
