# Phase 6: Reliability — Completion Detection, RPC Events, E2E Tests

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the three actionable REQUIRED items: (1) reduce false positives in `detect_completion()` by using harness-specific multi-line patterns anchored to the last non-empty line, (2) implement `RpcSideband::get_events()` to read JSON-RPC notifications non-blockingly, (3) add end-to-end tests that spawn a real process, feed input, and verify output + completion detection through the full PTY→terminal→adapter pipeline.

**Architecture:** Completion detection improvements are per-adapter changes — each adapter gets more specific patterns and error detection. RPC `get_events()` uses `tokio::io::AsyncBufReadExt::read_line` with a zero-duration timeout to non-blockingly read available notification lines. E2E tests spawn `sh -c 'echo ...'` via `PtyManager`, feed output through `Vt100Backend`, and verify `ScreenSnapshot` contents.

**Tech Stack:** Rust 2024, hom-core 0.1, hom-adapters 0.1, hom-pty 0.1, portable-pty 0.9, vt100 0.16, tokio 1.x. Build with `CARGO_TARGET_DIR=/tmp/hom-target`.

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `crates/hom-core/src/traits.rs` | ScreenSnapshot helpers | Modify — add `last_non_empty_line()` method |
| `crates/hom-adapters/src/claude_code.rs` | Claude Code adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/codex.rs` | Codex adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/gemini.rs` | Gemini adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/pi_mono.rs` | pi-mono adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/kimi.rs` | kimi adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/opencode.rs` | OpenCode adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/copilot.rs` | Copilot adapter | Modify — improve detect_completion patterns |
| `crates/hom-adapters/src/sideband/rpc.rs` | RPC sideband | Modify — implement get_events() |

---

### Task 1: Add `last_non_empty_line()` to ScreenSnapshot + Adapter Tests

The current `detect_completion()` implementations use `last_n_lines(3)` which includes blank padding rows from the terminal buffer. A `>` anywhere in those 3 lines (including inside code output) triggers a false positive. The fix: add a `last_non_empty_line()` method that finds the actual last line with content, then match harness-specific prompt patterns anchored to the start of that line.

**Files:**
- Modify: `crates/hom-core/src/traits.rs` — add `last_non_empty_line()`
- Modify: all 7 adapter files — improve `detect_completion()` patterns
- Add: tests in each adapter file

- [ ] **Step 1: Write failing test for `last_non_empty_line()`**

In `crates/hom-core/src/traits.rs`, the `ScreenSnapshot` doesn't have inline tests yet. Add at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_snapshot(lines: &[&str], cols: u16) -> ScreenSnapshot {
        let rows: Vec<Vec<Cell>> = lines
            .iter()
            .map(|line| {
                let mut row: Vec<Cell> = line
                    .chars()
                    .map(|c| Cell {
                        character: c,
                        ..Cell::default()
                    })
                    .collect();
                // Pad to cols width
                while row.len() < cols as usize {
                    row.push(Cell::default());
                }
                row
            })
            .collect();
        let num_rows = rows.len() as u16;
        ScreenSnapshot {
            rows,
            cols,
            num_rows,
            cursor: CursorState::default(),
        }
    }

    #[test]
    fn test_last_non_empty_line_with_trailing_blanks() {
        let snap = make_snapshot(&["hello", "world", "", ""], 10);
        assert_eq!(snap.last_non_empty_line(), "world");
    }

    #[test]
    fn test_last_non_empty_line_all_blank() {
        let snap = make_snapshot(&["", "", ""], 10);
        assert_eq!(snap.last_non_empty_line(), "");
    }

    #[test]
    fn test_last_non_empty_line_with_prompt() {
        let snap = make_snapshot(&["output text", "❯ ", ""], 20);
        assert_eq!(snap.last_non_empty_line(), "❯");
    }
}
```

- [ ] **Step 2: Run test — verify it fails**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-core -- -v
```

Expected: FAIL — `last_non_empty_line` does not exist.

- [ ] **Step 3: Implement `last_non_empty_line()`**

In `crates/hom-core/src/traits.rs`, add to the `impl ScreenSnapshot` block:

```rust
    /// Get the last non-empty line as a trimmed string.
    /// Skips blank trailing rows that are padding from the terminal buffer.
    pub fn last_non_empty_line(&self) -> String {
        for row in self.rows.iter().rev() {
            let line: String = row.iter().map(|c| c.character).collect::<String>();
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        String::new()
    }
```

