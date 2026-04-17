use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

use termnes::audio::AudioOutput;
use termnes::bus::Bus;
use termnes::cartridge::Cartridge;
use termnes::cpu::opcodes::decode;
use termnes::cpu::Cpu;
use termnes::input::JoypadButton;
use termnes::remote_audio::{self, RemoteAudioSender};
use termnes::renderer::TuiRenderer;
use termnes::savestate::SaveState;

#[derive(Parser)]
#[command(name = "termnes", about = "NES TUI Emulator")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to the ROM file (.nes)
    rom: Option<String>,

    /// Disable audio output
    #[arg(long)]
    mute: bool,

    /// Stream audio to a remote listener (e.g. localhost:9001)
    #[arg(long)]
    stream_audio: Option<String>,

    /// Automatically save state on exit and resume on start
    #[arg(long)]
    autoresume: bool,

    /// Run in headless trace mode
    #[arg(long)]
    trace: bool,

    /// Path for trace log output
    #[arg(long, default_value = "nes_trace.log")]
    trace_log: String,

    /// Resize terminal to 256x120
    #[arg(long)]
    resize: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Listen for a remote audio stream and play it locally
    Listen {
        /// Port to listen on
        #[arg(long, default_value_t = 9001)]
        port: u16,
    },
}

const AUDIO_SAMPLE_RATE: u32 = 44_100;

/// ~60.0988 Hz NTSC frame duration
const FRAME_DURATION: Duration = Duration::from_micros(16_639);

fn main() {
    // Restore terminal on panic
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        default_hook(info);
    }));

    let args = Args::parse();

    // Handle `listen` subcommand
    if let Some(Command::Listen { port }) = args.command {
        if let Err(e) = remote_audio::run_listen(port) {
            eprintln!("Listen error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if args.resize {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::SetSize(256, 120));
        return;
    }

    let rom_path = match &args.rom {
        Some(p) => p.clone(),
        None => {
            eprintln!("Error: ROM path is required");
            eprintln!("Usage: termnes <rom.nes> [OPTIONS]");
            std::process::exit(1);
        }
    };

    let cartridge = match Cartridge::from_file(&rom_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load ROM: {}", e);
            std::process::exit(1);
        }
    };

    eprintln!(
        "Loaded ROM: mapper {}, {} PRG banks",
        cartridge.mapper_id,
        cartridge.prg_rom.len() / 16384
    );
    eprintln!();
    eprintln!("NES TUI Emulator");
    eprintln!("\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}");
    eprintln!("Controls:");
    eprintln!("  Arrow keys  - D-Pad");
    eprintln!("  Z           - B button");
    eprintln!("  X           - A button");
    eprintln!("  A           - Select");
    eprintln!("  S           - Start");
    eprintln!("  Esc         - Quit");
    eprintln!("  F5          - Save state");
    eprintln!("  F9          - Load state");
    eprintln!("\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}");

    let bus = Bus::new(cartridge);
    let mut cpu = Cpu::new(bus);
    cpu.reset();
    cpu.bus.load_battery_save();

    // Audio setup (non-fatal if it fails)
    let (audio, mut remote_audio) = if args.mute {
        (None, None)
    } else {
        cpu.bus.apu.set_sample_rate(AUDIO_SAMPLE_RATE);
        if let Some(ref addr) = args.stream_audio {
            // Stream audio to remote listener instead of local device
            match RemoteAudioSender::connect(addr, AUDIO_SAMPLE_RATE) {
                Ok(sender) => {
                    eprintln!("Audio: streaming to {addr} at {AUDIO_SAMPLE_RATE} Hz");
                    (None, Some(sender))
                }
                Err(e) => {
                    eprintln!("Remote audio disabled: {e}");
                    (None, None)
                }
            }
        } else {
            match AudioOutput::new(AUDIO_SAMPLE_RATE) {
                Ok(a) => {
                    eprintln!("Audio: {} Hz", AUDIO_SAMPLE_RATE);
                    (Some(a), None)
                }
                Err(e) => {
                    eprintln!("Audio disabled: {}", e);
                    (None, None)
                }
            }
        }
    };

    if args.trace {
        eprintln!("Trace mode (headless). Writing to {}", args.trace_log);
        run_trace(&mut cpu, &args.trace_log);
        return;
    }

    let mut renderer = match TuiRenderer::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Renderer init failed: {}", e);
            std::process::exit(1);
        }
    };

    run_emulation(
        &mut cpu,
        &mut renderer,
        audio.as_ref(),
        remote_audio.as_mut(),
        &rom_path,
        args.autoresume,
    );
}

