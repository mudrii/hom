# Phase 5: Production Blockers — Graceful Shutdown, Crash Handling, DB Reliability

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three blockers that prevent HOM from being safely usable: (1) orphaned PTY processes on quit, (2) silent harness crash handling, (3) database failures silently degrading persistence.

**Architecture:** Add `App::shutdown()` to kill all PTY processes on exit. Add process exit detection in the main event loop that marks panes as exited, notifies pending workflow completions, and shows status in the TUI. Make database opening fail fast with a clear error, and add a `--no-db` flag for explicit opt-out.

**Tech Stack:** Rust 2024, tokio 1.x, portable-pty 0.9, sqlx 0.8 (SQLite). Build with `CARGO_TARGET_DIR=/tmp/hom-target`.

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `crates/hom-tui/src/app.rs` | App state + lifecycle | Modify — add `shutdown()`, add `PaneStatus` enum |
| `crates/hom-pty/src/manager.rs` | PTY lifecycle | Modify — add `kill_all()` |
| `src/main.rs` | Event loop, startup | Modify — call `shutdown()` on exit, handle process exits, make DB critical |
| `crates/hom-tui/src/pane_render.rs` | Pane rendering | Modify — show exited pane visual indicator |
| `crates/hom-core/src/error.rs` | Error types | Modify — add `DatabaseRequired` variant |

---

### Task 1: Graceful PTY Cleanup on Shutdown

When the user presses Ctrl-Q or `:quit`, all spawned harness processes must be killed before HOM exits. Currently they are orphaned.

**Files:**
- Modify: `crates/hom-pty/src/manager.rs` — add `kill_all()`
- Modify: `crates/hom-tui/src/app.rs` — add `shutdown()`
- Modify: `src/main.rs` — call `app.shutdown()` before terminal restore

- [ ] **Step 1: Write failing test — PtyManager::kill_all()**

In `crates/hom-pty/src/manager.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_all_empties_instances() {
        let mut mgr = PtyManager::new();
        // Spawn a simple process we can kill
        let spec = CommandSpec {
            program: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id1 = mgr.spawn(&spec, 80, 24).unwrap();
        let id2 = mgr.spawn(&spec, 80, 24).unwrap();
        assert_eq!(mgr.active_panes().len(), 2);

        mgr.kill_all();
        assert!(mgr.active_panes().is_empty());
        assert!(!mgr.has_pane(id1));
        assert!(!mgr.has_pane(id2));
    }

    #[test]
    fn test_kill_all_on_empty_manager() {
        let mut mgr = PtyManager::new();
        mgr.kill_all(); // should not panic
        assert!(mgr.active_panes().is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-pty -- -v
```

Expected: FAIL — `kill_all` method does not exist.

- [ ] **Step 3: Implement `PtyManager::kill_all()`**

In `crates/hom-pty/src/manager.rs`, add this method to the `impl PtyManager` block, after `kill()`:

```rust
    /// Kill all active PTY processes. Used during shutdown cleanup.
    pub fn kill_all(&mut self) {
        let pane_ids: Vec<PaneId> = self.instances.keys().copied().collect();
        for pane_id in pane_ids {
            if let Some(mut instance) = self.instances.remove(&pane_id) {
                if let Err(e) = instance.child.kill() {
                    debug!(pane_id, error = %e, "failed to kill PTY during shutdown");
                }
            }
        }
        info!("all PTY processes killed");
    }
```

- [ ] **Step 4: Run test to verify it passes**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-pty -- -v
```

Expected: 2 new tests pass.

- [ ] **Step 5: Add `App::shutdown()` method**

In `crates/hom-tui/src/app.rs`, add this method to the `impl App` block:

```rust
    /// Clean shutdown: kill all PTY processes and drain pending completions.
    pub fn shutdown(&mut self) {
        // Resolve pending workflow completions with an error
        for pending in self.pending_completions.drain(..) {
            let _ = pending
                .reply
                .send(Err(hom_core::HomError::Other("shutting down".to_string())));
        }
        // Kill all PTY child processes
        self.pty_manager.kill_all();
        self.panes.clear();
        self.pane_order.clear();
        info!("app shutdown complete");
    }
```

Add `use tracing::info;` if not already imported.

- [ ] **Step 6: Call `app.shutdown()` before terminal restore in main.rs**

In `src/main.rs`, change the section after `run_app()` returns (around line 137-148):

```rust
    let result = run_app(&mut terminal, &mut app, tick_rate, workflow_rx, bridge).await;

    // Clean up all PTY processes before restoring the terminal
    app.shutdown();

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
```

- [ ] **Step 7: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 8: Commit**

```bash
git add crates/hom-pty/src/manager.rs crates/hom-tui/src/app.rs src/main.rs
git commit -m "fix: graceful PTY cleanup on shutdown

