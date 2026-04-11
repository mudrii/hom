# Rust Rig Process Discipline

This reference preserves the important execution guidance from `.claude/skills/rust-rig/SKILL.md`.

## Objective

Apply strict design and testing discipline for Rust work in HOM.

This guidance complements the repository-level instructions and the Rust style reference. If repository-level rules are stricter on any point, follow the repository-level rule.

## Core Process

1. Inspect the crate or workspace first.
   Read `Cargo.toml`, local repository instructions, crate layout, feature flags, tool configs, and existing tests.
2. Define acceptance behavior first.
   Express the user-visible outcome before writing implementation details.
3. Add or update an acceptance-level test when the project has that layer.
   Otherwise write the closest boundary-level integration test.
4. Add the next smallest failing test.
   Prefer a focused unit or module test for the next behavior increment.
5. Implement the minimum change that makes the tests pass.
   Keep the diff tight and avoid rewriting unrelated code.
6. Refactor while green.
   Improve naming, cohesion, dependency flow, and readability without changing behavior.
7. Keep standards in sync.
   If the task materially changes project conventions, architecture, or workflow expectations, update repository instructions or the relevant skill in the same change.
8. Verify locally.
   Run formatting, linting, and tests appropriate to the affected crate or workspace.

## Design Rules

- Keep project structure clean and predictable
- Apply SRP to modules, types, and functions
- Remove repeated validation, mapping, branching, and policy logic when the abstraction improves clarity
- Extend behavior through composition, traits, enums, and additive configuration instead of invasive branching
- Prefer the smallest coherent abstraction that solves the real problem
- Do not introduce trait-first abstractions without real consumer pressure

## Dependency Injection

- Use constructor injection for long-lived collaborators
- Use function parameters for short-lived collaborators and pure logic inputs
- Pass dependencies through typed config, constructors, or arguments
- Do not instantiate external clients, repositories, clocks, or runtime collaborators inside core domain logic
- Do not hardcode dependency selection, URLs, ports, credentials, or feature switches
- Avoid globals and hidden singletons
- Prefer concrete types internally
- Introduce traits at consumer boundaries or when multiple implementations are real

## Readability

- Write modern stable Rust and prefer Rust 2024 idioms
- Keep functions short, explicit, and focused on one job
- Let `rustfmt` define layout
- Use whitespace to separate concepts, not decorate code
- Keep naming concrete and intention-revealing
- Prefer straightforward control flow over clever compression
- Avoid hardcoded values; move runtime values and environment behavior into typed config, constants, or inputs

## Comments

Good comments explain:
- intent
- invariants
- ownership or concurrency rules
- safety conditions for `unsafe`
- non-obvious tradeoffs

Do not write comments that:
- restate the code
- narrate simple assignments
- explain obvious syntax
- leave vague TODOs without context

## Error and Type Rules

- Prefer compile-time guarantees over runtime interpretation whenever practical
- Make invalid states unrepresentable with enums, newtypes, constructors, and validated inputs
- Keep types specific and explicit; avoid stringly typed state and loosely shaped maps where a real type belongs
- Keep error handling clear, concise, and contextual
- Return specific error types from internal APIs rather than `String`
- Use `Result` when callers need failure information and `Option` only when absence is the whole story
- Do not use casual `unwrap()` or `expect()` in production code

## Concurrency Judgment

- Require an explicit shutdown path and error owner before approving any new background task
- Flag lock-across-await immediately
- Reject adding a second async runtime to a project that already has one
- Treat shared mutable state as a design smell and push toward ownership transfer or channels

## Tooling Rules

Verification is part of implementation, not optional cleanup:
- run `cargo fmt --all`
- run `cargo clippy --all-targets --all-features -- -D warnings`
- run `cargo nextest run` or `cargo test`
- if features or public docs changed, also run `cargo test --all-features`, `cargo test --no-default-features`, `cargo test --doc`, and `cargo doc --no-deps`

## Review Checklist

- module and crate boundaries are coherent
- responsibilities are not mixed across domain, transport, persistence, and config
- dependencies are injected explicitly
- no hardcoded runtime values or hidden collaborator construction remain
- functions are short and readable in one pass
- comments explain intent or invariants instead of restating the code
- public API remains minimal, intentional, and stable
- types are specific, strict, and intentional
- error handling is clear, concise, and contextual
- async and concurrency behavior follow the repository runtime and locking rules
- tests cover acceptance behavior and the next unit-level behavior
- formatting, clippy, and tests pass for the affected scope
