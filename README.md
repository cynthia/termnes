# Termnes

A terminal-based NES/Famicom emulator written in Rust.
Supports UNROM, MMC1, MMC2, and MMC3.

## Run
```bash
cargo run --release -- <path-to-rom.nes>
```

### Controls
| Key        | Action   |
|------------|----------|
| Arrow keys | D-Pad    |
| Z          | B button |
| X          | A button |
| A          | Select   |
| S          | Start    |
| F5         | Save state |
| F9         | Load state |
| Esc / Ctrl+C / Ctrl+D | Quit |

## Test
```bash
cargo test
```

## Save States

Press **F5** to save and **F9** to load. The state file is written next to
the ROM as `<rom-name>.state`.

### Autoresume

Pass `--autoresume` to automatically load the state on startup and save it
on clean exit:
```bash
cargo run --release -- mario.nes --autoresume
```
The state is only saved on a graceful quit (Esc / Ctrl+C / Ctrl+D); crashes
will not overwrite it.

## Audio

Audio is enabled by default and played through the system's default output
device. Pass `--mute` to disable it entirely.

### Remote Audio (for SSH sessions)

SSH has no native audio channel, so when running the emulator on a remote
host you can forward audio over TCP to a local `listen` process:

1. On your **local machine** (where the speakers are), start a listener:
   ```bash
   cargo run --release -- listen --port 9001
   ```
2. Connect to the remote host with port forwarding:
   ```bash
   ssh -R 9001:localhost:9001 remote-host
   ```
3. On the **remote host**, run the emulator with `--stream-audio`:
   ```bash
   termnes mario.nes --stream-audio localhost:9001
   ```

The wire format is raw 32-bit float PCM at 44.1 kHz mono.

## Terminal Configuration

To run this emulator effectively, your terminal must meet several requirements.

### Minimum Size
The emulator requires a minimum terminal size of 256x120.

To resize your terminal to the required size, you can run:
```bash
cargo run -- --resize
```
Or use the ANSI escape code directly:
```bash
printf "\033[8;120;256t"
```
*Note: Terminal resizing via escape codes is an xterm extension and must be supported/enabled by your terminal emulator (e.g., in Alacritty, it is often supported but may be restricted by configuration).*

### Keyboard Protocol
Only tested on Alacritty. Might not work well elsewhere.

This emulator supports the [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) for key-release events. While not required, terminals that support this protocol (like Alacritty, Kitty, and WezTerm) provide a more responsive experience.

## License
BSD 3-Clause
