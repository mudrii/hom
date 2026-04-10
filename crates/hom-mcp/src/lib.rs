//! # hom-mcp
//!
//! MCP (Model Context Protocol) server for HOM.
//! Exposes harness control as tool calls over JSON-RPC 2.0 stdin/stdout.
//!
//! Start with `hom --mcp`. The server runs as a tokio task alongside the TUI,
//! communicating via `McpRequest` channels defined in `hom-core::types`.

pub mod protocol;
pub mod server;
pub mod tools;
pub mod handler;

pub use server::McpServer;
