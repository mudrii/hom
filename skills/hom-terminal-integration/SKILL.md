---
name: hom-terminal-integration
description: Use when working on terminal emulation, the TerminalBackend trait, libghostty integration, or PTY management
---

# HOM Terminal Integration

## When to Use

Invoke this skill when:
- Implementing or modifying `GhosttyBackend` in `crates/hom-terminal/src/ghostty.rs`
- Modifying `Vt100Backend` in `crates/hom-terminal/src/fallback_vt100.rs`
- Changing the `TerminalBackend` trait in `crates/hom-core/src/traits.rs`
- Working on PTY management in `crates/hom-pty/src/manager.rs`
- Modifying the async PTY reader in `crates/hom-pty/src/async_reader.rs`
- Debugging terminal rendering issues (wrong colors, missing characters, cursor position)
- Enabling the `ghostty-backend` feature flag

## Architecture: TUI-inside-TUI

This is the hardest part of HOM. Each pane runs a REAL terminal emulator:

```
Harness Process (e.g., claude)
    ↓ writes ANSI escape sequences to stdout
PTY slave → PTY master
    ↓ raw bytes
AsyncPtyReader (tokio channel)
    ↓ Vec<u8> chunks
terminal.process(bytes)    ← TerminalBackend implementation
    ↓ updates internal screen state
terminal.screen_snapshot() → ScreenSnapshot { rows: Vec<Vec<Cell>> }
    ↓ cell-by-cell mapping
pane_render.rs → ratatui Buffer
    ↓
ratatui renders to real terminal
```

Every byte from the harness flows through this pipeline. A bug anywhere means garbled output.

## The TerminalBackend Trait

Defined in `crates/hom-core/src/traits.rs`. Any terminal emulator must implement:

| Method | What it does | Failure mode |
|--------|-------------|--------------|
| `new(cols, rows, scrollback)` | Create terminal state | — |
| `process(bytes)` | Feed PTY output into VT state machine | Garbled screen |
| `resize(cols, rows)` | Update terminal dimensions | Content clipped or wrapped wrong |
| `screen_snapshot()` | Return all cells with colors and attributes | Wrong rendering |
| `cursor()` | Return cursor position and visibility | Cursor in wrong place |
| `title()` | Return window title set by child process | Cosmetic only |

## libghostty-rs Integration (Feature: `ghostty-backend`, DEFAULT)

**Current status:** Fully implemented in `ghostty.rs`. This is the default backend.

**To build (default):**
```sh
cargo build   # ghostty-backend is the default feature
```
Requires Zig ≥0.15.x at build time. Install Zig: `brew install zig` or download from ziglang.org.

**Critical risk:** libghostty-rs is v0.1.1, pre-1.0. Pin commits. The API WILL change.

## vt100 Backend (Opt-in fallback: `vt100-backend`)

The opt-in fallback in `fallback_vt100.rs`. Uses the `vt100` crate (v0.16).

**To build with vt100 fallback (no Zig required):**
```sh
cargo build --no-default-features --features vt100-backend
```

**Known limitations:**
- No Kitty graphics protocol
- No sixel support
- `title()` always returns `None` (vt100 0.16 doesn't expose it)
- Resize uses `screen_mut().set_size()` (not `parser.set_size()`)
- No dim attribute detection

**These limitations are acceptable** for the current implementation. Most harness TUIs use basic text, colors, and cursor positioning.

## Color Mapping

`crates/hom-terminal/src/color_map.rs` maps `TermColor` → `ratatui::style::Color`.

When adding a new backend, you must map its color representation to `TermColor`. The 16 named colors (Black through BrightWhite) plus Indexed(0-255) and Rgb(r,g,b) must all be handled.

## PTY Management

`crates/hom-pty/src/manager.rs` uses `portable-pty` to:
- Open a PTY pair (master + slave)
- Spawn the harness command on the slave
- Read from master (→ terminal emulator)
- Write to master (← user input / orchestrator commands)
- Resize the PTY when the pane resizes

**Critical timing issue:** After spawning, the harness needs time to initialize before accepting input. Don't send prompts immediately — wait for `detect_completion() == WaitingForInput`.

## Testing Terminal Emulation

```rust
#[test]
fn test_process_simple_text() {
    let mut term = Vt100Backend::new(80, 24, 0).unwrap();
    term.process(b"Hello, World!");
    let snap = term.screen_snapshot();
    let first_row: String = snap.rows[0].iter().map(|c| c.character).collect();
    assert!(first_row.starts_with("Hello, World!"));
}

#[test]
fn test_process_color_escape() {
    let mut term = Vt100Backend::new(80, 24, 0).unwrap();
    term.process(b"\x1b[31mRed text\x1b[0m");
    let snap = term.screen_snapshot();
    assert!(matches!(snap.rows[0][0].fg, TermColor::Red));
}

#[test]
fn test_cursor_movement() {
    let mut term = Vt100Backend::new(80, 24, 0).unwrap();
    term.process(b"Line 1\nLine 2");
    let cursor = term.cursor();
    assert_eq!(cursor.row, 1);
}

#[test]
fn test_resize() {
    let mut term = Vt100Backend::new(80, 24, 0).unwrap();
    term.resize(40, 12);
    let snap = term.screen_snapshot();
    assert_eq!(snap.cols, 40);
    assert_eq!(snap.num_rows, 12);
}
```

## Red Flags — STOP and Rethink

- **Modifying TerminalBackend trait** — This affects BOTH backends. Test both paths.
- **Ignoring the feature flag boundary** — Code guarded by `#[cfg(feature = "ghostty-backend")]` must never reference `vt100` and vice versa.
- **Blocking reads on PTY** — Always use `AsyncPtyReader`. Blocking the tokio runtime freezes the entire TUI.
- **Skipping color mapping for a cell attribute** — Missing bold/italic mapping makes harness TUIs look wrong.

## Checklist Before Committing

- [ ] `cargo check` passes with default features (ghostty)
- [ ] `cargo check --no-default-features --features vt100-backend` passes (vt100 fallback path)
- [ ] Terminal emulation tests pass: `cargo test -p hom-terminal`
- [ ] PTY tests pass: `cargo test -p hom-pty`
- [ ] Color mapping covers all `TermColor` variants
- [ ] No blocking I/O on the tokio runtime
- [ ] Feature flag boundaries are clean (no cross-contamination)
