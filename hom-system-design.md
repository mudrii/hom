# HOM: Harness Orchestration Management TUI — System Design Document

**Version:** 3.3 | **Date:** April 11, 2026 | **Status:** Architecture & Implementation Status

---

## 1. Overview

HOM is a Rust-based TUI that acts as an intelligent terminal multiplexer and orchestrator for AI coding agent CLI harnesses. It spawns real harness processes (Claude Code, Codex CLI, Gemini CLI, pi-mono, kimi-cli, OpenCode, GitHub Copilot CLI) in visual panes where each renders its native TUI. The orchestrator coordinates inputs, outputs, and workflows between them.

HOM does not replace any harness — it sits above them as a coordinator, translator, and workflow engine.

### 1.1 Target Users

Developers and DevOps engineers who work with multiple AI harnesses on the same or multiple codebases and need centralized orchestration to manage them.

### 1.2 Success Criteria

A full working product that can:
- Spawn and visually display 2-7 harnesses simultaneously in panes
- Allow direct user interaction with any focused pane
- Translate orchestrator commands to harness-native input
- Execute user-defined workflows (plan → execute → validate)
- Pass structured data between harnesses
- Persist sessions and track cost across harnesses

---

## 2. Requirements

### 2.1 Functional Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| F1 | Spawn any supported harness in a visual PTY pane | P0 |
| F2 | Render each harness's native TUI faithfully (colors, cursor, mouse, alternate screen) | P0 |
| F3 | Allow user to focus a pane and type directly into the harness | P0 |
| F4 | Orchestrator command bar for meta-operations (`:spawn`, `:pipe`, `:run`, `:focus`) | P0 |
| F5 | Pipe output from one harness pane as input to another | P0 |
| F6 | Define workflows in YAML (step DAGs with dependencies) | P1 |
| F7 | Execute workflows: sequential, parallel, conditional branching, bounded retries | P1 |
| F8 | Translate orchestrator-level prompts to harness-native syntax | P1 |
| F9 | Display workflow progress (DAG visualization, step status) | P1 |
| F10 | Track and display aggregate token usage and cost across all panes | P2 |
| F11 | Session persistence — save/restore pane layout and harness state | P2 |
| F12 | Workflow template library (built-in common patterns) | P2 |

### 2.2 Non-Functional Requirements

| ID | Requirement | Target | Measured (vt100, 5k scrollback) |
|----|-------------|--------|----------------------------------|
| NF1 | Rendering latency | < 16ms per frame (60fps capable) | **47µs** ✅ (340× headroom) |
| NF2 | Input-to-pane latency | < 50ms keystroke delivery | **12.8µs / 1000 keys** ✅ |
| NF3 | Memory per pane (terminal emulation) | < 30MB including scrollback | **20.2MB** ✅ (default 5k scrollback) |
| NF4 | Startup time | < 500ms to first render | **9.3µs config+terminal init** ✅ |
| NF5 | Supported harnesses | 7 (all listed) | **7** ✅ |
| NF6 | Platform support | Linux and macOS | macOS validated ✅ |

### 2.3 Constraints

| Constraint | Detail |
|------------|--------|
| Language | Rust (performance, safety, stability) — 2024 edition, MSRV 1.85 |
| Terminal emulation (default) | `libghostty-rs` (`libghostty-vt 0.1.1`) — default backend, best-in-class VT emulation, Kitty protocol support. Fully implemented. |
| Terminal emulation (opt-in fallback) | `vt100` crate — pure Rust, no external build deps. Enable with `--no-default-features --features vt100-backend` |
| Build dependency | Zig ≥0.15.x required for default build (`ghostty-backend`). No external deps when using `vt100-backend` fallback. |
| API stability risk | libghostty-rs is v0.1.1, pre-1.0 — plan for API churn. Abstracted behind `TerminalBackend` trait with vt100 as opt-in fallback |

---

## 3. High-Level Architecture

All 7 crates compile clean. Core types, traits, adapters, workflow engine, TUI, and storage layer are implemented.

```
                            ┌─────────────────────────────────────────────────┐
                            │               HOM TUI PROCESS                    │
                            │                                                 │
 User                       │  ┌───────────────────────────────────────────┐  │
 Keyboard ──────────────────┤  │            Input Router                   │  │
 Mouse                      │  │  (focused pane gets raw input;            │  │
                            │  │   command bar gets : prefixed input)      │  │
                            │  └──────┬──────────────────┬─────────────────┘  │
                            │         │                  │                    │
                            │         ▼                  ▼                    │
                            │  ┌─────────────┐    ┌─────────────────────┐    │
                            │  │ Command Bar │    │   App State         │    │
                            │  │ Parser      │    │                     │    │
                            │  │             │    │  ┌─────┐ ┌─────┐   │    │
                            │  │ :spawn      │    │  │Pane1│ │Pane2│   │    │
                            │  │ :pipe       │    │  │     │ │     │   │    │
                            │  │ :run        │    │  │ PTY │ │ PTY │   │    │
                            │  │ :focus      │    │  │  +  │ │  +  │   │    │
                            │  │ :broadcast  │    │  │vt100│ │vt100│   │    │
                            │  │ :kill       │    │  │/ghst│ │/ghst│   │    │
                            │  └──────┬──────┘    │  └──┬──┘ └──┬──┘   │    │
                            │         │           │     │       │      │    │
                            │         ▼           └─────┼───────┼──────┘    │
                            │  ┌─────────────┐          │       │           │
                            │  │ Workflow     │          │       │           │
                            │  │ Engine       │          │       │           │
                            │  │             │          │       │           │
                            │  │ DAG exec    │◄─────────┘       │           │
                            │  │ YAML parse  │◄─────────────────┘           │
                            │  │ Retry logic │                              │
                            │  │ Templating  │                              │
                            │  └──────┬──────┘                              │
                            │         │                                     │
                            │         ▼                                     │
                            │  ┌─────────────────────────────────────────┐  │
                            │  │          Adapter Registry                │  │
                            │  │                                         │  │
                            │  │  ┌──────┐ ┌──────┐ ┌──────┐ ┌───────┐  │  │
                            │  │  │Claude│ │Codex │ │pi-   │ │Copilot│  │  │
                            │  │  │Code  │ │CLI   │ │mono  │ │CLI    │  │  │
                            │  │  │Adapt.│ │Adapt.│ │Adapt.│ │Adapt. │  │  │
                            │  │  └──────┘ └──────┘ └──────┘ └───────┘  │  │
                            │  │  ┌──────┐ ┌──────┐ ┌──────┐           │  │
                            │  │  │Gemini│ │kimi- │ │Open- │           │  │
                            │  │  │CLI   │ │cli   │ │Code  │           │  │
                            │  │  │Adapt.│ │Adapt.│ │Adapt.│           │  │
                            │  │  └──────┘ └──────┘ └──────┘           │  │
                            │  └─────────────────────────────────────────┘  │
                            │                                                 │
                            │  ┌─────────────────────────────────────────┐  │
                            │  │          Rendering Engine                │  │
                            │  │  ratatui + crossterm                    │  │
                            │  │  Reads TerminalBackend screen snapshots  │  │
                            │  │  Composites panes + status + cmd bar   │  │
                            │  └─────────────────────────────────────────┘  │
                            └─────────────────────────────────────────────────┘
                                          │              │
                              ┌───────────▼──┐    ┌──────▼───────┐
                              │ SQLite DB    │    │ Filesystem   │
                              │ (sqlx)       │    │              │
                              │ - sessions   │    │ - workflows/ │
                              │ - steps      │    │ - config/    │
                              │ - checkpoints│    │ - adapters/  │
                              └──────────────┘    └──────────────┘
```

