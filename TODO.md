# HOM — Complete TODO List

**Generated:** 2026-04-10
**Updated:** 2026-04-10 — All 15 items resolved

---

## P1 — ✅ All Complete

### 1. ✅ Workflow parallel execution
Refactored `WorkflowRuntime` from `&dyn` to `Arc<dyn>`, concurrent batch execution via `JoinSet`.
- **Commit:** `feat: parallel workflow execution with Arc<dyn WorkflowRuntime> + JoinSet`

### 2. ✅ Full sideband execution path (async bridge)
`SendAndWait` uses `sideband.send_prompt()` for sideband-capable panes via spawned tokio task.
- **Commit:** `feat: sideband async bridge for SendAndWait`

### 3. ✅ Wire :save/:restore to hom-db session CRUD
Commands wired to `hom_db::session::save_session/load_session` with JSON serialization of layout + pane configs.
- **Commit:** `feat: wire :save/:restore to hom-db session CRUD`

### 4. ✅ Wire cost tracking
`log_cost()` called from workflow step completion and `TokenUsage` events from `parse_screen()`.
- **Commit:** `feat: wire cost tracking from workflow and token usage events`

---

## P2 — ✅ All Complete

### 5. ✅ RPC sideband (pi-mono) — full implementation
Real JSON-RPC subprocess with stdin/stdout communication, request IDs, error handling, health check.
- **Commit:** `feat: real JSON-RPC subprocess sideband for pi-mono`

### 6. ✅ OpenCode SSE event polling
`get_events()` polls `GET /global/event` SSE stream, parses token_usage, task_completed, error events.
- **Commit:** `feat: OpenCode SSE event polling via GET /global/event`

### 7. ✅ OpenCode sideband integration tests
Tests for construction, session binding, unreachable server handling, send_prompt failure, get_events.
- **Commit:** `test: OpenCode sideband integration tests`

### 8. ✅ Adapter parse_screen() implementations (all 7)
All adapters have real `parse_screen()`: JSONL parsing (codex, kimi), screen text patterns (gemini, pi_mono, opencode, copilot).
- **Commit:** `feat: implement parse_screen() for all 6 remaining adapters`

### 9. ✅ Copilot ACP integration
`--acp --stdio` mode support with JSON-RPC sideband via `RpcSideband`.
- **Commit:** (included in parse_screen commit)

### 10. ✅ Config env var expansion
`${VAR}` syntax interpolated in TOML values after loading. Unknown vars left as-is.
- **Commit:** `feat: config env var expansion and LayoutKind serde fix`

### 11. ✅ Keybinding config wiring
`KeybindingsConfig` applied to `InputRouter::from_config()`. Configurable: toggle_command_bar, next_pane, kill_pane.
- **Commit:** `feat: wire keybinding config to InputRouter`

---

## P3 — ✅ All Complete

### 12. ✅ GhosttyBackend stub hardening
Detailed wiring steps documented in `ghostty.rs`. Blocked on libghostty-vt publication + Zig ≥0.15.x.
- **Commit:** `docs: harden GhosttyBackend stub with detailed wiring steps`

### 13. ✅ NFR benchmark coverage
Added Criterion benchmarks: startup time, memory per pane, input encoding latency.
- **Commit:** `feat: add NFR benchmarks for startup, memory, and input latency`

### 14. ✅ Design doc dependency versions
Section 7 verified against Cargo.toml — versions match.
- **Commit:** `docs: update design doc remaining work to reflect completed items`

### 15. ✅ Design doc architecture reconciliation
PaneManager references already removed in prior work. Section 8 remaining work table updated.
- **Commit:** (included in design doc update commit)

---

## Summary

| Priority | Count | Status |
|----------|-------|--------|
| P1       | 4     | ✅ All complete |
| P2       | 7     | ✅ All complete |
| P3       | 4     | ✅ All complete |
| **Total**| **15**| **✅ All resolved** |

**Test count:** 35 tests across 4 crates (config, adapters, tui, workflow)
**No FIXME, HACK, or XXX comments.**
**Zero cargo check warnings. Zero clippy warnings.**
