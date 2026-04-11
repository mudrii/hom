use hom_core::types::McpRequest;
use serde_json::json;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
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

const MAX_LINE_BYTES: usize = 1024 * 1024;

impl McpServer {
    pub fn new(tx: mpsc::Sender<McpRequest>) -> Self {
        McpServer { tx }
    }

    /// Run the server. Blocks until stdin closes.
    /// Spawned via `tokio::spawn` — uses async stdin/stdout to stay `Send`.
    pub async fn run(self) {
        self.run_until_shutdown(std::future::pending()).await;
    }

    /// Run the server until stdin closes or a shutdown signal arrives.
    pub async fn run_until_shutdown<F>(self, shutdown: F)
    where
        F: std::future::Future<Output = ()> + Send,
    {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        self.run_transport(BufReader::new(stdin), &mut stdout, shutdown)
            .await;
    }

    async fn run_transport<R, W, F>(self, reader: BufReader<R>, stdout: &mut W, shutdown: F)
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
        F: std::future::Future<Output = ()> + Send,
    {
        let mut reader = reader;
        tokio::pin!(shutdown);

        loop {
            let line = match tokio::select! {
                _ = &mut shutdown => break,
                line = read_capped_line(&mut reader) => line,
            } {
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

            let write_result = tokio::select! {
                _ = &mut shutdown => break,
                result = async {
                stdout.write_all(json_line.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await
                } => result,
            };

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

async fn read_capped_line<R>(reader: &mut R) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }

        let remaining = MAX_LINE_BYTES.saturating_sub(line.len());
        let inspect_len = available.len().min(remaining.saturating_add(1));

        if let Some(pos) = available[..inspect_len].iter().position(|b| *b == b'\n') {
            line.extend_from_slice(&available[..pos]);
            reader.consume(pos + 1);
            break;
        }

        if available.len() > remaining {
            reader.consume(inspect_len);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("MCP input line exceeds {MAX_LINE_BYTES} bytes"),
            ));
        }

        line.extend_from_slice(available);
        let consumed = available.len();
        reader.consume(consumed);
    }

    if matches!(line.last(), Some(b'\r')) {
        line.pop();
    }

    String::from_utf8(line)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
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

    #[tokio::test]
    async fn shutdown_signal_stops_transport_loop() {
        let (tx, _rx) = mpsc::channel(1);
        let server = McpServer::new(tx);
        let (client, server_side) = tokio::io::duplex(64);
        let (_read_half, mut write_half) = tokio::io::split(client);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            server
                .run_transport(BufReader::new(server_side), &mut write_half, async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        shutdown_tx.send(()).unwrap();

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn oversized_line_terminates_transport_without_response() {
        let (tx, _rx) = mpsc::channel(1);
        let server = McpServer::new(tx);
        let (mut client_in, server_in) = tokio::io::duplex(MAX_LINE_BYTES + 1024);
        let (mut server_out, mut client_out) = tokio::io::duplex(256);

        let task = tokio::spawn(async move {
            server
                .run_transport(
                    BufReader::new(server_in),
                    &mut server_out,
                    std::future::pending(),
                )
                .await;
        });

        let oversized = vec![b'a'; MAX_LINE_BYTES + 1];
        client_in.write_all(&oversized).await.unwrap();
        client_in.write_all(b"\n").await.unwrap();
        drop(client_in);

        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();

        let mut output = Vec::new();
        client_out.read_to_end(&mut output).await.unwrap();
        assert!(output.is_empty());
    }
}