fn save_state_path(rom_path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(rom_path);
    p.with_extension("state")
}

fn do_save_state(cpu: &Cpu, rom_path: &str) {
    let state = SaveState::new(
        cpu.capture_state(),
        cpu.bus.ppu.capture_state(),
        cpu.bus.apu.capture_state(),
        cpu.bus.capture_state(),
        cpu.bus.joypad1.capture_state(),
        cpu.bus.joypad2.capture_state(),
        cpu.bus.cartridge.save_mapper_state(),
    );
    let path = save_state_path(rom_path);
    match state.to_bytes() {
        Ok(bytes) => match std::fs::write(&path, &bytes) {
            Ok(()) => eprintln!("State saved to {}", path.display()),
            Err(e) => eprintln!("Failed to write state: {}", e),
        },
        Err(e) => eprintln!("Failed to serialize state: {}", e),
    }
}

fn do_load_state(cpu: &mut Cpu, rom_path: &str) {
    let path = save_state_path(rom_path);
    match std::fs::read(&path) {
        Ok(bytes) => match SaveState::from_bytes(&bytes) {
            Ok(state) => {
                cpu.restore_state(&state.cpu);
                cpu.bus.ppu.restore_state(&state.ppu);
                cpu.bus.apu.restore_state(&state.apu);
                cpu.bus.restore_state(&state.bus);
                cpu.bus.joypad1.restore_state(&state.joypad1);
                cpu.bus.joypad2.restore_state(&state.joypad2);
                cpu.bus.cartridge.load_mapper_state(&state.mapper);
                eprintln!("State loaded from {}", path.display());
            }
            Err(e) => eprintln!("Failed to parse state: {}", e),
        },
        Err(_) => {} // no save state file, silently ignore
    }
}

fn run_emulation(
    cpu: &mut Cpu,
    renderer: &mut TuiRenderer,
    audio: Option<&AudioOutput>,
    mut remote_audio: Option<&mut RemoteAudioSender>,
    rom_path: &str,
    autoresume: bool,
) {
    eprintln!("Emulation started. Press Esc or Ctrl+C to quit.");
    let mut frame_count: u64 = 0;
    let mut input = InputState::new(renderer.has_keyboard_enhancement);

    if autoresume {
        do_load_state(cpu, rom_path);
    }

    loop {
        let frame_start = Instant::now();

        // Run CPU until PPU completes a frame (~29780 CPU cycles for NTSC)
        while !cpu.bus.ppu.frame_complete {
            // Handle pending OAM DMA (CPU is halted for 513-514 cycles while
            // PPU/APU/mapper continue ticking; do_dma handles that internally).
            if cpu.bus.do_dma() {
                if cpu.bus.poll_nmi() {
                    cpu.nmi();
                }
                if cpu.bus.poll_irq() {
                    cpu.irq();
                }
                continue;
            }

            // step() handles bus.tick() and NMI polling internally
            cpu.step();
        }
        cpu.bus.ppu.frame_complete = false;
        frame_count += 1;

        if frame_count % 60 == 0 {
            // Log progress every 60 frames (approx 1 second) to stderr
            // eprintln!("Frames completed: {}", frame_count);
        }

        // Drain audio samples and send to output device (local or remote)
        if audio.is_some() || remote_audio.is_some() {
            let samples = cpu.bus.apu.drain_samples();
            if let Some(a) = audio {
                a.queue_samples(&samples);
            }
            if let Some(ref mut r) = remote_audio {
                r.queue_samples(&samples);
            }
        }

        // Render the frame
        if let Err(e) = renderer.render_frame(&cpu.bus.ppu.framebuffer) {
            eprintln!("Render error: {}", e);
            break;
        }

        // Handle input once per frame (non-blocking)
        if handle_input(cpu, &mut input, rom_path) {
            break;
        }
        // Tick the auto-release timers so terminals without key-release
        // events still "unpress" held buttons.
        input.tick_frame(cpu);

        // Frame timing — use audio buffer level as master clock when audio
        // is active; fall back to wall-clock sleep otherwise.
        // Target ~3 frames of audio buffered (~50 ms latency at 44.1 kHz).
        if let Some(audio) = audio {
            const TARGET_SAMPLES: usize = 44_100 / 60 * 3; // ~2205
            while audio.buffered_samples() > TARGET_SAMPLES {
                std::thread::sleep(Duration::from_millis(1));
            }
        } else {
            let elapsed = frame_start.elapsed();
            if elapsed < FRAME_DURATION {
                std::thread::sleep(FRAME_DURATION - elapsed);
            }
        }
    }
    if autoresume {
        do_save_state(cpu, rom_path);
    }
    cpu.bus.save_battery();
}

