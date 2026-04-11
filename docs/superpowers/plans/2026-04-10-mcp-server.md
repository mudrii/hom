# MCP Server Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose HOM as an MCP (Model Context Protocol) server so Claude and other MCP clients can spawn panes, send text, run workflows, and read pane output as structured tool calls.

**Architecture:** New crate `crates/hom-mcp/` implements JSON-RPC 2.0 MCP over stdin/stdout. The server runs as a tokio task within the main HOM process, started with `hom --mcp`. It communicates with the TUI app state via `McpRequest` channel with oneshot response handles. No external MCP SDK — implements only the 4 protocol messages needed (initialize, tools/list, tools/call, notifications/cancelled). Six tools exposed: spawn_pane, send_to_pane, run_workflow, list_panes, get_pane_output, kill_pane.

**Tech Stack:** `serde_json` (already in workspace), `tokio` (already in workspace), no new dependencies

---

## File Map

| File | Action | Why |
|------|--------|-----|
| `crates/hom-mcp/Cargo.toml` | Create | New crate definition |
| `crates/hom-mcp/src/lib.rs` | Create | Public API: `McpServer`, `McpRequest`, `McpResponse` |
| `crates/hom-mcp/src/protocol.rs` | Create | JSON-RPC 2.0 types: `RpcRequest`, `RpcResponse`, `RpcError` |
| `crates/hom-mcp/src/server.rs` | Create | `McpServer::run()` — reads stdin, dispatches to handler, writes stdout |
| `crates/hom-mcp/src/tools.rs` | Create | Tool definitions (name, description, input schema) returned by tools/list |
| `crates/hom-mcp/src/handler.rs` | Create | `handle_tool_call()` — dispatches tool name → `McpRequest`, awaits response |
| `Cargo.toml` (workspace root) | Modify | Add `crates/hom-mcp` to workspace members and dev-dep |
| `src/main.rs` | Modify | Add `--mcp` flag; spawn `McpServer` task when flag is set |
| `crates/hom-core/src/types.rs` | Modify | Add `McpRequest` and `McpResponse` types |
| `crates/hom-tui/src/app.rs` | Modify | Add `mcp_rx: Option<mpsc::Receiver<McpRequest>>` to `App`; handle in event loop |

---

## Task 1: Define McpRequest / McpResponse in hom-core

These types live in `hom-core` so both `hom-mcp` and `hom-tui` can depend on them without circular dependencies.

**Files:**
- Modify: `crates/hom-core/src/types.rs`

- [ ] **Step 1: Read types.rs to find the right insertion point**

```bash
grep -n "pub struct\|pub enum" crates/hom-core/src/types.rs | tail -20
```

- [ ] **Step 2: Add McpRequest and McpResponse to types.rs**

Append to `crates/hom-core/src/types.rs`:

```rust
// ── MCP server ────────────────────────────────────────────────────────

use tokio::sync::oneshot;

/// A command sent from the MCP server to the TUI app, with a channel to receive
/// the result. The app processes this in its event loop and sends back a McpResponse.
#[derive(Debug)]
pub struct McpRequest {
    pub command: McpCommand,
    pub reply: oneshot::Sender<McpResponse>,
}

/// The action the MCP server wants the app to perform.
#[derive(Debug)]
pub enum McpCommand {
    SpawnPane { harness: String, model: Option<String> },
    SendToPane { pane_id: String, text: String },
    RunWorkflow { path: String, vars: std::collections::HashMap<String, String> },
    ListPanes,
    GetPaneOutput { pane_id: String, lines: usize },
    KillPane { pane_id: String },
}

/// The result the app sends back to the MCP server.
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
pub enum McpResponse {
    SpawnPane { pane_id: String },
    SendToPane { ok: bool },
    RunWorkflow { workflow_id: String },
    ListPanes { panes: Vec<PaneSummary> },
    GetPaneOutput { lines: Vec<String> },
    KillPane { ok: bool },
    Error { error: String },
}

/// Summary of a single pane returned by list_panes.
#[derive(Debug, serde::Serialize)]
pub struct PaneSummary {
    pub pane_id: String,
    pub harness: String,
    pub status: String,
}
```

- [ ] **Step 3: Run cargo check on hom-core**

```bash
cargo check -p hom-core
```
Expected: `Finished` with zero errors.

- [ ] **Step 4: Commit**

```bash
git add crates/hom-core/src/types.rs
git commit -m "feat(hom-core): add McpRequest, McpResponse, McpCommand types"
```

