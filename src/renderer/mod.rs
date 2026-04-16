use crossterm::event::{
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{cursor, execute, terminal};
use std::fmt::Write as FmtWrite;
use std::io::{self, BufWriter, Write};
use std::time::Duration;

pub const TARGET_FPS: u64 = 60;
pub const FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / TARGET_FPS);

const MIN_COLS: u16 = 256;
const MIN_ROWS: u16 = 120;

pub struct TuiRenderer {
    stdout: io::Stdout,
    /// Pre-allocated string buffer — built per frame, written in one shot.
    buffer: String,
    /// Previous framebuffer for delta comparison.
    prev_fb: Box<[u8; 256 * 240 * 3]>,
    /// True if the terminal accepted keyboard-enhancement flags (kitty
    /// protocol), meaning we'll get real KeyEventKind::Release events.
    /// When false, input handling must auto-release buttons on a timer.
    pub has_keyboard_enhancement: bool,
}

impl TuiRenderer {
    pub fn new() -> Result<Self, String> {
        Self::wait_for_terminal_size()?;

        terminal::enable_raw_mode().map_err(|e| format!("Raw mode failed: {e}"))?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(terminal::ClearType::All),
        )
        .map_err(|e| format!("Terminal setup failed: {e}"))?;

        // Try to opt into the kitty keyboard protocol so we receive explicit
        // Release events. If the terminal doesn't support it the push is a
        // no-op and we fall back to timer-based button release in main.rs.
        let has_keyboard_enhancement =
            crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        if has_keyboard_enhancement {
            let _ = execute!(
                stdout,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
                ),
            );
        }

        Ok(Self {
            stdout,
            // 256 cols × 120 rows × ~40 bytes/cell (worst-case with escape seqs)
            buffer: String::with_capacity(256 * 120 * 40),
            prev_fb: Box::new([0u8; 256 * 240 * 3]),
            has_keyboard_enhancement,
        })
    }

    /// Blocks (polling every 200 ms) until the terminal meets the minimum size.
    fn wait_for_terminal_size() -> Result<(), String> {
        loop {
            let (cols, rows) =
                terminal::size().map_err(|e| format!("Cannot query terminal size: {e}"))?;

            if cols == MIN_COLS && rows == MIN_ROWS {
                return Ok(());
            }

            // Attempt to resize to the exact target size.
            let _ = execute!(io::stdout(), terminal::SetSize(MIN_COLS, MIN_ROWS));

            // If the terminal is at least large enough, we can proceed.
            // render_frame now handles wider terminals correctly.
            if cols >= MIN_COLS && rows >= MIN_ROWS {
                return Ok(());
            }

            // Carriage-return before each line so the box re-draws cleanly in
            // case the terminal is in cooked mode at this point.
            print!(
                "\r╔══════════════════════════════════════════════════╗\r\n\
                 ║  NES Emulator requires terminal size: {:3} x {:3}  ║\r\n\
                 ║  Current size: {:3} x {:3}{}║\r\n\
                 ║  Please resize your terminal window or run:      ║\r\n\
                 ║  printf \"\\033[8;{};{}t\"                         ║\r\n\
                 ╚══════════════════════════════════════════════════╝\r",
                MIN_COLS,
                MIN_ROWS,
                cols,
                rows,
                // pad to keep column width constant (max 3+3 digits shown)
                " ".repeat(25usize.saturating_sub(
                    cols.to_string().len() + rows.to_string().len()
                )),
                MIN_ROWS,
                MIN_COLS,
            );
            io::stdout().flush().ok();
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Renders a 256×240 RGB framebuffer to the terminal.
    ///
    /// Each terminal cell = 1 col × 2 pixel rows:
    ///   foreground color → top pixel, background color → bottom pixel
    ///   character: ▀ (U+2580 UPPER HALF BLOCK)
    ///
    /// Optimisations applied:
    /// 1. Entire frame built into one String, written via a single `write_all`.
    /// 2. Color escape sequences are skipped when fg/bg are unchanged from the
    ///    previous cell in the same row (common on NES backgrounds).
    /// 3. Row-level delta: rows where neither top nor bottom pixels changed are
    ///    skipped; a cursor-move escape repositions for the next dirty row.
    pub fn render_frame(&mut self, framebuffer: &[u8; 256 * 240 * 3]) -> Result<(), String> {
        self.buffer.clear();

        for row in 0..120usize {
            let top_y = row * 2;
            let bot_y = top_y + 1;
            let row_start_top = top_y * 256 * 3;
            let row_start_bot = bot_y * 256 * 3;

            // Check whether this 2-pixel row is identical to the previous frame.
            let top_slice = &framebuffer[row_start_top..row_start_top + 256 * 3];
            let bot_slice = &framebuffer[row_start_bot..row_start_bot + 256 * 3];
            let prev_top = &self.prev_fb[row_start_top..row_start_top + 256 * 3];
            let prev_bot = &self.prev_fb[row_start_bot..row_start_bot + 256 * 3];

            if top_slice == prev_top && bot_slice == prev_bot {
                // Nothing changed in this row
                continue;
            }

            // \x1b[<row>;<col>H  (1-indexed). Always move to the start of the 
            // row to handle terminals wider than 256 columns.
            write!(self.buffer, "\x1b[{};1H", row + 1).unwrap();

            // Emit cells with color-diff optimisation.
            let mut prev_fg = (0u8, 0u8, 0u8);
            let mut prev_bg = (0u8, 0u8, 0u8);
            let mut force = true; // first cell always needs escape seqs

            for col in 0..256usize {
                let ti = (top_y * 256 + col) * 3;
                let bi = (bot_y * 256 + col) * 3;

                let fg = (framebuffer[ti], framebuffer[ti + 1], framebuffer[ti + 2]);
                let bg = (framebuffer[bi], framebuffer[bi + 1], framebuffer[bi + 2]);

                if force || fg != prev_fg || bg != prev_bg {
                    write!(
                        self.buffer,
                        "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m",
                        fg.0, fg.1, fg.2, bg.0, bg.1, bg.2
                    )
                    .unwrap();
                    prev_fg = fg;
                    prev_bg = bg;
                    force = false;
                }

                self.buffer.push('▀');
            }

            // Reset colors at end of row to avoid bleed into terminal chrome.
            self.buffer.push_str("\x1b[0m");
        }

        let mut out = BufWriter::new(&mut self.stdout);
        out.write_all(self.buffer.as_bytes())
            .map_err(|e| format!("Render write failed: {e}"))?;
        out.flush().map_err(|e| format!("Render flush failed: {e}"))?;

        self.prev_fb.copy_from_slice(framebuffer);
        Ok(())
    }

    pub fn cleanup(&mut self) {
        if self.has_keyboard_enhancement {
            let _ = execute!(self.stdout, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            self.stdout,
            terminal::Clear(terminal::ClearType::All),
            cursor::Show,
            terminal::LeaveAlternateScreen,
        );
        let _ = terminal::disable_raw_mode();
    }
}

impl Drop for TuiRenderer {
    fn drop(&mut self) {
        self.cleanup();
    }
}
