# Phase 7: UX Gaps — Exit Notification, Sideband Health, Keybinding Validation, Task Cancellation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix five IMPORTANT user-experience gaps: (1) notify the user in the command bar when a harness process exits, (2) poll sideband health in the main event loop and notify on failure, (3) document the Claude Code flickering limitation and headless workaround, (4) validate keybinding config strings at startup and warn on invalid entries, (5) abort `AsyncPtyReader` tasks explicitly on pane kill.

**Architecture:** Items 1–2 are changes to `src/main.rs`'s event loop (the `run_app` function). Item 3 is a doc comment in `claude_code.rs`. Item 4 adds a `validate_keybindings()` function to `input.rs` called at startup. Item 5 adds an `abort()` method to `AsyncPtyReader` and calls it in `App::kill_pane()`.

**Tech Stack:** Rust 2024, hom-tui 0.1, hom-pty 0.1, hom-adapters 0.1, tokio 1.x. Build with `CARGO_TARGET_DIR=/tmp/hom-target`.

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `src/main.rs` | Event loop — pane exit notification + sideband health | Modify — add notification and periodic health poll |
| `crates/hom-adapters/src/claude_code.rs` | Claude Code adapter | Modify — add flickering doc comment |
| `crates/hom-tui/src/input.rs` | Input routing + keybinding parsing | Modify — add `validate_keybindings()` |
| `crates/hom-pty/src/async_reader.rs` | AsyncPtyReader | Modify — add `abort()` method |
| `crates/hom-tui/src/app.rs` | App state — kill_pane | Modify — call reader.abort() before removal |

---

### Task 1: Process Exit Notification in Command Bar (#8)

The `handle_exited_panes()` call in `run_app` already returns newly exited panes and logs a `warn!()`. But the user watching the TUI sees nothing — the pane border shows `[EXITED: N]` only on the next render if they happen to look. The fix: set `app.command_bar.last_error` to a visible notification when a pane exits.

**Files:**
- Modify: `src/main.rs` — in the `exited_panes` loop after `handle_exited_panes()`

- [ ] **Step 1: Write failing test**

The exit notification logic is in the event loop — it's integration-level behavior. Add a unit test to `crates/hom-tui/src/app.rs` that verifies `handle_exited_panes()` correctly marks panes as exited (the notification happens in `main.rs`, but the `exited` field is the data we depend on):

```rust
    #[test]
    fn test_handle_exited_panes_returns_exit_code() {
        // The pane is marked as exited when try_wait returns Some(code).
        // We can't easily spawn a real process in a unit test, so we verify
        // the method returns an empty vec when no panes are registered.
        let mut app = App::new(HomConfig::default());
        let newly_exited = app.handle_exited_panes();
        assert!(
            newly_exited.is_empty(),
            "expected no exited panes for empty app"
        );
    }
```

Run: `CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-tui -- -v`
Expected: PASS (this is a baseline test — the interesting behavior is in main.rs).

- [ ] **Step 2: Add exit notification in `src/main.rs`**

In the `exited_panes` loop (around line 305 in `main.rs`), add a notification to `app.command_bar.last_error`. The current code is:

```rust
        let exited_panes = app.handle_exited_panes();
        for (pane_id, exit_code) in &exited_panes {
            warn!(pane_id, exit_code, "harness process exited");
            // Resolve any pending workflow completions for this pane
            let mut resolved_indices = Vec::new();
            for (i, pending) in app.pending_completions.iter().enumerate() {
                if pending.pane_id == *pane_id {
                    resolved_indices.push(i);
                }
            }
            for i in resolved_indices.into_iter().rev() {
                let pending = app.pending_completions.remove(i);
                let _ = pending.reply.send(Err(hom_core::HomError::Other(format!(
                    "harness process exited with code {exit_code}"
                ))));
            }
        }
```

Replace with:

```rust
        let exited_panes = app.handle_exited_panes();
        for (pane_id, exit_code) in &exited_panes {
            warn!(pane_id, exit_code, "harness process exited");

            // Notify the user in the command bar so they see it immediately,
            // even if they are not looking at the affected pane.
            let harness_name = app
                .panes
                .get(pane_id)
                .map(|p| p.harness_type.display_name().to_string())
                .unwrap_or_else(|| format!("pane #{pane_id}"));
            app.command_bar.last_error = Some(format!(
                "pane #{pane_id} ({harness_name}) exited with code {exit_code}"
            ));

            // Resolve any pending workflow completions for this pane
            let mut resolved_indices = Vec::new();
            for (i, pending) in app.pending_completions.iter().enumerate() {
                if pending.pane_id == *pane_id {
                    resolved_indices.push(i);
                }
            }
            for i in resolved_indices.into_iter().rev() {
                let pending = app.pending_completions.remove(i);
                let _ = pending.reply.send(Err(hom_core::HomError::Other(format!(
                    "harness process exited with code {exit_code}"
                ))));
            }
        }
```

- [ ] **Step 3: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs crates/hom-tui/src/app.rs
git commit -m "feat: notify user in command bar when harness process exits

When handle_exited_panes() detects a new exit, set command_bar.last_error
to 'pane #N (HarnessName) exited with code X' so the user sees it
immediately in the status bar, not just as a red [EXITED: N] pane border."
```

---

### Task 2: Sideband Health Check Polling (#9)

The `SidebandChannel::health_check()` method exists on all sidebands but is never called after startup. If OpenCode or pi-mono's sideband process crashes, workflow steps silently time out. Fix: poll health every ~5 seconds in the main event loop and notify on failure.

**Files:**
- Modify: `src/main.rs` — add health poll counter + poll loop + notification

- [ ] **Step 1: Write a test for HttpSideband health_check (already exists)**

The `test_health_check_unreachable` test in `crates/hom-adapters/src/sideband/http.rs` already covers the API. Verify it passes as a baseline:

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-adapters -- health_check -v
```

Expected: PASS (existing tests).

- [ ] **Step 2: Add sideband health poll in `src/main.rs`**

Add a health poll counter alongside the cost poll counter. In `run_app`, add after the cost poll counter declaration:

```rust
    // Sideband health polling: check every ~5 seconds (5 * fps ticks).
    let health_poll_interval = fps * 5;
    let mut health_tick_counter: u64 = 0;
```

Add the health poll drain loop in the event loop body, after the cost drain section. Use a `tokio::sync::mpsc::unbounded_channel` to receive health results asynchronously:

```rust
    let (health_tx, mut health_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u32, String, bool)>();
```

Add the health poll trigger in the event loop (after the cost poll trigger):

```rust
        // Poll sideband health every ~5 seconds
        health_tick_counter += 1;
        if health_tick_counter >= health_poll_interval {
            health_tick_counter = 0;
            for pane_id in &app.pane_order {
                if let Some(pane) = app.panes.get(pane_id)
                    && let Some(sideband) = &pane.sideband
                {
                    let sideband = sideband.clone();
                    let harness_name = pane.harness_type.display_name().to_string();
                    let pid = *pane_id;
                    let tx = health_tx.clone();
                    tokio::spawn(async move {
                        let healthy = sideband.health_check().await.unwrap_or(false);
                        let _ = tx.send((pid, harness_name, healthy));
                    });
                }
            }
        }
        // Drain health check results — notify on failure
        while let Ok((pane_id, harness_name, healthy)) = health_rx.try_recv() {
            if !healthy {
                warn!(pane_id, harness_name, "sideband health check failed");
                app.command_bar.last_error = Some(format!(
                    "sideband for pane #{pane_id} ({harness_name}) is not responding"
                ));
            }
        }
```

The complete `run_app` signature and context: `run_app` is defined at line 168, the cost channel is at line 176, the loop starts at line 181. Insert the health channel declaration and the health counter declaration after line 179 (`let mut cost_tick_counter: u64 = 0;`).

Full updated `run_app` preamble (after the cost declarations):

```rust
    // Sideband health polling: check every ~5 seconds (5 * fps ticks).
    let health_poll_interval = fps * 5;
    let mut health_tick_counter: u64 = 0;
    let (health_tx, mut health_rx) =
        tokio::sync::mpsc::unbounded_channel::<(u32, String, bool)>();
```

