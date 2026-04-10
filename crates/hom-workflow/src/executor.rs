//! Workflow step executor with retry, timeout, and templating.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use minijinja::Environment;
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use uuid::Uuid;

use hom_core::{HomError, HomResult};

use crate::checkpoint::WorkflowCheckpoint;
use crate::condition::evaluate_condition;
use crate::dag::WorkflowDag;
use crate::parser::{BackoffKind, FailureAction, WorkflowDef};

/// Result of executing a single step.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_id: String,
    pub status: StepStatus,
    pub output: String,
    pub duration: Duration,
    pub attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Completed,
    Failed,
    Skipped,
    TimedOut,
}

/// Result of an entire workflow execution.
#[derive(Debug)]
pub struct WorkflowResult {
    pub workflow_id: String,
    pub name: String,
    pub status: WorkflowStatus,
    pub step_results: HashMap<String, StepResult>,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowStatus {
    Completed,
    Failed { step: String, error: String },
    Aborted,
}

/// Callback trait for the executor to interact with the pane manager.
///
/// The TUI layer implements this to spawn harnesses, send prompts, and
/// collect outputs from panes.
#[async_trait::async_trait]
pub trait WorkflowRuntime: Send + Sync {
    /// Spawn a harness pane for a workflow step.
    async fn spawn_pane(&self, harness: &str, model: Option<&str>) -> HomResult<u32>;

    /// Send a prompt to a pane and wait for completion.
    async fn send_and_wait(
        &self,
        pane_id: u32,
        prompt: &str,
        timeout: Duration,
    ) -> HomResult<String>;

    /// Kill a pane.
    async fn kill_pane(&self, pane_id: u32) -> HomResult<()>;
}

/// Callback trait for persisting workflow checkpoints to durable storage.
///
/// Data for persisting a completed step result.
pub struct StepResultRecord<'a> {
    pub workflow_id: &'a str,
    pub step_id: &'a str,
    pub harness: &'a str,
    pub model: Option<&'a str>,
    pub status: &'a str,
    pub prompt: &'a str,
    pub output: &'a str,
    pub duration_ms: i64,
    pub attempt: i32,
}

/// Implemented by the DB layer. The executor calls this after each
/// successful step so that progress can survive crashes.
#[async_trait::async_trait]
pub trait CheckpointStore: Send + Sync {
    /// Persist a checkpoint's JSON representation.
    async fn save_checkpoint(&self, workflow_id: &str, checkpoint_json: &str) -> HomResult<()>;

    /// Persist a completed step result.
    async fn save_step_result(&self, record: StepResultRecord<'_>) -> HomResult<()>;
}

/// The main workflow executor.
pub struct WorkflowExecutor {
    _id: String,
}

impl WorkflowExecutor {
    pub fn new() -> Self {
        Self {
            _id: Uuid::new_v4().to_string(),
        }
    }

    /// Execute a workflow definition with the given runtime and variables.
    ///
    /// If `checkpoint_store` is provided, step results and checkpoints are
    /// persisted to durable storage after each successful step.
    pub async fn execute(
        &self,
        def: &WorkflowDef,
        runtime: Arc<dyn WorkflowRuntime>,
        variables: HashMap<String, String>,
    ) -> HomResult<WorkflowResult> {
        self.execute_with_store(def, runtime, variables, None).await
    }

    /// Execute with an optional checkpoint store for persistence.
    ///
    /// If `workflow_id` is `Some`, the executor uses that ID (allowing the
    /// caller to match it with an existing DB row). Otherwise a fresh UUID
    /// is generated.
    pub async fn execute_with_store(
        &self,
        def: &WorkflowDef,
        runtime: Arc<dyn WorkflowRuntime>,
        variables: HashMap<String, String>,
        checkpoint_store: Option<&dyn CheckpointStore>,
    ) -> HomResult<WorkflowResult> {
        self.execute_inner(def, runtime, variables, checkpoint_store, None)
            .await
    }

    /// Execute with an optional checkpoint store and caller-provided workflow ID.
    pub async fn execute_with_id(
        &self,
        def: &WorkflowDef,
        runtime: Arc<dyn WorkflowRuntime>,
        variables: HashMap<String, String>,
        checkpoint_store: Option<&dyn CheckpointStore>,
        workflow_id: String,
    ) -> HomResult<WorkflowResult> {
        self.execute_inner(def, runtime, variables, checkpoint_store, Some(workflow_id))
            .await
    }

