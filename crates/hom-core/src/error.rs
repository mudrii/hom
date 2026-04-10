use thiserror::Error;

use crate::types::{HarnessType, PaneId};

#[derive(Error, Debug)]
pub enum HomError {
    // ── Pane errors ──────────────────────────────────────────────────
    #[error("pane {0} not found")]
    PaneNotFound(PaneId),

    #[error("maximum pane count ({0}) reached")]
    MaxPanesReached(usize),

    #[error("pane {0} is not responding")]
    PaneUnresponsive(PaneId),

    // ── Harness errors ───────────────────────────────────────────────
    #[error("unsupported harness type: {0:?}")]
    UnsupportedHarness(HarnessType),

    #[error("harness binary not found: {binary}")]
    HarnessBinaryNotFound { binary: String },

    #[error("harness spawn failed for {harness:?}: {reason}")]
    HarnessSpawnFailed {
        harness: HarnessType,
        reason: String,
    },

    #[error("harness adapter error: {0}")]
    AdapterError(String),

    // ── Workflow errors ──────────────────────────────────────────────
    #[error("workflow not found: {0}")]
    WorkflowNotFound(String),

    #[error("workflow parse error: {0}")]
    WorkflowParseError(String),

    #[error("workflow step {step} failed: {reason}")]
    WorkflowStepFailed { step: String, reason: String },

    #[error("workflow cycle detected in DAG")]
    WorkflowCycleDetected,

    #[error("workflow variable not set: {0}")]
    WorkflowVariableMissing(String),

    #[error("workflow timeout after {0}s")]
    WorkflowTimeout(u64),

    // ── Terminal errors ──────────────────────────────────────────────
    #[error("terminal emulation error: {0}")]
    TerminalError(String),

    #[error("PTY error: {0}")]
    PtyError(String),

    // ── Config errors ────────────────────────────────────────────────
    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("config file not found at: {0}")]
    ConfigNotFound(String),

    // ── Storage errors ───────────────────────────────────────────────
    #[error("database error: {0}")]
    DatabaseError(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    // ── Plugin errors ────────────────────────────────────────────────
    #[error("plugin error: {0}")]
    PluginError(String),

    // ── IO / general ─────────────────────────────────────────────────
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type HomResult<T> = Result<T, HomError>;
