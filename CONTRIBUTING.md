# Contributing to HOM

Thank you for your interest in contributing. HOM is a Rust TUI project — contributions of all kinds are welcome: bug reports, new harness adapters, workflow templates, documentation, and code improvements.

## Ways to Contribute

- **Bug reports** — open a GitHub issue using the bug report template
- **Feature requests** — open a GitHub issue using the feature request template
- **New harness adapter** — open a GitHub issue using the new adapter template, then submit a PR
- **Workflow templates** — add a YAML file to `workflows/` following the existing patterns
- **Documentation** — improve `README.md`, `hom-system-design.md`, or code comments
- **Code** — fix bugs, improve performance, add tests

## Development Setup

**Prerequisites:**
- Rust 1.85 or later: https://rustup.rs
- `cargo-nextest` (preferred test runner): `cargo install cargo-nextest`

**Build and test:**

```sh
git clone https://github.com/mudrii/hom
cd hom
cargo build
cargo nextest run
```

**Optional — GhosttyBackend (requires Zig ≥ 0.15.x):**

```sh
cargo build --features ghostty-backend
```

## Before Submitting a PR

Every PR must pass this gate before review:

```sh
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run
```

All three commands must exit clean. PRs that fail any of these will not be reviewed until they pass.

## Adding a New Harness Adapter

Read `skills/hom-adapter-development/SKILL.md` before starting. Every adapter must implement four methods on the `HarnessAdapter` trait:

| Method | Purpose |
|---|---|
| `build_command` | Construct the `CommandSpec` for spawning the process |
| `translate_input` | Encode an `OrchestratorCommand` into PTY bytes |
| `detect_completion` | Classify a `ScreenSnapshot` as `Running`, `WaitingForInput`, or `Failed` |
| `parse_screen` | Extract structured `HarnessEvent`s from a `ScreenSnapshot` |

Tests are mandatory — cover `build_command`, `translate_input`, and `detect_completion` at minimum. See any existing adapter in `crates/hom-adapters/src/` for the pattern.

## Adding a Workflow Template

Add a YAML file to `workflows/`. Follow the structure of an existing template. The workflow engine supports:

- `depends_on` for DAG ordering
- `condition` for conditional steps
- `retry` with `exponential`, `linear`, or `fixed` backoff
- `fallback` steps on failure
- `{{ variable }}` minijinja templating

## Commit Style

Use [Conventional Commits](https://www.conventionalcommits.org/):

| Prefix | When to use |
|---|---|
| `feat:` | New feature or capability |
| `fix:` | Bug fix |
| `test:` | Adding or fixing tests |
| `docs:` | Documentation only |
| `refactor:` | Code change with no behaviour change |
| `perf:` | Performance improvement |
| `chore:` | Tooling, deps, CI |

Keep the subject line under 72 characters. Add a body when the why is not obvious from the diff.

## PR Checklist

Before marking a PR ready for review:

- [ ] `cargo fmt --all` — no formatting changes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` — zero warnings
- [ ] `cargo nextest run` — all tests pass
- [ ] Tests added for any new behaviour
- [ ] `CLAUDE.md` updated if architecture changed
- [ ] `hom-system-design.md` updated if design changed

## Project Structure

See [README.md](README.md) for a quick overview, and [hom-system-design.md](hom-system-design.md) for the full architecture reference.

The dependency rules are strict:

- `hom-core` has zero internal dependencies — it is the root
- All other crates depend on `hom-core` only (except `hom-tui` which pulls everything)
- Never add a dependency that violates this rule

## License

By contributing you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