### 3.1 Data Flow

**User types into focused pane:**
```
Keystroke → Input Router → is pane focused? → yes → PTY stdin of that pane
                                             → no  → Command Bar Parser
```

**Orchestrator command `:pipe pane-a → pane-b`:**
```
Command Bar → parse "pipe" → terminal.screen_snapshot() on source pane
           → extract screen text
           → pty_manager.write_to(target_pane, text) → PTY stdin of target
```

**Workflow execution:**
```
YAML file → WorkflowDef::from_file() → WorkflowDag::from_steps()
  → topological sort → for each ready batch:
    → evaluate_condition() → skip if false
    → render_template(prompt, nested context) via minijinja
    → render_template(harness/model fields) for variable resolution
    → runtime.spawn_pane(harness, model)
    → runtime.send_and_wait(pane, prompt, timeout)
    → retry with compute_backoff() on failure
    → execute fallback step if configured
    → checkpoint after each successful step
    → store output in step_outputs for downstream templates
    → next batch...
```

---

## 4. Component Deep Dive

### 4.1 Terminal Emulation Layer

Each pane embeds a full terminal emulator instance behind the `TerminalBackend` trait. The default backend is `libghostty-rs` (`GhosttyBackend`), which provides best-in-class VT emulation with Kitty keyboard and graphics protocol support — fully implemented with `libghostty-vt 0.1.1`, requires Zig ≥0.15.x at build time. The opt-in fallback is the `vt100` crate (`Vt100Backend`), which provides solid VT100/VT220 emulation with zero external build dependencies, enabled via `--no-default-features --features vt100-backend`.

#### Architecture

```
┌─────────────────────────────────────────────┐
│                   Pane                       │
│                                             │
│  ┌───────────────────────────────────────┐  │
│  │  portable-pty::PtyPair               │  │
│  │                                       │  │
│  │  master_fd ←→ child process           │  │
│  │  (e.g. "claude -p ..." or "codex")   │  │
│  └───────────┬───────────────────────────┘  │
│              │ raw bytes                     │
│              ▼                               │
│  ┌───────────────────────────────────────┐  │
│  │  TerminalBackend (trait)              │  │
│  │                                       │  │
│  │  Default: GhosttyBackend              │  │
│  │    (libghostty-rs, needs Zig ≥0.15.x)  │  │
│  │  Fallback: Vt100Backend (vt100 crate) │  │
│  │                                       │  │
│  │  - Processes VT escape sequences      │  │
│  │  - Maintains screen buffer            │  │
│  │  - Tracks cursor position + style     │  │
│  │  - Handles alternate screen           │  │
│  │  - Manages scrollback history         │  │
│  └───────────┬───────────────────────────┘  │
│              │ ScreenSnapshot                │
│              ▼                               │
│  ┌───────────────────────────────────────┐  │
│  │  Ratatui Renderer (pane_render.rs)    │  │
│  │                                       │  │
│  │  - Iterates ScreenSnapshot rows/cells │  │
│  │  - Maps colors → ratatui Style        │  │
│  │  - Maps attrs → ratatui Modifier      │  │
│  │  - Renders into ratatui::Buffer       │  │
│  └───────────────────────────────────────┘  │
└─────────────────────────────────────────────┘
```

#### Key Design Decisions

**Why libghostty-rs as the target primary:**
1. Most battle-tested VT emulation core (powers Ghostty, cmux, 12+ commercial products)
2. Full Kitty keyboard + graphics protocol support (future-proof)
3. Zero-dependency core (performance)
4. cmux proves this exact use case (terminal multiplexer on libghostty) works
5. GPU rendering pipeline available for complex terminal output

