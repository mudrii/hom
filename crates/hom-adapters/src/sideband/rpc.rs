//! RPC sideband channel — used by pi-mono's stdin/stdout RPC.
//!
//! pi-mono exposes a JSON-RPC interface over stdin/stdout of a second process.
//! This sideband spawns `<program> --rpc` as a child and communicates via
//! JSON-RPC 2.0 messages: write requests to stdin, parse frames from stdout,
//! route responses by request id, and queue notifications as harness events.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, oneshot};
use tracing::{debug, warn};

use hom_core::{HarnessEvent, HomError, HomResult, SidebandChannel};

type PendingSender = oneshot::Sender<HomResult<serde_json::Value>>;
type PendingMap = Arc<Mutex<HashMap<u64, PendingSender>>>;
type EventQueue = Arc<Mutex<VecDeque<HarnessEvent>>>;

#[derive(Clone)]
struct RpcHandles {
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: PendingMap,
    events: EventQueue,
}

struct RpcProcess {
    child: tokio::process::Child,
    handles: RpcHandles,
    reader_task: tokio::task::JoinHandle<()>,
}

impl RpcProcess {
    fn handles(&self) -> RpcHandles {
        self.handles.clone()
    }
}

/// RPC sideband for pi-mono.
///
/// Spawns a JSON-RPC subprocess and communicates via stdin/stdout.
/// A single stdout reader task owns the stream and routes responses by request id.
pub struct RpcSideband {
    program: String,
    process: Mutex<Option<RpcProcess>>,
    next_id: AtomicU64,
}