- [ ] **Step 4: Run tests — verify they pass**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-core -- -v
```

Expected: 3 new tests + existing 6 config tests = 9 pass.

- [ ] **Step 5: Improve `detect_completion()` in all 7 adapters**

The pattern for each adapter: use `last_non_empty_line()` instead of `last_n_lines(3)`, and match harness-specific prompt patterns anchored to the start of the line. Add error detection.

**claude_code.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        if last_line.starts_with('❯') || last_line.starts_with("> ") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error:") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

**codex.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        // Codex shows "$ " or "codex>" when waiting for input
        if last_line.starts_with("$ ") || last_line.starts_with("codex>") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

**gemini.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        // Gemini CLI shows "❯" or "> " at start of prompt line
        if last_line.starts_with('❯') || last_line.starts_with("> ") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("ERROR") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

**pi_mono.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        // pi-mono shows "❯" or "pi>" at its prompt
        if last_line.starts_with('❯') || last_line.starts_with("pi>") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error:") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

**kimi.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        // kimi-cli shows "❯" or "kimi>" at its prompt
        if last_line.starts_with('❯') || last_line.starts_with("kimi>") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

**opencode.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        // OpenCode shows "❯" or "> " at its prompt
        if last_line.starts_with('❯') || last_line.starts_with("> ") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

**copilot.rs** — replace `detect_completion`:

```rust
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        // Copilot CLI shows "$ " or "copilot>" at its prompt
        if last_line.starts_with("$ ") || last_line.starts_with("copilot>") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines,
            }
        } else {
            CompletionStatus::Running
        }
    }