Add PtyManager::kill_all() to terminate all child processes.
Add App::shutdown() to drain pending completions and kill PTYs.
Call shutdown() before terminal restore so harness processes are
not orphaned when user presses Ctrl-Q or :quit."
```

---

### Task 2: Handle Process Crashes — Mark Exited Panes

When a harness process crashes or exits, the current code detects this via `try_wait()` but does nothing. Users see a frozen pane with no indication that the process died. Workflow steps waiting on that pane hang until timeout.

**Files:**
- Modify: `crates/hom-tui/src/app.rs` — add `exited: Option<u32>` field to `Pane`, add `handle_exited_panes()`
- Modify: `crates/hom-tui/src/pane_render.rs` — show "[EXITED]" indicator for dead panes
- Modify: `src/main.rs` — replace the empty `try_wait` handler with real logic

- [ ] **Step 1: Add `exited` field to `Pane`**

In `crates/hom-tui/src/app.rs`, add to the `Pane` struct:

```rust
    /// Exit code if the process has terminated. None while running.
    pub exited: Option<u32>,
```

Set to `None` in the `Pane` construction inside `spawn_pane_inner()`.

- [ ] **Step 2: Add `handle_exited_panes()` method to App**

In `crates/hom-tui/src/app.rs`, add:

```rust
    /// Check for processes that have exited and mark their panes.
    /// Returns a list of (pane_id, exit_code) for newly exited panes.
    pub fn handle_exited_panes(&mut self) -> Vec<(PaneId, u32)> {
        let mut newly_exited = Vec::new();
        let pane_ids: Vec<PaneId> = self.pane_order.clone();

        for pane_id in pane_ids {
            // Skip already-marked panes
            if let Some(pane) = self.panes.get(&pane_id)
                && pane.exited.is_some()
            {
                continue;
            }

            if let Ok(Some(exit_code)) = self.pty_manager.try_wait(pane_id) {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.exited = Some(exit_code);
                }
                newly_exited.push((pane_id, exit_code));
            }
        }

        newly_exited
    }
```

- [ ] **Step 3: Write test for handle_exited_panes**

In the `#[cfg(test)] mod tests` block in `app.rs`, add:

```rust
    #[test]
    fn test_shutdown_clears_state() {
        let mut app = App::new(HomConfig::default());
        app.shutdown();
        assert!(app.panes.is_empty());
        assert!(app.pane_order.is_empty());
        assert!(app.pending_completions.is_empty());
    }
```

- [ ] **Step 4: Update pane_render.rs to show exited indicator**

Read `crates/hom-tui/src/pane_render.rs`. Find where the pane border/title is rendered. Add an exited indicator. The `render_pane` function needs an `exited: Option<u32>` parameter. When `exited.is_some()`, change the border color to red and append `[EXITED: N]` to the title.

In `pane_render.rs`, update the `render_pane` signature to accept `exited: Option<u32>`:

```rust
pub fn render_pane(
    frame: &mut Frame,
    area: Rect,
    terminal: &impl TerminalBackend,
    title: &str,
    harness_name: &str,
    focused: bool,
    exited: Option<u32>,
) {
```

In the title construction, append exit status:

```rust
    let display_title = if let Some(code) = exited {
        format!(" {title} [EXITED: {code}] ")
    } else {
        format!(" {title} ")
    };
```

Change border color to red when exited:

```rust
    let border_style = if exited.is_some() {
        Style::default().fg(Color::Red)
    } else if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
```

- [ ] **Step 5: Update render.rs to pass exited status**

In `crates/hom-tui/src/render.rs`, find the `render_pane` call and add `pane.exited`:

```rust
render_pane(
    frame,
    *area,
    &pane.terminal,
    &pane.title,
    pane.harness_type.display_name(),
    is_focused,
    pane.exited,
);
```

- [ ] **Step 6: Replace empty try_wait handler in main.rs**

In `src/main.rs`, replace the empty process exit check (around line 287-293):

```rust
        // Check for exited processes and handle them
        let exited_panes = app.handle_exited_panes();
        for (pane_id, exit_code) in &exited_panes {
            warn!(pane_id, exit_code, "harness process exited");
            // Resolve any pending workflow completions for this pane with an error
            let mut resolved = Vec::new();
            for (i, pending) in app.pending_completions.iter().enumerate() {
                if pending.pane_id == *pane_id {
                    resolved.push(i);
                }
            }
            for i in resolved.into_iter().rev() {
                let pending = app.pending_completions.remove(i);
                let _ = pending.reply.send(Err(hom_core::HomError::Other(
                    format!("harness process exited with code {exit_code}"),
                )));
            }
        }
```

