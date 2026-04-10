# P4: Mouse Passthrough, Adapter Smoke Tests, GhosttyBackend CI Strategy

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three independent improvements: (1) forward non-focus mouse events from HOM to PTY processes using X10 encoding so harnesses can react to scroll/click; (2) add missing `build_command` + `translate_input` unit tests and `AdapterRegistry` smoke tests to hom-adapters; (3) document and wire a GhosttyBackend CI job for self-hosted runners so ghostty-backend is validated in CI.

**Architecture:** Mouse passthrough adds an `encode_mouse_event` function to `input.rs` and a new match arm in `handle_event` that fires for `PaneInput` mode + any mouse event — left-clicking a different pane still switches focus, all other events are encoded X10 and forwarded via `WriteToPty`. Adapter tests are pure unit tests (no PTY, no async): they feed `HarnessConfig` into `build_command`/`translate_input` and assert on `CommandSpec`/`Vec<u8>`. GhosttyBackend CI is a single `ghostty` job in `ci.yml` gated on a `self-hosted` runner label, plus a `scripts/seed-zig-cache.sh` helper for first-time runner provisioning.

**Tech Stack:** Rust 2024, crossterm 0.29 (`EnableMouseCapture`, `DisableMouseCapture`, `MouseEventKind`), hom-tui, hom-adapters, GitHub Actions.

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `src/main.rs` | Terminal setup / teardown | Modify — add `EnableMouseCapture` / `DisableMouseCapture` |
| `crates/hom-tui/src/input.rs` | Input routing, key encoding | Modify — add `encode_mouse_event`, new mouse arm in `handle_event` |
| `crates/hom-adapters/src/claude_code.rs` | Claude Code adapter | Modify — add `build_command` + `translate_input` tests |
| `crates/hom-adapters/src/codex.rs` | Codex adapter | Modify — add `build_command` + `translate_input` tests |
| `crates/hom-adapters/src/lib.rs` | AdapterRegistry | Modify — add registry smoke tests |
| `.github/workflows/ci.yml` | CI pipeline | Modify — add `ghostty` job for self-hosted runner |
| `scripts/seed-zig-cache.sh` | Runner provisioning helper | Create — seeds Zig package cache on new self-hosted runner |

---

### Task 1: `encode_mouse_event` + enable mouse capture in terminal setup

The terminal currently does NOT call `EnableMouseCapture`, so crossterm never forwards mouse events to HOM beyond left-click focus switching. This task adds the crossterm calls and the encoding function.

X10 mouse protocol format: `ESC [ M <Cb> <Cx> <Cy>` where:
- `Cb` = button_code + 32 + modifier_flags (Shift=4, Alt=8, Ctrl=16)
- `Cx` = (1-based column) + 32 = col + 33
- `Cy` = (1-based row) + 32 = row + 33
- Button codes: Left=0, Middle=1, Right=2, Release=3, ScrollUp=64, ScrollDown=65
- `Moved` / `Drag` → return empty Vec (not forwarded)

**Files:**
- Modify: `src/main.rs`
- Modify: `crates/hom-tui/src/input.rs`

- [ ] **Step 1: Write failing tests for `encode_mouse_event`**

Add to the `tests` mod in `crates/hom-tui/src/input.rs` (at the end of the existing `mod tests` block):

