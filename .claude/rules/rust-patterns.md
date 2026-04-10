# Rust Patterns and Style Reference

Detailed conventions that complement `CLAUDE.md`. The rust-rig skill owns process discipline (ATDD/TDD, DI, review workflow). This file owns language-specific style, API, type, testing, and structural patterns.

## Clean Project Structure

- Each crate has one clear domain — no crate should mix unrelated concerns
- Each module has one responsibility — if it does two things, it should be two modules
- Group files by domain, not by technical layer (no `models/`, `services/`, `controllers/`)
- Keep modules small and focused — a 500-line module is a smell; a 1000-line module must be split
- Follow established crate layout conventions; do not reorganize without a clear reason

## Rust Style

- Write modern stable Rust; match the crate's edition and style
- Prefer Rust 2024 idioms in new code
- Prefer `match`, `if let`, `let ... else`, `matches!`, and iterator adapters over deeply nested branching
- Types should encode intent: enums over strings, newtypes over loosely validated primitives, `Result` over sentinel values
- Arguments convey meaning through types, not ambiguous `bool` flags or loosely interpreted `Option`s
- Prefer `&str`, `&[T]`, iterators, and borrowing over unnecessary allocations and cloning
- Use `struct` + `impl` first; introduce traits only when multiple implementations or consumer-side abstraction are real
- Prefer `From`/`TryFrom`, `AsRef`, `Borrow` over ad hoc conversion helpers
- Prefer `OnceLock` and `LazyLock` from stdlib over extra lazy-init crates
- Use `#[must_use]` where ignoring a value is likely a bug
- Avoid `Box<dyn Trait>` unless dynamic dispatch is actually required
- Avoid self-referential patterns and pinning complexity unless truly needed

## Readability and Comments

- Keep code readable; cleverness needs a measurable payoff
- Keep functions short, explicit, and focused on one job — under 40 lines preferred, hard limit 80
- Let `rustfmt` define layout; use consistent 4-space indentation; no manual overrides
- Use whitespace to separate logical sections within a function — blank lines between concerns
- Prefer explicit imports over glob imports outside tests or prelude modules
- Comments explain **intent**, **invariants**, **tradeoffs**, or **non-obvious constraints**
- Do not write comments that restate the code or narrate assignments
- Document every `unsafe` block with a `// SAFETY:` comment explaining the invariant

## SRP — Single Responsibility

- Each function does one thing and does it well
- Each struct has one reason to change
- If a function validates AND persists AND logs — split it into three
- If a match arm is longer than 10 lines — extract it into a named function
- Constructors construct; validators validate; processors process — do not mix

## DRY — Don't Repeat Yourself

- Factor out repeated patterns when the abstraction is clearer than the duplication
- Constants for magic numbers and repeated string literals
- Shared helper functions for repeated validation or mapping logic
- Do NOT abstract prematurely — three similar lines are better than one confusing abstraction
- When you extract, the extracted function must have a clear name and single purpose

## OCP — Open/Closed Principle

- Add new behavior by adding new types, not by modifying stable existing code
- Use traits and enums for extension points — the compiler enforces completeness
- `#[non_exhaustive]` on public enums that will grow
- Prefer additive changes over invasive modifications

## Dependency Injection

- Pass dependencies as constructor parameters or function arguments
- Never hardcode URLs, ports, credentials, file paths, or feature switches
- Never instantiate external clients or DB connections inside domain logic
- No globals, no `static mut`, no hidden singletons
- Traits at consumer boundaries; concrete types internally
- `Arc` only when shared ownership is genuinely required

## Public API

- Keep the public surface minimal and intentional
- Prefer private fields on public structs unless field access is deliberately part of the API
- Implement standard traits when meaningful: `Debug`, `Clone`, `Eq`, `PartialEq`, `Hash`, `Default`, `Ord`, `PartialOrd`, `Display`
- Use `#[non_exhaustive]` for public enums and structs expected to grow
- Do not implement `Deref` for wrapper types unless genuinely pointer-like
- Avoid exposing unstable dependency types in public APIs

## Types and Safety — STRICT

- Prefer compile-time guarantees over runtime checks whenever practical
- Make invalid states unrepresentable with enums, newtypes, constructors, and validated inputs
- Keep types strict; avoid stringly typed state and loosely shaped maps where a real type belongs
- Use enums for finite value sets — never match on strings when an enum exists
- Prefer immutable bindings; use `mut` only when it improves clarity
- Isolate `unsafe`, minimize it, and wrap it in safe APIs

## Error Handling

- Return `Result<T, E>` with specific error types — never `Result<T, String>`
- Use `thiserror` for library errors; `anyhow` only at binary boundaries
- Add context to errors: what operation, what input, why it failed
- No casual `unwrap()` or `expect()` in production — use `?`, `ok_or_else()`, or `map_err()`
- `Option` only when absence is the whole story — not as a substitute for `Result`

## Testing — TDD/ATDD Mandatory

- **ATDD first**: write the acceptance test describing user-visible behavior before implementation
- **TDD cycle**: failing test → minimal implementation → refactor while green → commit
- Cover: happy path, invalid input, edge cases, error/failure paths
- Behavior-focused assertions — test what, not how
- Deterministic seams — no sleep-based timing; use channels/semaphores
- Add concurrency tests when async/locking/task orchestration can fail
- Public API doctests must compile and pass

## Linting and Static Analysis

- `cargo fmt` and `cargo clippy` are development tools, not release-only checks
- Do not silence lints casually; fix the code or document why the lint is wrong
- Prefer narrowly scoped `#[expect(...)]` or `#[allow(...)]` with rationale over broad suppression
- Match existing project choices for logging, tracing, config, serialization, database, and CLI libraries
- Run `cargo doc --no-deps` when public APIs change — documentation must build clean

## Project Profiles

- **Library crates**: small stable APIs, typed errors, strong rustdoc, additive features, no leaked internals
- **Backend services**: fail fast on invalid config, propagate cancellation and timeouts, structured logs and metrics, transport ≠ domain
- **CLI apps**: stable exit codes, clear stderr errors, no non-TTY assumptions, machine-readable output only when intentionally designed
- **Embedded / systems**: minimize allocation, document memory and interrupt assumptions, keep `unsafe` and FFI boundaries narrow, prefer `core`/`alloc`-friendly designs