**Default — `GhosttyBackend`:**
- Feature flag: `ghostty-backend` (default)
- Dependency: `libghostty-vt = "0.1.1"` (requires Zig ≥0.15.x at build time; Zig compiles Ghostty's C VT library)
- Capabilities: Full Kitty keyboard + graphics protocol, alternate screen, scrollback, GPU rendering
- Status: **Fully implemented** — all `TerminalBackend` trait methods wired; 8 unit tests pass; `unsafe impl Send + Sync` with documented single-threaded safety invariant
- API churn note: pre-1.0 library — abstracted behind `TerminalBackend` trait so vt100 works as opt-in fallback

**Opt-in fallback — `Vt100Backend`:**
- Feature flag: `vt100-backend` (opt-in)
- Build: `cargo build --no-default-features --features vt100-backend`
- Dependency: `vt100 = "0.16"` — stable, pure Rust, no external build deps
- Capabilities: VT100/VT220 escape sequences, color, cursor, alternate screen, scrollback
- Status: **Fully implemented and working** — use for environments without Zig ≥0.15.x

**Build system:**
```toml
# crates/hom-terminal/Cargo.toml
[features]
default = ["ghostty-backend"]
ghostty-backend = ["dep:libghostty-vt"]  # default; requires Zig ≥0.15.x
vt100-backend = ["dep:vt100"]            # opt-in fallback: --no-default-features --features vt100-backend
```

#### The `TerminalBackend` Trait

```rust
pub trait TerminalBackend: Send + Sync {
    /// Create a new terminal with the given dimensions.
    fn new(cols: u16, rows: u16, scrollback: usize) -> Self
    where
        Self: Sized;

    /// Feed raw bytes from the PTY into the terminal emulator.
    fn process(&mut self, bytes: &[u8]);

    /// Resize the terminal.
    fn resize(&mut self, cols: u16, rows: u16);

    /// Get a snapshot of the current screen state for rendering.
    fn screen_snapshot(&self) -> ScreenSnapshot;

    /// Get the current cursor state.
    fn cursor(&self) -> CursorState;

    /// Get the terminal title (if set by the child process).
    fn title(&self) -> Option<&str>;
}
```

#### Color Mapping

The `color_map` module converts terminal emulator colors to ratatui `Color` values. Both backends produce `ScreenSnapshot` cells with color and attribute information that the renderer maps cell-by-cell into ratatui buffers. Both backends are fully mapped: vt100 via `map_vt100_color()`, ghostty via `map_style_color()` (palette 0-255, RGB, and default).

### 4.2 Pane Management and App State

The `App` struct in `hom-tui` owns all runtime state: panes, PTY manager, adapters, layout, config, and command bar.

#### Pane Spawning

`App::spawn_pane()` and `App::spawn_pane_with_opts()` create a new harness pane:

1. Check `max_panes` limit from config — reject if exceeded
2. Look up `HarnessEntry` from `HomConfig.harnesses` for binary override, default model, and env vars
3. Build the command via the adapter's `build_command()` with effective model and extra args
4. Spawn the PTY process via `PtyManager::spawn()` with working directory support
5. Create a `TerminalBackend` instance (`Vt100Backend` by default, `GhosttyBackend` when enabled) at the pane's dimensions
6. Start an `AsyncPtyReader` tokio task to bridge PTY output into a channel
7. Register the pane in `App.panes` and `App.pane_order`

```rust
pub fn spawn_pane_with_opts(
    &mut self,
    harness_type: HarnessType,
    model: Option<String>,
    working_dir: Option<PathBuf>,
    extra_args: Vec<String>,
    cols: u16,
    rows: u16,
) -> HomResult<PaneId>
```

The pane title displays the harness type and effective model (from config default or explicit override).

#### PTY Output Polling

`App::poll_pty_output()` drains each pane's `AsyncPtyReader` channel and feeds bytes into the pane's `TerminalBackend::process()`. This runs every tick in the event loop.

#### Resize Propagation

Terminal resize events (`Event::Resize`) propagate to all panes:
- `PtyManager::resize(pane_id, cols, rows)` — sends `SIGWINCH` to the PTY child
- `TerminalBackend::resize(cols, rows)` — resizes the emulator's internal buffer

Layout changes (`:layout` command) similarly recompute pane areas and resize all PTYs and emulators to match.

### 4.3 Harness Adapters

Each harness has an adapter implementing `HarnessAdapter`:

```rust
#[async_trait]
pub trait HarnessAdapter: Send + Sync {
    /// Which harness this adapter handles.
    fn harness_type(&self) -> HarnessType;

    /// Human-readable name for display.
    fn display_name(&self) -> &str;

    /// Build the command + arguments to spawn this harness.
    fn build_command(&self, config: &HarnessConfig) -> CommandSpec;

    /// Translate an orchestrator command into raw bytes for the PTY.
    fn translate_input(&self, command: &OrchestratorCommand) -> Vec<u8>;

    /// Parse the terminal screen to extract structured events.
    fn parse_screen(&self, screen: &ScreenSnapshot) -> Vec<HarnessEvent>;

    /// Detect whether the harness has finished its current task.
    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus;

    /// Report this harness's capabilities.
    fn capabilities(&self) -> HarnessCapabilities;

    /// Optional sideband channel (HTTP, RPC, etc.).
    fn sideband(&self) -> Option<Box<dyn SidebandChannel>> { None }
}

/// Specification for spawning a harness process.
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: PathBuf,
}
```

#### Adapter Registry

`AdapterRegistry` holds all 7 adapters and provides lookup by `HarnessType`:

```rust
impl AdapterRegistry {
    pub fn new() -> Self { /* registers all 7 adapters */ }
    pub fn get(&self, harness: HarnessType) -> &dyn HarnessAdapter;
}
```

#### Harness Tiers

**Tier 1 — Full orchestration/steering:**

| Harness | Binary | Integration | Sideband |
|---------|--------|-------------|----------|
| Claude Code CLI | `claude` | PTY + `--output-format stream-json` | stdin/stdout client mode |
| pi-mono | `pi` | PTY + RPC stdin/stdout | Steering queue |
| OpenCode | `opencode` | PTY + HTTP REST | localhost:4096 |
| GitHub Copilot CLI | `copilot` | PTY + JSON-RPC 2.0 | ACP server |

**Tier 2 — Headless, limited steering:**

| Harness | Binary | Integration |
|---------|--------|-------------|
| Codex CLI | `codex` | PTY + JSONL events |
| Gemini CLI | `gemini` | PTY + JSON output |
| kimi-cli | `kimi` | PTY + stream-json, ACP server |

#### Sideband Channels

The `SidebandChannel` trait abstracts non-PTY communication:

```rust
#[async_trait]
pub trait SidebandChannel: Send + Sync {
    /// Send a prompt via the sideband.
    async fn send_prompt(&self, prompt: &str) -> HomResult<String>;

    /// Poll for events from the sideband.
    async fn get_events(&self) -> HomResult<Vec<HarnessEvent>>;

    /// Check if the sideband is connected/healthy.
    async fn health_check(&self) -> HomResult<bool>;
}
```

Implementations:
- `HttpSideband` — for OpenCode's REST API (reqwest + rustls)
- `RpcSideband` — for pi-mono and Copilot CLI's JSON-RPC/ACP

### 4.4 Workflow Engine

The workflow engine parses YAML definitions, builds a DAG, and executes steps with templating, conditions, retries, checkpointing, and fallback.

#### YAML Schema

```yaml
name: plan-execute-validate
description: Multi-harness plan-execute-validate workflow
variables:
  planner: claude
  executor: codex
  reviewer: gemini
  task: ""

steps:
  - id: plan
    harness: "{{ planner }}"
    model: opus
    prompt: |
      Create a detailed implementation plan for: {{ task }}
    timeout: 5m

  - id: execute
    harness: "{{ executor }}"
    depends_on: [plan]
    prompt: |
      Implement the following plan:
      {{ steps.plan.output }}
    timeout: 10m
    retry:
      max_attempts: 2
      backoff: exponential

  - id: validate
    harness: "{{ reviewer }}"
    depends_on: [execute]
    condition: "steps.execute.status == completed"
    prompt: |
      Review and validate the implementation:
      {{ steps.execute.output }}
    timeout: 5m
    on_failure: skip

  - id: fallback-review
    harness: claude
    prompt: |
      The primary reviewer failed. Please review:
      {{ steps.execute.output }}
    timeout: 5m
```

#### DAG Construction

`WorkflowDag::from_steps()` builds a `petgraph::DiGraph` from step definitions:
- Nodes are step IDs
- Edges are `depends_on` relationships
- Cycle detection via `petgraph::algo::toposort()`
- `ready_steps(completed)` returns steps whose dependencies are all satisfied

#### Execution Flow

The `WorkflowExecutor` drives the main loop:

```rust
pub async fn execute(
    &self,
    def: &WorkflowDef,
    runtime: &dyn WorkflowRuntime,
    variables: HashMap<String, String>,
) -> HomResult<WorkflowResult>
```

For each batch of ready steps:

1. **Condition evaluation** — `evaluate_condition()` checks expressions like `steps.plan.status == completed` or `steps.execute.output contains "success"` against the accumulated step outputs and statuses. Steps with unmet conditions are skipped.

2. **Template rendering** — `build_template_context()` constructs a nested `serde_json::Value` so minijinja can resolve dot-access like `{{ steps.plan.output }}`:

```rust
fn build_template_context(
    vars: &HashMap<String, String>,
    step_outputs: &HashMap<String, String>,
    step_statuses: &HashMap<String, String>,
) -> serde_json::Value {
    // Top-level: all workflow variables
    // Nested: { "steps": { "plan": { "output": "...", "status": "..." }, ... } }
}
```

3. **Step field templating** — The `harness` and `model` fields are themselves templated, so `"{{ planner }}"` resolves to `"claude"` from variables.

4. **Retry with backoff** — Each step runs up to `max_attempts` times with configurable backoff:
   - `exponential`: 1s, 2s, 4s, 8s... (capped at 30s)
   - `linear`: 2s, 4s, 6s, 8s...
   - `fixed`: 2s constant

5. **Failure handling** — Per-step `on_failure` policy:
   - `abort` (default) — stop the workflow
   - `skip` — mark step as failed, continue to dependents
   - `fallback: <step_id>` — execute an alternative step

6. **Checkpointing** — After each successful step, a `WorkflowCheckpoint` is serialized to JSON (ready for SQLite persistence via hom-db).

#### WorkflowRuntime Trait

The TUI layer implements this to bridge the executor with the pane manager:

```rust
#[async_trait]
pub trait WorkflowRuntime: Send + Sync {
    async fn spawn_pane(&self, harness: &str, model: Option<&str>) -> HomResult<u32>;
    async fn send_and_wait(&self, pane_id: u32, prompt: &str, timeout: Duration) -> HomResult<String>;
    async fn kill_pane(&self, pane_id: u32) -> HomResult<()>;
}
```

### 4.5 Input Router

Handles the split between "user types into harness" and "user issues orchestrator command."

```rust
pub struct InputRouter {
    mode: InputMode,
}

pub enum InputMode {
    /// All input goes to the focused pane's PTY
    PaneInput { focused: PaneId },
    /// Input goes to the command bar (triggered by Ctrl-` hotkey)
    CommandBar,
    /// Workflow is running — input restricted to control commands
    WorkflowControl,
}
```

Input routing:
- **Ctrl-`** toggles between pane input and command bar
- **Ctrl-Tab / Ctrl-Shift-Tab** cycles pane focus
- **Ctrl-Q** quits
- **Mouse click on a different pane** — focuses that pane (`Action::FocusPane`)
- **Mouse events on the focused pane** — encoded as X10 protocol bytes (`ESC [ M <Cb> <Cx> <Cy>`) and written to the focused PTY; button/scroll events are forwarded, drag and move events are ignored
- **Escape** in command bar returns focus to the active pane
- **Terminal resize events** (`Event::Resize`) are handled in the main event loop — resize is propagated to all PTYs and terminal emulators

