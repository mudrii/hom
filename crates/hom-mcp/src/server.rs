use hom_core::types::McpRequest;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

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
                Err(e) => Some(RpcResponse::err(None, -32700, format!("Parse error: {e}"))),
                Ok(req) => self.dispatch(req).await,
            };

            let Some(response) = response else {
                continue;
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

    async fn dispatch(&self, req: RpcRequest) -> Option<RpcResponse> {
        let id = req.id.clone();
        let is_notification = id.is_none();

        let response = match req.method.as_str() {
            "notifications/initialized" => {
                debug!("MCP client initialized");
                return None;
            }

            "notifications/cancelled" => {
                warn!("MCP: client cancelled request");
                return None;
            }

            "initialize" => RpcResponse::ok(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": server_capabilities(),
                    "serverInfo": server_info()
                }),
            ),

            "tools/list" => RpcResponse::ok(id, tool_list()),

            "tools/call" => {
                let tool_name = match req.params["name"].as_str() {
                    Some(n) => n,
                    None => {
                        if is_notification {
                            return None;
                        }
                        return Some(RpcResponse::err(
                            id,
                            -32602,
                            "tools/call: 'name' is required",
                        ));
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

            unknown => {
                warn!("MCP: unknown method {unknown}");
                RpcResponse::err(id, -32601, format!("Method not found: {unknown}"))
            }
        };

        if is_notification {
            None
        } else {
            Some(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;

    #[tokio::test]
    async fn notification_request_returns_no_response() {
        let (tx, _rx) = mpsc::channel(1);
        let server = McpServer::new(tx);
        let req = RpcRequest {
            jsonrpc: "2.0".into(),
            id: None,
            method: "notifications/initialized".into(),
            params: json!({}),
        };

        assert!(server.dispatch(req).await.is_none());
    }

    #[tokio::test]
    async fn regular_request_returns_response() {
        let (tx, _rx) = mpsc::channel(1);
        let server = McpServer::new(tx);
        let req = RpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: json!({}),
        };

        assert!(server.dispatch(req).await.is_some());
    }
}