    async fn execute_inner(
        &self,
        def: &WorkflowDef,
        runtime: Arc<dyn WorkflowRuntime>,
        variables: HashMap<String, String>,
        checkpoint_store: Option<&dyn CheckpointStore>,
        caller_workflow_id: Option<String>,
    ) -> HomResult<WorkflowResult> {
        let start = std::time::Instant::now();
        let workflow_id = caller_workflow_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        // Validate and build DAG
        def.validate()?;
        let dag = WorkflowDag::from_steps(&def.steps)?;

        // Merge provided variables with defaults
        let mut vars = def.variables.clone();
        vars.extend(variables);

        // Step results accumulator
        let mut step_outputs: HashMap<String, String> = HashMap::new();
        let mut step_statuses: HashMap<String, String> = HashMap::new();
        let mut step_results: HashMap<String, StepResult> = HashMap::new();
        let mut completed: Vec<String> = Vec::new();

        info!(workflow_id, name = %def.name, "starting workflow");

        loop {
            let ready = dag.ready_steps(&completed);
            if ready.is_empty() {
                if completed.len() == def.steps.len() {
                    break;
                }
                let remaining: Vec<_> = def
                    .steps
                    .iter()
                    .filter(|s| !completed.contains(&s.id))
                    .collect();
                if remaining.is_empty() {
                    break;
                }
                return Err(HomError::WorkflowStepFailed {
                    step: "unknown".to_string(),
                    reason: "DAG has unreachable steps".to_string(),
                });
            }

            // ── Evaluate conditions synchronously (needs current state) ──
            // Steps with unmet conditions are skipped before spawning.
            let mut runnable_steps = Vec::new();
            for step_id in &ready {
                let step_def = def.steps.iter().find(|s| s.id == *step_id).ok_or_else(|| {
                    HomError::WorkflowStepFailed {
                        step: step_id.clone(),
                        reason: "DAG returned unknown step ID".to_string(),
                    }
                })?;

                if let Some(condition) = &step_def.condition
                    && !evaluate_condition(condition, &step_outputs, &step_statuses)
                {
                    info!(step = %step_id, condition, "condition not met, skipping step");
                    let result = StepResult {
                        step_id: step_id.clone(),
                        status: StepStatus::Skipped,
                        output: String::new(),
                        duration: Duration::ZERO,
                        attempt: 0,
                    };
                    step_results.insert(step_id.clone(), result);
                    step_statuses.insert(step_id.clone(), "skipped".to_string());
                    completed.push(step_id.clone());
                    continue;
                }

                // Pre-render templates while we have mutable access to state
                let template_ctx = build_template_context(&vars, &step_outputs, &step_statuses);

                let rendered_prompt = match render_template(&step_def.prompt, &template_ctx) {
                    Ok(p) => p,
                    Err(e) => {
                        error!(step = %step_id, error = %e, "template render failed");
                        return Err(HomError::WorkflowStepFailed {
                            step: step_id.clone(),
                            reason: format!("template error: {e}"),
                        });
                    }
                };

                let rendered_harness = render_template(&step_def.harness, &template_ctx)
                    .unwrap_or_else(|_| step_def.harness.clone());
                let rendered_model = step_def
                    .model
                    .as_ref()
                    .map(|m| render_template(m, &template_ctx).unwrap_or_else(|_| m.clone()));

                let timeout = step_def
                    .timeout
                    .as_ref()
                    .and_then(|t| WorkflowDef::parse_timeout(t))
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_secs(600));

                let max_attempts = step_def.retry.as_ref().map(|r| r.max_attempts).unwrap_or(1);
                let backoff_kind = step_def
                    .retry
                    .as_ref()
                    .map(|r| r.backoff.clone())
                    .unwrap_or_default();

                let on_failure = step_def.on_failure.clone();

                // Collect fallback step info if needed
                let fallback_info = if let Some(FailureAction::Fallback(fb_id)) = &on_failure {
                    def.steps.iter().find(|s| s.id == *fb_id).map(|fb_def| {
                        let fb_prompt = render_template(&fb_def.prompt, &template_ctx)
                            .unwrap_or_else(|_| fb_def.prompt.clone());
                        let fb_timeout = fb_def
                            .timeout
                            .as_ref()
                            .and_then(|t| WorkflowDef::parse_timeout(t))
                            .map(Duration::from_secs)
                            .unwrap_or(Duration::from_secs(600));
                        (
                            fb_id.clone(),
                            fb_def.harness.clone(),
                            fb_def.model.clone(),
                            fb_prompt,
                            fb_timeout,
                        )
                    })
                } else {
                    None
                };

                runnable_steps.push(RunnableStep {
                    step_id: step_id.clone(),
                    rendered_harness,
                    rendered_model,
                    rendered_prompt,
                    timeout,
                    max_attempts,
                    backoff_kind,
                    on_failure,
                    fallback_info,
                });
            }

            // ── Execute runnable steps concurrently via JoinSet ──────
            let mut join_set: JoinSet<StepOutcome> = JoinSet::new();

            for step in runnable_steps {
                let rt = runtime.clone();
                join_set.spawn(execute_step(rt, step));
            }

            // Collect results
            while let Some(join_result) = join_set.join_next().await {
                let outcome =
                    join_result.map_err(|e| HomError::Other(format!("task join: {e}")))?;

                match outcome {
                    StepOutcome::Completed {
                        step_id,
                        result,
                        output,
                        harness,
                        model,
                        prompt,
                    } => {
                        let duration_ms = result.duration.as_millis() as i64;
                        let attempt = result.attempt as i32;

                        step_statuses.insert(step_id.clone(), "completed".to_string());
                        step_results.insert(step_id.clone(), result);
                        completed.push(step_id.clone());
                        info!(step = %step_id, "step completed");

                        // Checkpoint after each successful step
                        let checkpoint = WorkflowCheckpoint::from_results(
                            &workflow_id,
                            &def.name,
                            &vars,
                            &step_results,
                        );
                        if let Some(store) = checkpoint_store {
                            let checkpoint_json = checkpoint.to_json();
                            if let Err(e) =
                                store.save_checkpoint(&workflow_id, &checkpoint_json).await
                            {
                                warn!(step = %step_id, error = %e, "checkpoint persistence failed");
                            }
                            if let Err(e) = store
                                .save_step_result(StepResultRecord {
                                    workflow_id: &workflow_id,
                                    step_id: &step_id,
                                    harness: &harness,
                                    model: model.as_deref(),
                                    status: "completed",
                                    prompt: &prompt,
                                    output: &output,
                                    duration_ms,
                                    attempt,
                                })
                                .await
                            {
                                warn!(step = %step_id, error = %e, "step result persistence failed");
                            }
                        }

                        step_outputs.insert(step_id, output);
                    }
                    StepOutcome::Failed {
                        step_id,
                        error: err_msg,
                        result,
                        on_failure,
                        fallback_result,
                    } => {
                        step_statuses.insert(step_id.clone(), "failed".to_string());
                        step_results.insert(step_id.clone(), result);

                        // Handle fallback result if present
                        if let Some((fb_id, fb_result, fb_output)) = fallback_result {
                            step_outputs.insert(fb_id.clone(), fb_output);
                            step_statuses.insert(fb_id.clone(), "completed".to_string());
                            step_results.insert(fb_id.clone(), fb_result);
                            completed.push(fb_id);
                        }

                        match on_failure {
                            Some(FailureAction::Skip) => {
                                warn!(step = %step_id, "skipping failed step per on_failure policy");
                                completed.push(step_id);
                            }
                            Some(FailureAction::Abort) | None => {
                                return Ok(WorkflowResult {
                                    workflow_id,
                                    name: def.name.clone(),
                                    status: WorkflowStatus::Failed {
                                        step: step_id,
                                        error: err_msg,
                                    },
                                    step_results,
                                    duration: start.elapsed(),
                                });
                            }
                            Some(FailureAction::Fallback(_)) => {
                                completed.push(step_id);
                            }
                        }
                    }
                }
            }
        }