```

- [ ] **Step 6: Add `detect_completion` tests for each adapter**

Add a `#[cfg(test)] mod tests` block to each adapter file. Example for `claude_code.rs` (repeat the pattern for all 7):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use hom_core::traits::{Cell, CursorState, ScreenSnapshot};

    fn make_screen(lines: &[&str]) -> ScreenSnapshot {
        let rows: Vec<Vec<Cell>> = lines
            .iter()
            .map(|line| {
                let mut row: Vec<Cell> = line
                    .chars()
                    .map(|c| Cell { character: c, ..Cell::default() })
                    .collect();
                while row.len() < 80 {
                    row.push(Cell::default());
                }
                row
            })
            .collect();
        // Pad to 24 rows
        let mut all_rows = rows;
        while all_rows.len() < 24 {
            all_rows.push(vec![Cell::default(); 80]);
        }
        ScreenSnapshot {
            cols: 80,
            num_rows: 24,
            rows: all_rows,
            cursor: CursorState::default(),
        }
    }

    #[test]
    fn test_detect_waiting_for_input() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["some output", "❯ "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing files..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_no_false_positive_on_code_output() {
        let adapter = ClaudeCodeAdapter::new();
        // Code that contains > in the middle should NOT trigger completion
        let screen = make_screen(&[
            "if x > 0 {",
            "    println!(\"positive\");",
            "}",
            "Still working...",
        ]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }
}
```

For each of the other 6 adapters, write equivalent tests using their specific prompt pattern (e.g., `"$ "` for codex, `"pi>"` for pi_mono, etc.) and their adapter type (e.g., `CodexAdapter::new()`). The `make_screen` helper can be duplicated in each file (it's test-only and each file is independently testable).

- [ ] **Step 7: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 8: Commit**

```bash
git add crates/hom-core/src/traits.rs crates/hom-adapters/src/*.rs
git commit -m "fix: improve detect_completion with anchored prompt patterns

Add ScreenSnapshot::last_non_empty_line() to skip blank terminal padding.
Replace generic '>' / '$' matching with harness-specific patterns anchored
to start of last non-empty line (e.g., 'codex>', 'pi>', '❯ ').
Add error detection (Failed status) to all 7 adapters.
Add 4 tests per adapter: waiting, running, error, no-false-positive.
Reduces false positive completion detection significantly."
```

---

### Task 2: Implement RPC `get_events()` — Non-Blocking Notification Read

The RPC sideband's `get_events()` always returns an empty Vec. JSON-RPC notifications are lines on stdout without an `id` field. We need to non-blockingly read any available lines and parse them.

**Files:**
- Modify: `crates/hom-adapters/src/sideband/rpc.rs`

- [ ] **Step 1: Write failing test**

Add to the existing `#[cfg(test)] mod tests` in `rpc.rs`:

```rust
    #[test]
    fn test_rpc_sideband_initial_events_empty() {
        let rpc = RpcSideband::new("nonexistent".to_string());
        // Before child is spawned, get_events should return empty
        let rt = tokio::runtime::Runtime::new().unwrap();
        let events = rt.block_on(rpc.get_events()).unwrap();
        assert!(events.is_empty());
    }
```

- [ ] **Step 2: Run test — verify it passes (baseline)**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo test -p hom-adapters rpc -- -v
```

Expected: PASS (existing behavior returns empty).

- [ ] **Step 3: Implement non-blocking `get_events()`**

Replace the `get_events()` method in `rpc.rs`:

```rust
    async fn get_events(&self) -> HomResult<Vec<HarnessEvent>> {
        let stdout_lock = match self.stdout.get() {
            Some(lock) => lock,
            None => return Ok(Vec::new()),
        };

        let mut events = Vec::new();

        // Try to acquire stdout lock without blocking — if busy (send_prompt
        // is reading a response), skip this poll cycle.
        let mut stdout = match stdout_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => return Ok(Vec::new()),
        };

        // Non-blocking read: try to read available lines with zero timeout.
        // JSON-RPC notifications are lines without an "id" field.
        loop {
            let mut line = String::new();
            match tokio::time::timeout(
                std::time::Duration::from_millis(1),
                stdout.read_line(&mut line),
            )
            .await
            {
                Ok(Ok(0)) => break, // EOF
                Ok(Ok(_)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        // Notifications have no "id" field
                        if val.get("id").is_none() {
                            if let Some(method) = val.get("method").and_then(|m| m.as_str()) {
                                match method {
                                    "task_started" => {
                                        let desc = val
                                            .get("params")
                                            .and_then(|p| p.get("description"))
                                            .and_then(|d| d.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        events.push(HarnessEvent::TaskStarted {
                                            description: desc,
                                        });
                                    }
                                    "task_completed" => {
                                        let summary = val
                                            .get("params")
                                            .and_then(|p| p.get("summary"))
                                            .and_then(|s| s.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        events.push(HarnessEvent::TaskCompleted { summary });
                                    }
                                    "error" => {
                                        let message = val
                                            .get("params")
                                            .and_then(|p| p.get("message"))
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        events.push(HarnessEvent::Error { message });
                                    }
                                    _ => {
                                        debug!(method, "unknown RPC notification");
                                    }
                                }
                            }
                        }
                        // Lines with "id" are responses — already handled by send_prompt
                    }
                }
                Ok(Err(_)) => break, // Read error
                Err(_) => break,     // Timeout — no more data available
            }
        }

        Ok(events)
    }
```

- [ ] **Step 4: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 5: Commit**

```bash
git add crates/hom-adapters/src/sideband/rpc.rs
git commit -m "feat: implement RPC get_events() with non-blocking notification read

Replace stub get_events() with real implementation that reads available
JSON-RPC notification lines from stdout using try_lock + 1ms timeout.
Parses task_started, task_completed, and error notifications.
Skips response lines (have id field) — those are handled by send_prompt."
```

---

### Task 3: End-to-End PTY Pipeline Tests

Add tests that spawn a real process through `PtyManager`, read its output through `AsyncPtyReader` + `Vt100Backend`, and verify the full pipeline works.

**Files:**
- Modify: `crates/hom-pty/src/manager.rs` — add e2e test
- Modify: `crates/hom-terminal/src/fallback_vt100.rs` — add e2e test

- [ ] **Step 1: Write PTY spawn + read test**

Add to the existing `#[cfg(test)] mod tests` in `crates/hom-pty/src/manager.rs`:

```rust
    #[test]
    fn test_spawn_and_read_output() {
        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo hello_from_pty".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id = mgr.spawn(&spec, 80, 24).unwrap();

        // Give the process a moment to produce output
        std::thread::sleep(std::time::Duration::from_millis(200));

        // Read output from the PTY reader
        let mut reader = mgr.take_reader(id).unwrap();
        let mut buf = [0u8; 1024];
        // Set non-blocking read with a short timeout
        let n = reader.read(&mut buf).unwrap_or(0);
        let output = String::from_utf8_lossy(&buf[..n]);

        assert!(
            output.contains("hello_from_pty"),
            "expected PTY output to contain 'hello_from_pty', got: {output}"
        );

        mgr.kill_all();
    }

    #[test]
    fn test_spawn_and_write_input() {
        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "cat".to_string(),
            args: vec![],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id = mgr.spawn(&spec, 80, 24).unwrap();

        // Write to stdin
        mgr.write_to(id, b"test_input\n").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(200));

        // cat echoes input back
        let mut reader = mgr.take_reader(id).unwrap();
        let mut buf = [0u8; 1024];
        let n = reader.read(&mut buf).unwrap_or(0);
        let output = String::from_utf8_lossy(&buf[..n]);

        assert!(
            output.contains("test_input"),
            "expected echo of 'test_input', got: {output}"
        );

        mgr.kill_all();
    }
```

- [ ] **Step 2: Write full pipeline test (PTY → terminal emulator → screen)**

Add to `crates/hom-terminal/src/fallback_vt100.rs` existing `mod tests`:

```rust
    #[test]
    fn test_pty_to_terminal_pipeline() {
        use hom_core::CommandSpec;
        use hom_pty::PtyManager;

        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo PIPELINE_TEST_OUTPUT".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id = mgr.spawn(&spec, 80, 24).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(300));

        // Read PTY output
        let mut reader = mgr.take_reader(id).unwrap();
        let mut buf = [0u8; 4096];
        let n = std::io::Read::read(&mut reader, &mut buf).unwrap_or(0);

        // Feed into terminal emulator
        let mut term = Vt100Backend::new(80, 24, 100);
        term.process(&buf[..n]);

        // Verify it appears in the screen snapshot
        let snap = term.screen_snapshot();
        let text = snap.text();
        assert!(
            text.contains("PIPELINE_TEST_OUTPUT"),
            "expected 'PIPELINE_TEST_OUTPUT' in terminal snapshot, got: {text}"
        );

        mgr.kill_all();
    }
```

- [ ] **Step 3: Run full gate**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
```

- [ ] **Step 4: Commit**

```bash
git add crates/hom-pty/src/manager.rs crates/hom-terminal/src/fallback_vt100.rs
git commit -m "test: add end-to-end PTY pipeline tests

Test spawn→read (echo), spawn→write→read (cat), and the full
PTY→Vt100Backend→ScreenSnapshot pipeline. Verifies real process
output appears correctly in the terminal emulator's screen buffer."
```

---

### Task 4: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update implementation status**

Add to the Phase 5 resolved section or create Phase 6:

```markdown
**Resolved (April 10, 2026 — Phase 6):**
- detect_completion() improved — anchored to last non-empty line, harness-specific patterns, error detection
- RPC get_events() implemented — non-blocking JSON-RPC notification parsing
- End-to-end PTY pipeline tests — spawn→read, spawn→write→read, PTY→terminal→screen
```

- [ ] **Step 2: Run gate and commit**

```bash
CARGO_TARGET_DIR=/tmp/hom-target cargo fmt --all && \
CARGO_TARGET_DIR=/tmp/hom-target cargo clippy --all-targets --all-features -- -D warnings && \
CARGO_TARGET_DIR=/tmp/hom-target cargo test --workspace
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for Phase 6 completions"
```

---

## Self-Review

**Spec coverage:**
- REQUIRED 1 (detect_completion): Task 1 — anchored patterns + tests ✓
- REQUIRED 2 (RPC get_events): Task 2 — non-blocking notification read ✓
- REQUIRED 3 (E2E tests): Task 3 — real PTY spawn + pipeline test ✓
- REQUIRED 4 (GhosttyBackend): Blocked on external dep — not in plan (correct) ✓
- Docs: Task 4 ✓

**Placeholder scan:** No TBD/TODO/placeholders.

**Type consistency:**
- `last_non_empty_line()` → `String` — used in all 7 adapters ✓
- `make_screen()` test helper — duplicated per adapter file (intentional — test isolation) ✓
- `CompletionStatus::Failed { error }` — all adapters produce it consistently ✓
- `HarnessEvent::TaskStarted/TaskCompleted/Error` — matches existing enum variants in types.rs ✓