/// Headless trace mode: runs the emulator, keeps a ring buffer of the last N
/// CPU instructions, logs frame-completion summaries, and dumps the ring buffer
/// on hang (too many cycles without a frame completing) or timeout.
fn run_trace(cpu: &mut Cpu, log_path: &str) {
    const RING_SIZE: usize = 8192;
    const HANG_CYCLE_LIMIT: u64 = 200_000; // expected ~29_780 per frame
    const TIMEOUT: Duration = Duration::from_secs(90);

    let file = match File::create(log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open trace log {}: {}", log_path, e);
            return;
        }
    };
    let mut log = BufWriter::new(file);

    let _ = writeln!(log, "NES Trace Log");
    let _ = writeln!(
        log,
        "Format: [#INSTR] PC OP:OPBYTES  A X Y SP P  OPCODE MODE  SL:scanline DOT:cycle"
    );
    let _ = writeln!(log, "---");

    let mut frame_count: u64 = 0;
    let mut instr_count: u64 = 0;
    let mut total_cycles: u64 = 0;
    let start = Instant::now();
    let mut ring: VecDeque<String> = VecDeque::with_capacity(RING_SIZE + 1);

    loop {
        let mut cycles_in_frame: u64 = 0;
        let frame_start_instr = instr_count;

        while !cpu.bus.ppu.frame_complete {
            if cpu.bus.do_dma() {
                if cpu.bus.poll_nmi() {
                    cpu.nmi();
                }
                if cpu.bus.poll_irq() {
                    cpu.irq();
                }
                cycles_in_frame += 513;
                total_cycles += 513;
                push_ring(&mut ring, "  [OAM DMA]".to_string(), RING_SIZE);
                continue;
            }

            // Peek at instruction bytes without side effects
            let pc = cpu.pc;
            let b0 = cpu.bus.peek(pc);
            let info = decode(b0);
            let b1 = cpu.bus.peek(pc.wrapping_add(1));
            let b2 = cpu.bus.peek(pc.wrapping_add(2));
            let bytes_str = match info.bytes {
                1 => format!("{:02X}      ", b0),
                2 => format!("{:02X} {:02X}   ", b0, b1),
                3 => format!("{:02X} {:02X} {:02X}", b0, b1, b2),
                _ => format!("{:02X}      ", b0),
            };
            let line = format!(
                "[{:>10}] {:04X} {}  A:{:02X} X:{:02X} Y:{:02X} SP:{:02X} P:{:02X}  {:?} {:?}  SL:{:>3} DOT:{:>3}",
                instr_count,
                pc,
                bytes_str,
                cpu.a, cpu.x, cpu.y, cpu.sp, cpu.status.bits(),
                info.opcode, info.mode,
                cpu.bus.ppu.scanline, cpu.bus.ppu.cycle
            );
            push_ring(&mut ring, line, RING_SIZE);

            let cycles = cpu.step();
            cycles_in_frame += cycles as u64;
            total_cycles += cycles as u64;
            instr_count += 1;

            // Hang detection within a single frame
            if cycles_in_frame > HANG_CYCLE_LIMIT {
                let _ =
                    writeln!(log,
                    "\n!!! HANG DETECTED at frame {} after {} in-frame cycles ({} total instr) !!!",
                    frame_count, cycles_in_frame, instr_count);
                dump_diagnostics(
                    &mut log,
                    cpu,
                    &ring,
                    frame_count,
                    instr_count,
                    total_cycles,
                    start.elapsed(),
                );
                let _ = log.flush();
                eprintln!(
                    "HANG DETECTED at frame {}. Trace dumped to {}",
                    frame_count, log_path
                );
                return;
            }
        }

        cpu.bus.ppu.frame_complete = false;
        frame_count += 1;

        let _ = writeln!(log,
            "=== FRAME {:>5} done: {:>6} in-frame cycles, {:>6} instr  (total: {} instr, {} cyc, elapsed {:?}) ===",
            frame_count,
            cycles_in_frame,
            instr_count - frame_start_instr,
            instr_count, total_cycles, start.elapsed());
        // Flush occasionally so we don't lose data on a hard hang
        if frame_count % 30 == 0 {
            let _ = log.flush();
        }

        if start.elapsed() > TIMEOUT {
            let _ = writeln!(
                log,
                "\nTIMEOUT after {:?} ({} frames, {} instr)",
                start.elapsed(),
                frame_count,
                instr_count
            );
            dump_diagnostics(
                &mut log,
                cpu,
                &ring,
                frame_count,
                instr_count,
                total_cycles,
                start.elapsed(),
            );
            let _ = log.flush();
            eprintln!(
                "Timeout reached at frame {}. Trace dumped to {}",
                frame_count, log_path
            );
            return;
        }
    }
}

