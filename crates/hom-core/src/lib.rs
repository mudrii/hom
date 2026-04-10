//! # hom-core
//!
//! Core types, traits, and configuration for the HOM AI harness orchestrator.
//!
//! This crate defines the shared vocabulary used by all other HOM crates:
//! - **Types**: `HarnessType`, `PaneId`, `CompletionStatus`, `OrchestratorCommand`, etc.
//! - **Traits**: `TerminalBackend`, `HarnessAdapter`, `SidebandChannel`
//! - **Config**: `HomConfig` loaded from TOML
//! - **Errors**: `HomError` and `HomResult`

pub mod config;
pub mod error;
pub mod traits;
pub mod types;

// Re-export the most commonly used items at crate root.
pub use config::{HomConfig, KeybindingsConfig};
pub use error::{HomError, HomResult};
pub use traits::*;
pub use types::*;
