# HOM — Harness Orchestration Management TUI

## Project Identity

HOM is a Rust-based TUI terminal multiplexer and orchestrator for 7 AI coding agent CLI harnesses. It spawns REAL native harness TUIs in visual panes — users see and interact with each harness directly. The orchestrator coordinates inputs/outputs between harnesses, translates commands to harness-native formats, and executes user-defined YAML workflows.

**This is NOT a headless orchestrator.** Each pane runs a real harness process in a pseudoterminal, rendered through a full terminal emulator (ghostty default, vt100 opt-in fallback) mapped cell-by-cell into ratatui buffers.

## Environment

- Rust 1.94.1 / Cargo 1.94.1 on darwin/arm64
- Installed via Homebrew; `rustup` is not available by default
- Rust 2024 edition for all crates
- Declare `rust-version` in `Cargo.toml` for explicit MSRV

## Supported Harnesses (7 total)

### Tier 1 — Full orchestration/steering
- **Claude Code CLI** (`claude`) — client mode stdin/stdout, `--output-format stream-json`
- **pi-mono** (`pi`) — RPC stdin/stdout, steering queue. Models: MiniMax 2.7, GLM-5, Kimi K2.5, DeepSeek, Nvidia NIM
- **OpenCode** (`opencode`) — HTTP REST API sideband on localhost:4096
- **GitHub Copilot CLI** (`copilot`) — JSON-RPC 2.0, ACP server

### Tier 2 — Headless, limited steering
- **Codex CLI** (`codex`) — JSONL events
- **Gemini CLI** (`gemini`) — JSON output
- **kimi-cli** (`kimi`) — stream-json, ACP server

## Architecture

7-crate Rust workspace:

| Crate | Purpose |
|-------|---------|
| `hom-core` | Shared types, `TerminalBackend` trait, `HarnessAdapter` trait, `SidebandChannel` trait, config, errors |
| `hom-terminal` | Terminal emulation — `Vt100Backend` (working), `GhosttyBackend` (stubbed, behind feature flag) |
| `hom-pty` | `PtyManager` (spawn/read/write/resize/kill) + `AsyncPtyReader` (tokio channel bridge) |
| `hom-adapters` | All 7 harness adapters + `AdapterRegistry` + HTTP/RPC sideband channels |
| `hom-workflow` | YAML parser, petgraph DAG, step executor with retry/timeout/templating, condition evaluator |
| `hom-tui` | App state, pane rendering, input router, command bar parser, layout engine, status rail |
| `hom-db` | SQLite via sqlx — workflows, steps, sessions, cost_log |
| `hom-mcp` | MCP server — JSON-RPC 2.0 over stdin/stdout, 6 tools, `--mcp` flag |
| `hom-web` | WebSocket HTTP server — Canvas2D live pane viewer, axum 0.8, WebFrame serialisation |
| `hom-plugin` | C ABI vtable (`HomPluginVtable`), `PluginLoader`, `PluginAdapter` |

## Key Technical Decisions

1. **libghostty-rs** for terminal emulation (user decision — accepts Zig 0.15.x build dependency). Abstracted behind `TerminalBackend` trait with vt100 fallback. Ghostty feature flag is `ghostty-backend`.
2. **TUI-inside-TUI**: Each pane is a real terminal emulator whose screen state is mapped cell-by-cell to a ratatui buffer. This is the hardest technical challenge.
3. **Claude Code flickering**: Ink/React renderer causes 4,000-6,700 scroll events/sec in ANY multiplexer. Mitigation: headless mode for automated workflow steps, visual only for direct interaction.
4. **Dual integration**: PTY + screen parsing (6 harnesses) vs HTTP sideband (OpenCode). The `SidebandChannel` trait abstracts this.
5. **YAML workflows with DAG execution**: petgraph topological sort, minijinja templating, backoff retries, SQLite checkpointing.

## Development Rules

### Agent workflow
- Inspect `Cargo.toml`, crate layout, local CLAUDE.md, Makefile/justfile, tests before editing
- Match existing patterns; prefer smallest coherent change
- Keep behavior changes, tests, and docs in the same change
- Pass dependencies explicitly; do not hardcode ports, paths, URLs, credentials
- Verify locally before handing off

### Commands

