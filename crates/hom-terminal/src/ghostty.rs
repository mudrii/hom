//! libghostty-vt backed terminal emulator.
//!
//! Gated behind the `ghostty-backend` feature flag.
//! Build with: `cargo build --features ghostty-backend`
//!
//! Requires Zig ≥0.15.x at build time (libghostty-vt uses Zig to compile
//! Ghostty's C VT emulation layer into a static library).

#[cfg(feature = "ghostty-backend")]
use libghostty_vt::{
    Terminal,
    ffi::GhosttyPointCoordinate,
    style::{StyleColor, Underline},
    terminal::{Options as TerminalOptions, Point},
};

#[cfg(feature = "ghostty-backend")]
use hom_core::{HomError, HomResult};

#[cfg(feature = "ghostty-backend")]
use hom_core::traits::{
    Cell, CellAttributes, CursorState, ScreenSnapshot, TermColor, TerminalBackend,
};

/// GhosttyBackend wraps `libghostty_vt::Terminal` for full VT520-compliant emulation,
/// including Kitty graphics protocol, sixels, and complete ANSI/xterm support.
///
/// # Safety
///
/// `libghostty_vt::Terminal` is `!Send + !Sync` by design (it wraps C-side state).
/// `GhosttyBackend` is sound as `Send + Sync` because HOM's architecture guarantees
/// that all terminal access happens on the single-threaded event loop — no terminal
/// instance is shared or accessed concurrently across threads.
#[cfg(feature = "ghostty-backend")]
pub struct GhosttyBackend {
    terminal: Terminal<'static, 'static>,
    cols: u16,
    rows: u16,
}

// SAFETY: libghostty_vt::Terminal is !Send + !Sync (C FFI state machine).
// GhosttyBackend is only ever accessed from HOM's single-threaded event loop;
// no GhosttyBackend instance is shared across threads.
#[cfg(feature = "ghostty-backend")]
unsafe impl Send for GhosttyBackend {}

#[cfg(feature = "ghostty-backend")]
unsafe impl Sync for GhosttyBackend {}

#[cfg(feature = "ghostty-backend")]
impl GhosttyBackend {
    fn build(cols: u16, rows: u16, scrollback: usize) -> HomResult<Self> {
        let terminal = Terminal::new(TerminalOptions {
            cols,
            rows,
            max_scrollback: scrollback,
        })
        .map_err(|error| HomError::TerminalError(format!("libghostty-vt init failed: {error}")))?;

        Ok(Self {
            terminal,
            cols,
            rows,
        })
    }
}

#[cfg(feature = "ghostty-backend")]
impl TerminalBackend for GhosttyBackend {
    fn new(cols: u16, rows: u16, scrollback: usize) -> HomResult<Self> {
        Self::build(cols, rows, scrollback)
    }

