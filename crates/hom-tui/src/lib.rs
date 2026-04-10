//! # hom-tui
//!
//! TUI rendering, input routing, pane layout, and command bar for HOM.
//!
//! This crate ties together all the lower-level crates (`hom-core`,
//! `hom-terminal`, `hom-pty`, `hom-adapters`) into a working
//! terminal user interface powered by ratatui + crossterm.

pub mod app;
pub mod command_bar;
pub mod db_checkpoint;
pub mod input;
pub mod layout;
pub mod pane_render;
pub mod render;
pub mod status_rail;
pub mod workflow_bridge;

pub use app::App;