```sh
cargo build
cargo check                          # must pass after every change — zero errors
cargo test
cargo nextest run                    # preferred: faster, better output
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
cargo audit                          # vulnerability scan
cargo deny check                     # license + supply-chain policy
```

Workspace: add `--workspace` to check/test/nextest/clippy. Feature/doc changes: also run `cargo test --all-features`, `--no-default-features`, `--doc`, `cargo doc --no-deps`.

### Mandatory for all changes
- Run `cargo check` after every change — zero errors required
- Run `cargo clippy` before committing
- Use `cargo fmt` for formatting
- Write tests for any new adapter, workflow parser, or condition evaluator logic
- Never modify `hom-core` traits without considering all 7 adapters

### Crate dependency rules
- `hom-core` has ZERO internal crate dependencies — it is the root
- `hom-terminal` depends on `hom-core` only
- `hom-pty` depends on `hom-core` only
- `hom-plugin` depends on `hom-core` only
- `hom-adapters` depends on `hom-core`, `hom-plugin`
- `hom-workflow` depends on `hom-core` only
- `hom-tui` depends on `hom-core`, `hom-terminal`, `hom-pty`, `hom-adapters`, `hom-workflow`, `hom-db`, `hom-web`
- `hom-db` depends on `hom-core` only
- `hom-mcp` depends on `hom-core` only
- `hom-web` depends on `hom-core` only
- The binary (`src/main.rs`) depends on all crates

### Feature flags
- `ghostty-backend` (default) — builds with libghostty-vt, requires Zig ≥0.15.x
- `vt100-backend` (opt-in fallback) — builds with vt100 crate, no external deps; use `--no-default-features --features vt100-backend`

## Design Principles

### Clean Project Structure
- Each crate, module, and file has one clear responsibility — domain-oriented boundaries
- Avoid dumping-ground modules named `utils`, `helpers`, `common`
- Files that change together live together; split by responsibility, not technical layer
- Keep the project structure predictable — follow the file layout below

### Single Responsibility Principle (SRP) — STRICT
- Every module, type, and function has exactly one reason to change
- If a function validates, orchestrates, AND persists — split it
- If a struct mixes domain logic with transport/serialization — split it
- One public type per module when the type is complex; group small related types

### DRY — Don't Repeat Yourself
- Remove repeated validation, mapping, branching, and policy logic
- Extract only when the abstraction improves clarity (not for the sake of it)
- Three similar lines is better than a premature abstraction; six is not
- Constants for repeated magic values; types for repeated validation patterns

### Open/Closed Principle (OCP)
- Extend behavior through composition, traits, enums, and additive configuration
- Do not modify existing stable code to add new variants — add new implementations
- Use `#[non_exhaustive]` on public enums and structs expected to grow
- Prefer match arms over if-else chains; the compiler enforces exhaustiveness

### Dependency Injection — MANDATORY
- Pass all dependencies as constructor arguments or function parameters
- Never hardcode dependency selection, URLs, ports, credentials, or feature switches
- Never instantiate external clients, repositories, or runtime collaborators inside core domain
- No globals, no hidden singletons, no `static mut`, no `OnceLock` for DI workarounds
- `Arc` only when shared ownership is actually required — prefer ownership transfer
- Traits at consumer boundaries when multiple implementations exist; concrete types internally

### Clean Code and Readability
- Keep functions short, explicit, and focused on one job
- Prefer straightforward control flow over clever compression
- Use consistent formatting — `cargo fmt` defines layout, no manual overrides
- Use consistent indentation — `rustfmt` default (4 spaces)
- Use meaningful whitespace to separate logical sections; do not decorate
- Keep naming concrete and intention-revealing; avoid abbreviations
- Separate domain logic from transport, persistence, configuration, and presentation
- No hardcoded values — move runtime values into typed config, constants, or inputs

### Comments — Thoughtful, Not Redundant
- Comments explain **intent**, **invariants**, **ownership rules**, **safety conditions**, **non-obvious tradeoffs**
- Do NOT write comments that restate the code, narrate assignments, or explain obvious syntax
- Do NOT leave vague `TODO` without context — explain what and why
- Document every `unsafe` block with a `// SAFETY:` comment
- Prefer self-documenting code (good names, small functions) over comment-heavy code