Mouse capture is enabled at startup via `crossterm::execute!(EnableMouseCapture)` and disabled on teardown, so all mouse events (not just left-click) flow through to the input router.

### 4.6 Command Bar

Parses and executes orchestrator-level commands:

| Command | Syntax | Status |
|---------|--------|--------|
| `:spawn` | `:spawn claude opus --dir /path -- extra args` | Implemented — reads config, supports model/dir/args |
| `:kill` | `:kill 1` or `:kill claude` | Implemented |
| `:focus` | `:focus 1` or `:focus claude` | Implemented |
| `:send` | `:send 1 "analyze this"` | Implemented — strips quotes, adapter-translated with newline |
| `:pipe` | `:pipe 1 -> 2` | Implemented — pipes screen snapshot text (not structured data) from source to target PTY |
| `:broadcast` | `:broadcast "stop"` | Implemented — adapter-translated per-pane |
| `:run` | `:run code-review --var task="add auth"` | Implemented — parses YAML, validates DAG, spawns WorkflowExecutor via WorkflowBridge |
| `:layout` | `:layout grid \| hsplit \| vsplit` | Implemented — recomputes areas, resizes all PTYs |
| `:save` | `:save my-session` | Implemented — serialises pane layout + harness config to SQLite via `hom-db::session::save_session` |
| `:restore` | `:restore my-session` | Implemented — loads session from SQLite and re-spawns panes via `hom-db::session::load_session` |
| `:help` | `:help` | Implemented — lists all commands |
| `:quit` | `:quit` | Implemented |

```rust
pub enum Command {
    Spawn { harness: HarnessType, model: Option<String>, working_dir: Option<PathBuf>, extra_args: Vec<String> },
    Kill(PaneSelector),
    Focus(PaneSelector),
    Send { target: PaneSelector, input: String },
    Pipe { source: PaneSelector, target: PaneSelector },
    Broadcast(String),
    Run { workflow: String, variables: HashMap<String, String> },
    Layout(LayoutKind),
    Save(String),
    Restore(String),
    Help,
    Quit,
}

pub enum PaneSelector {
    Id(u32),
    Name(String),  // case-insensitive substring match on pane title
}
```

### 4.7 Rendering Engine

Composites all panes plus chrome (status rail, command bar) into a single ratatui frame.

```rust
pub fn render(frame: &mut Frame, app: &App) {
    // Main layout: [status_rail] [pane_grid] [command_bar]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),      // Status rail
            Constraint::Min(10),        // Pane grid
            Constraint::Length(3),      // Command bar
        ])
        .split(frame.area());

    // Status rail: pane count, focused pane, workflow status, key hints
    render_status_rail(frame, chunks[0], pane_count, focused_pane, workflow_status);

    // Pane grid: compute areas from layout, render each pane
    let pane_areas = compute_pane_areas(chunks[1], &app.pane_order, &app.layout);
    for (pane_id, area) in &pane_areas {
        render_pane(frame, *area, pane, pane_id == focused);
    }

    // Command bar
    render_command_bar(frame, chunks[2], &app.command_bar);
}
```

**Pane rendering** (`pane_render.rs`) reads the `TerminalBackend::screen_snapshot()` and maps each cell to the ratatui buffer:

```rust
fn render_pane(frame: &mut Frame, area: Rect, pane: &Pane, focused: bool) {
    // Draw border (cyan bold if focused, dark gray otherwise)
    // Title: " harness_type [model] "

    let screen = pane.terminal.screen_snapshot();

    // Map each cell: character, fg/bg color, bold/italic/underline/dim/strikethrough
    for (row_idx, row) in screen.rows.iter().enumerate() {
        for (col_idx, cell) in row.cells.iter().enumerate() {
            // ... map to ratatui buffer cell
        }
    }

    // Set cursor position if this pane is focused and cursor is visible
}
```

**Layout engine** (`layout.rs`) supports three modes:
- `HSplit` — horizontal split (panes stacked vertically)
- `VSplit` — vertical split (panes side by side)
- `Grid` — automatic grid based on pane count

### 4.8 Web UI

`hom-web` is an axum 0.8 HTTP/WebSocket server that broadcasts live pane views to any browser.

**Architecture:**

```
hom-tui (tick loop)
  └── after each render tick:
        ScreenSnapshot serialised → WebFrame (JSON)
          └── broadcast channel → all connected WebSocket clients
                                       └── browser: Canvas2D cell renderer
```

