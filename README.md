# HOM — Harness Orchestration Management

[![CI](https://github.com/mudrii/hom/actions/workflows/ci.yml/badge.svg)](https://github.com/mudrii/hom/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

A Rust TUI terminal multiplexer that orchestrates 7 AI coding agent CLIs in a single window. Each pane runs a **real terminal emulator** — you see and interact with each agent exactly as if it were running in its own terminal.

```
┌─────────────────────────────────────────────────────────────────────┐
│ ⏵ workflow: code-review (step 2/5) │ panes: 4 │ cost: $0.42 │ HOM  │
├──────────────────────────┬──────────────────────────────────────────┤
│ [1] Claude Code [opus] ★ │ [2] Codex CLI [codex-5.4]               │
│                          │                                          │
│ ❯ Analyzing codebase...  │ Waiting for input...                     │
│ ✓ Found 47 files         │ (will receive plan from pane 1           │
│ ✓ Auth module at src/    │  via workflow pipe)                      │
│                          │                                          │
├──────────────────────────┼──────────────────────────────────────────┤
│ [3] Copilot [sonnet-4.5] │ [4] kimi-cli [kimi-2.5]                 │
│                          │                                          │
│ Waiting: validate        │ Waiting: security-review                 │
│ depends_on: [implement]  │ depends_on: [implement]                  │
│                          │                                          │
├──────────────────────────┴──────────────────────────────────────────┤
│ : _                                                                 │
└─────────────────────────────────────────────────────────────────────┘
```

## What it does

HOM lets you run multiple AI coding agents side by side, coordinate them with YAML workflows, and pipe their outputs into each other — all without leaving your terminal. Workflows execute as DAGs: steps run in dependency order, in parallel where possible, with retries, conditions, and SQLite checkpointing.

## Supported Harnesses

| Harness | Binary | Tier | Sideband |
|---|---|---|---|
| Claude Code CLI | `claude` | Full steering | stream-json stdin/stdout |
| pi-mono | `pi` | Full steering | JSON-RPC subprocess |
| OpenCode | `opencode` | Full steering | HTTP REST + SSE (localhost:4096) |
| GitHub Copilot CLI | `copilot` | Full steering | JSON-RPC ACP (`--acp --stdio`) |
| Codex CLI | `codex` | Screen parsing | JSONL events |
| Gemini CLI | `gemini` | Screen parsing | JSON output |
| kimi-cli | `kimi` | Screen parsing | stream-json |

## Installation

**Prerequisites:** The AI CLI tools you want to use must be installed separately (e.g. `claude`, `codex`, `gemini`).

**Build from source:**

```sh
git clone https://github.com/mudrii/hom
cd hom
cargo build --release   # uses ghostty backend by default (requires Zig ≥ 0.15.x)
./target/release/hom
```

**No Zig? Use the vt100 fallback backend:**

```sh
cargo build --release --no-default-features --features vt100-backend
```

## Quick Start

```sh
# Spawn a Claude Code pane
:spawn claude

# Spawn a second pane with Codex
:spawn codex

# Send a prompt to the focused pane
:send "refactor the auth module to use JWT"

# Broadcast the same prompt to all panes
:broadcast "explain the database schema"

# Run a workflow
:run workflows/code-review.yaml
```

## Commands

| Command | Description |
|---|---|
| `:spawn <harness> [--model M] [--dir D]` | Open a new pane running the named harness |
| `:send <text>` | Send text to the focused pane |
| `:broadcast <text>` | Send text to all active panes |
| `:pipe <src> <dst>` | Pipe output from one pane into another |
| `:run <workflow.yaml> [--var k=v]` | Execute a YAML workflow |
| `:save <name>` | Save the current session layout to SQLite |
| `:restore <name>` | Restore a saved session |
| `:focus <n>` | Focus pane number n |
| `:layout <mode>` | Switch layout (single / hsplit / vsplit / grid / tabbed) |
| `:kill [n]` | Kill the focused or numbered pane |
| `:quit` | Exit HOM cleanly |

**Keyboard shortcuts:** `Tab` — next pane, `Ctrl-Q` — quit, `:` — enter command mode, `F9` — workflow progress, `F10` — cost display.

## Workflow Automation

Workflows are YAML files that define steps with dependencies. HOM builds a DAG, runs steps in topological order, and supports parallel execution, retries, and conditions.

```yaml
name: code-review
steps:
  plan:
    harness: claude
    model: opus
    prompt: "Analyse {{ target_dir }} and produce an implementation plan."

  implement:
    harness: codex
    depends_on: [plan]
    prompt: "Implement the plan:\n{{ steps.plan.output }}"
    retry:
      max_attempts: 3
      strategy: exponential

  validate:
    harness: copilot
    depends_on: [implement]
    prompt: "Review the implementation for correctness and style."

  security-review:
    harness: kimi
    depends_on: [implement]
    prompt: "Check for security issues in the implementation."
```

Eight built-in templates are included in `workflows/`:
`code-review`, `plan-execute-validate`, `multi-model-consensus`, `test-driven-development`, `debugging`, `refactor-with-tests`, `documentation`, `parallel-analysis`.

## Architecture

HOM is a 7-crate Rust workspace. The hardest technical challenge is **TUI-inside-TUI**: each pane runs a real terminal emulator (`libghostty-rs` default, `vt100` opt-in fallback) whose screen state is mapped cell-by-cell into ratatui buffers.

- [`hom-system-design.md`](hom-system-design.md) — full architecture reference
- [`hom-architecture.html`](hom-architecture.html) — interactive diagram (open in browser)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports, new adapters, workflow templates, and code contributions are all welcome.

## License

Apache License 2.0 — see [LICENSE](LICENSE).