```rust
#[test]
fn test_encode_mouse_left_click_origin() {
    // col=0, row=0, no mods → Cb=32, Cx=33, Cy=33
    let bytes = encode_mouse_event(
        &MouseEventKind::Down(MouseButton::Left),
        0, 0,
        KeyModifiers::empty(),
    );
    assert_eq!(bytes, vec![0x1b, b'[', b'M', 32, 33, 33]);
}

#[test]
fn test_encode_mouse_scroll_up() {
    // ScrollUp at col=4, row=2, no mods → Cb=64+32=96, Cx=4+33=37, Cy=2+33=35
    let bytes = encode_mouse_event(
        &MouseEventKind::ScrollUp,
        4, 2,
        KeyModifiers::empty(),
    );
    assert_eq!(bytes, vec![0x1b, b'[', b'M', 96, 37, 35]);
}

#[test]
fn test_encode_mouse_scroll_down() {
    // ScrollDown at col=0, row=0 → Cb=65+32=97
    let bytes = encode_mouse_event(
        &MouseEventKind::ScrollDown,
        0, 0,
        KeyModifiers::empty(),
    );
    assert_eq!(bytes, vec![0x1b, b'[', b'M', 97, 33, 33]);
}

#[test]
fn test_encode_mouse_release() {
    // Up(Left) → button code 3, Cb=3+32=35
    let bytes = encode_mouse_event(
        &MouseEventKind::Up(MouseButton::Left),
        0, 0,
        KeyModifiers::empty(),
    );
    assert_eq!(bytes, vec![0x1b, b'[', b'M', 35, 33, 33]);
}

#[test]
fn test_encode_mouse_ctrl_modifier() {
    // Left+Ctrl → Cb = 0 + 32 | 16 = 48
    let bytes = encode_mouse_event(
        &MouseEventKind::Down(MouseButton::Left),
        0, 0,
        KeyModifiers::CONTROL,
    );
    assert_eq!(bytes, vec![0x1b, b'[', b'M', 48, 33, 33]);
}

#[test]
fn test_encode_mouse_moved_returns_empty() {
    let bytes = encode_mouse_event(
        &MouseEventKind::Moved,
        5, 5,
        KeyModifiers::empty(),
    );
    assert!(bytes.is_empty(), "Moved events must not be forwarded");
}

#[test]
fn test_encode_mouse_right_click() {
    // Right → button code 2, Cb=2+32=34
    let bytes = encode_mouse_event(
        &MouseEventKind::Down(MouseButton::Right),
        0, 0,
        KeyModifiers::empty(),
    );
    assert_eq!(bytes, vec![0x1b, b'[', b'M', 34, 33, 33]);
}
```

- [ ] **Step 2: Run test to verify it fails**

```sh
cargo test -p hom-tui encode_mouse
```

Expected: `error[E0425]: cannot find function encode_mouse_event in module super`

- [ ] **Step 3: Implement `encode_mouse_event`**

Add after `encode_key_event` in `crates/hom-tui/src/input.rs` (before the `#[cfg(test)]` line):