- **`--web` flag** — enables the web server at startup; binds `localhost:4242` by default
- **`--web-port <N>` flag** — overrides the bind port
- **`WebFrame`** — serialised `ScreenSnapshot`: rows of cells with character, fg/bg color (ANSI 256-color palette), and text attributes
- **Canvas2D renderer** — XSS-safe (`fillText`, no `innerHTML`); renders the full ANSI 256-color palette
- **Keyboard input** — browser keystrokes are forwarded to the target pane via a `WebInput` channel; routed by `pane_id` so the web client can target any pane, not just the focused one
- **Error handling** — `WebServer::run()` returns `anyhow::Result<()>`; bind/serve errors are propagated and logged, not panicked

**Key types (`hom-web`):**

```rust
pub struct WebFrame {
    pub pane_id: u32,
    pub rows: Vec<Vec<WebCell>>,
    pub cursor: Option<(u16, u16)>,
    pub timestamp_ms: u64,
}

pub struct WebCell {
    pub ch: char,
    pub fg: u32,   // ANSI 256-color index or RGB packed
    pub bg: u32,
    pub attrs: u8, // bold | italic | underline | dim | ...
}

pub struct WebInput {
    pub pane_id: u32,
    pub key: String,   // e.g. "Enter", "Backspace", printable chars
}
```

### 4.9 Remote Pane

Remote panes run a harness process on a remote machine over SSH, bridging the remote PTY to a local pane as if the harness were running locally.

**Command syntax:**

```
:spawn claude --remote user@host
:spawn codex  --remote user@myserver.example.com:2222
```

**Architecture:**

- **`RemoteTarget`** — parsed from `user@host[:port]`; stored in `PaneKind::Remote { target: RemoteTarget }`
- **`RemotePtyManager`** in `crates/hom-pty/src/remote.rs` — uses `ssh2 = "0.9"` to establish an SSH session, open a channel with a PTY, and spawn the remote harness command
- **Auth chain** — tried in order: SSH agent (via `$SSH_AUTH_SOCK`) → `~/.ssh/id_ed25519` → `~/.ssh/id_rsa`
- **Remote command** — the harness `CommandSpec` is shell-quoted and executed on the remote host; environment variables from the adapter config are forwarded via `channel.setenv()`
- **PTY bridging** — the `RemotePtyManager` exposes the same read/write/resize interface as the local `PtyManager`; the rest of the TUI stack (terminal emulation, rendering, input routing) is unaware of the transport

**Key types (`hom-pty`):**

```rust
pub struct RemoteTarget {
    pub user: String,
    pub host: String,
    pub port: u16,  // default 22
}

pub enum PaneKind {
    Local,
    Remote { target: RemoteTarget },
}
```

**Dependency:** `ssh2 = "0.9"` added to `crates/hom-pty/Cargo.toml`.

### 4.10 Plugin System

The plugin system lets users load custom harness adapters at runtime without recompiling HOM. Plugins expose a stable C ABI so they can be built independently — even in languages other than Rust.

**Command syntax:**

```
:load-plugin /path/to/adapter.dylib
```

**Auto-load at startup:** HOM scans `~/.config/hom/plugins/` and loads all `*.dylib` / `*.so` files found there.

**Architecture:**

- **`hom-plugin` crate** — owns the C ABI definition, the `PluginLoader`, and a `PluginAdapter` wrapper that implements `HarnessAdapter` by calling through the vtable
- **`HomPluginVtable`** — `#[repr(C)]` struct of function pointers; ABI version guard constant `HOM_PLUGIN_ABI_VERSION = 1` prevents loading incompatible plugins
- **`HomInputKind`** — `#[repr(u32)]` enum used in the vtable for `translate_input` so plugins receive a stable integer discriminant rather than a Rust enum
- **`PluginLoader::scan_dir(path)`** — iterates a directory, `dlopen`s each shared library, reads the vtable version, and registers a `PluginAdapter` in the `AdapterRegistry` for each valid plugin
- **`hom_plugin_destroy`** — cleanup contract called by the loader when the plugin is unregistered or HOM exits; plugins must free any resources allocated by their vtable functions
- **Safety** — `dlopen` + FFI calls are wrapped in `unsafe` blocks; each block carries a `// SAFETY:` comment documenting the ABI contract and lifetime requirements

**C ABI vtable (`hom-plugin/src/lib.rs`):**

```rust
#[repr(C)]
pub struct HomPluginVtable {
    pub abi_version: u32,  // must equal HOM_PLUGIN_ABI_VERSION
    pub harness_type: extern "C" fn() -> *const c_char,
    pub display_name: extern "C" fn() -> *const c_char,
    pub build_command: extern "C" fn(config_json: *const c_char, out_json: *mut c_char, out_len: usize) -> i32,
    pub translate_input: extern "C" fn(kind: HomInputKind, text: *const c_char, out: *mut c_char, out_len: usize) -> i32,
    pub detect_completion: extern "C" fn(screen_json: *const c_char) -> i32,
    pub destroy: extern "C" fn(),
}

#[repr(u32)]
pub enum HomInputKind {
    Prompt  = 0,
    Cancel  = 1,
    Accept  = 2,
    Reject  = 3,
}

pub const HOM_PLUGIN_ABI_VERSION: u32 = 1;
```

---

## 5. Storage & State

### 5.1 SQLite Schema

Schema and CRUD functions are implemented in `hom-db`. Session save/restore and cost tracking are fully wired: `:save`/`:restore` call `hom-db::session` CRUD and `:run` logs cost via `hom-db::cost`.

```sql
-- Workflow executions
CREATE TABLE workflows (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    definition_path TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    variables       TEXT,  -- JSON
    started_at      INTEGER,
    completed_at    INTEGER,
    error           TEXT
);

-- Individual step results
CREATE TABLE steps (
    id              TEXT PRIMARY KEY,
    workflow_id     TEXT NOT NULL REFERENCES workflows(id),
    step_name       TEXT NOT NULL,
    harness         TEXT NOT NULL,
    model           TEXT,
    status          TEXT NOT NULL DEFAULT 'pending',
    prompt          TEXT,
    output          TEXT,
    error           TEXT,
    pane_id         INTEGER,
    tokens_input    INTEGER DEFAULT 0,
    tokens_output   INTEGER DEFAULT 0,
    cost_usd        REAL DEFAULT 0.0,
    started_at      INTEGER,
    completed_at    INTEGER,
    duration_ms     INTEGER,
    attempt         INTEGER DEFAULT 1
);

-- Session persistence
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    layout          TEXT NOT NULL,  -- JSON serialized Layout
    panes           TEXT NOT NULL,  -- JSON array of pane configs
    created_at      INTEGER,
    updated_at      INTEGER
);

-- Cost tracking
CREATE TABLE cost_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    pane_id         INTEGER,
    harness         TEXT NOT NULL,
    model           TEXT,
    tokens_input    INTEGER,
    tokens_output   INTEGER,
    cost_usd        REAL,
    timestamp       INTEGER
);

-- Workflow checkpoints (crash recovery)
CREATE TABLE checkpoints (
    workflow_id     TEXT NOT NULL,
    checkpoint_json TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    PRIMARY KEY (workflow_id)
);
```

### 5.2 Configuration