fn push_ring(ring: &mut VecDeque<String>, line: String, max: usize) {
    if ring.len() >= max {
        ring.pop_front();
    }
    ring.push_back(line);
}

fn dump_diagnostics(
    log: &mut BufWriter<File>,
    cpu: &Cpu,
    ring: &VecDeque<String>,
    frame_count: u64,
    instr_count: u64,
    total_cycles: u64,
    elapsed: Duration,
) {
    let _ = writeln!(log, "\n--- Summary ---");
    let _ = writeln!(log, "frames:       {}", frame_count);
    let _ = writeln!(log, "instructions: {}", instr_count);
    let _ = writeln!(log, "cpu cycles:   {}", total_cycles);
    let _ = writeln!(log, "elapsed:      {:?}", elapsed);

    let _ = writeln!(log, "\n--- CPU state ---");
    let _ = writeln!(
        log,
        "PC:{:04X} A:{:02X} X:{:02X} Y:{:02X} SP:{:02X} P:{:02X}",
        cpu.pc,
        cpu.a,
        cpu.x,
        cpu.y,
        cpu.sp,
        cpu.status.bits()
    );

    let _ = writeln!(log, "\n--- PPU state ---");
    let _ = writeln!(
        log,
        "scanline:{} cycle:{} frame_complete:{} nmi_triggered:{}",
        cpu.bus.ppu.scanline,
        cpu.bus.ppu.cycle,
        cpu.bus.ppu.frame_complete,
        cpu.bus.ppu.nmi_triggered
    );
    let _ = writeln!(
        log,
        "ctrl:{:02X} mask:{:02X} status:{:02X} oam_addr:{:02X}",
        cpu.bus.ppu.ctrl, cpu.bus.ppu.mask, cpu.bus.ppu.status, cpu.bus.ppu.oam_addr
    );
    let _ = writeln!(
        log,
        "vram_addr:{:04X} temp_vram_addr:{:04X} fine_x:{} write_latch:{}",
        cpu.bus.ppu.vram_addr,
        cpu.bus.ppu.temp_vram_addr,
        cpu.bus.ppu.fine_x,
        cpu.bus.ppu.write_latch
    );
    let _ = writeln!(
        log,
        "t:{:04X} v:{:04X} fx:{}  [coarse_x(t)={}, coarse_y(t)={}, nt(t)={}, fine_y(t)={}]",
        cpu.bus.ppu.temp_vram_addr,
        cpu.bus.ppu.vram_addr,
        cpu.bus.ppu.fine_x,
        cpu.bus.ppu.temp_vram_addr & 0x1F,
        (cpu.bus.ppu.temp_vram_addr >> 5) & 0x1F,
        (cpu.bus.ppu.temp_vram_addr >> 10) & 0x03,
        (cpu.bus.ppu.temp_vram_addr >> 12) & 0x07
    );

    let _ = writeln!(log, "\n--- OAM (first 4 sprites) ---");
    for i in 0..4 {
        let y = cpu.bus.ppu.oam[i * 4];
        let t = cpu.bus.ppu.oam[i * 4 + 1];
        let a = cpu.bus.ppu.oam[i * 4 + 2];
        let x = cpu.bus.ppu.oam[i * 4 + 3];
        let _ = writeln!(log,
            "  sprite {}: Y={:02X}({:>3}) tile={:02X} attr={:02X} X={:02X}({:>3})  [flip_v={} flip_h={} behind_bg={} palette={}]",
            i, y, y, t, a, x, x,
            a & 0x80 != 0, a & 0x40 != 0, a & 0x20 != 0, a & 0x03);
    }

    let _ = writeln!(log, "\n--- Sprite-0 Hit Diagnostics ---");
    let _ = writeln!(
        log,
        "collected (spr0 on scanline):    {}",
        cpu.bus.ppu.dbg_sprite0_collected
    );
    let _ = writeln!(
        log,
        "opaque-pixel scanlines:          {}",
        cpu.bus.ppu.dbg_sprite0_opaque_scanlines
    );
    let _ = writeln!(
        log,
        "opaque-spr but BG transparent:   {}",
        cpu.bus.ppu.dbg_sprite0_opaque_but_bg_transparent
    );
    let _ = writeln!(
        log,
        "hits fired:                      {}",
        cpu.bus.ppu.dbg_sprite0_hits
    );
    let _ = writeln!(
        log,
        "PPUSCROLL writes (SL 0-239):     {}",
        cpu.bus.ppu.dbg_scroll_writes_visible
    );
    let _ = writeln!(
        log,
        "PPUSCROLL writes (VBlank/-1):    {}",
        cpu.bus.ppu.dbg_scroll_writes_vblank
    );

    // Dump the HUD rows of each nametable bank. Sprite 0 is at tile row 3-4.
    // Vertical mirroring: bank 0 = NT0/NT2 (left), bank 1 = NT1/NT3 (right).
    let _ = writeln!(
        log,
        "\n--- Nametable bank 0, rows 0-4 (HUD area, $2000/$2800) ---"
    );
    for row in 0..5 {
        let _ = write!(log, "  row {}: ", row);
        for col in 0..32 {
            let idx = row * 32 + col;
            let _ = write!(log, "{:02X} ", cpu.bus.ppu.vram[idx]);
        }
        let _ = writeln!(log);
    }
    let _ = writeln!(
        log,
        "\n--- Nametable bank 1, rows 0-4 (HUD area, $2400/$2C00) ---"
    );
    for row in 0..5 {
        let _ = write!(log, "  row {}: ", row);
        for col in 0..32 {
            let idx = 0x400 + row * 32 + col;
            let _ = write!(log, "{:02X} ", cpu.bus.ppu.vram[idx]);
        }
        let _ = writeln!(log);
    }
    // Sprite 0 is at X=88, Y=25-32. That's tile (col=11, row=3-4). Show those specifically.
    let _ = writeln!(log, "\nSprite-0 covers tile (col=11, row=3-4).");
    let _ = writeln!(
        log,
        "  bank 0 [3][11] = {:02X}, [4][11] = {:02X}",
        cpu.bus.ppu.vram[3 * 32 + 11],
        cpu.bus.ppu.vram[4 * 32 + 11]
    );
    let _ = writeln!(
        log,
        "  bank 1 [3][11] = {:02X}, [4][11] = {:02X}",
        cpu.bus.ppu.vram[0x400 + 3 * 32 + 11],
        cpu.bus.ppu.vram[0x400 + 4 * 32 + 11]
    );
    if let Some(d) = cpu.bus.ppu.dbg_last_sprite0 {
        let _ = writeln!(log, "last sprite-0 rendered:");
        let _ = writeln!(log,
            "  SL={} OAM[Y={:02X},tile={:02X},attr={:02X},X={:02X}] row_used={} lo={:02X} hi={:02X}",
            d.scanline, d.oam_y, d.oam_tile, d.oam_attr, d.oam_x,
            d.row_used, d.lo, d.hi);
        let _ = writeln!(log,
            "  mask={:02X} bg_enabled={} spr_enabled={}  had_opaque={} first_opaque_x={} had_opaque_bg_overlap={} fired={}",
            d.mask_at_check, d.bg_enabled, d.spr_enabled,
            d.had_opaque, d.first_opaque_x, d.had_opaque_bg_at_opaque_spr, d.fired);
    } else {
        let _ = writeln!(log, "last sprite-0 rendered:  <never>");
    }
    if let Some(d) = cpu.bus.ppu.dbg_last_sprite0_hit {
        let _ = writeln!(log, "last sprite-0 HIT:");
        let _ = writeln!(
            log,
            "  SL={} OAM[Y={:02X},tile={:02X},attr={:02X},X={:02X}] lo={:02X} hi={:02X}",
            d.scanline, d.oam_y, d.oam_tile, d.oam_attr, d.oam_x, d.lo, d.hi
        );
    } else {
        let _ = writeln!(log, "last sprite-0 HIT:       <never>");
    }

    // Stack dump ($0100-$01FF, show around SP)
    let _ = writeln!(log, "\n--- Stack (around SP=${:02X}) ---", cpu.sp);
    let sp_lo = cpu.sp.saturating_sub(8);
    let sp_hi = cpu.sp.saturating_add(16);
    for i in sp_lo..=sp_hi {
        let addr = 0x0100u16 | i as u16;
        let val = cpu.bus.cpu_ram[(addr & 0x07FF) as usize];
        let marker = if i == cpu.sp { " <- SP" } else { "" };
        let _ = writeln!(log, "  ${:04X} = {:02X}{}", addr, val, marker);
    }

    // Zero-page dump (common for NES game state)
    let _ = writeln!(log, "\n--- Zero page ($0000-$00FF) ---");
    for row in 0..16 {
        let base = row * 16;
        let _ = write!(log, "  ${:02X}:", base);
        for col in 0..16 {
            let _ = write!(log, " {:02X}", cpu.bus.cpu_ram[base + col]);
        }
        let _ = writeln!(log);
    }

    let _ = writeln!(log, "\n--- Last {} instructions ---", ring.len());
    for entry in ring {
        let _ = writeln!(log, "{}", entry);
    }
}