```rust
/// Encode a mouse event into X10 mouse protocol bytes for a PTY.
///
/// Format: `ESC [ M <Cb> <Cx> <Cy>` (6 bytes total)
/// - Cb = button_code OR modifier_flags, then add 32
/// - Cx = (col + 1) + 32  (1-based, +32 offset)
/// - Cy = (row + 1) + 32  (1-based, +32 offset)
///
/// col/row are 0-based pane-relative coordinates.
/// Returns empty Vec for Moved/Drag events (not forwarded).
pub fn encode_mouse_event(
    kind: &MouseEventKind,
    col: u16,
    row: u16,
    modifiers: KeyModifiers,
) -> Vec<u8> {
    let button_code: u8 = match kind {
        MouseEventKind::Down(MouseButton::Left) => 0,
        MouseEventKind::Down(MouseButton::Middle) => 1,
        MouseEventKind::Down(MouseButton::Right) => 2,
        MouseEventKind::Up(_) => 3,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::Moved | MouseEventKind::Drag(_) => return Vec::new(),
        _ => return Vec::new(),
    };

    let mut cb = button_code + 32;
    if modifiers.contains(KeyModifiers::SHIFT) {
        cb |= 4;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        cb |= 8;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        cb |= 16;
    }

    // Coordinates are 1-based; clamped to 1–223 so the byte stays in 33–255.
    let cx = ((col as u32 + 1).min(223) as u8) + 32;
    let cy = ((row as u32 + 1).min(223) as u8) + 32;

    vec![0x1b, b'[', b'M', cb, cx, cy]
}
```

- [ ] **Step 4: Add `EnableMouseCapture` / `DisableMouseCapture` to `src/main.rs`**

Change the import block (lines 12–16) from:

```rust
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
```

to:

```rust
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
```

Change the terminal setup (the `execute!(stdout, EnterAlternateScreen)?;` line) to:

```rust
execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
```

Change the teardown in the cleanup block from:

```rust
disable_raw_mode()?;
execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
terminal.show_cursor()?;
```

to:

```rust
disable_raw_mode()?;
execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
terminal.show_cursor()?;
```

- [ ] **Step 5: Run tests and check**

```sh
cargo test -p hom-tui encode_mouse
```

Expected: 7 passing tests

```sh
cargo check --workspace
```

Expected: zero errors, zero warnings

- [ ] **Step 6: Commit**

```sh
git add crates/hom-tui/src/input.rs src/main.rs
git commit -m "feat: add X10 mouse encoding + enable mouse capture"
```

---

### Task 2: Mouse event forwarding in `InputRouter::handle_event`

Wire up the new `encode_mouse_event` function so that non-focus mouse events are forwarded to the focused pane's PTY. Left-clicking a *different* pane still switches focus (same as today). Left-clicking the *current* pane forwards the click to the PTY.

The new match arm must come **before** the existing `(_, Event::Mouse(Down(Left), ...))` catch-all arm to take priority when in `PaneInput` mode.

**Files:**
- Modify: `crates/hom-tui/src/input.rs`

- [ ] **Step 1: Write failing tests**

Add to `mod tests` in `crates/hom-tui/src/input.rs`:

```rust
fn make_two_pane_areas() -> Vec<(PaneId, ratatui::layout::Rect)> {
    use ratatui::layout::Rect;
    vec![
        (1, Rect { x: 0,  y: 0, width: 40, height: 24 }),
        (2, Rect { x: 40, y: 0, width: 40, height: 24 }),
    ]
}

#[test]
fn test_scroll_up_on_focused_pane_writes_to_pty() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let mut router = InputRouter::new();
    router.focus_pane(1);
    let areas = make_two_pane_areas();
    let event = Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 10, // inside pane 1 (x: 0..40)
        row: 5,
        modifiers: KeyModifiers::empty(),
    });
    let action = router.handle_event(event, &areas);
    // pane-relative col = 10 - (0+1) = 9, row = 5 - (0+1) = 4
    // Cb=96, Cx=(9+1)+32=42, Cy=(4+1)+32=37
    assert!(
        matches!(action, Action::WriteToPty(1, _)),
        "expected WriteToPty(1, ...), got: {action:?}"
    );
    if let Action::WriteToPty(_, bytes) = action {
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 96, 42, 37]);
    }
}

#[test]
fn test_left_click_on_different_pane_switches_focus() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut router = InputRouter::new();
    router.focus_pane(1);
    let areas = make_two_pane_areas();
    let event = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 45, // inside pane 2 (x: 40..80)
        row: 5,
        modifiers: KeyModifiers::empty(),
    });
    let action = router.handle_event(event, &areas);
    assert!(
        matches!(action, Action::FocusPane(2)),
        "expected FocusPane(2), got: {action:?}"
    );
    assert!(
        matches!(router.mode, InputMode::PaneInput { focused: 2 }),
        "mode should be PaneInput {{ focused: 2 }}"
    );
}

#[test]
fn test_left_click_on_focused_pane_writes_to_pty() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut router = InputRouter::new();
    router.focus_pane(1);
    let areas = make_two_pane_areas();
    let event = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5, // inside pane 1
        row: 5,
        modifiers: KeyModifiers::empty(),
    });
    let action = router.handle_event(event, &areas);
    // pane-relative col=5-(0+1)=4, row=5-(0+1)=4
    // Cb=32 (left+no mods), Cx=(4+1)+32=37, Cy=(4+1)+32=37
    assert!(
        matches!(action, Action::WriteToPty(1, _)),
        "expected WriteToPty(1, ...), got: {action:?}"
    );
}

#[test]
fn test_mouse_move_in_pane_input_produces_no_action() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let mut router = InputRouter::new();
    router.focus_pane(1);
    let areas = make_two_pane_areas();
    let event = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 5,
        row: 5,
        modifiers: KeyModifiers::empty(),
    });
    let action = router.handle_event(event, &areas);
    assert!(
        matches!(action, Action::None),
        "Moved events should not produce actions, got: {action:?}"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

```sh
cargo test -p hom-tui test_scroll_up_on_focused test_left_click_on_different test_left_click_on_focused test_mouse_move_in_pane
```

Expected: 4 failures (mouse events still only handled by the existing Left-Down catch-all)

- [ ] **Step 3: Add the new mouse forwarding arm in `handle_event`**

In `crates/hom-tui/src/input.rs`, inside the `handle_event` function, find the existing mouse click arm:

```rust
// ── Mouse click focuses a pane ────────────────────────
(
    _,
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        ..
    }),
) => {
```

Insert the following new arm **immediately before** it (keep the existing arm unchanged):

```rust
// ── In pane mode, forward all mouse events to the focused PTY ─────
// Exception: a left-click landing on a *different* pane switches focus
// instead of forwarding (same behaviour as tmux click-to-focus).
(InputMode::PaneInput { .. }, Event::Mouse(mouse_evt)) => {
    // Copy focused_id without retaining a reference into self.mode,
    // so we can safely reassign self.mode below if needed.
    let focused_id = if let InputMode::PaneInput { focused } = &self.mode {
        *focused
    } else {
        return Action::None;
    };

    // Left-click outside the focused pane → switch focus.
    if matches!(mouse_evt.kind, MouseEventKind::Down(MouseButton::Left)) {
        if let Some(clicked) =
            super::layout::pane_at_position(pane_areas, mouse_evt.column, mouse_evt.row)
        {
            if clicked != focused_id {
                self.mode = InputMode::PaneInput { focused: clicked };
                return Action::FocusPane(clicked);
            }
        }
    }

    // All other events → encode X10 and forward to PTY.
    let focused_area = pane_areas
        .iter()
        .find(|(id, _)| *id == focused_id)
        .map(|(_, a)| *a);
    let bytes = match focused_area {
        Some(area) => encode_mouse_event(
            &mouse_evt.kind,
            mouse_evt.column.saturating_sub(area.x + 1),
            mouse_evt.row.saturating_sub(area.y + 1),
            mouse_evt.modifiers,
        ),
        None => Vec::new(),
    };
    if bytes.is_empty() {
        Action::None
    } else {
        Action::WriteToPty(focused_id, bytes)
    }
}
```

- [ ] **Step 4: Verify tests pass**

```sh
cargo test -p hom-tui
```

Expected: all tests pass, including the 4 new ones

```sh
cargo clippy -p hom-tui -- -D warnings
```

Expected: zero warnings

- [ ] **Step 5: Commit**

```sh
git add crates/hom-tui/src/input.rs
git commit -m "feat: forward mouse events to focused PTY via X10 encoding"
```

---

### Task 3: Adapter smoke tests — `build_command`, `translate_input`, `AdapterRegistry`

Currently, each adapter only has `detect_completion` tests. `build_command` and `translate_input` are the primary public contract methods and have zero test coverage. The `AdapterRegistry` has no tests at all — we can't verify all 7 adapters actually register.

**Files:**
- Modify: `crates/hom-adapters/src/claude_code.rs`
- Modify: `crates/hom-adapters/src/codex.rs`
- Modify: `crates/hom-adapters/src/lib.rs`

- [ ] **Step 1: Write failing tests for `ClaudeCodeAdapter::build_command` and `translate_input`**

Add to the existing `mod tests` in `crates/hom-adapters/src/claude_code.rs`, after the last existing test:

```rust
    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::ClaudeCode, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = ClaudeCodeAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "claude");
        assert!(spec.args.is_empty(), "no args when no model or extra_args");
    }

    #[test]
    fn test_build_command_with_model() {
        let adapter = ClaudeCodeAdapter::new();
        let config = default_config().with_model("claude-opus-4-6");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.program, "claude");
        assert_eq!(spec.args, vec!["--model", "claude-opus-4-6"]);
    }

    #[test]
    fn test_build_command_with_binary_override() {
        let adapter = ClaudeCodeAdapter::new();
        let mut config = default_config();
        config.binary_override = Some("/usr/local/bin/claude".to_string());
        let spec = adapter.build_command(&config);
        assert_eq!(spec.program, "/usr/local/bin/claude");
    }

    #[test]
    fn test_build_command_extra_args_appended_after_model() {
        let adapter = ClaudeCodeAdapter::new();
        let mut config = default_config().with_model("opus");
        config.extra_args = vec!["--no-auto-update".to_string()];
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "opus", "--no-auto-update"]);
    }

    #[test]
    fn test_translate_prompt() {
        let adapter = ClaudeCodeAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("hello".to_string()));
        assert_eq!(bytes, b"hello\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = ClaudeCodeAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Cancel);
        assert_eq!(bytes, vec![0x03]);
    }

    #[test]
    fn test_translate_accept() {
        let adapter = ClaudeCodeAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Accept);
        assert_eq!(bytes, b"y\n");
    }

    #[test]
    fn test_translate_reject() {
        let adapter = ClaudeCodeAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Reject);
        assert_eq!(bytes, b"n\n");
    }

    #[test]
    fn test_translate_raw_passthrough() {
        let adapter = ClaudeCodeAdapter::new();
        let payload = vec![0x1b, b'[', b'A'];
        let bytes = adapter.translate_input(&OrchestratorCommand::Raw(payload.clone()));
        assert_eq!(bytes, payload);
    }