### Strict Types
- Prefer compile-time guarantees over runtime interpretation
- Make invalid states unrepresentable: enums, newtypes, constructors, validated inputs
- No stringly-typed state — use enums where a finite set of values exists
- `Result` when callers need failure info; `Option` only when absence is the whole story
- Types encode intent: enums over strings, newtypes over raw primitives

### Error Handling — Clear, Concise, Contextual
- `thiserror` for typed library errors; `anyhow` only at binary/orchestration boundaries
- No `unwrap()`/`expect()` in production without strong invariant + comment
- `Option` only when absence is the whole story; panics for programmer bugs only
- Return specific error types from internal APIs — never `String`
- Add context to errors: which operation failed, with what input

### Concurrency
- Only when it materially helps; ownership transfer or message passing preferred
- Never hold a lock across `.await`; no detached tasks without shutdown path
- Match existing async runtime (tokio); `spawn_blocking` for blocking work in async

### Testing — TDD and ATDD are MANDATORY
- **ATDD first**: define the acceptance scenario before writing implementation code
- **TDD for every increment**: write the smallest failing test → implement minimally → refactor while green
- Every meaningful change covers: happy path, invalid input, edge cases, failure paths
- `#[cfg(test)] mod tests { use super::*; ... }` for unit tests; `tests/` for integration
- Behavior-focused tests; deterministic seams; no sleep-based flakiness
- Add concurrency tests when async/locking/task behavior can fail
- Doctests and examples for public APIs; test relevant feature combinations
- **No code ships without tests. No exceptions.**

### Linting and Static Analysis — Part of Development
- Run `cargo fmt --all` before every commit
- Run `cargo clippy --all-targets --all-features -- -D warnings` — treat warnings as errors
- Run `cargo test --workspace` — all tests must pass
- When public APIs change: also run `cargo test --doc`, `cargo doc --no-deps`
- Do not silence lints casually; fix the code or document why the lint is wrong
- Prefer narrowly scoped `#[expect(...)]` with rationale over broad `#[allow(...)]`