---

## Task 2: Create the hom-mcp crate — protocol types

**Files:**
- Create: `crates/hom-mcp/Cargo.toml`
- Create: `crates/hom-mcp/src/lib.rs`
- Create: `crates/hom-mcp/src/protocol.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create crates/hom-mcp/Cargo.toml**

```toml
[package]
name = "hom-mcp"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
description = "MCP server for HOM — exposes harness control as Model Context Protocol tools"

[dependencies]
hom-core.workspace = true
tokio.workspace = true
serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
thiserror.workspace = true
```

- [ ] **Step 2: Add hom-mcp to workspace members in root Cargo.toml**

In `Cargo.toml`, find:
```toml
members = [
    "crates/hom-core",
```
Change to:
```toml
members = [
    "crates/hom-core",
    "crates/hom-mcp",
```

Also add the workspace dependency:
```toml
hom-mcp = { path = "crates/hom-mcp" }
```
after the `hom-db` workspace dependency line.

- [ ] **Step 3: Write the failing test for protocol parsing**

Create `crates/hom-mcp/src/protocol.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An incoming JSON-RPC 2.0 request from the MCP client.
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// An outgoing JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl RpcResponse {
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        RpcResponse { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    pub fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        RpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcError { code, message: message.into() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_initialize_request() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#;
        let req: RpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(json!(1)));
    }

    #[test]
    fn parse_tools_call_request() {
        let raw = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_panes","arguments":{}}}"#;
        let req: RpcRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(req.method, "tools/call");
        assert_eq!(req.params["name"], "list_panes");
    }

    #[test]
    fn serialize_ok_response() {
        let resp = RpcResponse::ok(Some(json!(1)), json!({"panes":[]}));
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"result\""));
        assert!(!s.contains("\"error\""));
    }

    #[test]
    fn serialize_error_response() {
        let resp = RpcResponse::err(Some(json!(1)), -32601, "Method not found");
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"error\""));
        assert!(!s.contains("\"result\""));
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

```bash
cargo test -p hom-mcp -- protocol
```
Expected: 4 tests pass.

- [ ] **Step 5: Create lib.rs**

Create `crates/hom-mcp/src/lib.rs`:

```rust
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
```

- [ ] **Step 6: Commit**

```bash
git add crates/hom-mcp/ Cargo.toml
git commit -m "feat(hom-mcp): new crate with JSON-RPC 2.0 protocol types"
```

---

## Task 3: Tool definitions

**Files:**
- Create: `crates/hom-mcp/src/tools.rs`

- [ ] **Step 1: Write the failing test for tool listing**

Create `crates/hom-mcp/src/tools.rs`:

```rust
use serde_json::{json, Value};

/// Returns the list of tools exposed by the HOM MCP server.
/// Each entry matches the MCP tools/list response format.
pub fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "spawn_pane",
                "description": "Spawn a new pane running a harness. Returns the pane_id.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "harness": {
                            "type": "string",
                            "description": "Harness name: claude, codex, gemini, pi, kimi, opencode, copilot",
                            "enum": ["claude", "codex", "gemini", "pi", "kimi", "opencode", "copilot"]
                        },
                        "model": {
                            "type": "string",
                            "description": "Optional model override (e.g. claude-opus-4-5)"
                        }
                    },
                    "required": ["harness"]
                }
            },
            {
                "name": "send_to_pane",
                "description": "Send text to a pane's stdin. Use to issue prompts or commands.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "string", "description": "Pane ID from spawn_pane or list_panes" },
                        "text": { "type": "string", "description": "Text to send (newline appended automatically)" }
                    },
                    "required": ["pane_id", "text"]
                }
            },
            {
                "name": "run_workflow",
                "description": "Execute a YAML workflow file. Returns the workflow_id for tracking.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the workflow YAML file" },
                        "vars": {
                            "type": "object",
                            "description": "Template variables passed to the workflow (key-value pairs)",
                            "additionalProperties": { "type": "string" }
                        }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "list_panes",
                "description": "List all open panes with their IDs, harness names, and status.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "get_pane_output",
                "description": "Read the last N lines of visible output from a pane.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "string", "description": "Pane ID" },
                        "lines": {
                            "type": "integer",
                            "description": "Number of lines to return (default 20, max 200)",
                            "default": 20,
                            "minimum": 1,
                            "maximum": 200
                        }
                    },
                    "required": ["pane_id"]
                }
            },
            {
                "name": "kill_pane",
                "description": "Kill a pane and its harness process.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pane_id": { "type": "string", "description": "Pane ID to kill" }
                    },
                    "required": ["pane_id"]
                }
            }
        ]
    })
}

/// Returns the MCP capabilities advertised during initialize.
pub fn server_capabilities() -> Value {
    json!({
        "tools": { "listChanged": false }
    })
}

/// Returns the server info block sent in the initialize response.
pub fn server_info() -> Value {
    json!({
        "name": "hom",
        "version": env!("CARGO_PKG_VERSION")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_six_tools() {
        let list = tool_list();
        assert_eq!(list["tools"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn all_tools_have_required_fields() {
        let list = tool_list();
        for tool in list["tools"].as_array().unwrap() {
            assert!(tool["name"].is_string(), "tool missing name");
            assert!(tool["description"].is_string(), "tool missing description");
            assert!(tool["inputSchema"].is_object(), "tool missing inputSchema");
        }
    }

    #[test]
    fn spawn_pane_requires_harness() {
        let list = tool_list();
        let spawn = list["tools"].as_array().unwrap()
            .iter().find(|t| t["name"] == "spawn_pane").unwrap();
        let required = spawn["inputSchema"]["required"].as_array().unwrap();
        assert!(required.iter().any(|r| r == "harness"));
    }
}
```

