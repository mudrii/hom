---
name: rust-rig
description: Use this skill when building, reviewing, or refactoring Rust code that requires strong maintainability discipline: SRP, DRY, OCP, explicit dependency injection, TDD/ATDD workflow, architecture review, and clean project structure. Complements the project's Rust CLAUDE.md with process rigor.
metadata:
  slash-command: enabled
---

<objective>
Apply strict design and testing discipline for Rust projects.

This skill complements `CLAUDE.md` and `.claude/rules/rust-patterns.md`. It must stay aligned with both and applies as an execution discipline layer on top of the project's Rust standards.

This skill adds execution rigor:

- ATDD/TDD workflow
- SRP, DRY, and OCP decision rules
- explicit dependency injection discipline
- comment quality standards
- structured implementation and review checks

If `CLAUDE.md` is stricter on any point, follow `CLAUDE.md`.
</objective>

<when_to_use>
Use this skill when:

- implementing a new feature
- refactoring Rust code for maintainability
- reviewing architecture, module boundaries, or dependency flow
- tightening tests around user-visible behavior
- replacing hardcoded wiring with explicit dependency injection
</when_to_use>

<process>
Follow this workflow:

1. Inspect the crate or workspace first.
   Read `Cargo.toml`, local `CLAUDE.md`, crate layout, feature flags, tool configs, and existing tests.

2. Define acceptance behavior first.
   Express the user-visible outcome before writing implementation details.

3. Add or update an acceptance-level test when the project has that layer.
   Otherwise, write the closest boundary-level integration test.

4. Add the next smallest failing test.
   Prefer a focused unit or module test for the next behavior increment.

5. Implement the minimum change that makes the test pass.
   Keep the diff tight. Do not rewrite unrelated code.

6. Refactor while green.
   Improve naming, cohesion, dependency flow, and readability without changing behavior.

7. Keep standards in sync.
   If the task materially changes project conventions, architecture, or workflow expectations, update `CLAUDE.md` or the relevant skill in the same change.

8. Verify locally.
   Run formatting, linting, and tests appropriate to the affected crate or workspace.
</process>

<design_rules>
Apply these rules during implementation:

- Keep project structure clean and predictable
- **SRP**: each module, type, and function should have one clear reason to change
- **DRY**: remove repeated validation, mapping, branching, and policy logic when the abstraction improves clarity
- **OCP**: extend behavior through composition, traits, enums, and additive configuration instead of invasive branching or copy-paste forks
- Prefer domain-oriented module boundaries over technical dumping grounds
- Keep domain logic separate from transport, persistence, configuration, and presentation concerns
- Prefer the smallest coherent abstraction that solves the real duplication or extension point
- Do not introduce trait-first abstractions without real consumer pressure
</design_rules>

<dependency_injection>
Manage code relationships explicitly.

- Use constructor injection for long-lived collaborators
- Use function parameters for short-lived collaborators and pure logic inputs
- Pass dependencies through typed config, constructors, or arguments
- Do not instantiate external clients, repositories, clocks, or runtime collaborators inside core domain logic
- Do not hardcode dependency selection, URLs, ports, credentials, or feature switches
- Avoid globals and hidden singletons

Rust-specific guidance:

- Prefer concrete types internally
- Introduce traits at consumer boundaries or when multiple implementations are real
- Prefer generic parameters or references for local flexibility
- Use `Arc` only when shared ownership is actually required
</dependency_injection>

<style_and_readability>
Keep the code easy to read and maintain.

- Write modern stable Rust and prefer Rust 2024 idioms in new code; key 2024 changes: `let`-chains (`if let Ok(x) = a && let Some(y) = b { … }`), `unsafe` blocks required inside `unsafe fn`, stricter `impl Trait` lifetime capture, `gen` keyword reserved
- Keep functions short, explicit, and focused on one job
- Use consistent formatting, indentation, and whitespace; let `rustfmt` define layout
- Use whitespace to separate concepts, not decorate code
- Keep naming concrete and intention-revealing
- Prefer straightforward control flow over clever compression
- Separate logic clearly so domain rules, I/O, transport, persistence, and orchestration are easy to trace
- Avoid hardcoded values; move runtime values and environment behavior into typed config, constants, or inputs
</style_and_readability>

<public_api_rules>
Keep public APIs small, intentional, and stable.