Config is loaded from `~/.config/hom/config.toml`. If the user file doesn't exist, the bundled `config/default.toml` is loaded (compiled into the binary via `include_str!`). Harness entries are looked up by canonical config key (e.g. `claude-code`, `pi-mono`) at spawn time — binary override, default model, and environment variables propagate through `spawn_pane()`.

```toml
# ~/.config/hom/config.toml

[general]
default_layout = "hsplit"
max_scrollback = 5000
max_panes = 8
render_fps = 30

[keybindings]
toggle_command_bar = "ctrl-`"
next_pane = "ctrl-tab"
prev_pane = "ctrl-shift-tab"
kill_pane = "ctrl-w"

[harnesses.claude-code]
command = "claude"
default_model = "opus"

[harnesses.codex]
command = "codex"
default_model = "codex-5.4"

[harnesses.pi-mono]
command = "pi"
default_model = "minimax-2.7"

[harnesses.copilot]
command = "copilot"
default_model = "sonnet-4.5"

[harnesses.gemini]
command = "gemini"
default_model = "gemini-2.0-flash"

[harnesses.kimi]
command = "kimi"
default_model = "kimi-2.5"

[harnesses.opencode]
command = "opencode"
default_model = "anthropic/claude-sonnet-4-5"
sideband = "http"
sideband_url = "http://localhost:4096"
```

The render FPS from `config.general.render_fps` controls the tick rate of the main event loop.

---

## 6. Project Structure (Workspace)

```
hom/
├── Cargo.toml                    # Workspace root (Rust 2024 edition)
├── CLAUDE.md                     # Development rules and project context
├── hom-system-design.md          # This document
├── .claude/
│   ├── rules/rust-patterns.md    # Rust style, API, type, and readability patterns
│   └── skills/rust-rig/SKILL.md  # Execution discipline: ATDD/TDD, DI, review workflow
├── config/
│   └── default.toml              # Default configuration (compiled in via include_str!)
├── workflows/                    # Built-in workflow templates (8 total)
│   ├── code-review.yaml
│   ├── plan-execute-validate.yaml
│   ├── multi-model-consensus.yaml
│   ├── test-driven-development.yaml
│   ├── debugging.yaml
│   ├── refactor-with-tests.yaml
│   ├── documentation.yaml
│   └── parallel-analysis.yaml
├── scripts/
│   └── seed-zig-cache.sh         # One-time runner provisioning for ghostty-backend CI
├── .github/
│   └── workflows/
│       └── ci.yml                # CI: fmt, clippy, test, ghostty (self-hosted, zig label)
│
├── crates/
│   ├── hom-core/                 # Core types, traits, errors (ZERO internal deps)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs          # PaneId, HarnessType, LayoutKind, etc.
│   │       ├── traits.rs         # TerminalBackend, HarnessAdapter, SidebandChannel
│   │       ├── error.rs          # HomError (thiserror)
│   │       └── config.rs         # HomConfig, GeneralConfig, HarnessEntry
│   │
│   ├── hom-terminal/             # Terminal emulation (ghostty default, vt100 opt-in fallback)
│   │   ├── Cargo.toml            # depends on libghostty-vt (default); vt100-backend opt-in
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ghostty.rs        # GhosttyBackend — fully implemented, default (needs Zig ≥0.15.x)
│   │       ├── fallback_vt100.rs # Vt100Backend — opt-in fallback (no external deps)
│   │       └── color_map.rs      # Terminal color → ratatui color mapping
│   │
│   ├── hom-pty/                  # PTY management
│   │   ├── Cargo.toml            # depends on portable-pty, tokio
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── manager.rs        # PtyManager: spawn, read, write, resize, kill
│   │       └── async_reader.rs   # AsyncPtyReader: tokio channel bridge
│   │
│   ├── hom-adapters/             # Harness adapters (all 7)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # AdapterRegistry
│   │       ├── claude_code.rs
│   │       ├── codex.rs
│   │       ├── pi_mono.rs
│   │       ├── copilot.rs
│   │       ├── gemini.rs
│   │       ├── kimi.rs
│   │       ├── opencode.rs
│   │       └── sideband/
│   │           ├── mod.rs
│   │           ├── http.rs       # HttpSideband (OpenCode REST API)
│   │           └── rpc.rs        # RpcSideband (pi-mono, Copilot JSON-RPC)
│   │
│   ├── hom-workflow/             # Workflow engine
│   │   ├── Cargo.toml            # depends on petgraph, minijinja, serde_yaml_ng
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── parser.rs         # YAML → WorkflowDef, StepDef, RetryDef, FailureAction
│   │       ├── dag.rs            # WorkflowDag: petgraph DAG + topological sort
│   │       ├── executor.rs       # WorkflowExecutor: retry, conditions, templating
│   │       ├── condition.rs      # evaluate_condition(): ==, !=, contains
│   │       └── checkpoint.rs     # WorkflowCheckpoint: JSON serialization
│   │
│   ├── hom-tui/                  # TUI rendering + input handling
│   │   ├── Cargo.toml            # depends on ratatui, crossterm
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── app.rs            # App state, spawn_pane, poll_pty_output
│   │       ├── render.rs         # Frame rendering, welcome screen
│   │       ├── pane_render.rs    # Cell-by-cell terminal → ratatui mapping
│   │       ├── input.rs          # InputRouter: pane input vs command bar vs WorkflowControl
│   │       ├── command_bar.rs    # Command parsing with --var, --dir, quote stripping
│   │       ├── layout.rs         # HSplit, VSplit, Grid layout computation
│   │       ├── status_rail.rs    # Top bar: HOM branding, pane count, workflow status
│   │       ├── db_checkpoint.rs  # DbCheckpointStore: CheckpointStore trait → hom-db
│   │       ├── workflow_bridge.rs # Channel bridge between TUI event loop and WorkflowExecutor
│   │       └── workflow_progress.rs # WorkflowProgress type for F9 step-count display
│   │
│   └── hom-db/                   # Storage layer
│       ├── Cargo.toml            # depends on sqlx (SQLite)
│       └── src/
│           ├── lib.rs
│           ├── migrations/       # SQL migrations (001_initial.sql)
│           ├── session.rs        # Session save/restore CRUD
│           ├── workflow.rs       # Workflow + step state persistence
│           └── cost.rs           # Cost tracking (log_cost, total_cost)
│
├── src/
│   └── main.rs                   # Binary entry point: CLI, event loop, command dispatch
│
├── skills/                       # Superpowers-compatible skill definitions
│   ├── hom-adapter-development/SKILL.md
│   ├── hom-tui-testing/SKILL.md
│   ├── hom-workflow-authoring/SKILL.md
│   └── hom-terminal-integration/SKILL.md
│
└── docs/superpowers/plans/       # Implementation plans
```

### Crate Dependency Rules

```
hom-core         → (no internal deps — root of the dependency tree)
hom-terminal     → hom-core
hom-pty          → hom-core
hom-adapters     → hom-core
hom-workflow     → hom-core
hom-db           → hom-core
hom-tui          → hom-core, hom-terminal, hom-pty, hom-adapters, hom-workflow, hom-db
src/main.rs      → all crates
```

---

## 7. Dependency Map

```toml
# Workspace Cargo.toml [workspace.dependencies]

