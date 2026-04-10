use std::io::{BufRead, Write};
use serde_json::json;
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
                Err(e) => {
                    error!("MCP stdin read error: {e}");
                    break;
                }
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
            "initialize" => RpcResponse::ok(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": server_capabilities(),
                    "serverInfo": server_info()
                }),
            ),

            "notifications/initialized" => {
                // No response required for notifications
                RpcResponse::ok(None, json!(null))
            }

            "tools/list" => RpcResponse::ok(id, tool_list()),

            "tools/call" => {
                let tool_name = match req.params["name"].as_str() {
                    Some(n) => n,
                    None => {
                        return RpcResponse::err(id, -32602, "tools/call: 'name' is required")
                    }
                };
                let args = &req.params["arguments"];
                match handle_tool_call(tool_name, args, &self.tx).await {
                    Ok(result) => RpcResponse::ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": result.to_string() }],
                            "isError": false
                        }),
                    ),
                    Err(e) => RpcResponse::err(id, -32000, e),
                }
            }

            "notifications/cancelled" => {
                warn!("MCP: client cancelled request");
                RpcResponse::ok(None, json!(null))
            }

            unknown => {
                warn!("MCP: unknown method {unknown}");
                RpcResponse::err(id, -32601, format!("Method not found: {unknown}"))
            }
        }
    }
}
