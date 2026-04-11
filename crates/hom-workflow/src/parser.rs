//! YAML workflow definition parser.

use std::collections::HashMap;
use std::path::Path;

use serde::de::{self, Deserializer, MapAccess, Visitor};
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
#[derive(Debug, Clone, Serialize)]
pub enum FailureAction {
    /// Stop the entire workflow.
    Abort,
    /// Skip this step and continue.
    Skip,
    /// Run a fallback step.
    Fallback(String),
}

impl<'de> Deserialize<'de> for FailureAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FailureActionVisitor;

        impl<'de> Visitor<'de> for FailureActionVisitor {
            type Value = FailureAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(r#"one of: "abort", "skip", or { fallback: <step-id> }"#)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                match value {
                    "abort" => Ok(FailureAction::Abort),
                    "skip" => Ok(FailureAction::Skip),
                    other => Err(E::unknown_variant(other, &["abort", "skip", "fallback"])),
                }
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let Some(key) = map.next_key::<String>()? else {
                    return Err(de::Error::custom("expected a failure action mapping"));
                };

                let action = match key.as_str() {
                    "fallback" => FailureAction::Fallback(map.next_value::<String>()?),
                    "abort" => {
                        let _: Option<serde::de::IgnoredAny> = map.next_value()?;
                        FailureAction::Abort
                    }
                    "skip" => {
                        let _: Option<serde::de::IgnoredAny> = map.next_value()?;
                        FailureAction::Skip
                    }
                    other => {
                        return Err(de::Error::unknown_field(
                            other,
                            &["abort", "skip", "fallback"],
                        ));
                    }
                };

                if map.next_key::<String>()?.is_some() {
                    return Err(de::Error::custom(
                        "failure action mapping must contain exactly one key",
                    ));
                }

                Ok(action)
            }
        }

        deserializer.deserialize_any(FailureActionVisitor)
    }
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
        // Check for duplicate IDs
        let mut seen = std::collections::HashSet::new();
        for step in &self.steps {
            if !seen.insert(step.id.as_str()) {
                return Err(HomError::WorkflowParseError(format!(
                    "duplicate step ID: {}",
                    step.id
                )));
            }
        }

        crate::dag::WorkflowDag::from_steps(&self.steps)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_failure_action_skip() {
        let yaml = r#"
name: failure-actions
steps:
  - id: a
    harness: claude
    prompt: "hi"
    on_failure: skip
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(matches!(def.steps[0].on_failure, Some(FailureAction::Skip)));
    }

    #[test]
    fn test_parse_failure_action_mapping_fallback() {
        let yaml = r#"
name: failure-actions
steps:
  - id: a
    harness: claude
    prompt: "hi"
    on_failure:
      fallback: recover
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        assert!(matches!(
            def.steps[0].on_failure.as_ref(),
            Some(FailureAction::Fallback(id)) if id == "recover"
        ));
    }

    #[test]
    fn validate_rejects_duplicate_step_ids() {
        let yaml = r#"
name: duplicate-ids
steps:
  - id: shared
    harness: claude
    prompt: "one"
  - id: shared
    harness: claude
    prompt: "two"
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        let err = def.validate().unwrap_err();
        assert!(err.to_string().contains("duplicate step ID: shared"));
    }

    #[test]
    fn parse_timeout_supports_seconds_minutes_and_plain_ints() {
        assert_eq!(WorkflowDef::parse_timeout("15s"), Some(15));
        assert_eq!(WorkflowDef::parse_timeout("2m"), Some(120));
        assert_eq!(WorkflowDef::parse_timeout("45"), Some(45));
        assert_eq!(WorkflowDef::parse_timeout("bogus"), None);
    }
}