# Terminal emulation
libghostty-vt = "0.1.1"           # GhosttyBackend — default backend, requires Zig ≥0.15.x
vt100 = "0.16"                    # Vt100Backend — opt-in fallback, pure Rust, no external build deps

# PTY management
portable-pty = "0.9"

# TUI
ratatui = "0.30"
crossterm = "0.29"

# Async runtime
tokio = { version = "1", features = ["full"] }

# Workflow engine
petgraph = "0.8"
serde = { version = "1.0", features = ["derive"] }
serde_yaml_ng = "0.10"              # YAML support (migrated from serde_yml; RUSTSEC-2025-0068 resolved)
serde_json = "1.0"
minijinja = "2"
backoff = { version = "0.4", features = ["tokio"] }

# Storage
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }

# HTTP client (OpenCode sideband) — rustls to avoid native OpenSSL dep
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls"] }

# Configuration
toml = "1.1"
dirs = "6.0"

# Utilities
uuid = { version = "1.0", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow = "1.0"
thiserror = "2.0"
clap = { version = "4.0", features = ["derive"] }
async-trait = "0.1"
```

---

## 8. Remaining Work

| Area | Item | Status |
|------|------|--------|
| Terminal | GhosttyBackend implementation | **RESOLVED** — `libghostty-vt 0.1.1` fully wired; all trait methods implemented; 8 unit tests pass; `unsafe Send + Sync` with documented invariant |
| Workflow | Parallel execution | **RESOLVED** — `Arc<dyn WorkflowRuntime>` + `JoinSet` for concurrent batch execution |
| Workflow | SendAndWait completion | **RESOLVED** — `PendingCompletion` polling via `detect_completion()` |
| Workflow | Sideband async bridge | **RESOLVED** — SendAndWait uses `sideband.send_prompt()` for sideband-capable panes |
| Workflow | Template library | **RESOLVED** — 8 built-in templates: code-review, plan-execute-validate, multi-model-consensus, TDD, debugging, refactor-with-tests, documentation, parallel-analysis |
| Adapters | `parse_screen()` | **RESOLVED** — all 7 adapters have real implementations (JSONL, screen text patterns) |
| Adapters | RPC get_events() | **RESOLVED** — non-blocking `try_lock` + 1ms timeout; parses JSON-RPC notifications |
| Adapters | `detect_completion()` | **RESOLVED** — `last_non_empty_line()` + anchored `starts_with()` patterns per adapter; error detection added |
| Adapters | RPC sideband | **RESOLVED** — full JSON-RPC subprocess implementation with stdin/stdout communication |
| Adapters | HTTP sideband | **RESOLVED** — SSE event polling via GET /global/event; in-module tests in `crates/hom-adapters/src/sideband/http.rs` |
| Adapters | Copilot ACP | **RESOLVED** — `--acp --stdio` mode support, sideband via JSON-RPC |
| Adapters | `build_command`/`translate_input` tests | **RESOLVED** — all 7 adapters have `build_command` (default, with model, binary override, extra args) and `translate_input` (prompt, cancel, accept, reject) tests; 91 total tests in hom-adapters |
| DB | Session save/restore | **RESOLVED** — `:save`/`:restore` wired to `hom_db::session` CRUD with JSON serialization |
| DB | Cost tracking | **RESOLVED** — `log_cost()` called from workflow steps and token usage events; total displayed in status rail (F10) |
| DB | Fail-fast on error | **RESOLVED** — DB open failure exits with clear message; `--no-db` flag opts out explicitly |
| Config | Env var expansion | **RESOLVED** — `${VAR}` interpolated in TOML values after loading |
| Config | Keybinding config | **RESOLVED** — `KeybindingsConfig` applied to `InputRouter::from_config()`; invalid strings warned at startup |
| TUI | Mouse passthrough | **RESOLVED** — `encode_mouse_event()` X10-encodes scroll/button events; `EnableMouseCapture`/`DisableMouseCapture` wired in main.rs; click-to-focus still works |
| TUI | Cost display | **RESOLVED** — total cost polled from DB and shown as `$X.XX` in status rail (F10) |
| TUI | Workflow progress | **RESOLVED** — `WorkflowProgress` type replaces stringly-typed status; step counts shown in status rail (F9) |
| TUI | Graceful PTY shutdown | **RESOLVED** — `App::shutdown()` + `PtyManager::kill_all()` called on Ctrl-Q/`:quit` |
| TUI | Process crash handling | **RESOLVED** — exited panes show `[EXITED: N]` in red; command bar notified; pending workflow steps unblocked |
| TUI | Sideband health polling | **RESOLVED** — `health_check()` called every ~5s in main loop; notifies on failure |
| TUI | AsyncPtyReader cancellation | **RESOLVED** — `abort()` method added; called in `kill_pane()` before pane removal |
| TUI | Claude Code flickering | **DOCUMENTED** — Ink/React renderer causes 4,000-6,700 scroll events/sec in any multiplexer; headless workaround documented in `claude_code.rs` |
| TUI | handle_command refactor | **RESOLVED** — per-command handler functions extracted; `handle_spawn`, `handle_save`, `handle_restore`, `handle_run`, etc. |
| CI | GhosttyBackend CI job | **RESOLVED** — `ghostty` job in `.github/workflows/ci.yml` targets `[self-hosted, zig]` runner; `scripts/seed-zig-cache.sh` provisions Zig package cache |
| Tests | E2E PTY pipeline | **RESOLVED** — spawn→read (echo), spawn→write→read (cat), PTY→Vt100→ScreenSnapshot |
| Tests | Terminal emulator integration | **RESOLVED** — 6 vt100 unit tests (plain text, ANSI color, resize, cursor, scroll, attrs) + 4 async pipeline integration tests in `crates/hom-terminal/tests/async_pipeline.rs` |
| Performance | NFR benchmarks | **VALIDATED** — all 4 measurable NFRs pass: NF1 47µs (<16ms), NF2 12.8µs/1kkeys (<50ms), NF3 20.2MB (<30MB at default 5k scrollback), NF4 9.3µs (<500ms) |
| Web UI | WebSocket live pane viewer | **RESOLVED** — `hom-web` crate with axum 0.8; `WebFrame` broadcast to all clients after each tick; Canvas2D cell renderer; per-pane keyboard input via `pane_id`; `--web` / `--web-port` flags |
| Remote Pane | SSH remote harness spawning | **RESOLVED** — `RemoteTarget` + `PaneKind::Remote`; `RemotePtyManager` in `hom-pty/src/remote.rs`; `ssh2 = "0.9"` dep; auth chain: agent → id_ed25519 → id_rsa; `:spawn <harness> --remote user@host[:port]` |
| Plugin System | Runtime harness adapter loading | **RESOLVED** — `hom-plugin` crate; `HomPluginVtable` `#[repr(C)]`; `HomInputKind` `#[repr(u32)]`; `HOM_PLUGIN_ABI_VERSION = 1` guard; `PluginLoader::scan_dir`; auto-scan `~/.config/hom/plugins/` at startup; `:load-plugin <path>` command |

---

## 9. Trade-Off Analysis

### 9.1 Terminal backend strategy

| Decision | Trade-off |
|----------|-----------|
| **Default: libghostty-rs** | Best VT emulation quality, Kitty keyboard + graphics protocol, proven in cmux |
| **Opt-in fallback: vt100** | Stable, pure Rust, zero external build deps — for environments without Zig ≥0.15.x |
| **Accepted** | Zig ≥0.15.x build dependency and API instability risk for default ghostty path |
| **Mitigated by** | `TerminalBackend` trait abstraction — vt100 works as fallback; ghostty is default but swappable without touching other layers |
| **Revisit when** | libghostty-rs hits 1.0 (expected late 2026) — stabilise API, consider removing vt100 fallback |

### 9.2 PTY-first vs headless-first

| Decision | Trade-off |
|----------|-----------|
| **Chose visual PTY panes** | Users see real harness TUIs — full transparency and direct interaction |
| **Accepted** | Harder to extract structured data (screen parsing vs JSON) |
| **Mitigated by** | Optional sideband channels, dual-process pattern for critical workflows |
| **Revisit when** | Harnesses add dual-mode support (visual TUI + JSON sideband in same process) |

### 9.3 YAML workflows vs code-defined

| Decision | Trade-off |
|----------|-----------|
| **Chose YAML** | Accessible to non-Rust users, portable, matches industry convention |
| **Accepted** | Less expressive than Rust code for complex logic |
| **Mitigated by** | Condition expressions, minijinja templating, retry policies in YAML |
| **Revisit when** | Users need Turing-complete workflow logic — add Lua or Rhai scripting |

### 9.4 Single binary + plugin system

| Decision | Trade-off |
|----------|-----------|
| **Chose C ABI shared library plugins** | Community can add custom adapters without recompiling HOM |
| **Accepted** | `unsafe` FFI at the plugin boundary; ABI version must be respected |
| **Mitigated by** | `HOM_PLUGIN_ABI_VERSION` guard rejects incompatible plugins; `// SAFETY:` on every `unsafe` block; `hom_plugin_destroy` contract for clean teardown |
| **Revisit when** | WASM runtimes (e.g. wasmtime) stabilise enough to replace `dlopen` — WASM would provide memory isolation at the plugin boundary |

### 9.5 Claude Code rendering quality

| Decision | Trade-off |
|----------|-----------|
| **Known issue** | Claude Code (Ink/React) flickers in ALL multiplexers (4,000-6,700 scroll events/sec) |
| **Accepted** | Visual rendering of Claude Code will have artifacts |
| **Mitigated by** | For automated workflow steps, use headless mode + separate visual monitoring pane |
| **Revisit when** | Claude Code ships improved differential renderer or switches to ratatui |

---

## 10. Risk Register

| Risk | Severity | Probability | Mitigation |
|------|----------|-------------|------------|
| libghostty-rs API breaking change | High | High (pre-1.0) | Pin commit hash; TerminalBackend trait abstraction; vt100 opt-in fallback available if ghostty API breaks |
| Zig build fails on user's system | Medium | Low | Fallback build uses vt100 (no Zig needed): `--no-default-features --features vt100-backend`; README documents this clearly |
| Claude Code flickering in panes | Medium | Certain | Headless mode for workflows, visual for direct interaction only |
| Screen parsing unreliable for output extraction | Medium | Medium | Start with prompt-based patterns, add sideband channels progressively |
| PTY input race conditions | Medium | Medium | Wait for shell readiness, configurable delay, use CommandBuilder for initial commands |
| Harness CLI updates break adapters | Low | Medium | Version-pin harness CLIs, adapter trait isolates changes, integration tests |
| Performance with 7+ concurrent panes | Low | Low | Lazy rendering (only update visible panes), throttle PTY reads |

---

## 11. Future Directions

**Resolved across all phases:**
- ~~Wire session save/restore~~ → `:save`/`:restore` wired to hom-db (Phase 3)
- ~~Wire cost tracking~~ → `log_cost()` from workflow + token events, displayed in status rail (Phase 3/4)
- ~~Implement `parse_screen()`~~ → all 7 adapters have real implementations (Phase 3)
- ~~Integration-test sideband channels~~ → HTTP sideband tested, RPC implemented (Phase 3)
- ~~Parallel step execution~~ → `Arc<dyn WorkflowRuntime>` + JoinSet (Phase 3)
- ~~SendAndWait completion detection~~ → PendingCompletion polling (Phase 3)
- ~~Keybinding config wiring~~ → KeybindingsConfig applied to InputRouter (Phase 3)
- ~~Workflow template library~~ → 8 built-in templates (Phase 4)
- ~~Cost display in status rail~~ → total cost shown as `$X.XX` (Phase 4)
- ~~Workflow progress tracking~~ → `WorkflowProgress` type + step counts in status rail (Phase 4)
- ~~Terminal emulator integration tests~~ → 5 vt100 tests (Phase 4)
- ~~Graceful PTY shutdown~~ → `App::shutdown()` + `PtyManager::kill_all()` (Phase 5)
- ~~Process crash handling~~ → `[EXITED: N]` in red (Phase 5)
- ~~Database reliability~~ → fail fast on error, `--no-db` flag (Phase 5)
- ~~detect_completion() reliability~~ → `last_non_empty_line()` + anchored patterns (Phase 6)
- ~~E2E PTY pipeline tests~~ → spawn/read/write/Vt100 (Phase 6)
- ~~Process exit notification~~ → command bar notified on harness exit (Phase 7)
- ~~Sideband health polling~~ → `health_check()` every ~5s (Phase 7)
- ~~Keybinding validation~~ → warns on invalid config at startup (Phase 7)
- ~~AsyncPtyReader cancellation~~ → `abort()` on pane kill (Phase 7)
- ~~GhosttyBackend implementation~~ → `libghostty-vt 0.1.1` fully wired (Phase 7)
- ~~Mouse passthrough~~ → X10 encoding + `EnableMouseCapture` wired (P4 session)
- ~~Adapter build_command/translate_input tests~~ → all 7 adapters at 91 tests total (P4 session)
- ~~GhosttyBackend CI job~~ → `[self-hosted, zig]` runner + seed script (P4 session)

- ~~Web UI~~ → `hom-web` crate with axum 0.8 WebSocket server + Canvas2D renderer (April 11, 2026)
- ~~Remote pane support~~ → `RemotePtyManager` over ssh2, `:spawn --remote user@host[:port]` (April 11, 2026)
- ~~Plugin system for adapters~~ → `hom-plugin` crate, `HomPluginVtable` C ABI, auto-scan at startup (April 11, 2026)

**Active future work:**
1. **Linux platform validation** — `cargo check` + test suite on Linux; NF6 target not yet validated
2. **GPU rendering** — leverage libghostty's pipeline for complex output
3. **Workflow marketplace** — share and discover templates
4. **Agent-to-agent protocol** — standardized message format
