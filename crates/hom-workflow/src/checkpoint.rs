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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;
    use crate::executor::{StepResult, StepStatus};

    #[test]
    fn checkpoint_round_trip_preserves_results_and_variables() {
        let variables = HashMap::from([
            ("task".to_string(), "demo".to_string()),
            ("model".to_string(), "opus".to_string()),
        ]);
        let results = HashMap::from([
            (
                "plan".to_string(),
                StepResult {
                    step_id: "plan".to_string(),
                    status: StepStatus::Completed,
                    output: "ship it".to_string(),
                    duration: Duration::from_secs(3),
                    attempt: 1,
                },
            ),
            (
                "review".to_string(),
                StepResult {
                    step_id: "review".to_string(),
                    status: StepStatus::Failed,
                    output: "needs changes".to_string(),
                    duration: Duration::from_secs(5),
                    attempt: 2,
                },
            ),
        ]);

        let checkpoint = WorkflowCheckpoint::from_results("wf-1", "demo", &variables, &results);
        let json = checkpoint.to_json();
        let restored = WorkflowCheckpoint::from_json(&json).unwrap();

        assert_eq!(restored.workflow_id, "wf-1");
        assert_eq!(restored.workflow_name, "demo");
        assert_eq!(restored.variables, variables);
        assert_eq!(restored.completed_steps.len(), 2);
        assert!(restored.timestamp > 0);

        let restored_plan = restored
            .completed_steps
            .iter()
            .find(|step| step.step_id == "plan")
            .unwrap();
        assert_eq!(restored_plan.status, "Completed");
        assert_eq!(restored_plan.output, "ship it");
        assert_eq!(restored_plan.attempt, 1);
    }

    #[test]
    fn checkpoint_from_json_rejects_invalid_payload() {
        assert!(WorkflowCheckpoint::from_json("{not-json}").is_err());
    }
}