```

- [ ] **Step 2: Write failing tests for `CodexAdapter::build_command` and `translate_input`**

Add to the existing `mod tests` in `crates/hom-adapters/src/codex.rs`, after the last existing test:

```rust
    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::CodexCli, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = CodexAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "codex");
        assert!(spec.args.is_empty());
    }

    #[test]
    fn test_build_command_with_model() {
        let adapter = CodexAdapter::new();
        let config = default_config().with_model("o3");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "o3"]);
    }

    #[test]
    fn test_build_command_extra_args() {
        let adapter = CodexAdapter::new();
        let mut config = default_config();
        config.extra_args = vec!["--quiet".to_string()];
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--quiet"]);
    }

    #[test]
    fn test_translate_prompt() {
        let adapter = CodexAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("fix it".to_string()));
        assert_eq!(bytes, b"fix it\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = CodexAdapter::new();
        assert_eq!(adapter.translate_input(&OrchestratorCommand::Cancel), vec![0x03]);
    }

    #[test]
    fn test_translate_accept() {
        let adapter = CodexAdapter::new();
        assert_eq!(adapter.translate_input(&OrchestratorCommand::Accept), b"y\n");
    }

    #[test]
    fn test_translate_reject() {
        let adapter = CodexAdapter::new();
        assert_eq!(adapter.translate_input(&OrchestratorCommand::Reject), b"n\n");
    }
