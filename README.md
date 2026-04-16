# Termnes

A terminal-based NES/Famicom emulator written in Rust.
No audio support. Supports UNROM, MMC1, MMC2, and MMC3.

## Run
```bash
cargo run --release -- <path-to-rom.nes>
```

## Test
```bash
cargo test
```

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
