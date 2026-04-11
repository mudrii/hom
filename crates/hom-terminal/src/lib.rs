//! # hom-terminal
//!
//! Terminal emulation layer for HOM. Provides a `TerminalBackend` implementation
//! that can be swapped between ghostty (default) and vt100 (opt-in fallback).
//!
//! ## Feature flags
//!
//! - `ghostty-backend` (default): Use `libghostty-vt 0.1.1`. Requires Zig ≥0.15.x
//!   at build time (the Zig toolchain compiles Ghostty's C VT library).
//! - `vt100-backend` (opt-in fallback): Use the `vt100` crate. No external build deps.
//!   Enable with: `cargo build --no-default-features --features vt100-backend`

pub mod color_map;

use hom_core::HomResult;
use hom_core::TerminalBackend;

#[cfg(feature = "ghostty-backend")]
pub mod ghostty;

#[cfg(feature = "vt100-backend")]
pub mod fallback_vt100;

// ── Re-export the active backend as a type alias ─────────────────────

#[cfg(feature = "ghostty-backend")]
pub type ActiveBackend = ghostty::GhosttyBackend;

#[cfg(all(feature = "vt100-backend", not(feature = "ghostty-backend")))]
pub type ActiveBackend = fallback_vt100::Vt100Backend;

// Fail at compile time if neither backend is enabled
#[cfg(not(any(feature = "vt100-backend", feature = "ghostty-backend")))]
compile_error!(
    "hom-terminal requires at least one backend feature: `ghostty-backend` (default) or `vt100-backend` (opt-in fallback: --no-default-features --features vt100-backend)"
);

/// Convenience constructor: create a terminal with the active backend.
#[cfg(any(feature = "vt100-backend", feature = "ghostty-backend"))]
pub fn create_terminal(cols: u16, rows: u16, scrollback: usize) -> HomResult<ActiveBackend> {
    #[cfg(feature = "ghostty-backend")]
    {
        ghostty::GhosttyBackend::new(cols, rows, scrollback)
    }

    #[cfg(all(feature = "vt100-backend", not(feature = "ghostty-backend")))]
    {
        fallback_vt100::Vt100Backend::new(cols, rows, scrollback)
    }
}