        Ok(WorkflowResult {
            workflow_id,
            name: def.name.clone(),
            status: WorkflowStatus::Completed,
            step_results,
            duration: start.elapsed(),
        })
    }
}

impl Default for WorkflowExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a nested serde_json::Value template context so minijinja can resolve
/// dot-access like `steps.plan.output` correctly.
fn build_template_context(
    vars: &HashMap<String, String>,
    step_outputs: &HashMap<String, String>,
    step_statuses: &HashMap<String, String>,
) -> serde_json::Value {
    let mut ctx = serde_json::Map::new();

    // Add all workflow variables at the top level
    for (k, v) in vars {
        ctx.insert(k.clone(), serde_json::Value::String(v.clone()));
    }

    // Build nested `steps` object: { "plan": { "output": "...", "status": "..." }, ... }
    let mut steps = serde_json::Map::new();
    let all_ids: std::collections::HashSet<&String> =
        step_outputs.keys().chain(step_statuses.keys()).collect();
    for id in all_ids {
        let mut step_obj = serde_json::Map::new();
        if let Some(output) = step_outputs.get(id) {
            step_obj.insert(
                "output".to_string(),
                serde_json::Value::String(output.clone()),
            );
        }
        if let Some(status) = step_statuses.get(id) {
            step_obj.insert(
                "status".to_string(),
                serde_json::Value::String(status.clone()),
            );
        }
        steps.insert(id.clone(), serde_json::Value::Object(step_obj));
    }
    ctx.insert("steps".to_string(), serde_json::Value::Object(steps));

    serde_json::Value::Object(ctx)
}

