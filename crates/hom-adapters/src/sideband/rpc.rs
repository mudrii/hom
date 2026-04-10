//! RPC sideband channel — used by pi-mono's stdin/stdout RPC.
//!
//! pi-mono exposes a JSON-RPC interface over stdin/stdout of a second process.
//! This sideband spawns `<program> --rpc` as a child and communicates via
//! JSON-RPC 2.0 messages: write requests to stdin, read responses from stdout.
//!
//! Stdin and stdout are behind separate locks so that `get_events()` (reads)
//! does not block `send_prompt()` (writes) and vice versa. The request/response
//! pair in `send_prompt` acquires both locks sequentially — write first, then
//! read — with no lock held across both operations.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use hom_core::{HarnessEvent, HomError, HomResult, SidebandChannel};

/// RPC sideband for pi-mono.
///
/// Spawns a JSON-RPC subprocess and communicates via stdin/stdout.
/// Each I/O handle is behind its own lock to avoid holding a lock across `.await`.
pub struct RpcSideband {
    program: String,
    /// Separate locks for stdin/stdout to avoid lock-across-await.
    stdin: OnceLock<Mutex<tokio::process::ChildStdin>>,
    stdout: OnceLock<Mutex<BufReader<tokio::process::ChildStdout>>>,
    init: Mutex<bool>,
    next_id: AtomicU64,
}

impl RpcSideband {
    pub fn new(program: String) -> Self {
        Self {
            program,
            stdin: OnceLock::new(),
            stdout: OnceLock::new(),
            init: Mutex::new(false),
            next_id: AtomicU64::new(1),
        }
    }

    async fn ensure_child(&self) -> HomResult<()> {
        // Fast path: already initialized
        if self.stdin.get().is_some() {
            return Ok(());
        }

        // Slow path: hold init lock briefly to spawn
        let mut init_guard = self.init.lock().await;
        if *init_guard {
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

        let _ = self.stdin.set(Mutex::new(stdin));
        let _ = self.stdout.set(Mutex::new(BufReader::new(stdout)));
        *init_guard = true;

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

        let mut request_bytes = serde_json::to_vec(&request)
            .map_err(|e| HomError::AdapterError(format!("JSON serialize: {e}")))?;
        request_bytes.push(b'\n');

        // Lock stdin briefly for the write, then drop
        {
            let stdin_lock = self
                .stdin
                .get()
                .ok_or_else(|| HomError::AdapterError("RPC not initialized".to_string()))?;
            let mut stdin = stdin_lock.lock().await;
            stdin
                .write_all(&request_bytes)
                .await
                .map_err(|e| HomError::AdapterError(format!("RPC write: {e}")))?;
            stdin
                .flush()
                .await
                .map_err(|e| HomError::AdapterError(format!("RPC flush: {e}")))?;
        }

        // Lock stdout briefly for the read, then drop
        let line = {
            let stdout_lock = self
                .stdout
                .get()
                .ok_or_else(|| HomError::AdapterError("RPC not initialized".to_string()))?;
            let mut stdout = stdout_lock.lock().await;
            let mut line = String::new();
            stdout
                .read_line(&mut line)
                .await
                .map_err(|e| HomError::AdapterError(format!("RPC read: {e}")))?;
            line
        };

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
        if self.stdin.get().is_none() {
            return Ok(Vec::new());
        }
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

        let mut request_bytes = serde_json::to_vec(&request).unwrap_or_default();
        request_bytes.push(b'\n');

        // Write ping
        {
            let stdin_lock = match self.stdin.get() {
                Some(l) => l,
                None => return Ok(false),
            };
            let mut stdin = stdin_lock.lock().await;
            if stdin.write_all(&request_bytes).await.is_err() {
                return Ok(false);
            }
        }

        // Read pong
        let stdout_lock = match self.stdout.get() {
            Some(l) => l,
            None => return Ok(false),
        };
        let mut stdout = stdout_lock.lock().await;
        let mut line = String::new();
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stdout.read_line(&mut line),
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