/// Per-button auto-release state. On terminals that don't emit
/// KeyEventKind::Release (i.e. most terminals, when kitty keyboard protocol
/// isn't available), we treat every Press as pressing + arming a countdown.
/// Each frame the counter decrements; at 0 we force a release. As long as the
/// key is held, terminal auto-repeat refreshes the counter.
struct InputState {
    /// Remaining frames before each pressed button auto-releases.
    /// When `has_enhancement` is true, we rely on real Release events and
    /// never populate this map.
    timers: std::collections::HashMap<JoypadButton, u8>,
    has_enhancement: bool,
}

impl InputState {
    /// Frames to hold a button after its last Press event. Must exceed the
    /// terminal's initial auto-repeat delay (~500 ms on Linux = 30 frames at
    /// 60 Hz) so a sustained hold doesn't false-release before auto-repeat
    /// starts refreshing the timer. 35 frames ≈ 583 ms.
    const HOLD_FRAMES: u8 = 35;

    fn new(has_enhancement: bool) -> Self {
        Self {
            timers: std::collections::HashMap::new(),
            has_enhancement,
        }
    }

    /// Press a button. On non-enhanced terminals, arm the auto-release timer.
    fn press(&mut self, cpu: &mut Cpu, button: JoypadButton) {
        cpu.bus.joypad1.set_button(button, true);
        if !self.has_enhancement {
            self.timers.insert(button, Self::HOLD_FRAMES);
        }
    }