```

- [ ] **Step 3: Write failing tests for `AdapterRegistry`**

Add at the end of `crates/hom-adapters/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_all_seven_harnesses() {
        let registry = AdapterRegistry::new();
        let expected = [
            HarnessType::ClaudeCode,
            HarnessType::CodexCli,
            HarnessType::GeminiCli,
            HarnessType::PiMono,
            HarnessType::KimiCli,
            HarnessType::OpenCode,
            HarnessType::CopilotCli,
        ];
        for harness in &expected {
            assert!(
                registry.get(harness).is_some(),
                "AdapterRegistry missing adapter for {harness:?}"
            );
        }
    }

    #[test]
    fn test_registry_available_returns_seven() {
        let registry = AdapterRegistry::new();
        assert_eq!(
            registry.available().len(),
            7,
            "expected 7 registered adapters"
        );
    }

    #[test]
    fn test_registry_adapter_display_name_non_empty() {
        let registry = AdapterRegistry::new();
        for harness in registry.available() {
            let adapter = registry.get(&harness).unwrap();
            assert!(
                !adapter.display_name().is_empty(),
                "display_name() is empty for {harness:?}"
            );
        }
    }

    #[test]
    fn test_registry_adapter_harness_type_matches_key() {
        let registry = AdapterRegistry::new();
        for harness in registry.available() {
            let adapter = registry.get(&harness).unwrap();
            assert_eq!(
                adapter.harness_type(),
                harness,
                "adapter registered under {harness:?} returns wrong harness_type()"
            );
        }
    }
}
```

- [ ] **Step 4: Run tests to confirm they fail**

```sh
cargo test -p hom-adapters 2>&1 | head -30
```

Expected: The tests that reference `default_config()` etc. compile and pass since the implementation already exists (we're adding tests for existing functions — they should pass immediately). Verify they pass:

```sh
cargo test -p hom-adapters
```

Expected: all tests pass (the implementations are already correct — these tests verify the contract)

- [ ] **Step 5: Run clippy**

```sh
cargo clippy -p hom-adapters -- -D warnings
```

Expected: zero warnings

- [ ] **Step 6: Commit**

```sh
git add crates/hom-adapters/src/claude_code.rs crates/hom-adapters/src/codex.rs crates/hom-adapters/src/lib.rs
git commit -m "test: add build_command, translate_input, and registry smoke tests for adapters"
```

---

### Task 4: GhosttyBackend CI job + Zig cache seed script

GhosttyBackend can only be validated on a runner with Zig ≥0.15.x installed and initial network access to `deps.files.ghostty.org` (Zig's package manager fetches C sources on first build; subsequent builds use the local cache). This task adds a CI job that fires when a `zig` self-hosted runner is available, and a `seed-zig-cache.sh` script for one-time runner provisioning.

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `scripts/seed-zig-cache.sh`

- [ ] **Step 1: Create `scripts/seed-zig-cache.sh`**

```bash
#!/usr/bin/env bash
# scripts/seed-zig-cache.sh
#
# One-time helper: build hom-terminal with the ghostty-backend feature on a
# machine with internet access so Zig downloads and caches its C source
# packages in ~/.cache/zig/p/.
#
# Run this once on a new self-hosted CI runner (or developer machine) BEFORE
# going offline or before first-time CI use. After seeding, the cache can be
# saved/restored via GitHub Actions cache to avoid repeated network hits.
#
# Prerequisites:
#   - Zig ≥ 0.15.x  (zig version)
#   - Rust stable    (cargo version)
#   - Network access to deps.files.ghostty.org

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

