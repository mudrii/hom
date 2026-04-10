//! YAML workflow definition parser.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use hom_core::{HomError, HomResult};

/// A parsed workflow definition from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub variables: HashMap<String, String>,
    pub steps: Vec<StepDef>,
}

/// A single step in a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDef {
    pub id: String,
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub retry: Option<RetryDef>,
    #[serde(default)]
    pub on_failure: Option<FailureAction>,
}

/// Retry policy for a step.
/// Retry policy for a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryDef {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default)]
    pub backoff: BackoffKind,
}

/// Backoff strategy for step retries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackoffKind {
    #[default]
    Exponential,
    Linear,
    Fixed,
}

fn default_max_attempts() -> u32 {
    3
}

/// What to do when a step fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureAction {
    /// Stop the entire workflow.
    Abort,
    /// Skip this step and continue.
    Skip,
    /// Run a fallback step.
    Fallback(String),
}

impl WorkflowDef {
    /// Parse a workflow from a YAML file.
    pub fn from_file(path: &Path) -> HomResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            HomError::WorkflowParseError(format!("cannot read {}: {e}", path.display()))
        })?;
        Self::from_yaml(&content)
    }

    /// Parse a workflow from a YAML string.
    pub fn from_yaml(yaml: &str) -> HomResult<Self> {
        serde_yaml_ng::from_str(yaml)
            .map_err(|e| HomError::WorkflowParseError(format!("invalid YAML: {e}")))
    }

    /// Validate the workflow definition (unique IDs, valid deps, etc.).
    pub fn validate(&self) -> HomResult<()> {
        let step_ids: Vec<&str> = self.steps.iter().map(|s| s.id.as_str()).collect();

        // Check for duplicate IDs
        let mut seen = std::collections::HashSet::new();
        for id in &step_ids {
            if !seen.insert(id) {
                return Err(HomError::WorkflowParseError(format!(
                    "duplicate step ID: {id}"
                )));
            }
        }

        // Check that all depends_on references exist
        for step in &self.steps {
            for dep in &step.depends_on {
                if !step_ids.contains(&dep.as_str()) {
                    return Err(HomError::WorkflowParseError(format!(
                        "step '{}' depends on unknown step '{dep}'",
                        step.id
                    )));
                }
            }
        }

        Ok(())
    }

    /// Parse a timeout string like "300s" or "5m" into seconds.
    pub fn parse_timeout(s: &str) -> Option<u64> {
        let s = s.trim();
        if let Some(secs) = s.strip_suffix('s') {
            secs.parse().ok()
        } else if let Some(mins) = s.strip_suffix('m') {
            mins.parse::<u64>().ok().map(|m| m * 60)
        } else {
            s.parse().ok()
        }
    }
}
