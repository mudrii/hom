//! vt100-based fallback terminal emulator.
//!
//! Used when libghostty-rs is not available (no Zig toolchain)
//! or as a lightweight alternative for CI/testing.

#[cfg(feature = "vt100-backend")]
use hom_core::traits::{
    Cell, CellAttributes, CursorState, ScreenSnapshot, TermColor, TerminalBackend,
};

#[cfg(feature = "vt100-backend")]
use hom_core::HomResult;

/// Fallback terminal emulator backed by the `vt100` crate.
///
/// Good enough for most harnesses but lacks Kitty graphics protocol
/// and some advanced VT features that libghostty provides.
#[cfg(feature = "vt100-backend")]
pub struct Vt100Backend {
    parser: vt100::Parser,
}

#[cfg(feature = "vt100-backend")]
impl Vt100Backend {
    fn build(cols: u16, rows: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, 0),
        }
    }
}

#[cfg(feature = "vt100-backend")]
impl TerminalBackend for Vt100Backend {
    fn new(cols: u16, rows: u16, _scrollback: usize) -> HomResult<Self> {
        Ok(Self::build(cols, rows))
    }

    fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    fn screen_snapshot(&self) -> ScreenSnapshot {
        let screen = self.parser.screen();
        let (rows_count, cols_count) = screen.size();
        let mut rows = Vec::with_capacity(rows_count as usize);

        for row_idx in 0..rows_count {
            let mut row = Vec::with_capacity(cols_count as usize);
            for col_idx in 0..cols_count {
                let Some(vt_cell) = screen.cell(row_idx, col_idx) else {
                    row.push(Cell::default());
                    continue;
                };
                row.push(Cell {
                    character: vt_cell.contents().chars().next().unwrap_or(' '),
                    fg: map_vt100_color(vt_cell.fgcolor()),
                    bg: map_vt100_color(vt_cell.bgcolor()),
                    attrs: CellAttributes {
                        bold: vt_cell.bold(),
                        italic: vt_cell.italic(),
                        underline: vt_cell.underline(),
                        dim: false, // vt100 doesn't expose dim directly
                        strikethrough: false,
                        inverse: vt_cell.inverse(),
                        blink: false,
                    },
                });
            }
            rows.push(row);
        }

        let cursor_pos = screen.cursor_position();
        ScreenSnapshot {
            rows,
            cols: cols_count,
            num_rows: rows_count,
            cursor: CursorState {
                row: cursor_pos.0,
                col: cursor_pos.1,
                visible: !screen.hide_cursor(),
            },
        }
    }

    fn cursor(&self) -> CursorState {
        let screen = self.parser.screen();
        let pos = screen.cursor_position();
        CursorState {
            row: pos.0,
            col: pos.1,
            visible: !screen.hide_cursor(),
        }
    }

    fn title(&self) -> Option<&str> {
        // vt100 0.16 does not expose title — return None
        None
    }
}

/// Map vt100 color to our TermColor.
#[cfg(feature = "vt100-backend")]
fn map_vt100_color(color: vt100::Color) -> TermColor {
    match color {
        vt100::Color::Default => TermColor::Default,
        vt100::Color::Idx(0) => TermColor::Black,
        vt100::Color::Idx(1) => TermColor::Red,
        vt100::Color::Idx(2) => TermColor::Green,
        vt100::Color::Idx(3) => TermColor::Yellow,
        vt100::Color::Idx(4) => TermColor::Blue,
        vt100::Color::Idx(5) => TermColor::Magenta,
        vt100::Color::Idx(6) => TermColor::Cyan,
        vt100::Color::Idx(7) => TermColor::White,
        vt100::Color::Idx(8) => TermColor::BrightBlack,
        vt100::Color::Idx(9) => TermColor::BrightRed,
        vt100::Color::Idx(10) => TermColor::BrightGreen,
        vt100::Color::Idx(11) => TermColor::BrightYellow,
        vt100::Color::Idx(12) => TermColor::BrightBlue,
        vt100::Color::Idx(13) => TermColor::BrightMagenta,
        vt100::Color::Idx(14) => TermColor::BrightCyan,
        vt100::Color::Idx(15) => TermColor::BrightWhite,
        vt100::Color::Idx(idx) => TermColor::Indexed(idx),
        vt100::Color::Rgb(r, g, b) => TermColor::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hom_core::TerminalBackend;

    #[test]
    fn test_process_and_snapshot() {
        let mut term = Vt100Backend::new(80, 24, 100).unwrap();
        term.process(b"Hello, World!");
        let snap = term.screen_snapshot();
        let text = snap.text();
        assert!(text.contains("Hello, World!"), "got: {text}");
    }

    #[test]
    fn test_color_processing() {
        let mut term = Vt100Backend::new(80, 24, 100).unwrap();
        term.process(b"\x1b[31mRed\x1b[0m");
        let snap = term.screen_snapshot();
        assert!(
            matches!(snap.rows[0][0].fg, hom_core::TermColor::Red),
            "got: {:?}",
            snap.rows[0][0].fg
        );
    }

    #[test]
    fn test_resize() {
        let mut term = Vt100Backend::new(80, 24, 100).unwrap();
        term.resize(40, 12);
        let snap = term.screen_snapshot();
        assert_eq!(snap.cols, 40);
        assert_eq!(snap.num_rows, 12);
    }

    #[test]
    fn test_cursor_movement() {
        let mut term = Vt100Backend::new(80, 24, 100).unwrap();
        // LF moves down without carriage return; \r\n resets column
        term.process(b"abc\r\ndef");
        let cursor = term.cursor();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.col, 3);
    }

    #[test]
    fn test_newline_handling() {
        let mut term = Vt100Backend::new(80, 24, 100).unwrap();
        term.process(b"line1\nline2\nline3");
        let snap = term.screen_snapshot();
        let text = snap.text();
        assert!(text.contains("line1"));
        assert!(text.contains("line2"));
        assert!(text.contains("line3"));
    }

    #[test]
    fn test_pty_to_terminal_pipeline() {
        use hom_core::CommandSpec;
        use hom_pty::PtyManager;
        use std::io::Read;

        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo PIPELINE_TEST_OUTPUT".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id = mgr.spawn(&spec, 80, 24).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(300));

        let mut reader = mgr.take_reader(id).unwrap();
        let mut buf = [0u8; 4096];
        let n = reader.read(&mut buf).unwrap_or(0);

        let mut term = Vt100Backend::new(80, 24, 100).unwrap();
        term.process(&buf[..n]);

        let snap = term.screen_snapshot();
        let text = snap.text();
        assert!(
            text.contains("PIPELINE_TEST_OUTPUT"),
            "expected 'PIPELINE_TEST_OUTPUT' in terminal snapshot, got: {text}"
        );

        mgr.kill_all();
    }
}