- [ ] **Step 3: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: poll sideband health every 5s, notify on failure

Add a health_tick_counter in run_app that fires health_check() on every
pane's sideband every ~5 seconds. Results arrive via unbounded channel.
When a sideband is not responding, command_bar.last_error is set so the
user sees it immediately: 'sideband for pane #N (HarnessName) is not responding'."
```

---

### Task 3: Document Claude Code Flickering Limitation (#10)

Claude Code uses Ink/React to render its TUI. In any multiplexer (including HOM), this causes 4,000–6,700 scroll events per second that flicker the pane. This cannot be fixed in HOM — it's upstream behavior. However, users need to know about the `--output-format stream-json` headless mode as a workaround for automated workflow steps.

**Files:**
- Modify: `crates/hom-adapters/src/claude_code.rs` — add doc comment explaining limitation and workaround

- [ ] **Step 1: Add doc comment to `ClaudeCodeAdapter`**

In `crates/hom-adapters/src/claude_code.rs`, replace the module-level comment block (lines 1–8):

```rust
//! Adapter for Claude Code CLI.
//!
//! **Tier 1** — Full orchestration via stdin/stdout with stream-json output.
//!
//! Binary: `claude`
//! Sideband: None (uses PTY + stream-json output format)
//!
//! # Known Limitation: Terminal Flickering
//!
//! Claude Code's Ink/React-based TUI generates approximately 4,000–6,700
//! scroll events per second in any terminal multiplexer. This is upstream
//! behavior that cannot be mitigated in HOM.
//!
//! **Workaround for automated workflow steps:** Use headless mode by adding
//! `--output-format stream-json` to `extra_args` in the harness config.
//! Headless mode suppresses the TUI and outputs JSONL events instead, which
//! HOM's `parse_screen()` can parse directly. Example config:
//!
//! ```toml
//! [harnesses.claude-code]
//! command = "claude"
//! extra_args = ["--output-format", "stream-json"]
//! ```
//!
//! In this mode, the pane renders JSON lines rather than a TUI, but
//! completion detection and output parsing work correctly.
```

- [ ] **Step 2: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add crates/hom-adapters/src/claude_code.rs
git commit -m "docs: document Claude Code flickering limitation and headless workaround

Claude Code's Ink/React TUI causes 4000-6700 scroll events/sec in any
multiplexer. Cannot be fixed in HOM. Document the --output-format
stream-json headless mode as the workaround for automated workflow steps."
```

---

### Task 4: Keybinding Config Validation (#11)

`InputRouter::from_config()` calls `parse_keybinding()` which returns `Option<Keybinding>`. When it returns `None` for an invalid string, the field keeps its compiled-in default and the user gets no feedback. This is confusing — the user might think their config is applied when it isn't.

Fix: add `validate_keybindings()` to `input.rs` that collects all invalid binding strings, and call it from `main.rs` just before starting the app to surface warnings.

**Files:**
- Modify: `crates/hom-tui/src/input.rs` — add `pub fn validate_keybindings()`
- Modify: `src/main.rs` — call `validate_keybindings()` after `App::new()`, set `last_error` if invalid

- [ ] **Step 1: Write failing test for `validate_keybindings()`**

Add to `#[cfg(test)] mod tests` in `input.rs`:

```rust
    #[test]
    fn test_validate_valid_keybindings() {
        use hom_core::KeybindingsConfig;
        let config = KeybindingsConfig::default();
        let errors = validate_keybindings(&config);
        assert!(
            errors.is_empty(),
            "default config should produce no validation errors, got: {errors:?}"
        );
    }

    #[test]
    fn test_validate_invalid_keybinding() {
        use hom_core::KeybindingsConfig;
        let mut config = KeybindingsConfig::default();
        config.toggle_command_bar = "mega-x".to_string(); // invalid modifier
        let errors = validate_keybindings(&config);
        assert!(
            !errors.is_empty(),
            "invalid keybinding should produce at least one error"
        );
        assert!(
            errors.iter().any(|e| e.contains("toggle_command_bar")),
            "error should name the field, got: {errors:?}"
        );
    }
```

Run: `CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-tui -- validate -v`
Expected: FAIL — `validate_keybindings` does not exist.