Minimum gate (MUST pass before every commit):

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run          # or cargo test
```

### Review rejects
Unnecessary deps, speculative abstractions, hidden coupling, hardcoded values, `unwrap()` in prod, undocumented `unsafe`, detached tasks, lock across `.await`, blocking in async, stringly typed state, transport in domain, missing tests, missing public docs, secrets in code, functions mixing validation + orchestration + persistence, trait-per-struct without consumer need, comments that restate code, globals or singletons.

## File Layout

```
hom/
├── CLAUDE.md                    <- You are here
├── Cargo.toml                   # Workspace root
├── Cargo.lock
├── config/default.toml          # Default harness configuration
├── workflows/                   # Built-in workflow templates
│   ├── code-review.yaml
│   ├── plan-execute-validate.yaml
│   └── multi-model-consensus.yaml
├── crates/
│   ├── hom-core/src/            # lib.rs, types.rs, traits.rs, error.rs, config.rs
│   ├── hom-terminal/src/        # lib.rs, ghostty.rs, fallback_vt100.rs, color_map.rs
│   ├── hom-pty/src/             # lib.rs, manager.rs, async_reader.rs
│   ├── hom-adapters/src/        # lib.rs, claude_code.rs, codex.rs, gemini.rs, pi_mono.rs,
│   │                            # kimi.rs, opencode.rs, copilot.rs, sideband/{mod,http,rpc}.rs
│   ├── hom-workflow/src/        # lib.rs, parser.rs, dag.rs, executor.rs, condition.rs, checkpoint.rs
│   ├── hom-tui/src/             # lib.rs, app.rs, render.rs, pane_render.rs, input.rs,
│   │                            # command_bar.rs, layout.rs, status_rail.rs
│   ├── hom-db/src/              # lib.rs, workflow.rs, session.rs, cost.rs, migrations/001_initial.sql
│   └── hom-plugin/src/          # lib.rs, ffi.rs, loader.rs, adapter.rs
├── src/main.rs                  # Binary entry point — event loop, CLI, terminal setup
├── .claude/
│   ├── rules/rust-patterns.md   # Rust style, API, type, and readability patterns
│   └── skills/rust-rig/SKILL.md # Execution discipline: ATDD/TDD, DI, review workflow
├── skills/                      # Superpowers-compatible skill definitions
│   ├── hom-adapter-development/SKILL.md
│   ├── hom-tui-testing/SKILL.md
│   ├── hom-workflow-authoring/SKILL.md
│   └── hom-terminal-integration/SKILL.md
├── docs/superpowers/plans/      # Implementation plans (YYYY-MM-DD-feature.md)
└── hom-system-design.md         # Full system design document
```

## Implementation Status

**Implemented** — compiles clean with `cargo check`, zero warnings
- All 7 crates with real types, traits, and implementations
- vt100 backend fully wired to TerminalBackend trait
- PtyManager with spawn/read/write/resize/kill
- AsyncPtyReader with tokio channel bridge
- All 7 adapters with build_command, translate_input, detect_completion
- Workflow parser, DAG builder, executor with conditions, retries (exp/linear/fixed backoff), checkpointing, fallback steps, minijinja templating
- Full TUI with pane rendering, input routing, command bar, layout engine, status rail
- Commands wired: :spawn, :pipe, :send, :broadcast, :focus, :layout, :kill, :run (full executor), :help, :quit
- :send and :broadcast use adapter translation (harness-native format with newline)
- Terminal resize uses per-pane layout areas (not whole terminal)
- Config falls back to bundled default.toml (compiled in via include_str!)
- Harness config lookup uses canonical config_key() matching config/default.toml keys
- compile_error! when no terminal backend feature is enabled
- SQLite schema and migration (workflows, steps, sessions, cost_log, checkpoints)
- Config from TOML with harness entries, render_fps, max_panes
- HomDb opened at startup in main.rs, passed to App as `Option<Arc<HomDb>>`
- :run wired to WorkflowExecutor via WorkflowBridge (channel-based async bridge)
- CLI --run/--var launches workflow executor at startup
- Workflow checkpoints persisted to SQLite via DbCheckpointStore (CheckpointStore trait)
- Workflow step results persisted to hom-db steps table
- Sideband channels constructed from config in spawn_pane (http/rpc from HarnessEntry)
- Shell-quote-aware command bar parsing (handles "quoted values" and 'single quotes')
- Compound condition evaluator supports &&/|| with correct precedence
- :pipe uses adapter parse_screen() for structured output, falls back to last_n_lines(20)
- serde_yaml_ng replaces serde_yml (RUSTSEC-2025-0068 resolved)
- Criterion benchmarks for terminal render cycle (benches/terminal_render.rs)

**Remaining work — stubs (compiles but placeholder):**
- GhosttyBackend in ghostty.rs — all methods are TODO (requires Zig ≥0.15.x + libghostty-vt). Detailed wiring steps documented.

**Resolved (April 10, 2026 — Phase 3):**
- Workflow parallel execution — Arc<dyn WorkflowRuntime> + JoinSet for concurrent batch steps
- Sideband async bridge — SendAndWait uses sideband.send_prompt() for sideband-capable panes
- :save/:restore wired to hom-db session CRUD with JSON serialization
- Cost tracking wired — log_cost() called from workflow steps and token usage events
- RPC sideband fully implemented — real JSON-RPC subprocess for pi-mono
- OpenCode SSE event polling — get_events() via GET /global/event
- OpenCode sideband integration tests added
- All 7 adapters have real parse_screen() implementations (JSONL, screen text patterns)
- Copilot ACP integration — --acp --stdio mode with JSON-RPC sideband
- Config env var expansion — ${VAR} interpolated in TOML values
- Keybinding config wired — KeybindingsConfig applied to InputRouter
- NFR benchmarks added — startup time, memory per pane, input latency
- Design doc updated — remaining work table reflects current state
- LayoutKind serde fix — lowercase variants matching default.toml

**Recently fixed (April 10, 2026):**
- Terminal resize now uses compute_pane_areas() — each pane resizes to its layout area, not the whole terminal
- :send and :broadcast now use adapter.translate_input() — harness-native format with newline appended
- Config loading now falls back to bundled config/default.toml (via include_str!) instead of empty HomConfig::default()
- HarnessType::config_key() added — canonical config key matching [harnesses.<key>] in config.toml
- spawn_pane() uses config_key() then falls back to default_binary() for reliable config lookup
- Copilot adapter capabilities downgraded — sideband_type set to None (was incorrectly claiming JsonRpc)
- OpenCode HTTP sideband endpoints updated to match real API (/global/health, /session/:id/prompt_async)
- compile_error! added to hom-terminal when no backend feature is enabled
- ghostty.rs docs clarified — Zig ≥0.15.x, explains the commented-out Cargo.toml dependency
- hom-terminal lib.rs docstring corrected to "vt100 (current default)" not "libghostty-rs (primary)"
- Design doc re-labeled from "Final Product Design" to "Architecture & Implementation Status"
- Design doc command table corrected — :run/:save/:restore accurately show partial/stub status
- Design doc remaining work table expanded with all newly identified gaps
- Validation report 4.1 updated — condition evaluator is wired (was marked as disconnected)
- spawn_pane() now reads HomConfig.harnesses entries (binary, default_model, env) from config.toml
- spawn_pane() supports --dir and extra args from command bar; working_dir wired through
- Pane title shows effective model (from config default or explicit)
- Workflow template context now uses nested serde_json::Value — `steps.plan.output` dot-access works correctly
- Workflow step harness/model fields are now templated (e.g. `{{ planner }}` resolves)
- Workflow condition evaluator wired to executor — conditions are evaluated before each step
- Workflow retry logic implemented with exponential/linear/fixed backoff
- Workflow checkpointing called after each successful step
- Workflow fallback step execution implemented
- Layout change resizes all PTYs and terminal emulators to match new pane areas
- :run --var flag parsed correctly in command bar
- :run now loads and validates the workflow YAML (parse errors shown to user)
- :pipe, :send, :broadcast commands wired in main.rs handle_command()
- :send strips surrounding quotes from input
- :spawn parser handles --dir flag and -- extra args
- Escape in command bar returns to focused pane (was a no-op)
- Tick rate reads from config.general.render_fps instead of hardcoded 30
- max_panes limit enforced in spawn_pane()
- All unused imports cleaned up — zero cargo check warnings
- All clippy warnings resolved — zero cargo clippy warnings
- All 7 adapters have Default impl per clippy
- Ghostty feature flag is now a real (empty) Cargo feature — no check-cfg lint suppression needed
- hom-terminal package description corrected to "vt100 default, libghostty-rs planned"
- Startup log and welcome message corrected from "Hive" to "HOM"
- Workflow DB ID mismatch fixed — run_workflow_task generates a single wf_id, passes it to executor via execute_with_id()
- SendAndWait no longer returns placeholder — uses PendingCompletion polling via detect_completion() in the main event loop
- OpenCode HTTP sideband body format fixed — uses `{ "parts": [{ "type": "text", "text": "..." }] }` per OpenCode API
- OpenCode sideband always uses `/session/:id/prompt_async` (removed incorrect `/session/default/message` fallback)
- WorkflowExecutor.execute_with_id() added — accepts caller-provided workflow_id for DB consistency
- App.poll_pending_completions() added — checks detect_completion() and timeout on pending workflow steps
- Validation report (docs/superpowers/plans/) refreshed with current state

**Resolved (April 10, 2026 — Phase 4):**
- Cost display in status rail (F10) — total_cost polled from DB, shown as $X.XX in magenta
- Workflow progress tracking (F9) — WorkflowProgress type replaces stringly-typed status, shows step counts
- Terminal emulator integration tests — 5 vt100 tests covering text, colors, resize, cursor
- Workflow template library expanded (F12) — 8 total templates (TDD, debugging, refactor, docs, parallel analysis)
- handle_command refactored — extracted per-command handler functions

**Resolved (April 10, 2026 — Phase 5 Blockers):**
- Graceful PTY shutdown — App::shutdown() + PtyManager::kill_all() called on Ctrl-Q/:quit
- Process crash handling — exited panes show [EXITED: N] in red, pending workflow steps notified
- Database reliability — fail fast on DB error with clear message, --no-db for explicit opt-out

**Resolved (April 10, 2026 — Phase 6 Required):**
- detect_completion() improved — last_non_empty_line() + anchored starts_with() patterns per adapter, error detection added
- RPC get_events() implemented — non-blocking try_lock + 1ms timeout, parses JSON-RPC notifications
- End-to-end PTY pipeline tests — spawn→read (echo), spawn→write→read (cat), PTY→Vt100→ScreenSnapshot

**Resolved (April 10, 2026 — Phase 7 Important):**
- Process exit notification — command_bar.last_error set when a pane exits with code N
- Sideband health polling — health_check() called every ~5s in main loop; notifies on failure
- Claude Code flickering documented — headless mode (--output-format stream-json) workaround in claude_code.rs
- Keybinding validation — validate_keybindings() at startup, warns on invalid config strings
- AsyncPtyReader cancellation — abort() method added; called in kill_pane() before pane removal

**NFR benchmarks validated (April 10, 2026):**
- NF1 render cycle: 47µs per frame (target <16ms) ✅
- NF2 input encoding: 12.8µs per 1000 keystrokes (target <50ms total) ✅
- NF3 memory per pane: 20.2MB at default 5k scrollback (target <30MB) ✅ — default_scrollback lowered from 10k to 5k
- NF4 startup: 9.3µs config+terminal init (target <500ms) ✅

**Resolved (April 10, 2026 — GhosttyBackend):**
- GhosttyBackend fully implemented — `libghostty-vt 0.1.1` wired, `Terminal::new/vt_write/resize/grid_ref/cursor_x/y/title` mapped to `TerminalBackend` trait
- `unsafe impl Send + Sync` with documented safety invariant (single-threaded event loop access)
- `map_style_color()` maps Ghostty palette/RGB/None to `TermColor`; underline/bold/italic/dim/blink/inverse/strikethrough all mapped
- 7 tests added (dimensions, plain text, resize, cursor, title, ANSI color, color mapping)
- `libghostty-vt-sys` build requires Zig network access at first build (`deps.files.ghostty.org`); not validatable in offline environments

**Resolved (April 10, 2026 — P4 Session):**
- Mouse passthrough — `encode_mouse_event` encodes X10 protocol bytes; `EnableMouseCapture`/`DisableMouseCapture` wired in main.rs setup/teardown; `PaneInput` arm forwards non-focus mouse events to focused PTY
- Adapter smoke tests — `build_command`/`translate_input` tests added for all 7 adapters; `AdapterRegistry` smoke tests in `lib.rs` (91 tests total in hom-adapters)
- GhosttyBackend CI — `.github/workflows/ci.yml` `ghostty` job added (self-hosted, zig label); `scripts/seed-zig-cache.sh` documents one-time runner provisioning

**Resolved (April 10, 2026 — Feature flag swap):**
- `ghostty-backend` promoted to default feature — `crates/hom-terminal/Cargo.toml` default changed from `vt100-backend` to `ghostty-backend`
- `vt100-backend` is now the opt-in fallback: `cargo build --no-default-features --features vt100-backend`
- All docs updated: CLAUDE.md, hom-system-design.md, hom-architecture.html, README.md, skills/hom-terminal-integration/SKILL.md

**Resolved (April 10, 2026 — MCP Server):**
- hom-mcp crate — JSON-RPC 2.0 MCP server over stdin/stdout
- Six tools: spawn_pane, send_to_pane, run_workflow, list_panes, get_pane_output, kill_pane
- `--mcp` flag spawns McpServer as a tokio task alongside the TUI
- McpRequest/McpResponse types in hom-core; channel-based IPC with App
- RunWorkflow via MCP returns a diagnostic error (WorkflowBridge lives in main.rs — a future refactor can move it into App)

**Resolved (April 11, 2026 — Web UI):**
- hom-web crate — axum 0.8 WebSocket server on localhost:4242 (--web-port to override)
- Canvas2D cell rendering in browser — XSS-safe (fillText, no innerHTML); full ANSI 256-color palette
- Broadcast WebFrame (serialised ScreenSnapshot) to all connected WebSocket clients after each tick
- Browser keystrokes forwarded to target pane via WebInput channel (pane_id-routed, not just focused pane)
- WebServer::run() returns anyhow::Result<()> — bind/serve errors propagated and logged, not panicked
- `hom --web` or `hom --web --web-port 8080`

**Resolved (April 10, 2026 — Remote pane support):**
- `RemoteTarget` added to `hom-core/src/types.rs` — parse `user@host[:port]`, `shell_quote()`, `build_remote_command()`
- `PaneKind::Local` / `PaneKind::Remote(RemoteTarget)` discriminant added to `hom-core`
- `RemotePtyManager` in `crates/hom-pty/src/remote.rs` — SSH session + channel lifecycle via `ssh2 = "0.9"`
- `SshAuthMethod::defaults()` tries SSH agent then `~/.ssh/id_ed25519` then `~/.ssh/id_rsa`
- All remote command args are individually shell-quoted via `RemoteTarget::shell_quote()` before SSH exec
- `:spawn <harness> --remote user@host[:port]` parsed in command bar; routes to `App::spawn_remote_pane()`
- `App::shutdown()` calls `remote_ptys.kill_all()` for graceful cleanup
- 7 unit tests for `RemotePtyManager` + 3 for command bar `--remote` flag parsing

**Resolved (April 10, 2026 — Plugin system):**
- `crates/hom-plugin/` new crate — stable C ABI vtable (`HomPluginVtable`, ABI v1)
- `HomInputKind` `#[repr(u32)]` enum replaces stringly-typed `u32` in `translate_input` vtable fn
- `HomPluginVtable` uses JSON strings for all complex data crossing FFI (ScreenSnapshot, HarnessEvent, CompletionStatus)
- `PluginLoader::load(path)` — validates ABI version, calls `hom_plugin_init`, wraps vtable in `PluginAdapter`
- `PluginLoader::scan_dir(dir)` — discovers `.dylib`/`.so` files, logs failures, returns adapters
- `PluginLoader::default_plugin_dir()` — `~/.config/hom/plugins/`
- `PluginAdapter` implements `HarnessAdapter` — all methods dispatch through JSON FFI; `hom_plugin_destroy` called on drop
- `decode_hex_bytes()` converts hex-encoded PTY bytes returned by plugin `translate_input`
- `AdapterRegistry::load_plugin(path)` + `get_plugin(name)` + `load_plugins_from_dir(dir)` + `plugin_names()`
- `:load-plugin /path/to/plugin.dylib` command wired in command bar and `App::handle_load_plugin()`
- Auto-scan of `~/.config/hom/plugins/` at `App::new()` startup
- Unknown harness names fall through to plugin registry: `:spawn mycli` works if `mycli` plugin is loaded
- 10 unit tests in `hom-plugin` + 3 in `hom-adapters` + 3 in `hom-tui`

