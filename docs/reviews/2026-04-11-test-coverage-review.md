# Test Coverage Review

Date: 2026-04-11
Scope: full Rust source audit across `crates/` and `src/`

## Original Gaps

### High

- `src/main.rs` had zero tests around CLI/runtime orchestration paths.
- `crates/hom-db` had zero tests for migrations, sessions, workflows, or cost tracking.
- `crates/hom-web/src/server.rs` had no end-to-end server/WebSocket coverage.

### Medium

- `crates/hom-tui/src/layout.rs`, `pane_render.rs`, and `render.rs` had no direct rendering/layout tests.
- `crates/hom-workflow/src/dag.rs`, `checkpoint.rs`, `crates/hom-tui/src/workflow_bridge.rs`, and `db_checkpoint.rs` had no focused boundary tests.

### Low

- `crates/hom-terminal/src/color_map.rs` had no regression tests.

## Changes Added

### Runtime / Main

- Added tests in `src/main.rs` for:
  - `parse_var`
  - selector resolution
  - local command error states (`:help`, `:save`, `:restore`, missing workflow)
  - workflow request error handling
  - workflow progress updates

### Persistence

- Added `hom-db` tests for:
  - DB open + schema creation
  - cost logging / total aggregation / breakdown ordering
  - session save/load/list behavior
  - workflow lifecycle persistence
  - step result persistence

- Fixed a real bug found by the new tests:
  - `list_sessions()` now orders distinct names by `MAX(updated_at)` instead of relying on `DISTINCT ... ORDER BY updated_at`, which was not stable for “latest session first”.

### Workflow / Bridge / Checkpoint

- Added tests for:
  - checkpoint JSON round-trip and invalid payload handling
  - DAG roots, ready-steps, unknown dependency rejection, cycle rejection, topo ordering
  - workflow bridge request routing, closed-channel failures, status mapping
  - DB checkpoint persistence for checkpoint JSON and step rows

### TUI / Rendering

- Added tests for:
  - layout behavior for `Single`, `Grid`, `Tabbed`, and hit-testing
  - pane rendering of title, exited state, cell colors/modifiers, and cursor
  - empty-state render
  - command bar error render
  - command bar cursor placement

### Web

- Refactored `hom-web` server slightly to enable testable listener injection.
- Added end-to-end tests for:
  - `GET /`
  - WebSocket frame broadcast
  - browser input forwarding

### Terminal

- Added color-map regression tests for named, bright, indexed, and RGB colors.

## Validation

These passed after the changes:

- `cargo test --workspace`
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Remaining Coverage Work

The largest remaining gaps are broader acceptance scenarios rather than uncovered pure logic:

- real subprocess-style binary tests for `hom --mcp`, `hom --web`, and interactive startup
- end-to-end workflow execution through the actual executor/runtime boundary
- manual smoke testing of TUI, MCP, and web viewer with live harnesses
- installing a coverage tool such as `cargo-llvm-cov` if numeric coverage reporting is required
