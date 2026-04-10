//! Workflow checkpointing — save/restore step state for crash recovery.
//!
//! Uses SQLite via hom-db (or a simpler file-based approach for MVP).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::executor::StepResult;

/// Serializable checkpoint of a workflow execution in progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    pub workflow_id: String,
    pub workflow_name: String,
    pub variables: HashMap<String, String>,
    pub completed_steps: Vec<CheckpointStep>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointStep {
    pub step_id: String,
    pub status: String,
    pub output: String,
    pub attempt: u32,
}

impl WorkflowCheckpoint {
    /// Create a checkpoint from current execution state.
    pub fn from_results(
        workflow_id: &str,
        workflow_name: &str,
        variables: &HashMap<String, String>,
        results: &HashMap<String, StepResult>,
    ) -> Self {
        Self {
            workflow_id: workflow_id.to_string(),
            workflow_name: workflow_name.to_string(),
            variables: variables.clone(),
            completed_steps: results
                .values()
                .map(|r| CheckpointStep {
                    step_id: r.step_id.clone(),
                    status: format!("{:?}", r.status),
                    output: r.output.clone(),
                    attempt: r.attempt,
                })
                .collect(),
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Serialize to JSON for storage.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