**Resolved (April 11, 2026 — Round 2 correctness fixes):**
- `kill_pane()` now dispatches to `remote_ptys.kill()` for IDs ≥1000 (was calling `pty_manager.kill()` which returned `PaneNotFound` and early-returned before removing the pane — zombie pane leak)
- `Event::Resize` and `Command::Layout` both dispatch `remote_ptys.resize()` for remote pane IDs (was silently discarding `pty_manager.resize()` errors for remote IDs)
- `poll_pending_completions()` and `poll_pty_output()` use the plugin adapter (via `pane.plugin_name`) instead of the `ClaudeCode` placeholder harness type — plugin workflow completion detection now works
- `Pane` struct gains `plugin_name: Option<String>` field; set to `Some(harness_name)` in `spawn_pane_inner` when `harness: None`; `None` for all built-in and remote panes
- `handle_load_plugin()` success no longer sets `last_error` (render prefixes it with "Error:" in red; tracing info log is sufficient)
- `App::new()` uses `adapter_registry.scan_default_plugin_dir()` — removes direct `hom_plugin::` reference from `hom-tui`
- `hom-plugin` dependency removed from `hom-tui/Cargo.toml`; dep rules updated (hom-tui now correctly lists `hom-web` as a dep)

**No remaining stubs** — all features are implemented. GhosttyBackend runtime validation requires network access during Zig build.

## Superpowers Integration

This project is structured to work with the [superpowers](https://github.com/obra/superpowers) plugin. Skills in `skills/` follow superpowers conventions. Use them:

- **Before any Rust implementation or refactoring**: Read `.claude/skills/rust-rig/SKILL.md` (ATDD/TDD, DI, review discipline)
- **Before adding a new adapter**: Read `skills/hom-adapter-development/SKILL.md`
- **Before modifying TUI rendering**: Read `skills/hom-tui-testing/SKILL.md`
- **Before writing a workflow YAML**: Read `skills/hom-workflow-authoring/SKILL.md`
- **Before touching terminal emulation**: Read `skills/hom-terminal-integration/SKILL.md`
- **Before any feature work**: Write a plan in `docs/superpowers/plans/YYYY-MM-DD-<feature>.md`

Plans decompose into 2-5 minute tasks following TDD: write failing test -> verify failure -> implement minimally -> verify pass -> commit.
