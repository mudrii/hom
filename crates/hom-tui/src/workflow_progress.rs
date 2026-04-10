//! Workflow execution progress tracking.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepProgress {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct WorkflowProgress {
    pub name: String,
    pub total_steps: usize,
    pub steps: HashMap<String, StepProgress>,
}

impl WorkflowProgress {
    pub fn new(name: String, step_ids: Vec<String>) -> Self {
        let total_steps = step_ids.len();
        let steps = step_ids
            .into_iter()
            .map(|id| (id, StepProgress::Pending))
            .collect();
        Self {
            name,
            total_steps,
            steps,
        }
    }

    pub fn update_step(&mut self, step_id: &str, status: StepProgress) {
        self.steps.insert(step_id.to_string(), status);
    }

    pub fn completed_count(&self) -> usize {
        self.steps
            .values()
            .filter(|s| matches!(s, StepProgress::Completed))
            .count()
    }

    pub fn is_finished(&self) -> bool {
        self.steps.values().all(|s| {
            matches!(
                s,
                StepProgress::Completed | StepProgress::Failed | StepProgress::Skipped
            )
        })
    }

    pub fn summary(&self) -> String {
        let done = self.completed_count();
        let failed = self
            .steps
            .values()
            .filter(|s| matches!(s, StepProgress::Failed))
            .count();
        if failed > 0 {
            format!(
                "{}: {done}/{} done, {failed} failed",
                self.name, self.total_steps
            )
        } else {
            format!("{}: {done}/{}", self.name, self.total_steps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_workflow_progress() {
        let progress = WorkflowProgress::new(
            "test-wf".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(progress.name, "test-wf");
        assert_eq!(progress.total_steps, 3);
        assert_eq!(progress.steps.len(), 3);
        assert_eq!(progress.steps["a"], StepProgress::Pending);
        assert_eq!(progress.steps["b"], StepProgress::Pending);
        assert_eq!(progress.steps["c"], StepProgress::Pending);
        assert_eq!(progress.completed_count(), 0);
        assert!(!progress.is_finished());
    }

    #[test]
    fn test_update_and_count() {
        let mut progress = WorkflowProgress::new(
            "test-wf".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        progress.update_step("a", StepProgress::Completed);
        assert_eq!(progress.completed_count(), 1);
        assert!(!progress.is_finished());

        progress.update_step("b", StepProgress::Failed);
        progress.update_step("c", StepProgress::Skipped);
        assert_eq!(progress.completed_count(), 1);
        assert!(progress.is_finished());
    }

    #[test]
    fn test_summary_format() {
        let mut progress = WorkflowProgress::new(
            "deploy".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(progress.summary(), "deploy: 0/3");

        progress.update_step("a", StepProgress::Completed);
        assert_eq!(progress.summary(), "deploy: 1/3");

        progress.update_step("b", StepProgress::Failed);
        assert_eq!(progress.summary(), "deploy: 1/3 done, 1 failed");

        progress.update_step("c", StepProgress::Completed);
        assert_eq!(progress.summary(), "deploy: 2/3 done, 1 failed");
    }
}
