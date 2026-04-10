//! # hom-pty
//!
//! PTY management for HOM. Spawns child processes in pseudo-terminals,
//! provides read/write/resize operations, and wraps blocking reads in
//! async channels for integration with the tokio event loop.

pub mod async_reader;
pub mod manager;

pub use async_reader::AsyncPtyReader;
pub use manager::PtyManager;