- [ ] **Step 7: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 8: Commit**

```bash
git add crates/hom-tui/src/app.rs crates/hom-tui/src/pane_render.rs \
       crates/hom-tui/src/render.rs src/main.rs
git commit -m "fix: handle harness process crashes with visual feedback

Add exited: Option<u32> to Pane, detect process exit via try_wait,
mark panes with [EXITED: N] in red border. Resolve pending workflow
completions for crashed panes with an error instead of hanging until
timeout. Add App::handle_exited_panes() for clean exit detection."
```

---

### Task 3: Database Reliability — Fail Fast or Explicit Opt-Out

The database is currently optional — if it fails to open, the app continues silently without persistence. Workflows lose checkpointing, sessions can't be saved, costs aren't tracked. Users don't know this happened.

**Files:**
- Modify: `src/main.rs` — make DB open a hard error, add `--no-db` flag
- Modify: `crates/hom-core/src/error.rs` — add `DatabaseRequired` variant

- [ ] **Step 1: Add `DatabaseRequired` error variant**

In `crates/hom-core/src/error.rs`, add to the `HomError` enum:

```rust
    #[error("database is required but could not be opened: {0}")]
    DatabaseRequired(String),
```

- [ ] **Step 2: Add `--no-db` CLI flag**

In `src/main.rs`, add to the `Cli` struct:

```rust
    /// Run without database (disables session save, cost tracking, workflow checkpoints)
    #[arg(long)]
    no_db: bool,
```

- [ ] **Step 3: Make DB opening fail fast unless `--no-db`**

In `src/main.rs`, replace the current DB opening code (around line 89-101):

```rust
    // Open database (required unless --no-db)
    if cli.no_db {
        info!("running without database (--no-db)");
    } else {
        let db_path = app.config.db_path();
        let db = hom_db::HomDb::open(db_path.to_str().unwrap_or("hom.db"))
            .await
            .map_err(|e| {
                // Restore terminal before showing error
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                anyhow::anyhow!(
                    "Failed to open database at {}: {e}\n\
                     Use --no-db to run without persistence.",
                    db_path.display()
                )
            })?;
        let db = std::sync::Arc::new(db);
        app.db = Some(db.clone());
        info!(path = %db_path.display(), "database opened");
    }
```

- [ ] **Step 4: Show warning in status rail when running without DB**

The `command_bar.last_error` can show the warning on startup when `--no-db` is used:

```rust
    if cli.no_db {
        app.command_bar.last_error = Some("running without database (--no-db)".to_string());
    }
```

- [ ] **Step 5: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 6: Commit**

```bash
git add src/main.rs crates/hom-core/src/error.rs
git commit -m "fix: make database required, add --no-db for explicit opt-out

Database opening now fails fast with a clear error message and
instructions to use --no-db. Previously, DB failure was silently
ignored, causing workflows to lose checkpoints and sessions to
not persist. Users must now explicitly opt out of persistence."
```

---

### Task 4: Update CLAUDE.md and TODO.md

**Files:**
- Modify: `CLAUDE.md`
- Modify: `TODO.md`

- [ ] **Step 1: Update CLAUDE.md implementation status**

Add to the "Resolved" section:

```markdown
**Resolved (April 10, 2026 — Phase 5):**
- Graceful PTY shutdown — App::shutdown() kills all child processes on Ctrl-Q/:quit
- Process crash handling — exited panes show [EXITED: N] in red, workflow steps notified
- Database reliability — fail fast on DB error, --no-db for explicit opt-out
```

- [ ] **Step 2: Update TODO.md**

Mark the 3 blocker items as resolved.

- [ ] **Step 3: Run gate and commit**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
git add CLAUDE.md TODO.md
git commit -m "docs: update CLAUDE.md and TODO.md for Phase 5 blockers"
```

---

## Self-Review

**Spec coverage:**
- Blocker 1 (orphaned PTYs): Task 1 — `kill_all()` + `shutdown()` + call before exit ✓
- Blocker 2 (crash handling): Task 2 — `handle_exited_panes()` + visual indicator + workflow notification ✓
- Blocker 3 (DB reliability): Task 3 — fail fast + `--no-db` flag ✓
- Docs: Task 4 ✓

**Placeholder scan:** No TBD/TODO/placeholders found.

**Type consistency:**
- `kill_all()` on PtyManager ← called by `shutdown()` on App ← called from main.rs ✓
- `exited: Option<u32>` on Pane ← set by `handle_exited_panes()` ← read by `render_pane()` ✓
- `--no-db` flag ← `cli.no_db: bool` ← checked before DB open ✓
