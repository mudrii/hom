---
name: hom-tui-testing
description: Use when modifying TUI rendering, input routing, layout, command bar, or pane management
---

# HOM TUI Testing

## When to Use

Invoke this skill when:
- Modifying pane rendering in `crates/hom-tui/src/pane_render.rs`
- Changing layout logic in `crates/hom-tui/src/layout.rs`
- Adding or modifying commands in `crates/hom-tui/src/command_bar.rs`
- Changing input routing in `crates/hom-tui/src/input.rs`
- Modifying the main render pipeline in `crates/hom-tui/src/render.rs`
- Touching the `App` state machine in `crates/hom-tui/src/app.rs`

## Architecture Context

The TUI has a clear data flow:

```
User Input → InputRouter → Action → App state mutation → render() → Frame
                                                              ↓
                              pane.terminal.screen_snapshot() → pane_render → ratatui Buffer
```

Breaking any link in this chain means the user sees nothing, gets stuck, or loses input.

## Testing Approach

### Unit-testable components (test these directly)

**Command bar parsing** — Pure function, no side effects:
```rust
#[test]
fn test_parse_spawn_command() {
    let cmd = parse_command("spawn claude opus");
    assert!(matches!(cmd, Ok(Command::Spawn { harness: HarnessType::ClaudeCode, .. })));
}

#[test]
fn test_parse_unknown_harness() {
    let cmd = parse_command("spawn nonexistent");
    assert!(cmd.is_err());
}
```

**Layout computation** — Pure geometry:
```rust
#[test]
fn test_grid_layout_4_panes() {
    let area = Rect::new(0, 0, 100, 50);
    let panes = vec![1, 2, 3, 4];
    let areas = compute_pane_areas(area, &panes, &LayoutKind::Grid);
    assert_eq!(areas.len(), 4);
    // 2x2 grid: each pane gets ~50x25
    for (_, rect) in &areas {
        assert!(rect.width >= 49);
        assert!(rect.height >= 24);
    }
}
```

**Input encoding** — Verify key events produce correct PTY bytes:
```rust
#[test]
fn test_ctrl_c_encoding() {
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let bytes = encode_key_event(&key);
    assert_eq!(bytes, vec![0x03]);
}
```

**Pane hit testing** — Click coordinates → pane ID:
```rust
#[test]
fn test_pane_at_position() {
    let pane_areas = vec![
        (1, Rect::new(0, 0, 50, 25)),
        (2, Rect::new(50, 0, 50, 25)),
    ];
    assert_eq!(pane_at_position(&pane_areas, 25, 12), Some(1));
    assert_eq!(pane_at_position(&pane_areas, 75, 12), Some(2));
}
```

### Integration-testable (use ratatui's TestBackend)

**Full render cycle** — Create App, add panes, render to TestBackend, assert buffer contents:
```rust
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn test_empty_state_renders_welcome() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let app = App::new(HomConfig::default());
    terminal.draw(|f| render(f, &app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    // Check that "Welcome to HOM" appears in the buffer
    let text: String = buf.content().iter().map(|c| c.symbol().chars().next().unwrap_or(' ')).collect();
    assert!(text.contains("Welcome to HOM"));
}
```

### Not unit-testable (manual verification required)

- Actual terminal rendering appearance (colors, cursor position)
- PTY interaction timing (race conditions)
- Claude Code flickering behavior
- Mouse event handling with real terminal

For these, use `cargo run` and test manually. Document what you tested.

## Common Pitfalls

| Pitfall | Why it breaks | Prevention |
|---------|---------------|------------|
| Off-by-one in pane border rendering | Pane content overflows into border or adjacent pane | Always use `block.inner(area)` for content area |
| Forgetting to handle empty pane list | Index panic in layout computation | Guard with `if pane_ids.is_empty() { return }` |
| Cursor position outside visible area | Terminal shows cursor in wrong pane | Clamp cursor to `inner.right()` / `inner.bottom()` |
| Command bar eating Escape key | User can't exit command mode | Test mode transitions explicitly |
| InputRouter not updating mode | Clicks do nothing, keys go to wrong pane | Every Action that changes focus must also update `self.mode` |

## Checklist Before Committing

- [ ] Command bar parsing tests cover new/modified commands
- [ ] Layout tests verify pane areas for 1, 2, 4, 7 pane counts
- [ ] Input encoding tests cover special keys (Ctrl, F-keys, arrows)
- [ ] `cargo test -p hom-tui` passes
- [ ] Manual test: `cargo run`, spawn a pane, type into it, switch panes
- [ ] `cargo clippy` clean