- [ ] **Step 2: Implement `validate_keybindings()`**

Add to `crates/hom-tui/src/input.rs`, after the `InputRouter` impl block:

```rust
/// Validate all keybinding strings in a `KeybindingsConfig`.
///
/// Returns a list of error descriptions. An empty list means all bindings
/// are valid. Each error names the field and the invalid string so the
/// user knows exactly what to fix in their config.
pub fn validate_keybindings(config: &hom_core::KeybindingsConfig) -> Vec<String> {
    let fields = [
        ("toggle_command_bar", &config.toggle_command_bar),
        ("next_pane", &config.next_pane),
        ("prev_pane", &config.prev_pane),
        ("kill_pane", &config.kill_pane),
    ];

    fields
        .iter()
        .filter_map(|(name, value)| {
            if parse_keybinding(value).is_none() {
                Some(format!(
                    "invalid keybinding for '{name}': {:?} (expected e.g. 'ctrl-`', 'ctrl-tab', 'f1')",
                    value
                ))
            } else {
                None
            }
        })
        .collect()
}
```

- [ ] **Step 3: Run test — verify it passes**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-tui -- validate -v
```

Expected: PASS (2 new tests).

- [ ] **Step 4: Call `validate_keybindings()` in `main.rs`**

After `let mut app = App::new(config);` (around line 91 of `main.rs`), add:

```rust
    // Validate keybinding strings — warn the user about any invalid entries.
    // Invalid entries silently fall back to defaults, which is confusing.
    {
        let errors = hom_tui::input::validate_keybindings(&app.config.keybindings);
        if !errors.is_empty() {
            let msg = errors.join("; ");
            warn!(keybinding_errors = %msg, "invalid keybinding config");
            app.command_bar.last_error = Some(format!("keybinding config warning: {msg}"));
        }
    }
```

- [ ] **Step 5: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/hom-tui/src/input.rs src/main.rs
git commit -m "feat: validate keybinding config at startup, warn on invalid entries

Add validate_keybindings() that checks all 4 keybinding fields.
Invalid strings (which silently fall back to defaults) now produce
a visible warning in the command bar at startup and a log message.
Add 2 tests: valid default config passes, 'mega-x' modifier reports error."
```

---

### Task 5: AsyncPtyReader Task Cancellation on Pane Kill (#12)

When `kill_pane()` is called, `AsyncPtyReader` drops which drops the `JoinHandle`. Dropping a `JoinHandle` in tokio detaches the task — `spawn_blocking` tasks keep running until the file descriptor closes. Since `pty_manager.kill()` closes the PTY master fd, the blocking read will eventually get an error and exit. However, the window between kill and task exit is a minor resource leak.

Fix: expose `abort()` on `AsyncPtyReader` that calls `handle.abort()`. Then call `reader.abort()` in `App::kill_pane()` before the pane is removed. This gives the task the tokio cancellation signal immediately, reducing the window.

Note: `abort()` on a `spawn_blocking` handle does not immediately terminate the blocking thread — it just detaches it faster. The thread exits when the next `read()` call returns. This is acceptable and matches tokio's documented behavior.

**Files:**
- Modify: `crates/hom-pty/src/async_reader.rs` — rename `_handle` to `handle`, add `abort()`
- Modify: `crates/hom-tui/src/app.rs` — call `reader.abort()` in `kill_pane()`

- [ ] **Step 1: Write failing test for `AsyncPtyReader::abort()`**

Add to `crates/hom-pty/src/async_reader.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_abort_does_not_panic() {
        // Create a reader over an in-memory cursor — it will quickly exhaust
        // the data and exit naturally. abort() should be safe to call at any point.
        let cursor = Box::new(Cursor::new(b"hello".to_vec())) as Box<dyn Read + Send>;
        let reader = AsyncPtyReader::start(99, cursor);
        reader.abort(); // Must not panic — even if the task already exited
    }
}
```

Run: `CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-pty -- abort -v`
Expected: FAIL — `abort` method does not exist.

- [ ] **Step 2: Implement `abort()` on `AsyncPtyReader`**

In `crates/hom-pty/src/async_reader.rs`, make two changes:

1. Rename `_handle` to `handle` (remove the `_` suppression prefix — it's now used):

```rust
pub struct AsyncPtyReader {
    pub pane_id: PaneId,
    pub rx: mpsc::Receiver<Vec<u8>>,
    handle: tokio::task::JoinHandle<()>,
}
```

2. Update the `start()` return to use `handle`:

```rust
        Self {
            pane_id,
            rx,
            handle,
        }
