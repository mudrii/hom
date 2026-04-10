use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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
    /// Spawned via `tokio::spawn` — uses async stdin/stdout to stay `Send`.
    pub async fn run(self) {
        let stdin = tokio::io::stdin();
        let mut lines = BufReader::new(stdin).lines();
        let mut stdout = tokio::io::stdout();

        loop {
            let line = match lines.next_line().await {
                Ok(Some(l)) if l.is_empty() => continue,
                Ok(Some(l)) => l,
                Ok(None) => break,
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

            let json_line = match serde_json::to_string(&response) {
                Ok(s) => s,
                Err(e) => {
                    error!("MCP serialization error: {e}");
                    continue;
                }
            };
            debug!("MCP → {json_line}");

            let write_result = async {
                stdout.write_all(json_line.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await
            }
            .await;

            if let Err(e) = write_result {
                error!("MCP stdout write error: {e}");
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