- [ ] **Step 2: Run tools tests**

```bash
cargo test -p hom-mcp -- tools
```
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/hom-mcp/src/tools.rs crates/hom-mcp/src/lib.rs
git commit -m "feat(hom-mcp): tool definitions for all 6 MCP tools"
```

---

## Task 4: Tool call handler

Translates `tools/call` JSON params → `McpCommand` → sends to app → awaits `McpResponse` → serialises to JSON.

**Files:**
- Create: `crates/hom-mcp/src/handler.rs`

- [ ] **Step 1: Create handler.rs with tests first**

Create `crates/hom-mcp/src/handler.rs`:

```rust
use std::collections::HashMap;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use hom_core::types::{McpCommand, McpRequest, McpResponse};

/// Parse the `arguments` field from a tools/call request and dispatch
/// to the app via the McpRequest channel.
///
/// Returns the JSON value to include in the MCP tool result.
pub async fn handle_tool_call(
    tool_name: &str,
    args: &Value,
    tx: &mpsc::Sender<McpRequest>,
) -> Result<Value, String> {
    let command = parse_command(tool_name, args)?;
    let (reply_tx, reply_rx) = oneshot::channel();
    let req = McpRequest { command, reply: reply_tx };
    tx.send(req).await.map_err(|_| "App channel closed".to_string())?;
    let response = reply_rx.await.map_err(|_| "App dropped reply channel".to_string())?;
    Ok(serde_json::to_value(response).unwrap_or(Value::Null))
}

