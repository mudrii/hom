use async_trait::async_trait;

use crate::error::HomResult;
use crate::types::*;

// ═══════════════════════════════════════════════════════════════════════
// Terminal Backend Trait — abstracts libghostty-rs vs fallback (vt100)
// ═══════════════════════════════════════════════════════════════════════

/// A snapshot of the terminal screen at a point in time.
#[derive(Debug, Clone)]
pub struct ScreenSnapshot {
    pub rows: Vec<Vec<Cell>>,
    pub cols: u16,
    pub num_rows: u16,
    pub cursor: CursorState,
}

impl ScreenSnapshot {
    /// Get the last N lines as a single string (for pattern matching).
    pub fn last_n_lines(&self, n: usize) -> String {
        let start = self.rows.len().saturating_sub(n);
        self.rows[start..]
            .iter()
            .map(|row| row.iter().map(|c| c.character).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Get all visible text as a single string.
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|c| c.character)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A single cell on the terminal screen.
#[derive(Debug, Clone)]
pub struct Cell {
    pub character: char,
    pub fg: TermColor,
    pub bg: TermColor,
    pub attrs: CellAttributes,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            character: ' ',
            fg: TermColor::Default,
            bg: TermColor::Default,
            attrs: CellAttributes::default(),
        }
    }
}

/// Cell text attributes.
#[derive(Debug, Clone, Default)]
pub struct CellAttributes {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub strikethrough: bool,
    pub inverse: bool,
    pub blink: bool,
}

/// Terminal cursor state.
#[derive(Debug, Clone)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            visible: true,
        }
    }
}

/// Terminal colors — supports indexed (256), RGB, and named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermColor {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Abstraction over terminal emulation backends.
///
/// Implementations: `GhosttyBackend` (primary), `Vt100Backend` (fallback).
pub trait TerminalBackend: Send + Sync {
    /// Create a new terminal with the given dimensions.
    fn new(cols: u16, rows: u16, scrollback: usize) -> Self
    where
        Self: Sized;

    /// Feed raw bytes from the PTY into the terminal emulator.
    fn process(&mut self, bytes: &[u8]);

    /// Resize the terminal.
    fn resize(&mut self, cols: u16, rows: u16);

    /// Get a snapshot of the current screen state for rendering.
    fn screen_snapshot(&self) -> ScreenSnapshot;

    /// Get the current cursor state.
    fn cursor(&self) -> CursorState;

    /// Get the terminal title (if set by the child process).
    fn title(&self) -> Option<&str>;
}

// ═══════════════════════════════════════════════════════════════════════
// Harness Adapter Trait — one implementation per supported AI harness
// ═══════════════════════════════════════════════════════════════════════

/// An adapter knows how to spawn, drive, and interpret a specific AI harness.
#[async_trait]
pub trait HarnessAdapter: Send + Sync {
    /// Which harness this adapter handles.
    fn harness_type(&self) -> HarnessType;

    /// Human-readable name for display.
    fn display_name(&self) -> &str;

    /// Build the command + arguments to spawn this harness.
    fn build_command(&self, config: &HarnessConfig) -> CommandSpec;

    /// Translate an orchestrator command into raw bytes for the PTY.
    fn translate_input(&self, command: &OrchestratorCommand) -> Vec<u8>;

    /// Parse the terminal screen to extract structured events.
    fn parse_screen(&self, screen: &ScreenSnapshot) -> Vec<HarnessEvent>;

    /// Detect whether the harness has finished its current task.
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus;

    /// Report this harness's capabilities.
    fn capabilities(&self) -> HarnessCapabilities;

    /// Optional sideband channel (HTTP, RPC, etc.).
    fn sideband(&self) -> Option<Box<dyn SidebandChannel>> {
        None
    }
}

/// Specification for spawning a harness process.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub working_dir: std::path::PathBuf,
}

// ═══════════════════════════════════════════════════════════════════════
// Sideband Channel — out-of-band communication with a harness
// ═══════════════════════════════════════════════════════════════════════

/// A secondary communication channel (e.g., OpenCode's HTTP API, pi-mono's RPC).
#[async_trait]
pub trait SidebandChannel: Send + Sync {
    /// Send a prompt via the sideband.
    async fn send_prompt(&self, prompt: &str) -> HomResult<String>;

    /// Poll for events from the sideband.
    async fn get_events(&self) -> HomResult<Vec<HarnessEvent>>;

    /// Check if the sideband is connected/healthy.
    async fn health_check(&self) -> HomResult<bool>;
}
