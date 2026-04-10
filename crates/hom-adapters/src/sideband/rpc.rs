//! RPC sideband channel — used by pi-mono's stdin/stdout RPC.
//!
//! pi-mono exposes a JSON-RPC interface over stdin/stdout of a second process.
//! This sideband spawns `<program> --rpc` as a child and communicates via
//! JSON-RPC 2.0 messages: write requests to stdin, read responses from stdout.

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, warn};

use hom_core::{HarnessEvent, HomError, HomResult, SidebandChannel};

/// RPC sideband for pi-mono.
///
/// Spawns a JSON-RPC subprocess and communicates via stdin/stdout.
pub struct RpcSideband {
    program: String,
    child: Mutex<Option<RpcChild>>,
    next_id: AtomicU64,
}

struct RpcChild {
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    _child: Child,
}

impl RpcSideband {
    pub fn new(program: String) -> Self {
        Self {
            program,
            child: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    async fn ensure_child(&self) -> HomResult<()> {
        let mut guard = self.child.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let mut child = Command::new(&self.program)
            .arg("--rpc")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| HomError::AdapterError(format!("RPC spawn failed: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HomError::AdapterError("RPC stdin not available".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HomError::AdapterError("RPC stdout not available".to_string()))?;

        *guard = Some(RpcChild {
            stdin,
            stdout: BufReader::new(stdout),
            _child: child,
        });
        debug!(program = %self.program, "RPC subprocess spawned");
        Ok(())
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

#[async_trait]
impl SidebandChannel for RpcSideband {
    async fn send_prompt(&self, prompt: &str) -> HomResult<String> {
        self.ensure_child().await?;

        let id = self.next_request_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "prompt",
            "params": { "text": prompt },
            "id": id
        });

        let mut guard = self.child.lock().await;
        let rpc = guard
            .as_mut()
            .ok_or_else(|| HomError::AdapterError("RPC child not available".to_string()))?;

        // Write request
        let mut request_bytes = serde_json::to_vec(&request)
            .map_err(|e| HomError::AdapterError(format!("JSON serialize: {e}")))?;
        request_bytes.push(b'\n');
        rpc.stdin
            .write_all(&request_bytes)
            .await
            .map_err(|e| HomError::AdapterError(format!("RPC write: {e}")))?;
        rpc.stdin
            .flush()
            .await
            .map_err(|e| HomError::AdapterError(format!("RPC flush: {e}")))?;

        // Read response line
        let mut line = String::new();
        rpc.stdout
            .read_line(&mut line)
            .await
            .map_err(|e| HomError::AdapterError(format!("RPC read: {e}")))?;

        // Parse JSON-RPC response
        let response: serde_json::Value = serde_json::from_str(line.trim())
            .map_err(|e| HomError::AdapterError(format!("RPC parse: {e}")))?;

        if let Some(error) = response.get("error") {
            return Err(HomError::AdapterError(format!("RPC error: {error}")));
        }

        let result = response
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        debug!(id, "RPC prompt sent and response received");
        Ok(result)
    }

    async fn get_events(&self) -> HomResult<Vec<HarnessEvent>> {
        // Non-blocking check if RPC has any pending notifications
        let guard = self.child.lock().await;
        if guard.is_none() {
            return Ok(Vec::new());
        }
        // Events would come as JSON-RPC notifications (no id field)
        // For now return empty — real implementation would read available lines
        Ok(Vec::new())
    }

    async fn health_check(&self) -> HomResult<bool> {
        if let Err(e) = self.ensure_child().await {
            warn!(error = %e, "RPC health check failed");
            return Ok(false);
        }

        let id = self.next_request_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ping",
            "id": id
        });

        let mut guard = self.child.lock().await;
        let rpc = match guard.as_mut() {
            Some(r) => r,
            None => return Ok(false),
        };

        let mut request_bytes = serde_json::to_vec(&request).unwrap_or_default();
        request_bytes.push(b'\n');

        if rpc.stdin.write_all(&request_bytes).await.is_err() {
            return Ok(false);
        }

        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            rpc.stdout.read_line(&mut line),
        )
        .await
        {
            Ok(Ok(_)) => Ok(!line.is_empty()),
            _ => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_sideband_new() {
        let rpc = RpcSideband::new("pi".to_string());
        assert_eq!(rpc.program, "pi");
        assert_eq!(rpc.next_request_id(), 1);
        assert_eq!(rpc.next_request_id(), 2);
    }
}