/// Render a minijinja template with a serde_json::Value context.
fn render_template(template_str: &str, ctx: &serde_json::Value) -> Result<String, String> {
    let mut env = Environment::new();
    env.add_template("prompt", template_str)
        .map_err(|e| format!("add template: {e}"))?;

    let tmpl = env
        .get_template("prompt")
        .map_err(|e| format!("get template: {e}"))?;

    tmpl.render(ctx).map_err(|e| format!("render: {e}"))
}

/// Pre-rendered step data ready for concurrent execution.
struct RunnableStep {
    step_id: String,
    rendered_harness: String,
    rendered_model: Option<String>,
    rendered_prompt: String,
    timeout: Duration,
    max_attempts: u32,
    backoff_kind: BackoffKind,
    on_failure: Option<FailureAction>,
    fallback_info: Option<(String, String, Option<String>, String, Duration)>,
}

/// Outcome of a single step execution (returned from spawned task).
enum StepOutcome {
    Completed {
        step_id: String,
        result: StepResult,
        output: String,
        harness: String,
        model: Option<String>,
        prompt: String,
    },
    Failed {
        step_id: String,
        error: String,
        result: StepResult,
        on_failure: Option<FailureAction>,
        fallback_result: Option<(String, StepResult, String)>,
    },
}

/// Execute a single step with retry logic. Designed to run as a spawned task.
async fn execute_step(runtime: Arc<dyn WorkflowRuntime>, step: RunnableStep) -> StepOutcome {
    let step_start = std::time::Instant::now();
    let mut last_error: Option<String> = None;

    for attempt in 1..=step.max_attempts {
        if attempt > 1 {
            let delay = compute_backoff(&step.backoff_kind, attempt);
            info!(step = %step.step_id, attempt, delay_ms = delay.as_millis(), "retrying step");
            tokio::time::sleep(delay).await;
        }

        info!(step = %step.step_id, harness = %step.rendered_harness, attempt, "executing step");

        let pane_id = match runtime
            .spawn_pane(&step.rendered_harness, step.rendered_model.as_deref())
            .await
        {
            Ok(id) => id,
            Err(e) => {
                last_error = Some(e.to_string());
                continue;
            }
        };

        match runtime
            .send_and_wait(pane_id, &step.rendered_prompt, step.timeout)
            .await
        {
            Ok(output) => {
                let result = StepResult {
                    step_id: step.step_id.clone(),
                    status: StepStatus::Completed,
                    output: output.clone(),
                    duration: step_start.elapsed(),
                    attempt,
                };
                let _ = runtime.kill_pane(pane_id).await;
                return StepOutcome::Completed {
                    step_id: step.step_id,
                    result,
                    output,
                    harness: step.rendered_harness,
                    model: step.rendered_model,
                    prompt: step.rendered_prompt,
                };
            }
            Err(e) => {
                warn!(step = %step.step_id, attempt, error = %e, "step attempt failed");
                last_error = Some(e.to_string());
                let _ = runtime.kill_pane(pane_id).await;
            }
        }
    }

    // All retries exhausted
    let err_msg = last_error.unwrap_or_else(|| "unknown error".to_string());
    error!(step = %step.step_id, "step failed after {} attempt(s)", step.max_attempts);

    let result = StepResult {
        step_id: step.step_id.clone(),
        status: StepStatus::Failed,
        output: String::new(),
        duration: step_start.elapsed(),
        attempt: step.max_attempts,
    };

    // Execute fallback if configured
    let fallback_result =
        if let Some((fb_id, fb_harness, fb_model, fb_prompt, fb_timeout)) = step.fallback_info {
            warn!(step = %step.step_id, fallback = %fb_id, "executing fallback step");
            match runtime.spawn_pane(&fb_harness, fb_model.as_deref()).await {
                Ok(fb_pane) => match runtime.send_and_wait(fb_pane, &fb_prompt, fb_timeout).await {
                    Ok(fb_output) => {
                        let fb_result = StepResult {
                            step_id: fb_id.clone(),
                            status: StepStatus::Completed,
                            output: fb_output.clone(),
                            duration: step_start.elapsed(),
                            attempt: 1,
                        };
                        let _ = runtime.kill_pane(fb_pane).await;
                        Some((fb_id, fb_result, fb_output))
                    }
                    Err(fb_err) => {
                        warn!(fallback = %fb_id, error = %fb_err, "fallback also failed");
                        let _ = runtime.kill_pane(fb_pane).await;
                        None
                    }
                },
                Err(e) => {
                    warn!(fallback = %fb_id, error = %e, "fallback spawn failed");
                    None
                }
            }
        } else {
            None
        };

    StepOutcome::Failed {
        step_id: step.step_id,
        error: err_msg,
        result,
        on_failure: step.on_failure,
        fallback_result,
    }
}