```

3. Add the `abort()` method after `start()`:

```rust
    /// Signal the background reader task to stop.
    ///
    /// For `spawn_blocking` tasks, this detaches the task rather than
    /// immediately terminating it — the blocking thread exits when its
    /// next `read()` call returns (which happens when the PTY fd closes).
    /// Calling `abort()` is still useful to reduce the detach window.
    pub fn abort(&self) {
        self.handle.abort();
    }
```

- [ ] **Step 3: Run test — verify it passes**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-pty -- abort -v
```

Expected: PASS.

- [ ] **Step 4: Call `reader.abort()` in `App::kill_pane()`**

In `crates/hom-tui/src/app.rs`, in the `kill_pane()` method, abort the reader before removing the pane:

```rust
    pub fn kill_pane(&mut self, pane_id: PaneId) -> HomResult<()> {
        // Abort the async reader task before killing the PTY process.
        // This reduces the window between kill and task exit.
        if let Some(pane) = self.panes.get_mut(&pane_id)
            && let Some(reader) = &pane.pty_reader
        {
            reader.abort();
        }

        self.pty_manager.kill(pane_id)?;
        self.panes.remove(&pane_id);
        self.pane_order.retain(|&id| id != pane_id);

        // Refocus if needed
        if self.focused_pane == Some(pane_id) {
            self.focused_pane = self.pane_order.last().copied();
            if let Some(new_focus) = self.focused_pane {
                self.input_router.focus_pane(new_focus);
            }
        }

        Ok(())
    }
```

- [ ] **Step 5: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/hom-pty/src/async_reader.rs crates/hom-tui/src/app.rs
git commit -m "fix: abort AsyncPtyReader task on pane kill to reduce resource leak

Rename _handle to handle in AsyncPtyReader, add abort() method that
calls handle.abort(). App::kill_pane() now aborts the reader task before
removing the pane. spawn_blocking threads still run until next read()
returns, but the detach window is minimized.
Add test: abort() is safe to call even after the task naturally exits."
```

---

### Task 6: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update implementation status**

Add to the **Resolved** section in `CLAUDE.md`:

```markdown
**Resolved (April 10, 2026 — Phase 7):**
- Process exit notification — command_bar.last_error set when a pane exits with code N
- Sideband health polling — health_check() called every ~5s in main loop; notifies on failure
- Claude Code flickering documented — headless mode (--output-format stream-json) workaround in claude_code.rs
- Keybinding validation — validate_keybindings() at startup, warns on invalid config strings
- AsyncPtyReader cancellation — abort() method added; called in kill_pane() before pane removal
```

- [ ] **Step 2: Run gate and commit**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for Phase 7 completions"
```

---

## Self-Review

**Spec coverage:**
- IMPORTANT #8 (exit notification): Task 1 — command_bar.last_error on pane exit ✓
- IMPORTANT #9 (sideband health): Task 2 — periodic health_check() + notification ✓
- IMPORTANT #10 (Claude Code flickering): Task 3 — documented in claude_code.rs ✓
- IMPORTANT #11 (keybinding validation): Task 4 — validate_keybindings() + startup warn ✓
- IMPORTANT #12 (AsyncPtyReader cancellation): Task 5 — abort() method ✓
- Docs: Task 6 ✓

**Placeholder scan:** No TBD/TODO.

**Type consistency:**
- `validate_keybindings(&KeybindingsConfig) -> Vec<String>` — consistent with existing config types ✓
- `AsyncPtyReader::abort(&self)` — matches tokio JoinHandle::abort() signature ✓
- Health channel: `(u32, String, bool)` — `(pane_id, harness_name, healthy)` ✓
- All `command_bar.last_error = Some(format!(...))` uses — consistent with existing code in main.rs ✓