    /// Release a button. Clears the auto-release timer.
    fn release(&mut self, cpu: &mut Cpu, button: JoypadButton) {
        cpu.bus.joypad1.set_button(button, false);
        self.timers.remove(&button);
    }

    /// Advance per-frame timers. When a timer hits 0 the button is released.
    /// Only runs if we're on a terminal without real Release events.
    fn tick_frame(&mut self, cpu: &mut Cpu) {
        if self.has_enhancement {
            return;
        }
        self.timers.retain(|button, counter| {
            *counter = counter.saturating_sub(1);
            if *counter == 0 {
                cpu.bus.joypad1.set_button(*button, false);
                false
            } else {
                true
            }
        });
    }
}

/// Maps a keycode to a joypad button, if any.
fn key_to_button(code: KeyCode) -> Option<JoypadButton> {
    match code {
        KeyCode::Char('z') | KeyCode::Char('Z') => Some(JoypadButton::B),
        KeyCode::Char('x') | KeyCode::Char('X') => Some(JoypadButton::A),
        KeyCode::Char('a') | KeyCode::Char('A') => Some(JoypadButton::Select),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(JoypadButton::Start),
        KeyCode::Up => Some(JoypadButton::Up),
        KeyCode::Down => Some(JoypadButton::Down),
        KeyCode::Left => Some(JoypadButton::Left),
        KeyCode::Right => Some(JoypadButton::Right),
        _ => None,
    }
}