impl RpcSideband {
    pub fn new(program: String) -> Self {
        Self {
            program,
            process: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    async fn ensure_child(&self) -> HomResult<RpcHandles> {
        let mut process_guard = self.process.lock().await;

        if let Some(process) = process_guard.as_mut() {
            let child_exited = match process.child.try_wait() {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(e) => {
                    warn!(program = %self.program, error = %e, "RPC child status check failed");
                    true
                }
            };

            if !child_exited && !process.reader_task.is_finished() {
                return Ok(process.handles());
            }

            process.reader_task.abort();
            *process_guard = None;
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

        let stdin = Arc::new(Mutex::new(stdin));
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let events: EventQueue = Arc::new(Mutex::new(VecDeque::new()));
        let handles = RpcHandles {
            stdin: stdin.clone(),
            pending: pending.clone(),
            events: events.clone(),
        };

        let reader_task = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            let mut line = String::new();

            loop {
                line.clear();

                match stdout.read_line(&mut line).await {
                    Ok(0) => {
                        fail_pending(
                            &pending,
                            "RPC subprocess closed stdout before replying".to_string(),
                        )
                        .await;
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<serde_json::Value>(trimmed) {
                            Ok(value) => route_frame(value, &pending, &events).await,
                            Err(e) => {
                                warn!(error = %e, line = trimmed, "RPC parse failed");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "RPC stdout read failed");
                        fail_pending(&pending, format!("RPC read failed: {e}")).await;
                        break;
                    }
                }
            }
        });

        *process_guard = Some(RpcProcess {
            child,
            handles: handles.clone(),
            reader_task,
        });

        debug!(program = %self.program, "RPC subprocess spawned");
        Ok(handles)
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn dispatch_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> HomResult<oneshot::Receiver<HomResult<serde_json::Value>>> {
        let handles = self.ensure_child().await?;
        let id = self.next_request_id();

        let mut request = serde_json::Map::new();
        request.insert(
            "jsonrpc".to_string(),
            serde_json::Value::String("2.0".to_string()),
        );
        request.insert(
            "method".to_string(),
            serde_json::Value::String(method.to_string()),
        );
        request.insert("id".to_string(), serde_json::Value::from(id));
        if let Some(params) = params {
            request.insert("params".to_string(), params);
        }

        let mut request_bytes = serde_json::to_vec(&serde_json::Value::Object(request))
            .map_err(|e| HomError::AdapterError(format!("JSON serialize: {e}")))?;
        request_bytes.push(b'\n');

        let (tx, rx) = oneshot::channel();
        handles.pending.lock().await.insert(id, tx);

        {
            let mut stdin = handles.stdin.lock().await;
            if let Err(e) = stdin.write_all(&request_bytes).await {
                handles.pending.lock().await.remove(&id);
                self.reset_process().await;
                return Err(HomError::AdapterError(format!("RPC write: {e}")));
            }
            if let Err(e) = stdin.flush().await {
                handles.pending.lock().await.remove(&id);
                self.reset_process().await;
                return Err(HomError::AdapterError(format!("RPC flush: {e}")));
            }
        }

        Ok(rx)
    }

    async fn reset_process(&self) {
        let mut process_guard = self.process.lock().await;
        if let Some(mut process) = process_guard.take() {
            process.reader_task.abort();
            if let Err(e) = process.child.kill().await {
                warn!(program = %self.program, error = %e, "failed to kill broken RPC subprocess");
            }
        }
    }
}

#[async_trait]
impl SidebandChannel for RpcSideband {
    async fn send_prompt(&self, prompt: &str) -> HomResult<String> {
        let rx = self
            .dispatch_request("prompt", Some(serde_json::json!({ "text": prompt })))
            .await?;
        let response = rx
            .await
            .map_err(|_| HomError::AdapterError("RPC response channel closed".to_string()))??;

        if let Some(error) = response.get("error") {
            return Err(HomError::AdapterError(format!("RPC error: {error}")));
        }

        let result = response
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        debug!("RPC prompt sent and response received");
        Ok(result)
    }

    async fn get_events(&self) -> HomResult<Vec<HarnessEvent>> {
        let events = {
            let process_guard = self.process.lock().await;
            let Some(process) = process_guard.as_ref() else {
                return Ok(Vec::new());
            };

            process.handles.events.clone()
        };

        let mut events = events.lock().await;
        Ok(events.drain(..).collect())
    }

    async fn health_check(&self) -> HomResult<bool> {
        let rx = match self.dispatch_request("ping", None).await {
            Ok(rx) => rx,
            Err(e) => {
                warn!(error = %e, "RPC health check failed");
                return Ok(false);
            }
        };

        match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
            Ok(Ok(Ok(response))) => Ok(response.get("error").is_none()),
            _ => Ok(false),
        }
    }
}

async fn route_frame(frame: serde_json::Value, pending: &PendingMap, events: &EventQueue) {
    if let Some(id) = frame.get("id").and_then(|value| value.as_u64()) {
        let sender = pending.lock().await.remove(&id);
        if let Some(sender) = sender {
            let _ = sender.send(Ok(frame));
        } else {
            debug!(id, "unmatched RPC response");
        }
        return;
    }
    if let Some(event) = notification_to_event(&frame) {
        events.lock().await.push_back(event);
    }
}

fn notification_to_event(frame: &serde_json::Value) -> Option<HarnessEvent> {
    let method = frame.get("method")?.as_str()?;

    match method {
        "task_started" => {
            let description = frame
                .get("params")
                .and_then(|p| p.get("description"))
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            Some(HarnessEvent::TaskStarted { description })
        }
        "task_completed" => {
            let summary = frame
                .get("params")
                .and_then(|p| p.get("summary"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            Some(HarnessEvent::TaskCompleted { summary })
        }
        "error" => {
            let message = frame
                .get("params")
                .and_then(|p| p.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            Some(HarnessEvent::Error { message })
        }
        _ => {
            debug!(method, "unknown RPC notification");
            None
        }
    }
}

async fn fail_pending(pending: &PendingMap, message: String) {
    let mut pending = pending.lock().await;
    let senders: Vec<_> = pending.drain().map(|(_, sender)| sender).collect();
    drop(pending);

    for sender in senders {
        let _ = sender.send(Err(HomError::AdapterError(message.clone())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mock_rpc_program() -> String {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hom-mock-rpc-{unique}.sh"));
        let script = r#"#!/usr/bin/env python3
import json
import sys

for line in sys.stdin:
    req = json.loads(line)
    if req["method"] == "prompt":
        sys.stdout.write(json.dumps({
            "jsonrpc": "2.0",
            "method": "task_started",
            "params": {"description": "working"},
        }) + "\n")
        sys.stdout.write(json.dumps({
            "jsonrpc": "2.0",
            "result": req["params"]["text"].upper(),
            "id": req["id"],
        }) + "\n")
        sys.stdout.flush()
    elif req["method"] == "ping":
        sys.stdout.write(json.dumps({
            "jsonrpc": "2.0",
            "result": "pong",
            "id": req["id"],
        }) + "\n")
        sys.stdout.flush()
"#;

        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();

        path.to_string_lossy().into_owned()
    }

    fn flaky_stdin_rpc_program(counter_path: &std::path::Path) -> String {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hom-flaky-rpc-{unique}.sh"));
        let counter = counter_path.display();
        let script = format!(
            r#"#!/usr/bin/env python3
import json
import os
import pathlib
import sys
import time

counter = pathlib.Path(r"{counter}")
count = int(counter.read_text()) if counter.exists() else 0
count += 1
counter.write_text(str(count))

if count == 1:
    os.close(0)
    time.sleep(0.2)
    sys.exit(0)

for line in sys.stdin:
    req = json.loads(line)
    sys.stdout.write(json.dumps({{
        "jsonrpc": "2.0",
        "result": req["method"],
        "id": req["id"],
    }}) + "\n")
    sys.stdout.flush()
"#
        );

        fs::write(&path, script).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();

        path.to_string_lossy().into_owned()
    }

    #[test]
    fn test_rpc_sideband_new() {
        let rpc = RpcSideband::new("pi".to_string());
        assert_eq!(rpc.program, "pi");
        assert_eq!(rpc.next_request_id(), 1);
        assert_eq!(rpc.next_request_id(), 2);
    }

    #[tokio::test]
    async fn test_send_prompt_routes_response_by_id() {
        let rpc = RpcSideband::new(mock_rpc_program());
        let response = rpc.send_prompt("hello").await.unwrap();
        assert_eq!(response, "HELLO");
    }

    #[tokio::test]
    async fn test_notifications_are_queued_without_stealing_responses() {
        let rpc = RpcSideband::new(mock_rpc_program());
        let response = rpc.send_prompt("hello").await.unwrap();
        assert_eq!(response, "HELLO");

        let events = rpc.get_events().await.unwrap();
        assert!(matches!(
            events.as_slice(),
            [HarnessEvent::TaskStarted { description }] if description == "working"
        ));
    }

    #[tokio::test]
    async fn test_health_check_can_run_concurrently_with_prompt() {
        let rpc = Arc::new(RpcSideband::new(mock_rpc_program()));
        let prompt_rpc = rpc.clone();
        let health_rpc = rpc.clone();

        let (prompt, healthy) = tokio::join!(
            async move { prompt_rpc.send_prompt("hello").await },
            async move { health_rpc.health_check().await }
        );

        assert_eq!(prompt.unwrap(), "HELLO");
        assert!(healthy.unwrap());
    }

    #[tokio::test]
    async fn write_failure_resets_process_and_next_request_spawns_fresh_child() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let counter_path = std::env::temp_dir().join(format!("hom-rpc-count-{unique}.txt"));
        let rpc = RpcSideband::new(flaky_stdin_rpc_program(&counter_path));

        let first = rpc.send_prompt("hello").await;
        assert!(first.is_err());

        let second = rpc.send_prompt("hello").await.unwrap();
        assert_eq!(second, "prompt");

        let launch_count = fs::read_to_string(counter_path).unwrap();
        assert_eq!(launch_count.trim(), "2");
    }
}
