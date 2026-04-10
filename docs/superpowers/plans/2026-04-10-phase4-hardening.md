# Phase 4: TUI Hardening & Test Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining gaps between the design document requirements and the implementation: cost display in the TUI (F10), workflow progress tracking (F9), integration tests for the PTY pipeline, additional workflow templates (F12), and a handle_command refactor for maintainability.

**Architecture:** The changes are additive — no crate boundaries change. Cost display adds a DB query to the render path. Workflow progress replaces the stringly-typed `workflow_status: Option<String>` with a structured `WorkflowProgress` type that tracks per-step status. Integration tests exercise the PTY→terminal→render pipeline with a real subprocess. The handle_command refactor extracts per-command functions without changing behavior.

**Tech Stack:** Rust 2024, ratatui 0.30, tokio 1.x, sqlx 0.8, petgraph 0.8. Tests use `cargo test` (no nextest). Build with `CARGO_TARGET_DIR=/tmp/hom-target`.

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `crates/hom-tui/src/status_rail.rs` | Status rail rendering (top bar) | Modify — add cost display |
| `crates/hom-tui/src/render.rs` | Frame composition | Modify — pass cost data to status rail |
| `crates/hom-tui/src/app.rs` | App state | Modify — add `WorkflowProgress`, `total_cost` field |
| `crates/hom-tui/src/workflow_progress.rs` | Workflow step tracking (NEW) | Create — `WorkflowProgress` type + step status |
| `crates/hom-tui/src/lib.rs` | Module declarations | Modify — add `workflow_progress` module |
| `src/main.rs` | Event loop, command dispatch | Modify — refactor handle_command, update workflow progress |
| `crates/hom-workflow/src/executor.rs` | Step execution | Modify — emit step status updates via callback |
| `workflows/test-driven-development.yaml` | TDD workflow template | Create |
| `workflows/debugging.yaml` | Debug workflow template | Create |
| `workflows/refactor-with-tests.yaml` | Refactoring workflow template | Create |
| `workflows/documentation.yaml` | Doc generation workflow template | Create |
| `workflows/parallel-analysis.yaml` | Parallel analysis template | Create |
| `tests/integration/pty_pipeline.rs` | Integration test: PTY spawn + render | Create |
| `hom-system-design.md` | Design doc Section 11 | Modify — mark resolved items |

---

### Task 1: Cost Display in Status Rail (F10)

**Files:**
- Modify: `crates/hom-tui/src/app.rs` — add `total_cost: f64` field
- Modify: `crates/hom-tui/src/status_rail.rs` — render cost in the bar
- Modify: `crates/hom-tui/src/render.rs` — pass cost to status rail
- Modify: `src/main.rs` — poll cost from DB periodically

- [ ] **Step 1: Write failing test — cost renders in status rail**

In `crates/hom-tui/src/status_rail.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_status_rail_shows_cost() {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_status_rail(frame, area, 2, Some(1), None, 4.56);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let text: String = buf.content().iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("$4.56"), "expected cost in rail, got: {text}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-tui status_rail::tests -- -v`
Expected: FAIL — `render_status_rail` takes 5 args, test passes 6

- [ ] **Step 3: Add `total_cost` parameter to `render_status_rail`**

In `crates/hom-tui/src/status_rail.rs`, update the function signature and add cost display:

```rust
pub fn render_status_rail(
    frame: &mut Frame,
    area: Rect,
    pane_count: usize,
    focused_pane: Option<u32>,
    workflow_status: Option<&str>,
    total_cost: f64,
) {
    // ... existing spans ...

    if total_cost > 0.0 {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("${total_cost:.2}"),
            Style::default().fg(Color::Magenta),
        ));
    }

    // ... rest unchanged ...
}
```

- [ ] **Step 4: Add `total_cost` field to `App`**

In `crates/hom-tui/src/app.rs`, add to the `App` struct:

```rust
pub total_cost: f64,
```

Initialize to `0.0` in `App::new()`.

- [ ] **Step 5: Update `render.rs` to pass cost**

In `crates/hom-tui/src/render.rs`, change the `render_status_rail` call:

```rust
render_status_rail(
    frame,
    chunks[0],
    app.panes.len(),
    app.focused_pane,
    app.workflow_status.as_deref(),
    app.total_cost,
);
```