- Prefer private fields on public structs unless direct field access is a deliberate part of the API
- Use standard traits on public types when they are semantically correct
- Use `#[non_exhaustive]` for public enums and structs that are expected to grow
- Avoid exposing unstable dependency types in public APIs when stable local types will do
- Do not implement `Deref` for wrapper types unless they are genuinely pointer-like
</public_api_rules>

<testing_discipline>
Testing is mandatory.

- ATDD first for user-visible changes: define the acceptance scenario before writing implementation
- TDD for the next increment: write the smallest failing unit test, implement, then refactor
- Use ATDD extensively for user-visible behavior, acceptance flows, and cross-boundary scenarios
- Every meaningful change should cover happy path, invalid input, edge cases, and failure paths
- Add concurrency-focused tests when ownership, async behavior, locking, or task orchestration can fail
- Keep tests deterministic; avoid sleep-based async tests when possible
- Keep public API doctests and examples accurate when behavior changes
</testing_discipline>

<comment_rules>
Write comments only when they add information the code cannot carry cleanly on its own.

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
</comment_rules>

<error_and_type_rules>
Keep errors and types strict and readable.

- Prefer compile-time guarantees over runtime interpretation whenever practical
- Make invalid states unrepresentable with enums, newtypes, constructors, and validated inputs
- Keep types specific and explicit; avoid stringly typed state and loosely shaped maps where a real type belongs
- Keep error handling clear, concise, and contextual
- Return specific error types from internal APIs rather than `String`
- Use `Result` when callers need failure information and `Option` only when absence is the whole story
- Do not use casual `unwrap()` or `expect()` in production code
</error_and_type_rules>

<concurrency_rules>
See CLAUDE.md for the full concurrency rules. Apply this judgment discipline:

- Require an explicit shutdown path and error owner before approving any new background task
- Flag lock-across-await immediately; accept only with a documented reason
- Reject adding a second async runtime to a project that already has one
- Treat shared mutable state as a design smell; push back toward ownership transfer or channels
</concurrency_rules>

<tooling_rules>
Verification is part of the implementation, not an optional cleanup step.

- Run `cargo fmt --all`
- Run `cargo clippy --all-targets --all-features -- -D warnings`
- Run `cargo nextest run` (or `cargo test` if nextest is not installed)
- If features or public docs changed, also run `cargo test --all-features`, `cargo test --no-default-features --features vt100-backend`, `cargo test --doc`, and `cargo doc --no-deps`
- Treat linting and static analysis as normal development tools, not release-only checks
</tooling_rules>

<review_checklist>
Before finishing, verify:

- [ ] module and crate boundaries are coherent
- [ ] responsibilities are not mixed across domain, transport, persistence, and config
- [ ] structure is clean, predictable, and free of dumping-ground modules
- [ ] dependencies are injected explicitly
- [ ] no hardcoded runtime values or hidden collaborator construction remain
- [ ] functions are short and readable in one pass
- [ ] formatting, indentation, and whitespace follow project conventions and `rustfmt`
- [ ] comments explain intent or invariants instead of restating the code
- [ ] public API remains minimal, intentional, and stable
- [ ] types are specific, strict, and intentional, not stringly typed
- [ ] error handling is clear, concise, contextual, and free of casual `unwrap()`/`expect()`
- [ ] async and concurrency behavior follow the project's runtime and locking rules
- [ ] tests cover acceptance behavior and the next unit-level behavior
- [ ] formatting, clippy, and tests pass for the affected scope
- [ ] public API and docs stay aligned with the implementation
</review_checklist>

<reject_patterns>
Reject these patterns:

- giant functions mixing validation, orchestration, and persistence
- trait-per-struct abstraction without consumer need
- hardcoded configuration or collaborator construction
- comments that restate code
- hidden globals or singletons
- brittle mock-only tests when a fake or boundary test would be clearer
- transport or storage concerns embedded in core domain logic
- public APIs that leak unstable dependency details without need
- locks held across `.await` or blocking work inside async tasks
- production design distorted to satisfy a test double or framework
- large speculative refactors when a smaller coherent change would solve the task
</reject_patterns>

<success_criteria>
This skill is being followed correctly when:

- changes are small, test-backed, and easy to review
- dependency flow is explicit
- module responsibilities are cleaner after the change, not blurrier
- the implementation matches the Rust standards in `CLAUDE.md` and `.claude/rules/`
- the resulting code is easier to extend without rewriting stable behavior
</success_criteria>
