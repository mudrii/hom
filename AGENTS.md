# AGENTS.md

<!-- context7 -->
Use the `ctx7` CLI to fetch current documentation whenever the user asks about a library, framework, SDK, API, CLI tool, or cloud service. This includes API syntax, configuration, version migration, library-specific debugging, setup instructions, and CLI tool usage. Prefer this over web search for library docs.

Do not use `ctx7` for refactoring, writing scripts from scratch, debugging business logic, code review, or general programming concepts.

Steps:
1. Resolve library: `npx ctx7@latest library <name> "<user's question>"`
2. Pick the best match by exact name match, description relevance, code snippet count, source reputation, and benchmark score
3. Fetch docs: `npx ctx7@latest docs <libraryId> "<user's question>"`
4. Answer using the fetched documentation

Do not run more than 3 `ctx7` commands per question. If `ctx7` fails with a quota error, tell the user and suggest `npx ctx7@latest login` or setting `CONTEXT7_API_KEY`.
<!-- context7 -->

# HOM Repository Instructions

These instructions are the Codex-compatible version of the important repository guidance that was previously split across `CLAUDE.md`, `.claude/rules/`, and `.claude/skills/`.

## Always Load These Local Skills

For any meaningful code change in this repository:
- Read [skills/hom-workspace-standards/SKILL.md](skills/hom-workspace-standards/SKILL.md)

For Rust implementation, refactor, bugfix, architecture, or review work:
- Read [skills/rust-rig/SKILL.md](skills/rust-rig/SKILL.md)

Load the relevant domain skill before touching that area:
- [skills/hom-adapter-development/SKILL.md](skills/hom-adapter-development/SKILL.md)
- [skills/hom-terminal-integration/SKILL.md](skills/hom-terminal-integration/SKILL.md)
- [skills/hom-tui-testing/SKILL.md](skills/hom-tui-testing/SKILL.md)
- [skills/hom-workflow-authoring/SKILL.md](skills/hom-workflow-authoring/SKILL.md)

## Project Identity

HOM is a Rust-based TUI terminal multiplexer and orchestrator for AI coding harnesses. It spawns real native harness TUIs in visual panes using PTYs and a terminal emulator. This is not a headless orchestrator.

Supported harness tiers:
- Tier 1: `claude`, `pi`, `opencode`, `copilot`
- Tier 2: `codex`, `gemini`, `kimi`

## Environment

- Rust 2024 edition across the workspace
- `rust-version` must be declared explicitly
- `ghostty-backend` is the default terminal backend
- `vt100-backend` is the opt-in fallback path

## Mandatory Workflow

- Inspect `Cargo.toml`, crate layout, relevant skill files, and existing tests before editing
- Match existing patterns and prefer the smallest coherent change
- Keep behavior changes, tests, and docs in the same change
- Pass dependencies explicitly; do not hardcode ports, paths, URLs, credentials, or feature switches
- Verify locally before handing off

For feature work or multi-step changes, write a plan in:
- `docs/superpowers/plans/YYYY-MM-DD-<feature>.md`

## Architecture Rules

- `hom-core` has zero internal crate dependencies
- `hom-terminal`, `hom-pty`, `hom-plugin`, `hom-workflow`, `hom-db`, `hom-mcp`, and `hom-web` only depend on `hom-core`
- `hom-adapters` depends on `hom-core` and `hom-plugin`
- `hom-tui` is the integration crate and depends on the other workspace crates
- The binary in `src/main.rs` depends on all workspace crates
- Never modify `hom-core` traits without checking impact across all harness adapters and dependent crates

## Design and Code Rules

- Keep crates, modules, types, and functions focused on one clear responsibility
- Avoid dumping-ground modules like `utils`, `helpers`, or `common`
- Prefer strict types, enums, newtypes, and validated constructors over stringly typed state
- Use `thiserror` for library errors and keep `anyhow` at binary/orchestration boundaries
- Do not use casual `unwrap()` or `expect()` in production code
- No globals, hidden singletons, or hardcoded collaborator construction in core logic
- Separate domain logic from transport, persistence, configuration, and presentation
- Comments should explain intent, invariants, ownership, safety, or tradeoffs, not restate the code
- Every `unsafe` block must have a `// SAFETY:` comment

## Concurrency Rules

- Do not hold locks across `.await`
- Do not add detached tasks without a shutdown path and an owning error boundary
- Use `tokio` consistently
- Use `spawn_blocking` for blocking work from async code
- Prefer ownership transfer or message passing over shared mutable state

## Testing and Verification

- ATDD first for user-visible behavior, then TDD for each smaller increment
- Cover happy path, invalid input, edge cases, and failure paths for meaningful changes
- Add concurrency-focused tests when async, locking, or task behavior can fail
- Keep tests deterministic and avoid sleep-based timing where practical

Run these commands as appropriate to the affected scope:
- `cargo check`
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run` or `cargo test`
- `cargo test --all-features`
- `cargo test --no-default-features`
- `cargo test --doc`
- `cargo doc --no-deps`

Minimum gate before handing off code changes:
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run` or `cargo test`

## Reject Patterns

Reject these patterns unless the user explicitly requires them and the tradeoff is documented:
- unnecessary dependencies
- speculative abstractions
- hidden coupling
- hardcoded values
- undocumented `unsafe`
- blocking work in async code
- locks held across `.await`
- detached tasks without shutdown
- stringly typed state
- transport or storage concerns embedded in domain logic
- missing tests or stale docs