echo "=== HOM: Seeding Zig package cache for ghostty-backend ==="
echo ""
echo "Zig version: $(zig version)"
echo "Cargo version: $(cargo --version)"
echo ""
echo "Building hom-terminal --features ghostty-backend ..."
echo "(First build downloads C sources from deps.files.ghostty.org)"
echo ""

cargo build --features ghostty-backend -p hom-terminal

echo ""
echo "=== Zig cache seeded ==="
ZIG_CACHE="${HOME}/.cache/zig/p"
if [ -d "$ZIG_CACHE" ]; then
    echo "Zig packages at: ${ZIG_CACHE}"
    ls "$ZIG_CACHE"
else
    echo "Note: Zig cache not found at ${ZIG_CACHE} — check your Zig version."
fi
echo ""
echo "Next step: set up a GitHub Actions self-hosted runner with the 'zig' label"
echo "and ensure this cache path is preserved between runs (or use actions/cache)."
```

Make it executable. Since this is a plan step and you're creating the file, run:

```sh
chmod +x scripts/seed-zig-cache.sh
```

- [ ] **Step 2: Add the `ghostty` CI job to `.github/workflows/ci.yml`**

Append the following job at the end of the `jobs:` section in `.github/workflows/ci.yml`:

```yaml
  # ── GhosttyBackend (self-hosted, requires Zig ≥0.15.x) ───────────────
  # Runs only on runners labelled 'zig'. These runners must have:
  #   1. Zig ≥0.15.x in PATH
  #   2. Network access OR a pre-seeded ~/.cache/zig/p/ (see scripts/seed-zig-cache.sh)
  # To register a runner: Settings → Actions → Runners → New self-hosted runner
  # Add the runner labels: self-hosted, zig
  ghostty:
    name: ghostty-backend (self-hosted)
    runs-on: [self-hosted, zig]
    if: ${{ !cancelled() }}
    steps:
      - uses: actions/checkout@v4
      - name: Verify Zig version
        run: zig version
      - uses: Swatinem/rust-cache@v2
        with:
          key: ghostty
      - name: Restore Zig package cache
        uses: actions/cache@v4
        with:
          path: ~/.cache/zig/p
          key: zig-pkg-${{ runner.os }}-${{ hashFiles('Cargo.lock') }}
          restore-keys: |
            zig-pkg-${{ runner.os }}-
      - name: Build ghostty-backend
        run: cargo build --features ghostty-backend -p hom-terminal
      - name: Test ghostty-backend
        run: cargo test --features ghostty-backend -p hom-terminal
      - name: Clippy ghostty-backend
        run: cargo clippy --features ghostty-backend -p hom-terminal -- -D warnings
