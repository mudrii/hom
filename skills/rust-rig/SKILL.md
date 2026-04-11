---
name: rust-rig
description: Use when building, reviewing, or refactoring Rust code in HOM that requires strong maintainability, testing discipline, and explicit dependency boundaries
---

<objective>
Apply the Rust-specific design, testing, and review discipline for HOM.

This is the Codex-compatible form of the important guidance previously carried by `.claude/skills/rust-rig/SKILL.md` and `.claude/rules/rust-patterns.md`.
</objective>

<when_to_use>
Use this skill when:
- implementing a Rust feature
- refactoring Rust code for maintainability
- reviewing architecture, module boundaries, or dependency flow
- tightening tests around user-visible behavior
- replacing hardcoded wiring with explicit dependency injection
</when_to_use>

<required_reading>
Read these files before making a meaningful Rust change:
- `skills/rust-rig/references/process-discipline.md`
- `skills/rust-rig/references/rust-patterns.md`

Also read the relevant domain skill for the area you are changing.
</required_reading>

<process>
Follow this workflow:

1. Inspect the workspace, relevant crate, feature flags, tests, and existing patterns first.
2. Define acceptance behavior before implementation details.
3. Add or update an acceptance-level or closest boundary-level failing test first.
4. Add the next smallest failing unit or module test.
5. Implement the minimum change that makes the tests pass.
6. Refactor while green.
7. Verify formatting, linting, and tests for the affected scope before handing off.
</process>

<decision_rules>
Apply these rules during implementation:
- keep project structure clean and predictable
- enforce SRP for modules, types, and functions
- remove duplication when the abstraction improves clarity
- extend behavior additively through composition, traits, enums, and configuration
- do not introduce trait-per-struct abstractions without real consumer need
- keep domain logic separate from transport, persistence, configuration, and presentation
- keep public APIs minimal, intentional, and stable
</decision_rules>

<dependency_injection>
Manage collaborators explicitly:
- pass long-lived collaborators through constructors
- pass short-lived collaborators and pure inputs through function parameters
- do not instantiate external clients, repositories, clocks, or runtime collaborators inside core domain logic
- do not hardcode URLs, ports, file paths, credentials, or feature switches
- prefer concrete types internally and traits only at real consumer boundaries
- use `Arc` only when shared ownership is genuinely required
</dependency_injection>

<testing_discipline>
Testing is mandatory:
- use ATDD first for user-visible behavior
- use TDD for the next smallest increment
- cover happy path, invalid input, edge cases, and failure paths
- add concurrency-focused tests when async, task, or locking behavior can fail
- keep tests deterministic and avoid sleep-based timing where practical
</testing_discipline>

<verification>
Run these commands as appropriate to the affected scope:
- `cargo check`
- `cargo fmt --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run` or `cargo test`
- `cargo test --all-features`
- `cargo test --no-default-features --features vt100-backend`
- `cargo test --doc`
- `cargo doc --no-deps`
</verification>

<success_criteria>
This skill is being followed correctly when:
- changes are small, test-backed, and easy to review
- dependency flow is explicit
- module responsibilities are clearer after the change
- types and errors are strict and intentional
- formatting, clippy, and tests pass for the affected scope
</success_criteria>