/// Polls all pending input events. Returns true if quit was requested.
fn handle_input(cpu: &mut Cpu, input: &mut InputState, rom_path: &str) -> bool {
    use crossterm::event::KeyEventKind;

    while event::poll(Duration::from_millis(0)).unwrap_or(false) {
        let Ok(ev) = event::read() else { continue };
        let Event::Key(key_event) = ev else { continue };

        // Quit on Esc or Ctrl+C (only on initial press, not repeats)
        if key_event.kind == KeyEventKind::Press
            && (key_event.code == KeyCode::Esc
                || (key_event.code == KeyCode::Char('c')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL))
                || (key_event.code == KeyCode::Char('d')
                    && key_event.modifiers.contains(KeyModifiers::CONTROL)))
        {
            return true;
        }

        // Save state (F5) / Load state (F9)
        if key_event.kind == KeyEventKind::Press {
            match key_event.code {
                KeyCode::F(5) => {
                    do_save_state(cpu, rom_path);
                    continue;
                }
                KeyCode::F(9) => {
                    do_load_state(cpu, rom_path);
                    continue;
                }
                _ => {}
            }
        }

        let Some(button) = key_to_button(key_event.code) else {
            continue;
        };

        match key_event.kind {
            // Press or Repeat both re-arm the hold timer (and re-press, which
            // is idempotent). Repeat only fires on kitty-enhanced terminals.
            KeyEventKind::Press | KeyEventKind::Repeat => {
                input.press(cpu, button);
            }
            KeyEventKind::Release => {
                input.release(cpu, button);
            }
        }
    }
    false
}
