# System Design Validation Report

**Date:** 2026-04-10 (refreshed)  
**Scope:** `hom-system-design.md` cross-checked against implemented source code, `CLAUDE.md`, `Cargo.toml`, and all crate sources

---

## Status: Most Validation Issues Resolved

This report tracks the gap between the system design document and the actual implementation. Items marked **RESOLVED** were fixed during the April 10, 2026 implementation pass.

---

## 1. Dependency Version Mismatches

The design doc (Section 7) specifies dependency versions that may differ from `Cargo.toml`. The `Cargo.toml` is the source of truth.

**Status:** Documentation gap only. No code impact — the workspace compiles cleanly with the actual `Cargo.toml` versions.

---

## 2. Trait API Divergences

The `TerminalBackend` trait in the design doc (§4.1) differs slightly from `hom-core/src/traits.rs`:

- `new()` returns `Self` (infallible), not `Result<Self>`
- `render_state()` is named `screen_snapshot()` in code
- `is_alternate_screen()` and `scrollback_len()` are in the design doc but not implemented

**Status:** Low impact. The implementation is more practical. Design doc should be updated to match.

---

## 3. Structural Gaps — Design Doc vs Implementation

### 3.1 PaneManager vs App — Documentation gap

The design doc describes a `PaneManager` struct. The implementation uses `App` directly (cleaner approach). **Low impact — doc-only.**

### 3.2 Layout — Flat enum vs recursive tree

Implementation uses flat `LayoutKind` enum. Design doc describes recursive tree. **Low impact — flat is correct for MVP.**

### 3.3 Missing workflow_view.rs — Phase 3 work

Design doc lists `hom-tui/src/workflow_view.rs` which does not exist. **Low impact.**

---

## 4. Workflow Engine — MOSTLY RESOLVED

### 4.1 Condition Evaluator — RESOLVED

The condition evaluator is wired to the executor at `executor.rs`. Compound conditions (`&&`, `||`) are fully supported with correct operator precedence (AND binds tighter than OR). 7 tests cover all cases including precedence.

### 4.2 Template Variable Format — RESOLVED

Template context now uses nested `serde_json::Value` objects. `{{ steps.plan.output }}` resolves correctly via minijinja's dot access.

### 4.3 Workflow DB ID Mismatch — RESOLVED

`run_workflow_task` now generates a single `wf_id` and passes it to the executor via `execute_with_id()`, ensuring the DB row and executor use the same workflow ID.

### 4.4 SendAndWait Completion Polling — RESOLVED

`SendAndWait` no longer returns a placeholder string after a sleep. Instead it registers a `PendingCompletion` on the App, and the main event loop polls `detect_completion()` each tick. When the harness reports `WaitingForInput`, `Completed`, or `Failed`, the screen text is returned to the executor.

### 4.5 `parallel_with` Not Supported

The `parallel_with` field from the design doc is not parsed. The DAG handles parallelism implicitly via `ready_steps()`. Steps within a batch still execute sequentially — true concurrent execution requires an `Arc<dyn WorkflowRuntime>` refactor. **Medium impact for Phase 3.**

---

## 5. Adapter Accuracy

### 5.1 Copilot CLI

`sideband_type` correctly set to `None` (was incorrectly claiming JsonRpc). PTY-based interaction works; sideband steering is not available.

### 5.2 pi-mono Default Model — RESOLVED

Changed from `claude-sonnet-4` to `minimax-2.7` in `config/default.toml`.

### 5.3 OpenCode HTTP Sideband — RESOLVED

- Endpoint changed to `/session/:id/prompt_async` (was incorrect `/session/default/message`)
- Body format changed to `{ "parts": [{ "type": "text", "text": "..." }] }` (was incorrect `{ "message": "..." }`)
- Health check correctly uses `/global/health`

### 5.4 RPC Sideband (pi-mono) — Stub

`rpc.rs` is entirely stub. `send_prompt` returns `Ok("sent")`. Not integration-tested.

### 5.5 parse_screen() — Mostly Stub

Only `claude_code.rs` has real `parse_screen()` logic (detects Created/Updated files). The other 6 adapters return empty `Vec`. All 7 have real `detect_completion()` logic.

---

## 6. Rust Edition — RESOLVED

All crates use Rust 2024 edition. `CLAUDE.md` and `Cargo.toml` are aligned.

---

## 7. Schema vs Code — Consistent

SQLite schema (`001_initial.sql`) matches code. 5 tables: workflows, steps, sessions, cost_log, checkpoints. All CRUD functions in hom-db align with schema.

---

## 8. Config System

Environment variable expansion (`${VAR}` syntax in TOML) is not implemented. The `default.toml` shipped with the code doesn't use env vars, so this only affects the design doc's example. **Low impact — documented as remaining work.**

---

## 9. ghostty-backend Feature Flag — RESOLVED

`ghostty-backend` is now a real (empty) Cargo feature in `hom-terminal/Cargo.toml`. The `check-cfg` lint suppression has been removed. When `libghostty-vt` is published, the feature definition changes to `ghostty-backend = ["dep:libghostty-vt"]`.

---

## 10. Remaining Gaps (Prioritized)

### Must fix before Phase 3:

1. **Parallelize ready steps** — sequential within batch; needs `Arc<dyn WorkflowRuntime>` refactor
2. **Wire :save/:restore** to hom-db session CRUD (functions exist, handlers are stubs)
3. **Wire cost tracking** — `log_cost()`/`total_cost()` exist in hom-db but are never called

### Should fix:

4. **Full sideband integration** — sideband channels are stored on Pane but SendAndWait currently sends via PTY; an async bridge is needed for true sideband prompt delivery
5. **Implement env var expansion** in config or remove from design doc
6. **RPC sideband** (pi-mono) is entirely stub — needs real JSON-RPC implementation
7. **6 adapters** need real `parse_screen()` implementations

### Documentation:

8. Update design doc Section 7 dependency versions to match `Cargo.toml`
9. Reconcile PaneManager design with actual App-based architecture
10. Run NFR benchmarks against targets (60fps, <30MB, <50ms)