fn parse_command(tool_name: &str, args: &Value) -> Result<McpCommand, String> {
    match tool_name {
        "spawn_pane" => {
            let harness = args["harness"]
                .as_str()
                .ok_or("spawn_pane: 'harness' argument is required and must be a string")?
                .to_string();
            let model = args["model"].as_str().map(|s| s.to_string());
            Ok(McpCommand::SpawnPane { harness, model })
        }
        "send_to_pane" => {
            let pane_id = args["pane_id"]
                .as_str()
                .ok_or("send_to_pane: 'pane_id' required")?
                .to_string();
            let text = args["text"]
                .as_str()
                .ok_or("send_to_pane: 'text' required")?
                .to_string();
            Ok(McpCommand::SendToPane { pane_id, text })
        }
        "run_workflow" => {
            let path = args["path"]
                .as_str()
                .ok_or("run_workflow: 'path' required")?
                .to_string();
            let vars: HashMap<String, String> = args["vars"]
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            Ok(McpCommand::RunWorkflow { path, vars })
        }
        "list_panes" => Ok(McpCommand::ListPanes),
        "get_pane_output" => {
            let pane_id = args["pane_id"]
                .as_str()
                .ok_or("get_pane_output: 'pane_id' required")?
                .to_string();
            let lines = args["lines"].as_u64().unwrap_or(20).min(200) as usize;
            Ok(McpCommand::GetPaneOutput { pane_id, lines })
        }
        "kill_pane" => {
            let pane_id = args["pane_id"]
                .as_str()
                .ok_or("kill_pane: 'pane_id' required")?
                .to_string();
            Ok(McpCommand::KillPane { pane_id })
        }
        unknown => Err(format!("Unknown tool: {unknown}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_spawn_pane_with_model() {
        let args = json!({"harness": "claude", "model": "claude-opus-4-5"});
        let cmd = parse_command("spawn_pane", &args).unwrap();
        assert!(matches!(cmd, McpCommand::SpawnPane { ref harness, ref model }
            if harness == "claude" && model.as_deref() == Some("claude-opus-4-5")));
    }

    #[test]
    fn parse_spawn_pane_without_model() {
        let args = json!({"harness": "codex"});
        let cmd = parse_command("spawn_pane", &args).unwrap();
        assert!(matches!(cmd, McpCommand::SpawnPane { ref model, .. } if model.is_none()));
    }

    #[test]
    fn parse_spawn_pane_missing_harness_returns_error() {
        let args = json!({});
        let err = parse_command("spawn_pane", &args).unwrap_err();
        assert!(err.contains("harness"));
    }

    #[test]
    fn parse_send_to_pane() {
        let args = json!({"pane_id": "p1", "text": "hello world"});
        let cmd = parse_command("send_to_pane", &args).unwrap();
        assert!(matches!(cmd, McpCommand::SendToPane { ref text, .. } if text == "hello world"));
    }

    #[test]
    fn parse_run_workflow_with_vars() {
        let args = json!({"path": "workflows/tdd.yaml", "vars": {"planner": "claude"}});
        let cmd = parse_command("run_workflow", &args).unwrap();
        if let McpCommand::RunWorkflow { path, vars } = cmd {
            assert_eq!(path, "workflows/tdd.yaml");
            assert_eq!(vars.get("planner").map(|s| s.as_str()), Some("claude"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn parse_get_pane_output_default_lines() {
        let args = json!({"pane_id": "p1"});
        let cmd = parse_command("get_pane_output", &args).unwrap();
        assert!(matches!(cmd, McpCommand::GetPaneOutput { lines: 20, .. }));
    }

    #[test]
    fn parse_get_pane_output_caps_at_200() {
        let args = json!({"pane_id": "p1", "lines": 999});
        let cmd = parse_command("get_pane_output", &args).unwrap();
        assert!(matches!(cmd, McpCommand::GetPaneOutput { lines: 200, .. }));
    }

    #[test]
    fn parse_unknown_tool_returns_error() {
        let err = parse_command("fly_to_moon", &json!({})).unwrap_err();
        assert!(err.contains("Unknown tool"));
    }
}
```

- [ ] **Step 2: Run handler tests**

```bash
cargo test -p hom-mcp -- handler
```
Expected: 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/hom-mcp/src/handler.rs
git commit -m "feat(hom-mcp): tool call handler with argument parsing"
```

---

## Task 5: MCP server main loop

Reads JSON-RPC requests from stdin, writes responses to stdout, dispatches tool calls via the handler.

**Files:**
- Create: `crates/hom-mcp/src/server.rs`

- [ ] **Step 1: Create server.rs**

Create `crates/hom-mcp/src/server.rs`:

```rust
use std::io::{BufRead, Write};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};
use hom_core::types::McpRequest;

use crate::{
    handler::handle_tool_call,
    protocol::{RpcRequest, RpcResponse},
    tools::{server_capabilities, server_info, tool_list},
};

/// The MCP server. Reads JSON-RPC 2.0 line-delimited messages from stdin,
/// dispatches to the TUI app via the McpRequest channel, writes responses to stdout.
pub struct McpServer {
    tx: mpsc::Sender<McpRequest>,
}

impl McpServer {
    pub fn new(tx: mpsc::Sender<McpRequest>) -> Self {
        McpServer { tx }
    }

    /// Run the server. Blocks until stdin closes.
    /// Call via `tokio::task::spawn_blocking` or in a dedicated thread.
    pub async fn run(self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());

        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) if l.is_empty() => continue,
                Ok(l) => l,
                Err(e) => { error!("MCP stdin read error: {e}"); break; }
            };

            debug!("MCP ← {line}");

            let response = match serde_json::from_str::<RpcRequest>(&line) {
                Err(e) => RpcResponse::err(None, -32700, format!("Parse error: {e}")),
                Ok(req) => self.dispatch(req).await,
            };

            let json_line = serde_json::to_string(&response).unwrap_or_default();
            debug!("MCP → {json_line}");

            if writeln!(out, "{json_line}").is_err() || out.flush().is_err() {
                error!("MCP stdout write error");
                break;
            }
        }
    }

    async fn dispatch(&self, req: RpcRequest) -> RpcResponse {
        let id = req.id.clone();
        match req.method.as_str() {
            "initialize" => RpcResponse::ok(id, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": server_capabilities(),
                "serverInfo": server_info()
            })),

            "notifications/initialized" => {
                // No response required for notifications
                return RpcResponse::ok(None, json!(null));
            }

            "tools/list" => RpcResponse::ok(id, tool_list()),

            "tools/call" => {
                let tool_name = match req.params["name"].as_str() {
                    Some(n) => n,
                    None => return RpcResponse::err(id, -32602, "tools/call: 'name' is required"),
                };
                let args = &req.params["arguments"];
                match handle_tool_call(tool_name, args, &self.tx).await {
                    Ok(result) => RpcResponse::ok(id, json!({
                        "content": [{ "type": "text", "text": result.to_string() }],
                        "isError": false
                    })),
                    Err(e) => RpcResponse::err(id, -32000, e),
                }
            }

            "notifications/cancelled" => {
                warn!("MCP: client cancelled request");
                return RpcResponse::ok(None, json!(null));
            }

            unknown => {
                warn!("MCP: unknown method {unknown}");
                RpcResponse::err(id, -32601, format!("Method not found: {unknown}"))
            }
        }
    }
}
```

- [ ] **Step 2: Run cargo check on hom-mcp**

```bash
cargo check -p hom-mcp
```
Expected: zero errors.

- [ ] **Step 3: Commit**

```bash
git add crates/hom-mcp/src/server.rs crates/hom-mcp/src/lib.rs
git commit -m "feat(hom-mcp): MCP server main loop — JSON-RPC 2.0 over stdin/stdout"
```

---

## Task 6: Wire MCP server into the App and main.rs

**Files:**
- Modify: `crates/hom-tui/src/app.rs` — add `mcp_rx` field, handle McpCommands in event loop
- Modify: `src/main.rs` — add `--mcp` flag, create channel, spawn McpServer task
- Modify: `Cargo.toml` (root) — add `hom-mcp.workspace = true` to binary deps

- [ ] **Step 1: Read app.rs to find App struct and event loop**

```bash
grep -n "pub struct App\|mcp\|McpRequest\|fn run\|fn tick" crates/hom-tui/src/app.rs | head -30
```

- [ ] **Step 2: Add mcp_rx to App struct and handler in app.rs**

In `crates/hom-tui/src/app.rs`, add the import at the top:
```rust
use hom_core::types::{McpCommand, McpRequest, McpResponse, PaneSummary};
use tokio::sync::mpsc;
```

Add `mcp_rx` field to `App`:
```rust
/// Receives MCP requests from the McpServer task. None when not in MCP mode.
pub mcp_rx: Option<mpsc::Receiver<McpRequest>>,
```

In `App::new()` (or wherever App is constructed), add:
```rust
mcp_rx: None,
```

Add a method to handle pending MCP requests (call this once per tick in the event loop, before rendering):
```rust
/// Process up to 16 pending MCP requests per tick to avoid starving the render loop.
pub fn handle_mcp_requests(&mut self) {
    let Some(rx) = self.mcp_rx.as_mut() else { return };
    for _ in 0..16 {
        match rx.try_recv() {
            Ok(McpRequest { command, reply }) => {
                let response = self.execute_mcp_command(command);
                let _ = reply.send(response);
            }
            Err(_) => break,
        }
    }
}

fn execute_mcp_command(&mut self, command: McpCommand) -> McpResponse {
    match command {
        McpCommand::ListPanes => {
            let panes = self.panes.iter().map(|(id, pane)| PaneSummary {
                pane_id: id.to_string(),
                harness: pane.harness_type.to_string(),
                status: if pane.exited { "exited".into() } else { "running".into() },
            }).collect();
            McpResponse::ListPanes { panes }
        }
        McpCommand::SpawnPane { harness, model } => {
            let cmd_str = if let Some(m) = model {
                format!("spawn {} --model {}", harness, m)
            } else {
                format!("spawn {}", harness)
            };
            match self.handle_command(&cmd_str) {
                Ok(_) => {
                    // Return the most recently added pane ID
                    let pane_id = self.panes.keys().last()
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "unknown".into());
                    McpResponse::SpawnPane { pane_id }
                }
                Err(e) => McpResponse::Error { error: e.to_string() },
            }
        }
        McpCommand::SendToPane { pane_id, text } => {
            let cmd_str = format!("send {} {}", pane_id, text);
            match self.handle_command(&cmd_str) {
                Ok(_) => McpResponse::SendToPane { ok: true },
                Err(e) => McpResponse::Error { error: e.to_string() },
            }
        }
        McpCommand::RunWorkflow { path, vars } => {
            let vars_str: String = vars.iter()
                .map(|(k, v)| format!(" --var {}={}", k, v))
                .collect();
            let cmd_str = format!("run {}{}", path, vars_str);
            match self.handle_command(&cmd_str) {
                Ok(_) => McpResponse::RunWorkflow { workflow_id: uuid::Uuid::new_v4().to_string() },
                Err(e) => McpResponse::Error { error: e.to_string() },
            }
        }
        McpCommand::GetPaneOutput { pane_id, lines } => {
            let output_lines = self.panes.iter()
                .find(|(id, _)| id.to_string() == pane_id)
                .map(|(_, pane)| pane.terminal.screen_snapshot().last_n_lines(lines))
                .unwrap_or_default();
            McpResponse::GetPaneOutput { lines: output_lines }
        }
        McpCommand::KillPane { pane_id } => {
            let cmd_str = format!("kill {}", pane_id);
            match self.handle_command(&cmd_str) {
                Ok(_) => McpResponse::KillPane { ok: true },
                Err(e) => McpResponse::Error { error: e.to_string() },
            }
        }
    }
}
```

Then call `self.handle_mcp_requests()` in the tick handler (wherever the event loop calls per-tick logic).

- [ ] **Step 3: Read main.rs to find the CLI args and startup**

```bash
grep -n "clap\|Cli\|--mcp\|App::new\|tokio::spawn" src/main.rs | head -30
```

- [ ] **Step 4: Add --mcp flag and McpServer task to main.rs**

In `src/main.rs`, add to Cargo.toml binary deps first:
```toml
hom-mcp.workspace = true
```

In `src/main.rs` at the top:
```rust
use hom_mcp::McpServer;
use tokio::sync::mpsc;
```

Add `--mcp` to the Clap CLI struct:
```rust
/// Run as an MCP server (JSON-RPC 2.0 over stdin/stdout).
/// The TUI renders normally; MCP tool calls are dispatched to it via channel.
#[arg(long)]
pub mcp: bool,
```

After `App::new(...)`, add:
```rust
if cli.mcp {
    let (mcp_tx, mcp_rx) = mpsc::channel(64);
    app.mcp_rx = Some(mcp_rx);
    tokio::spawn(async move {
        McpServer::new(mcp_tx).run().await;
    });
    tracing::info!("MCP server started (JSON-RPC 2.0 on stdin/stdout)");
}
```

- [ ] **Step 5: Run cargo check**

```bash
cargo check --workspace --no-default-features --features vt100-backend
```
Expected: zero errors.

- [ ] **Step 6: Run all tests**

```bash
cargo nextest run --workspace --no-default-features --features vt100-backend
```
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/hom-tui/src/app.rs src/main.rs Cargo.toml
git commit -m "feat: wire MCP server into App — --mcp flag spawns McpServer task"
```

---

## Task 7: Update CLAUDE.md

- [ ] **Step 1: Add MCP Server to Implementation Status in CLAUDE.md**

In the `**No remaining stubs**` section, add before it:

```markdown
**Resolved (April 10, 2026 — MCP Server):**
- hom-mcp crate — JSON-RPC 2.0 MCP server over stdin/stdout
- Six tools: spawn_pane, send_to_pane, run_workflow, list_panes, get_pane_output, kill_pane
- `--mcp` flag spawns McpServer as a tokio task alongside the TUI
- McpRequest/McpResponse types in hom-core; channel-based IPC with App
```

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: record MCP server implementation in CLAUDE.md"
```

---

## Self-review

**Spec coverage:** MCP server with 6 tools, started with `--mcp`, communicates via channel. ✅

**Placeholder scan:** All type names and method signatures are consistent across tasks. `McpCommand`, `McpRequest`, `McpResponse` defined in Task 1 and used consistently. `handle_tool_call` in Task 4 matches the signature used in Task 5.

**Type consistency:** `McpResponse::ListPanes { panes: Vec<PaneSummary> }` defined in Task 1, serialised via `serde_json::to_value` in Task 5's `handle_tool_call`. `PaneSummary` derives `Serialize` in Task 1 — consistent.
