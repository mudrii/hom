//! Bridge between the async WorkflowExecutor and the TUI event loop.
//!
//! The executor runs in a spawned tokio task and communicates with the main
//! loop via channels. `WorkflowBridge` implements `WorkflowRuntime` by sending
//! requests through a `tokio::sync::mpsc` channel and awaiting responses on
//! per-request `oneshot` channels.

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use hom_core::{HomError, HomResult, PaneId};
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