    fn process(&mut self, bytes: &[u8]) {
        self.terminal.vt_write(bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        // cell_width_px / cell_height_px are 0 — HOM operates in character cells,
        // not pixels; Ghostty uses 0 to mean "inherit from config".
        let _ = self.terminal.resize(cols, rows, 0, 0);
        self.cols = cols;
        self.rows = rows;
    }

    fn screen_snapshot(&self) -> ScreenSnapshot {
        let mut rows = Vec::with_capacity(self.rows as usize);
        // Buffer for grapheme cluster chars — most cells are single chars, allow up to 4.
        let mut char_buf = [' '; 4];

        for row_idx in 0..self.rows {
            let mut row_cells = Vec::with_capacity(self.cols as usize);
            for col_idx in 0..self.cols {
                // PointCoordinate has private fields; construct via the public FFI type.
                let point = Point::Active(
                    GhosttyPointCoordinate {
                        x: col_idx,
                        y: row_idx as u32,
                    }
                    .into(),
                );

                let cell = match self.terminal.grid_ref(point) {
                    Ok(grid_ref) => {
                        let character = match grid_ref.graphemes(&mut char_buf) {
                            Ok(n) if n > 0 => char_buf[0],
                            _ => ' ',
                        };
                        let (fg, bg, attrs) = match grid_ref.style() {
                            Ok(style) => (
                                map_style_color(style.fg_color),
                                map_style_color(style.bg_color),
                                CellAttributes {
                                    bold: style.bold,
                                    italic: style.italic,
                                    underline: !matches!(style.underline, Underline::None),
                                    dim: style.faint,
                                    strikethrough: style.strikethrough,
                                    inverse: style.inverse,
                                    blink: style.blink,
                                },
                            ),
                            Err(_) => (
                                TermColor::Default,
                                TermColor::Default,
                                CellAttributes::default(),
                            ),
                        };
                        Cell {
                            character,
                            fg,
                            bg,
                            attrs,
                        }
                    }
                    Err(_) => Cell::default(),
                };
                row_cells.push(cell);
            }
            rows.push(row_cells);
        }

        ScreenSnapshot {
            rows,
            cols: self.cols,
            num_rows: self.rows,
            cursor: self.cursor(),
        }
    }

    fn cursor(&self) -> CursorState {
        CursorState {
            col: self.terminal.cursor_x().unwrap_or(0),
            row: self.terminal.cursor_y().unwrap_or(0),
            visible: self.terminal.is_cursor_visible().unwrap_or(true),
        }
    }

    fn title(&self) -> Option<&str> {
        self.terminal.title().ok().filter(|s| !s.is_empty())
    }
}

/// Map a libghostty-vt `StyleColor` to HOM's `TermColor`.
///
/// Palette indices follow the standard xterm-256 layout:
/// - 0–7: ANSI named colors (black through white)
/// - 8–15: bright variants
/// - 16–255: extended xterm-256 palette
#[cfg(feature = "ghostty-backend")]
fn map_style_color(color: StyleColor) -> TermColor {
    match color {
        StyleColor::None => TermColor::Default,
        StyleColor::Palette(idx) => match idx.0 {
            0 => TermColor::Black,
            1 => TermColor::Red,
            2 => TermColor::Green,
            3 => TermColor::Yellow,
            4 => TermColor::Blue,
            5 => TermColor::Magenta,
            6 => TermColor::Cyan,
            7 => TermColor::White,
            8 => TermColor::BrightBlack,
            9 => TermColor::BrightRed,
            10 => TermColor::BrightGreen,
            11 => TermColor::BrightYellow,
            12 => TermColor::BrightBlue,
            13 => TermColor::BrightMagenta,
            14 => TermColor::BrightCyan,
            15 => TermColor::BrightWhite,
            n => TermColor::Indexed(n),
        },
        StyleColor::Rgb(rgb) => TermColor::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

#[cfg(all(test, feature = "ghostty-backend"))]
mod tests {
    use super::*;

    #[test]
    fn test_new_creates_correct_dimensions() {
        let backend = GhosttyBackend::new(80, 24, 1_000).unwrap();
        let snap = backend.screen_snapshot();
        assert_eq!(snap.cols, 80);
        assert_eq!(snap.num_rows, 24);
        assert_eq!(snap.rows.len(), 24);
        assert_eq!(snap.rows[0].len(), 80);
    }

    #[test]
    fn test_process_plain_text() {
        let mut backend = GhosttyBackend::new(80, 24, 1_000).unwrap();
        backend.process(b"hello");
        let snap = backend.screen_snapshot();
        let first_chars: String = snap.rows[0].iter().take(5).map(|c| c.character).collect();
        assert_eq!(first_chars, "hello");
    }

    #[test]
    fn test_resize_updates_dimensions() {
        let mut backend = GhosttyBackend::new(80, 24, 1_000).unwrap();
        backend.resize(120, 40);
        let snap = backend.screen_snapshot();
        assert_eq!(snap.cols, 120);
        assert_eq!(snap.num_rows, 40);
        assert_eq!(snap.rows.len(), 40);
        assert_eq!(snap.rows[0].len(), 120);
    }

    #[test]
    fn test_cursor_starts_at_origin() {
        let backend = GhosttyBackend::new(80, 24, 1_000).unwrap();
        let cursor = backend.cursor();
        assert_eq!(cursor.row, 0);
        assert_eq!(cursor.col, 0);
        assert!(cursor.visible);
    }

    #[test]
    fn test_title_none_on_fresh_terminal() {
        let backend = GhosttyBackend::new(80, 24, 1_000).unwrap();
        assert_eq!(backend.title(), None);
    }

    #[test]
    fn test_ansi_red_foreground() {
        let mut backend = GhosttyBackend::new(80, 24, 1_000).unwrap();
        // ESC[31m = set fg red; write 'X'; ESC[m = reset
        backend.process(b"\x1b[31mX\x1b[m");
        let snap = backend.screen_snapshot();
        let cell = &snap.rows[0][0];
        assert_eq!(cell.character, 'X');
        assert_eq!(cell.fg, TermColor::Red);
    }

    #[test]
    fn test_map_style_color_palette() {
        assert_eq!(map_style_color(StyleColor::None), TermColor::Default);
        assert_eq!(
            map_style_color(StyleColor::Palette(libghostty_vt::style::PaletteIndex(1))),
            TermColor::Red
        );
        assert_eq!(
            map_style_color(StyleColor::Palette(libghostty_vt::style::PaletteIndex(9))),
            TermColor::BrightRed
        );
        assert_eq!(
            map_style_color(StyleColor::Palette(libghostty_vt::style::PaletteIndex(42))),
            TermColor::Indexed(42)
        );
    }

    #[test]
    fn test_map_style_color_rgb() {
        use libghostty_vt::style::RgbColor;
        assert_eq!(
            map_style_color(StyleColor::Rgb(RgbColor {
                r: 255,
                g: 128,
                b: 0
            })),
            TermColor::Rgb(255, 128, 0)
        );
    }
}