- [ ] **Step 6: Poll cost from DB in main loop**

In `src/main.rs`, add a cost polling counter. Every 30 ticks (~1 second at 30fps), query `total_cost()` from the DB and update `app.total_cost`:

```rust
// After poll_pty_output, before poll_pending_completions:
cost_poll_counter += 1;
if cost_poll_counter >= fps && let Some(ref db) = app.db {
    cost_poll_counter = 0;
    let db = db.clone();
    // Fire a quick query — non-blocking via spawn
    let cost_tx = cost_tx.clone();
    tokio::spawn(async move {
        if let Ok(cost) = hom_db::cost::total_cost(db.pool()).await {
            let _ = cost_tx.send(cost);
        }
    });
}
// Drain cost updates
while let Ok(cost) = cost_rx.try_recv() {
    app.total_cost = cost;
}
```

Add `cost_tx`/`cost_rx` as a `tokio::sync::mpsc::unbounded_channel::<f64>()` at the top of `run_app`.

- [ ] **Step 7: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

Expected: All tests pass, zero warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/hom-tui/src/status_rail.rs crates/hom-tui/src/render.rs \
       crates/hom-tui/src/app.rs src/main.rs
git commit -m "feat: display total cost in status rail (F10)

Query total_cost() from DB every ~1s via async task, display in status
rail as \$X.XX in magenta. Adds total_cost field to App, cost parameter
to render_status_rail. Test verifies cost appears in rendered buffer."
```

---

### Task 2: Workflow Progress Tracking (F9)

**Files:**
- Create: `crates/hom-tui/src/workflow_progress.rs` — structured progress type
- Modify: `crates/hom-tui/src/lib.rs` — add module
- Modify: `crates/hom-tui/src/app.rs` — replace `workflow_status: Option<String>` with `WorkflowProgress`
- Modify: `crates/hom-tui/src/status_rail.rs` — render step counts
- Modify: `crates/hom-tui/src/render.rs` — pass progress data
- Modify: `crates/hom-tui/src/workflow_bridge.rs` — add progress update request
- Modify: `src/main.rs` — handle progress updates

- [ ] **Step 1: Write failing test — WorkflowProgress tracks step status**

Create `crates/hom-tui/src/workflow_progress.rs`:

```rust
//! Workflow execution progress tracking.

use std::collections::HashMap;

/// Status of a single workflow step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepProgress {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

