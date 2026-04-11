# HOM Workspace Architecture and Rules

This reference preserves the important stable repository guidance from `CLAUDE.md`.

## Project Identity

HOM is a Rust-based TUI terminal multiplexer and orchestrator for AI coding harnesses. Each pane runs a real harness process inside a PTY and full terminal emulator, then maps the screen state cell-by-cell into ratatui buffers.

This is not a headless orchestrator.

## Environment

- Rust 2024 edition for all crates
- declare `rust-version` in `Cargo.toml`
- `ghostty-backend` is the default terminal backend
- `vt100-backend` is the opt-in fallback backend

## Supported Harnesses

Tier 1, full orchestration or steering:
- Claude Code CLI
- pi-mono
- OpenCode
- GitHub Copilot CLI

Tier 2, headless or limited steering:
- Codex CLI
- Gemini CLI
- kimi-cli

## Workspace Layout

Important crates:
- `hom-core`: shared types, traits, config, errors
- `hom-terminal`: terminal emulation backends
- `hom-pty`: PTY spawn, read, write, resize, kill
- `hom-adapters`: all harness adapters and sideband integrations
- `hom-workflow`: workflow parser, DAG, executor, conditions, checkpointing
- `hom-tui`: app state, rendering, input router, command bar, layout engine
- `hom-db`: SQLite persistence
- `hom-mcp`: MCP server
- `hom-web`: web viewer
- `hom-plugin`: plugin ABI and loading

## Dependency Rules

- `hom-core` has zero internal crate dependencies
- `hom-terminal` depends on `hom-core` only
- `hom-pty` depends on `hom-core` only
- `hom-plugin` depends on `hom-core` only
- `hom-adapters` depends on `hom-core` and `hom-plugin`
- `hom-workflow` depends on `hom-core` only
- `hom-db` depends on `hom-core` only
- `hom-mcp` depends on `hom-core` only
- `hom-web` depends on `hom-core` only
- `hom-tui` depends on the integration set of workspace crates
- the binary depends on all crates

Never modify `hom-core` traits without considering all adapters and dependent crates.

## Key Technical Decisions

- terminal emulation is abstracted behind `TerminalBackend`
- TUI-inside-TUI rendering is a core architectural constraint
- sideband channels are abstracted behind `SidebandChannel`
- workflow execution uses YAML plus DAG execution
- configuration, ports, models, paths, and runtime behavior must come from typed config or injected inputs, not hardcoded values

## Development Rules

- inspect workspace context before editing
- match existing patterns
- keep behavior changes, tests, and docs together
- prefer the smallest coherent change
- pass dependencies explicitly
- verify locally before handing off

## Code Quality Rules

- keep responsibilities separate across domain, transport, persistence, configuration, and presentation
- avoid dumping-ground modules
- keep code readable and intention-revealing
- use comments for intent, invariants, safety, and tradeoffs only
- do not ship undocumented `unsafe`
- prefer compile-time guarantees over runtime interpretation

## Concurrency Rules

- only use concurrency when it materially helps
- never hold a lock across `.await`
- do not create detached tasks without a shutdown path
- use `spawn_blocking` for blocking work in async contexts
- prefer ownership transfer or message passing to shared mutable state

## Testing Discipline

- no meaningful code change ships without tests
- define acceptance behavior first for user-visible changes
- use TDD for each implementation increment
- cover happy path, invalid input, edge cases, and failure paths
- add concurrency tests when task orchestration or locking behavior can fail

## Verification Commands

- `cargo check`
- `cargo build`
- `cargo test`
- `cargo nextest run`
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo doc --no-deps`
- `cargo audit`
- `cargo deny check`

When feature or public API changes are involved, also run:
- `cargo test --all-features`
- `cargo test --no-default-features`
- `cargo test --doc`
- `cargo doc --no-deps`

## Minimum Gate

These must pass before handing off code changes:
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run` or `cargo test`

## Review Rejects

Reject these patterns:
- unnecessary dependencies
- speculative abstractions
- hidden coupling
- hardcoded values
- `unwrap()` in production code without a strong invariant
- undocumented `unsafe`
- detached tasks without shutdown
- locks across `.await`
- blocking work in async contexts
- stringly typed state
- transport inside domain logic
- missing tests
- missing public docs when public APIs change
