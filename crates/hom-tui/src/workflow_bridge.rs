//! Bridge between the async WorkflowExecutor and the TUI event loop.
//!
//! The executor runs in a spawned tokio task and communicates with the main
//! loop via channels. `WorkflowBridge` implements `WorkflowRuntime` by sending
//! requests through a `tokio::sync::mpsc` channel and awaiting responses on
//! per-request `oneshot` channels.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use uuid::Uuid;

use hom_core::{HomError, HomResult, PaneId};
use hom_workflow::WorkflowDef;
use hom_workflow::WorkflowRuntime;

/// A command sent from the workflow executor to the TUI event loop.
#[derive(Debug)]
pub enum WorkflowRequest {
    SpawnPane {
        harness: String,
        model: Option<String>,
        reply: oneshot::Sender<HomResult<PaneId>>,
    },
    SendAndWait {
        pane_id: PaneId,
        prompt: String,
        timeout: Duration,
        reply: oneshot::Sender<HomResult<String>>,
    },
    KillPane {
        pane_id: PaneId,
        reply: oneshot::Sender<HomResult<()>>,
    },
    StepUpdate {
        step_id: String,
        status: crate::workflow_progress::StepProgress,
    },
}

/// Handle held by the TUI event loop to receive workflow requests.
pub type WorkflowRequestRx = mpsc::UnboundedReceiver<WorkflowRequest>;

/// Request to start a workflow executor task from the TUI side.
#[derive(Debug)]
pub struct WorkflowLaunchRequest {
    pub workflow_id: String,
    pub definition_path: String,
    pub def: WorkflowDef,
    pub variables: HashMap<String, String>,
}

/// Receiver held by the main loop for workflow launch requests.
pub type WorkflowLaunchRx = mpsc::UnboundedReceiver<WorkflowLaunchRequest>;

/// Handle used by the TUI to queue workflow launches.
#[derive(Clone)]
pub struct WorkflowLauncher {
    tx: mpsc::UnboundedSender<WorkflowLaunchRequest>,
}

impl WorkflowLauncher {
    pub fn new() -> (Self, WorkflowLaunchRx) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    /// Queue a workflow launch and return the assigned workflow ID.
    pub fn launch(
        &self,
        def: WorkflowDef,
        variables: HashMap<String, String>,
        definition_path: String,
    ) -> HomResult<String> {
        let workflow_id = Uuid::new_v4().to_string();
        self.tx
            .send(WorkflowLaunchRequest {
                workflow_id: workflow_id.clone(),
                definition_path,
                def,
                variables,
            })
            .map_err(|_| HomError::Other("workflow launch channel closed".to_string()))?;
        Ok(workflow_id)
    }
}

/// The `WorkflowRuntime` implementor — holds a channel sender to the TUI.
///
/// This is `Send + Sync` so it can be passed into the workflow executor's
/// async task.
pub struct WorkflowBridge {
    tx: mpsc::UnboundedSender<WorkflowRequest>,
}

impl WorkflowBridge {
    /// Create a new bridge, returning the bridge and the request receiver.
    pub fn new() -> (Self, WorkflowRequestRx) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }
}

#[async_trait]
impl WorkflowRuntime for WorkflowBridge {
    async fn spawn_pane(&self, harness: &str, model: Option<&str>) -> HomResult<u32> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(WorkflowRequest::SpawnPane {
                harness: harness.to_string(),
                model: model.map(String::from),
                reply: reply_tx,
            })
            .map_err(|_| HomError::Other("workflow bridge channel closed".to_string()))?;
        debug!(harness, "workflow bridge: spawn_pane request sent");
        reply_rx
            .await
            .map_err(|_| HomError::Other("workflow bridge: reply channel dropped".to_string()))?
    }

    async fn send_and_wait(
        &self,
        pane_id: u32,
        prompt: &str,
        timeout: Duration,
    ) -> HomResult<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(WorkflowRequest::SendAndWait {
                pane_id,
                prompt: prompt.to_string(),
                timeout,
                reply: reply_tx,
            })
            .map_err(|_| HomError::Other("workflow bridge channel closed".to_string()))?;
        debug!(pane_id, "workflow bridge: send_and_wait request sent");
        reply_rx
            .await
            .map_err(|_| HomError::Other("workflow bridge: reply channel dropped".to_string()))?
    }

    async fn kill_pane(&self, pane_id: u32) -> HomResult<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(WorkflowRequest::KillPane {
                pane_id,
                reply: reply_tx,
            })
            .map_err(|_| HomError::Other("workflow bridge channel closed".to_string()))?;
        debug!(pane_id, "workflow bridge: kill_pane request sent");
        reply_rx
            .await
            .map_err(|_| HomError::Other("workflow bridge: reply channel dropped".to_string()))?
    }

    async fn report_step_status(&self, step_id: &str, status: &str) {
        use crate::workflow_progress::StepProgress;
        let progress = match status {
            "running" => StepProgress::Running,
            "completed" => StepProgress::Completed,
            "failed" => StepProgress::Failed,
            "skipped" => StepProgress::Skipped,
            _ => StepProgress::Pending,
        };
        let _ = self.tx.send(WorkflowRequest::StepUpdate {
            step_id: step_id.to_string(),
            status: progress,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_progress::StepProgress;

    #[tokio::test]
    async fn spawn_pane_sends_request_and_returns_reply() {
        let (bridge, mut rx) = WorkflowBridge::new();

        let task = tokio::spawn(async move {
            bridge
                .spawn_pane("claude", Some("opus"))
                .await
                .expect("spawn_pane should succeed")
        });

        let request = rx.recv().await.unwrap();
        match request {
            WorkflowRequest::SpawnPane {
                harness,
                model,
                reply,
            } => {
                assert_eq!(harness, "claude");
                assert_eq!(model.as_deref(), Some("opus"));
                reply.send(Ok(42)).unwrap();
            }
            other => panic!("unexpected request: {other:?}"),
        }

        assert_eq!(task.await.unwrap(), 42);
    }

    #[tokio::test]
    async fn send_and_wait_reports_closed_channel() {
        let (bridge, rx) = WorkflowBridge::new();
        drop(rx);

        let err = bridge
            .send_and_wait(7, "hello", Duration::from_secs(1))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("workflow bridge channel closed"));
    }

    #[tokio::test]
    async fn report_step_status_maps_known_and_unknown_states() {
        let (bridge, mut rx) = WorkflowBridge::new();

        bridge.report_step_status("plan", "completed").await;
        match rx.recv().await.unwrap() {
            WorkflowRequest::StepUpdate { step_id, status } => {
                assert_eq!(step_id, "plan");
                assert!(matches!(status, StepProgress::Completed));
            }
            other => panic!("unexpected request: {other:?}"),
        }

        bridge.report_step_status("review", "mystery").await;
        match rx.recv().await.unwrap() {
            WorkflowRequest::StepUpdate { step_id, status } => {
                assert_eq!(step_id, "review");
                assert!(matches!(status, StepProgress::Pending));
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn workflow_launcher_returns_error_when_receiver_is_closed() {
        let (launcher, rx) = WorkflowLauncher::new();
        drop(rx);

        let err = launcher
            .launch(
                WorkflowDef {
                    name: "demo".to_string(),
                    description: String::new(),
                    variables: HashMap::new(),
                    steps: Vec::new(),
                },
                HashMap::new(),
                "/tmp/demo.yaml".to_string(),
            )
            .unwrap_err()
            .to_string();

        assert!(err.contains("workflow launch channel closed"));
    }
}
