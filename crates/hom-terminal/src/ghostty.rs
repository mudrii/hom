//! libghostty-rs backed terminal emulator.
//!
//! This module is gated behind the `ghostty-backend` feature flag.
//! Requires Zig ≥0.15.x installed at build time.
//!
//! **Status**: Stubbed out — all trait methods return placeholder values.
//! The `libghostty-vt` crate dependency is currently commented out in
//! `Cargo.toml`. To enable, uncomment the dependency, pin to a specific
//! commit, then build with:
//!   `cargo build --features ghostty-backend`
//!
//! ## Wiring Steps (when libghostty-vt is published)
//!
//! 1. Uncomment `libghostty-vt` in workspace `Cargo.toml` with pinned rev
//! 2. Uncomment `ghostty-backend = ["dep:libghostty-vt"]` in hom-terminal's `Cargo.toml`
//! 3. Replace each TODO with real libghostty_vt calls:
//!    - `new()`: `libghostty_vt::Terminal::new(cols, rows, scrollback)`
//!    - `process()`: `self.terminal.process(bytes)`
//!    - `resize()`: `self.terminal.resize(cols, rows)`
//!    - `screen_snapshot()`: iterate `self.terminal.screen()` rows/cells → map to `Cell`
//!    - `cursor()`: read `self.terminal.cursor_pos()` → `CursorState`
//!    - `title()`: read `self.terminal.title()` if available
//! 4. Add color mapping in `color_map.rs` for ghostty's color representation
//! 5. Run: `cargo test --features ghostty-backend -p hom-terminal`

#[cfg(feature = "ghostty-backend")]
use hom_core::traits::{CursorState, ScreenSnapshot, TerminalBackend};

/// GhosttyBackend wraps `libghostty_vt::Terminal` and implements `TerminalBackend`.
///
/// This provides the highest-quality terminal emulation, matching Ghostty's
/// own rendering capabilities including Kitty graphics protocol, sixels,
/// and full VT520 compliance.
#[cfg(feature = "ghostty-backend")]
pub struct GhosttyBackend {
    // terminal: libghostty_vt::Terminal,
    _cols: u16,
    _rows: u16,
    _scrollback: usize,
}

#[cfg(feature = "ghostty-backend")]
impl TerminalBackend for GhosttyBackend {
    fn new(cols: u16, rows: u16, scrollback: usize) -> Self {
        // TODO: Initialize libghostty_vt::Terminal with dimensions + scrollback
        // let terminal = libghostty_vt::Terminal::new(cols as usize, rows as usize);
        Self {
            _cols: cols,
            _rows: rows,
            _scrollback: scrollback,
        }
    }

    fn process(&mut self, _bytes: &[u8]) {
        // TODO: self.terminal.process(bytes)
        // Feed raw PTY output into the ghostty VT state machine.
    }

    fn resize(&mut self, _cols: u16, _rows: u16) {
        // TODO: self.terminal.resize(cols, rows)
    }

    fn screen_snapshot(&self) -> ScreenSnapshot {
        // TODO: Map libghostty_vt screen state → ScreenSnapshot
        // Iterate rows/cols from the ghostty terminal, map each cell's
        // character, fg, bg, and attributes to our Cell type.
        ScreenSnapshot {
            rows: Vec::new(),
            cols: self._cols,
            num_rows: self._rows,
            cursor: self.cursor(),
        }
    }

    fn cursor(&self) -> CursorState {
        // TODO: Read cursor from libghostty_vt
        CursorState::default()
    }

    fn title(&self) -> Option<&str> {
        // TODO: Read window title from libghostty_vt
        None
    }
}
