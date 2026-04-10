//! # hom-terminal
//!
//! Terminal emulation layer for HOM. Provides a `TerminalBackend` implementation
//! that can be swapped between vt100 (current default) and libghostty-rs (target).
//!
//! ## Feature flags
//!
//! - `vt100-backend` (default): Use the `vt100` crate. No external build deps.
//! - `ghostty-backend`: Use `libghostty-rs`. Requires Zig ≥0.15.x at build time.
//!   Currently stubbed — the feature flag exists but the crate dependency is
//!   commented out in `Cargo.toml` until libghostty-vt is available.

pub mod color_map;

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
    "hom-terminal requires at least one backend feature: `vt100-backend` (default) or `ghostty-backend`"
);

/// Convenience constructor: create a terminal with the active backend.
#[cfg(any(feature = "vt100-backend", feature = "ghostty-backend"))]
pub fn create_terminal(cols: u16, rows: u16, scrollback: usize) -> ActiveBackend {
    use hom_core::TerminalBackend;
    ActiveBackend::new(cols, rows, scrollback)
}