/// Compute backoff delay for a retry attempt.
fn compute_backoff(kind: &BackoffKind, attempt: u32) -> Duration {
    match kind {
        BackoffKind::Exponential => {
            let secs = (1u64 << (attempt - 1).min(4)).min(30);
            Duration::from_secs(secs)
        }
        BackoffKind::Linear => Duration::from_secs((attempt as u64) * 2),
        BackoffKind::Fixed => Duration::from_secs(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::parser::WorkflowDef;

    /// A mock runtime that records calls and tracks concurrency.
    struct MockRuntime {
        next_pane_id: AtomicU32,
        max_concurrent: AtomicU32,
        current_concurrent: AtomicU32,
        execution_order: Mutex<Vec<String>>,
        step_delay: Duration,
    }

    impl MockRuntime {
        fn new(step_delay: Duration) -> Self {
            Self {
                next_pane_id: AtomicU32::new(1),
                max_concurrent: AtomicU32::new(0),
                current_concurrent: AtomicU32::new(0),
                execution_order: Mutex::new(Vec::new()),
                step_delay,
            }
        }

        fn max_concurrent(&self) -> u32 {
            self.max_concurrent.load(Ordering::SeqCst)
        }

        fn execution_order(&self) -> Vec<String> {
            self.execution_order.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl WorkflowRuntime for MockRuntime {
        async fn spawn_pane(&self, _harness: &str, _model: Option<&str>) -> HomResult<u32> {
            Ok(self.next_pane_id.fetch_add(1, Ordering::SeqCst))
        }

        async fn send_and_wait(
            &self,
            _pane_id: u32,
            prompt: &str,
            _timeout: Duration,
        ) -> HomResult<String> {
            let current = self.current_concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            // Update max concurrent high-water mark
            self.max_concurrent.fetch_max(current, Ordering::SeqCst);

            self.execution_order
                .lock()
                .unwrap()
                .push(prompt.to_string());

            // Simulate work
            tokio::time::sleep(self.step_delay).await;

            self.current_concurrent.fetch_sub(1, Ordering::SeqCst);
            Ok(format!("output for: {prompt}"))
        }

        async fn kill_pane(&self, _pane_id: u32) -> HomResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_independent_steps_run_concurrently() {
        let yaml = r#"
name: parallel-test
steps:
  - id: a
    harness: claude
    prompt: "step-a"
  - id: b
    harness: claude
    prompt: "step-b"
  - id: c
    harness: claude
    prompt: "step-c"
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        // Steps have no deps → all should run in the same batch concurrently
        let runtime = Arc::new(MockRuntime::new(Duration::from_millis(50)));
        let executor = WorkflowExecutor::new();

        let result = executor
            .execute(&def, runtime.clone(), HashMap::new())
            .await
            .unwrap();

        assert_eq!(result.status, WorkflowStatus::Completed);
        assert_eq!(result.step_results.len(), 3);
        // With 50ms delay per step and concurrent execution, max concurrent should be 3
        assert_eq!(runtime.max_concurrent(), 3);
    }

    #[tokio::test]
    async fn test_dependent_steps_run_sequentially() {
        let yaml = r#"
name: sequential-test
steps:
  - id: first
    harness: claude
    prompt: "step-first"
  - id: second
    harness: claude
    prompt: "step-second"
    depends_on: [first]
  - id: third
    harness: claude
    prompt: "step-third"
    depends_on: [second]
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        let runtime = Arc::new(MockRuntime::new(Duration::from_millis(10)));
        let executor = WorkflowExecutor::new();

        let result = executor
            .execute(&def, runtime.clone(), HashMap::new())
            .await
            .unwrap();

        assert_eq!(result.status, WorkflowStatus::Completed);
        // Each batch has exactly 1 step → max concurrent = 1
        assert_eq!(runtime.max_concurrent(), 1);

        let order = runtime.execution_order();
        assert_eq!(order, vec!["step-first", "step-second", "step-third"]);
    }

    #[tokio::test]
    async fn test_mixed_parallel_and_sequential() {
        // a, b run in parallel; c depends on both
        let yaml = r#"
name: mixed-test
steps:
  - id: a
    harness: claude
    prompt: "step-a"
  - id: b
    harness: claude
    prompt: "step-b"
  - id: c
    harness: claude
    prompt: "step-c"
    depends_on: [a, b]
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        let runtime = Arc::new(MockRuntime::new(Duration::from_millis(50)));
        let executor = WorkflowExecutor::new();

        let result = executor
            .execute(&def, runtime.clone(), HashMap::new())
            .await
            .unwrap();

        assert_eq!(result.status, WorkflowStatus::Completed);
        assert_eq!(result.step_results.len(), 3);
        // a and b should run concurrently (max 2), then c runs alone
        assert!(runtime.max_concurrent() >= 2);
    }

    #[tokio::test]
    async fn test_step_output_available_to_dependents() {
        let yaml = r#"
name: template-test
steps:
  - id: plan
    harness: claude
    prompt: "create plan"
  - id: execute
    harness: claude
    prompt: "implement: {{ steps.plan.output }}"
    depends_on: [plan]
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        let runtime = Arc::new(MockRuntime::new(Duration::from_millis(10)));
        let executor = WorkflowExecutor::new();

        let result = executor
            .execute(&def, runtime.clone(), HashMap::new())
            .await
            .unwrap();

        assert_eq!(result.status, WorkflowStatus::Completed);
        let order = runtime.execution_order();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0], "create plan");
        // The second prompt should have the output from the first step templated in
        assert!(order[1].contains("output for: create plan"));
    }

    #[tokio::test]
    async fn test_condition_skips_step() {
        let yaml = r#"
name: condition-test
steps:
  - id: check
    harness: claude
    prompt: "run check"
  - id: deploy
    harness: claude
    prompt: "deploy"
    depends_on: [check]
    condition: 'steps.check.output contains "APPROVED"'
"#;
        let def = WorkflowDef::from_yaml(yaml).unwrap();
        let runtime = Arc::new(MockRuntime::new(Duration::from_millis(10)));
        let executor = WorkflowExecutor::new();

        let result = executor
            .execute(&def, runtime.clone(), HashMap::new())
            .await
            .unwrap();

        assert_eq!(result.status, WorkflowStatus::Completed);
        // "deploy" should be skipped because mock output doesn't contain "APPROVED"
        assert_eq!(result.step_results["deploy"].status, StepStatus::Skipped);
    }

    #[test]
    fn test_compute_backoff_exponential() {
        assert_eq!(
            compute_backoff(&BackoffKind::Exponential, 1),
            Duration::from_secs(1)
        );
        assert_eq!(
            compute_backoff(&BackoffKind::Exponential, 2),
            Duration::from_secs(2)
        );
        assert_eq!(
            compute_backoff(&BackoffKind::Exponential, 3),
            Duration::from_secs(4)
        );
        assert_eq!(
            compute_backoff(&BackoffKind::Exponential, 5),
            Duration::from_secs(16)
        );
        assert_eq!(
            compute_backoff(&BackoffKind::Exponential, 10),
            Duration::from_secs(16)
        );
    }

    #[test]
    fn test_compute_backoff_linear() {
        assert_eq!(
            compute_backoff(&BackoffKind::Linear, 1),
            Duration::from_secs(2)
        );
        assert_eq!(
            compute_backoff(&BackoffKind::Linear, 3),
            Duration::from_secs(6)
        );
    }

    #[test]
    fn test_compute_backoff_fixed() {
        assert_eq!(
            compute_backoff(&BackoffKind::Fixed, 1),
            Duration::from_secs(2)
        );
        assert_eq!(
            compute_backoff(&BackoffKind::Fixed, 5),
            Duration::from_secs(2)
        );
    }
}