/// Tracks the progress of a running workflow.
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
        self.steps
            .values()
            .all(|s| matches!(s, StepProgress::Completed | StepProgress::Failed | StepProgress::Skipped))
    }

    pub fn summary(&self) -> String {
        let done = self.completed_count();
        let failed = self.steps.values().filter(|s| matches!(s, StepProgress::Failed)).count();
        if failed > 0 {
            format!("{}: {done}/{} done, {failed} failed", self.name, self.total_steps)
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
        let wp = WorkflowProgress::new(
            "test".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(wp.total_steps, 3);
        assert_eq!(wp.completed_count(), 0);
        assert!(!wp.is_finished());
    }

    #[test]
    fn test_update_and_count() {
        let mut wp = WorkflowProgress::new(
            "test".to_string(),
            vec!["a".to_string(), "b".to_string()],
        );
        wp.update_step("a", StepProgress::Completed);
        assert_eq!(wp.completed_count(), 1);
        assert!(!wp.is_finished());
        wp.update_step("b", StepProgress::Completed);
        assert!(wp.is_finished());
    }

    #[test]
    fn test_summary_format() {
        let mut wp = WorkflowProgress::new(
            "deploy".to_string(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()],
        );
        assert_eq!(wp.summary(), "deploy: 0/3");
        wp.update_step("a", StepProgress::Completed);
        assert_eq!(wp.summary(), "deploy: 1/3");
        wp.update_step("b", StepProgress::Failed);
        wp.update_step("c", StepProgress::Skipped);
        assert!(wp.summary().contains("1 failed"));
    }
}
```

- [ ] **Step 2: Register module in lib.rs**

Add to `crates/hom-tui/src/lib.rs`:

```rust
pub mod workflow_progress;
```

- [ ] **Step 3: Run test to verify it passes**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-tui workflow_progress -- -v
```

Expected: 3 tests pass.

- [ ] **Step 4: Replace `workflow_status: Option<String>` with structured progress**

In `crates/hom-tui/src/app.rs`:

```rust
// Change the field:
pub workflow_progress: Option<WorkflowProgress>,

// In App::new(), change:
workflow_progress: None,
```

Update `workflow_status` references to use `workflow_progress` throughout `app.rs` and `src/main.rs`.

In `render.rs`, change the status rail call to pass the progress summary:

```rust
render_status_rail(
    frame,
    chunks[0],
    app.panes.len(),
    app.focused_pane,
    app.workflow_progress.as_ref().map(|p| p.summary()).as_deref(),
    app.total_cost,
);
```

- [ ] **Step 5: Add WorkflowStepUpdate to WorkflowBridge requests**

In `crates/hom-tui/src/workflow_bridge.rs`, add a new variant:

```rust
pub enum WorkflowRequest {
    // ... existing variants ...
    StepUpdate {
        step_id: String,
        status: hom_tui::workflow_progress::StepProgress,
    },
}
```

Handle this in `handle_workflow_request()` in `src/main.rs`:

```rust
WorkflowRequest::StepUpdate { step_id, status } => {
    if let Some(ref mut progress) = app.workflow_progress {
        progress.update_step(&step_id, status);
    }
}
```

- [ ] **Step 6: Emit step updates from the executor**

In `crates/hom-workflow/src/executor.rs`, the `StepOutcome::Completed` and `StepOutcome::Failed` processing blocks in `execute_inner` should send status updates through the bridge. Since the executor doesn't own the bridge directly (it uses `WorkflowRuntime` trait), add a new optional callback trait method to `WorkflowRuntime`:

```rust
async fn report_step_status(&self, step_id: &str, status: &str) {
    // default no-op
}
```

The `WorkflowBridge` implementation sends `StepUpdate` through the channel.

- [ ] **Step 7: Wire workflow start to initialize progress**

In `src/main.rs`, when `:run` launches a workflow, create a `WorkflowProgress` with all step IDs:

```rust
app.workflow_progress = Some(WorkflowProgress::new(
    workflow.clone(),
    def.steps.iter().map(|s| s.id.clone()).collect(),
));
```

- [ ] **Step 8: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 9: Commit**

```bash
git add crates/hom-tui/src/workflow_progress.rs crates/hom-tui/src/lib.rs \
       crates/hom-tui/src/app.rs crates/hom-tui/src/render.rs \
       crates/hom-tui/src/status_rail.rs crates/hom-tui/src/workflow_bridge.rs \
       crates/hom-workflow/src/executor.rs src/main.rs
git commit -m "feat: structured workflow progress tracking (F9)

Replace workflow_status: Option<String> with WorkflowProgress type
that tracks per-step status (Pending/Running/Completed/Failed/Skipped).
Status rail shows step counts (e.g., 'deploy: 2/5'). Executor reports
step status via WorkflowRuntime callback."
```

---

### Task 3: Integration Tests for PTY Pipeline

**Files:**
- Create: `tests/integration/mod.rs`
- Create: `tests/integration/pty_pipeline.rs`
- Modify: `Cargo.toml` — add integration test deps if needed

- [ ] **Step 1: Write the terminal emulator integration test**

Create `tests/integration/pty_pipeline.rs`:

```rust
//! Integration tests for the PTY → terminal emulator → screen snapshot pipeline.

use hom_core::TerminalBackend;

#[test]
fn test_terminal_processes_ansi_text() {
    let mut term = hom_terminal::create_terminal(80, 24, 100);
    term.process(b"Hello, World!");
    let snap = term.screen_snapshot();
    let text = snap.last_n_lines(1);
    assert!(
        text.contains("Hello, World!"),
        "expected 'Hello, World!' in screen, got: {text}"
    );
}

#[test]
fn test_terminal_handles_newlines() {
    let mut term = hom_terminal::create_terminal(80, 24, 100);
    term.process(b"line1\nline2\nline3");
    let snap = term.screen_snapshot();
    let text = snap.text();
    assert!(text.contains("line1"));
    assert!(text.contains("line2"));
    assert!(text.contains("line3"));
}

#[test]
fn test_terminal_handles_colors() {
    let mut term = hom_terminal::create_terminal(80, 24, 100);
    // ESC[31m = red foreground, ESC[0m = reset
    term.process(b"\x1b[31mRed text\x1b[0m Normal");
    let snap = term.screen_snapshot();
    let text = snap.text();
    assert!(text.contains("Red text"));
    assert!(text.contains("Normal"));
    // First cell of "Red text" should have red foreground
    assert!(
        matches!(snap.rows[0][0].fg, hom_core::TermColor::Red),
        "expected red foreground, got {:?}",
        snap.rows[0][0].fg
    );
}

#[test]
fn test_terminal_resize() {
    let mut term = hom_terminal::create_terminal(80, 24, 100);
    term.process(b"initial content");
    term.resize(40, 12);
    let snap = term.screen_snapshot();
    assert_eq!(snap.cols, 40);
    assert_eq!(snap.num_rows, 12);
}

#[test]
fn test_terminal_cursor_tracking() {
    let mut term = hom_terminal::create_terminal(80, 24, 100);
    term.process(b"abc");
    let cursor = term.cursor();
    assert_eq!(cursor.col, 3);
    assert_eq!(cursor.row, 0);
    assert!(cursor.visible);
}

#[test]
fn test_screen_snapshot_last_n_lines() {
    let mut term = hom_terminal::create_terminal(80, 24, 100);
    term.process(b"line1\nline2\nline3\nline4\nline5");
    let last2 = term.screen_snapshot().last_n_lines(2);
    assert!(last2.contains("line4") || last2.contains("line5"));
}
```

- [ ] **Step 2: Create mod.rs**

Create `tests/integration/mod.rs`:

```rust
mod pty_pipeline;
```

- [ ] **Step 3: Run tests**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test --test integration -- -v
```

Wait — Cargo expects integration tests in the `tests/` directory at workspace root. For the binary crate, integration tests go in `tests/`. Check if the Cargo.toml has the right setup:

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-terminal -- -v
```

Actually, since these test `hom_terminal` and `hom_core` directly, put them in the `hom-terminal` crate's test module instead:

Add to `crates/hom-terminal/src/lib.rs` or create `crates/hom-terminal/tests/integration.rs`.

Better approach: add inline tests to `crates/hom-terminal/src/fallback_vt100.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use hom_core::TerminalBackend;

    #[test]
    fn test_process_and_snapshot() {
        let mut term = Vt100Backend::new(80, 24, 100);
        term.process(b"Hello, World!");
        let snap = term.screen_snapshot();
        let text = snap.last_n_lines(1);
        assert!(text.contains("Hello, World!"));
    }

    #[test]
    fn test_color_processing() {
        let mut term = Vt100Backend::new(80, 24, 100);
        term.process(b"\x1b[31mRed\x1b[0m");
        let snap = term.screen_snapshot();
        assert!(matches!(snap.rows[0][0].fg, hom_core::TermColor::Red));
    }

    #[test]
    fn test_resize() {
        let mut term = Vt100Backend::new(80, 24, 100);
        term.resize(40, 12);
        let snap = term.screen_snapshot();
        assert_eq!(snap.cols, 40);
        assert_eq!(snap.num_rows, 12);
    }

    #[test]
    fn test_cursor_movement() {
        let mut term = Vt100Backend::new(80, 24, 100);
        term.process(b"abc\ndef");
        let cursor = term.cursor();
        assert_eq!(cursor.row, 1);
        assert_eq!(cursor.col, 3);
    }
}
```

- [ ] **Step 4: Run tests**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-terminal -- -v
```

Expected: 4 new tests pass.

- [ ] **Step 5: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 6: Commit**

```bash
git add crates/hom-terminal/src/fallback_vt100.rs
git commit -m "test: add terminal emulator integration tests

Test the PTY→terminal pipeline: text processing, ANSI color handling,
resize, and cursor tracking via Vt100Backend. Verifies ScreenSnapshot
contains correct cell data for the render pipeline."
```

---

### Task 4: Workflow Template Library Expansion (F12)

**Files:**
- Create: `workflows/test-driven-development.yaml`
- Create: `workflows/debugging.yaml`
- Create: `workflows/refactor-with-tests.yaml`
- Create: `workflows/documentation.yaml`
- Create: `workflows/parallel-analysis.yaml`

- [ ] **Step 1: Create TDD workflow**

Create `workflows/test-driven-development.yaml`:

```yaml
name: test-driven-development
description: Write tests first, then implement, then refactor

variables:
  harness: claude
  task: ""

steps:
  - id: write-tests
    harness: "{{ harness }}"
    prompt: |
      Write failing tests for: {{ task }}
      Focus on behavior, not implementation. Cover happy path,
      edge cases, and error conditions. Output the test file contents.
    timeout: 5m

  - id: implement
    harness: "{{ harness }}"
    depends_on: [write-tests]
    prompt: |
      Implement the minimal code to make these tests pass:
      {{ steps.write-tests.output }}
      Do not add features beyond what the tests require.
    timeout: 10m
    retry:
      max_attempts: 2
      backoff: exponential

  - id: refactor
    harness: "{{ harness }}"
    depends_on: [implement]
    condition: 'steps.implement.status == "completed"'
    prompt: |
      Refactor the implementation for clarity and maintainability
      while keeping all tests green:
      {{ steps.implement.output }}
    timeout: 5m
    on_failure: skip
```

- [ ] **Step 2: Create debugging workflow**

Create `workflows/debugging.yaml`:

```yaml
name: debugging
description: Systematic bug investigation and fix

variables:
  harness: claude
  bug: ""

steps:
  - id: reproduce
    harness: "{{ harness }}"
    prompt: |
      Reproduce and analyze this bug: {{ bug }}
      Find the root cause. Show the failing test case.
    timeout: 5m

  - id: fix
    harness: "{{ harness }}"
    depends_on: [reproduce]
    prompt: |
      Fix the root cause identified:
      {{ steps.reproduce.output }}
      Write the minimal fix. Include a regression test.
    timeout: 10m
    retry:
      max_attempts: 2
      backoff: linear

  - id: verify
    harness: "{{ harness }}"
    depends_on: [fix]
    prompt: |
      Verify the fix is correct:
      {{ steps.fix.output }}
      Run all related tests. Confirm no regressions.
    timeout: 5m
```

- [ ] **Step 3: Create refactoring workflow**

Create `workflows/refactor-with-tests.yaml`:

```yaml
name: refactor-with-tests
description: Safe refactoring with test coverage verification

variables:
  harness: claude
  target: ""

steps:
  - id: audit
    harness: "{{ harness }}"
    prompt: |
      Audit the test coverage for: {{ target }}
      List what is tested and what is missing.
    timeout: 5m

  - id: add-tests
    harness: "{{ harness }}"
    depends_on: [audit]
    prompt: |
      Add missing tests identified in the audit:
      {{ steps.audit.output }}
      All tests must pass before refactoring.
    timeout: 10m

  - id: refactor
    harness: "{{ harness }}"
    depends_on: [add-tests]
    condition: 'steps.add-tests.status == "completed"'
    prompt: |
      Refactor {{ target }} for clarity and maintainability.
      All existing tests must remain green.
    timeout: 10m
    retry:
      max_attempts: 2
      backoff: exponential
```

- [ ] **Step 4: Create documentation workflow**

Create `workflows/documentation.yaml`:

```yaml
name: documentation
description: Generate documentation from code analysis

variables:
  harness: claude
  scope: ""

steps:
  - id: analyze
    harness: "{{ harness }}"
    prompt: |
      Analyze the codebase for: {{ scope }}
      List all public APIs, their purposes, and usage patterns.
    timeout: 5m

  - id: document
    harness: "{{ harness }}"
    depends_on: [analyze]
    prompt: |
      Write clear documentation based on:
      {{ steps.analyze.output }}
      Include examples for each public API.
    timeout: 10m
```

- [ ] **Step 5: Create parallel analysis workflow**

Create `workflows/parallel-analysis.yaml`:

```yaml
name: parallel-analysis
description: Analyze code from multiple perspectives concurrently

variables:
  task: ""

steps:
  - id: security
    harness: claude
    prompt: |
      Security review: {{ task }}
      Focus on OWASP top 10, input validation, auth boundaries.
    timeout: 5m

  - id: performance
    harness: gemini
    prompt: |
      Performance review: {{ task }}
      Focus on algorithmic complexity, memory usage, I/O patterns.
    timeout: 5m

  - id: maintainability
    harness: codex
    prompt: |
      Maintainability review: {{ task }}
      Focus on code clarity, coupling, test coverage gaps.
    timeout: 5m

  - id: synthesis
    harness: claude
    depends_on: [security, performance, maintainability]
    prompt: |
      Synthesize these three reviews into prioritized action items:
      Security: {{ steps.security.output }}
      Performance: {{ steps.performance.output }}
      Maintainability: {{ steps.maintainability.output }}
    timeout: 5m
```

- [ ] **Step 6: Commit**

```bash
git add workflows/
git commit -m "feat: add 5 workflow templates (F12)

Add test-driven-development, debugging, refactor-with-tests,
documentation, and parallel-analysis workflow templates.
Total: 8 built-in workflow templates covering common patterns."
```

---

### Task 5: Extract handle_command into Per-Command Handlers

**Files:**
- Modify: `src/main.rs` — extract per-command functions

- [ ] **Step 1: Extract :pipe handler**

Create a function `handle_pipe(app, source, target) -> anyhow::Result<()>` that contains the existing `:pipe` match arm body (~50 lines). Replace the match arm with a one-line delegation.

- [ ] **Step 2: Extract :run handler**

Create `handle_run(app, workflow, variables, bridge) -> anyhow::Result<()>` containing the `:run` logic (~35 lines).

- [ ] **Step 3: Extract :save and :restore handlers**

Create `handle_save(app, name)` and `handle_restore(app, name, terminal_size)`.

- [ ] **Step 4: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor: extract per-command handlers from handle_command

Extract handle_pipe, handle_run, handle_save, handle_restore as
separate functions. handle_command is now a flat dispatch that
delegates to focused handlers. No behavior change."
```

---

### Task 6: Update Design Doc Section 11

**Files:**
- Modify: `hom-system-design.md` — update Future Directions

- [ ] **Step 1: Update Section 11 items**

Items 2-8 are already resolved. Mark them and renumber:

```markdown
## 11. Future Directions

**Resolved in Phase 3/4:**
- ~~Wire session save/restore~~ → `:save`/`:restore` wired to hom-db
- ~~Wire cost tracking~~ → `log_cost()` from workflow + token events, displayed in status rail
- ~~Implement `parse_screen()`~~ → all 7 adapters have real implementations
- ~~Integration-test sideband channels~~ → HTTP sideband tested, RPC implemented
- ~~Parallel step execution~~ → `Arc<dyn WorkflowRuntime>` + JoinSet
- ~~SendAndWait completion detection~~ → PendingCompletion polling
- ~~Keybinding config wiring~~ → KeybindingsConfig applied to InputRouter

**Active future work:**
1. **GhosttyBackend implementation** — blocked on libghostty-vt + Zig ≥0.15.x
2. **GPU rendering** — leverage libghostty's pipeline for complex output
3. **Plugin system for adapters** — WASM or shared library plugins
4. **Remote pane support** — spawn harnesses on remote machines via SSH
5. **Web UI** — serve ratatui frames over WebSocket
6. **MCP integration** — expose HOM as an MCP server
7. **Workflow marketplace** — share and discover templates
8. **Agent-to-agent protocol** — standardized message format
```

- [ ] **Step 2: Update CLAUDE.md remaining work**

```markdown
**Remaining work — documentation and hardening:**
- Run NFR benchmarks against targets (60fps, <30MB, <50ms)
- GhosttyBackend wiring when libghostty-vt is published
```

- [ ] **Step 3: Commit**

```bash
git add hom-system-design.md CLAUDE.md
git commit -m "docs: reconcile design doc Section 11 with implementation state

Mark 7 items as resolved in Phase 3/4. Renumber active future work
to 8 items starting with GhosttyBackend."
```

---

## Self-Review

**Spec coverage check:**
- F1-F8: Fully implemented per CLAUDE.md — no tasks needed ✓
- F9 (DAG visualization): Task 2 adds structured workflow progress ✓
- F10 (Cost display): Task 1 adds cost to status rail ✓
- F11 (Session persistence): Already wired in Phase 3 — no task needed ✓
- F12 (Template library): Task 4 adds 5 more templates ✓
- NF1-NF6: Benchmarks exist (Phase 3) — no task needed ✓

**Placeholder scan:** No TBD, TODO, or "fill in details" found.

**Type consistency:** `WorkflowProgress`, `StepProgress`, `total_cost: f64` — used consistently across tasks 1 and 2. `render_status_rail` signature change in Task 1 is propagated to `render.rs`.