```

- [ ] **Step 3: Verify the existing CI (no self-hosted runner) still passes**

```sh
cargo check --workspace
cargo test --workspace
```

Expected: zero errors (the ghostty job only fires on self-hosted runners — it never blocks the standard CI green path)

- [ ] **Step 4: Commit**

```sh
git add .github/workflows/ci.yml scripts/seed-zig-cache.sh
git commit -m "ci: add ghostty-backend self-hosted job + seed-zig-cache.sh"
```

---

## Self-Review

### Spec coverage

- Mouse passthrough: ✅ X10 encoding function (Task 1), terminal mouse capture (Task 1), forwarding + focus-switch logic (Task 2)
- Adapter smoke tests: ✅ `build_command` + `translate_input` for ClaudeCode and Codex (Task 3), AdapterRegistry all-7 + display_name + harness_type consistency (Task 3)
- GhosttyBackend CI: ✅ self-hosted job with Zig cache (Task 4), seed script (Task 4)

### Placeholder scan

No TBD, TODO, "implement later", or "similar to Task N" references in the code blocks above. All step code is complete and self-contained.

### Type consistency

- `encode_mouse_event` signature: `(kind: &MouseEventKind, col: u16, row: u16, modifiers: KeyModifiers) -> Vec<u8>` — used identically in Task 1 (implementation), Task 1 (tests), and Task 2 (forwarding arm call)
- `HarnessConfig::new(HarnessType::ClaudeCode, ".".into())` — matches `HarnessConfig::new(harness_type: HarnessType, working_dir: PathBuf)` from `crates/hom-core/src/types.rs:94`
- `HarnessConfig::with_model` — builder method at `types.rs:105`, returns `Self`
- `HarnessConfig::binary_override: Option<String>` — field at `types.rs:90`
- `OrchestratorCommand::{Prompt, Cancel, Accept, Reject, Raw}` — all variants confirmed in hom-core
- `AdapterRegistry::{new, get, available}` — all confirmed at `hom-adapters/src/lib.rs:38-73`
