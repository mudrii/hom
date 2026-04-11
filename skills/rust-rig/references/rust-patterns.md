# Rust Patterns and Style Reference

This reference preserves the important language-specific guidance from `.claude/rules/rust-patterns.md`.

## Clean Project Structure

- Each crate has one clear domain
- Each module has one responsibility
- Group files by domain, not by technical layer
- Keep modules small and focused
- Follow established crate layout conventions; do not reorganize without a clear reason

## Rust Style

- Write modern stable Rust and match the crate edition and style
- Prefer Rust 2024 idioms in new code
- Prefer `match`, `if let`, `let ... else`, `matches!`, and iterator adapters over deeply nested branching
- Types should encode intent: enums over strings, newtypes over loosely validated primitives, `Result` over sentinel values
- Arguments should convey meaning through types, not ambiguous `bool` flags or loosely interpreted `Option`s
- Prefer `&str`, `&[T]`, iterators, and borrowing over unnecessary allocations and cloning
- Use `struct` plus `impl` first; introduce traits only when multiple implementations or consumer-side abstraction are real
- Prefer `From`, `TryFrom`, `AsRef`, and `Borrow` over ad hoc conversion helpers
- Prefer `OnceLock` and `LazyLock` from stdlib over extra lazy-init crates
- Use `#[must_use]` where ignoring a value is likely a bug
- Avoid `Box<dyn Trait>` unless dynamic dispatch is actually required
- Avoid self-referential patterns and pinning complexity unless truly needed

## Readability and Comments

- Keep code readable; cleverness needs a measurable payoff
- Keep functions short, explicit, and focused on one job
- Let `rustfmt` define layout with consistent indentation
- Use whitespace to separate logical sections within a function
- Prefer explicit imports over glob imports outside tests or preludes
- Comments explain intent, invariants, tradeoffs, or non-obvious constraints
- Do not write comments that restate the code or narrate assignments
- Document every `unsafe` block with a `// SAFETY:` comment explaining the invariant

## SRP

- Each function does one thing and does it well
- Each struct has one reason to change
- If a function validates and persists and logs, split it
- If a match arm is longer than about 10 lines, consider extracting it into a named function
- Constructors construct; validators validate; processors process

## DRY

- Factor out repeated patterns when the abstraction is clearer than the duplication
- Use constants for magic numbers and repeated string literals
- Use shared helper functions for repeated validation or mapping logic
- Do not abstract prematurely
- Any extracted function should have a clear name and single purpose

## OCP

- Add new behavior by adding new types rather than editing stable code aggressively
- Use traits and enums for extension points
- Use `#[non_exhaustive]` on public enums that are expected to grow
- Prefer additive changes over invasive modifications

## Dependency Injection

- Pass dependencies as constructor parameters or function arguments
- Never hardcode URLs, ports, credentials, file paths, or feature switches
- Never instantiate external clients or database connections inside domain logic
- No globals, no hidden singletons, no `static mut`
- Traits at consumer boundaries; concrete types internally
- Use `Arc` only when shared ownership is genuinely required

## Public API

- Keep the public surface minimal and intentional
- Prefer private fields on public structs unless direct field access is deliberately part of the API
- Implement standard traits when meaningful
- Use `#[non_exhaustive]` for public enums and structs expected to grow
- Do not implement `Deref` for wrapper types unless they are genuinely pointer-like
- Avoid exposing unstable dependency types in public APIs

## Types and Safety

- Prefer compile-time guarantees over runtime checks whenever practical
- Make invalid states unrepresentable with enums, newtypes, constructors, and validated inputs
- Keep types strict; avoid stringly typed state and loosely shaped maps where a real type belongs
- Use enums for finite value sets instead of matching on strings
- Prefer immutable bindings; use `mut` only when it improves clarity
- Isolate `unsafe`, minimize it, and wrap it in safe APIs

## Error Handling

- Return `Result<T, E>` with specific error types, never `Result<T, String>`
- Use `thiserror` for library errors and `anyhow` only at binary boundaries
- Add context to errors: what operation, what input, and why it failed
- No casual `unwrap()` or `expect()` in production
- `Option` is only for absence, not as a substitute for `Result`

## Testing

- ATDD first for user-visible changes
- TDD cycle: failing test, minimal implementation, refactor while green
- Cover happy path, invalid input, edge cases, and error paths
- Use behavior-focused assertions
- Keep tests deterministic and avoid sleep-based timing
- Add concurrency tests when async, locking, or orchestration behavior can fail
- Public API doctests must compile and pass

## Linting and Static Analysis

- `cargo fmt` and `cargo clippy` are normal development tools, not release-only checks
- Do not silence lints casually
- Prefer narrowly scoped `#[expect(...)]` or `#[allow(...)]` with rationale over broad suppression
- Match existing project choices for logging, tracing, config, serialization, database, and CLI libraries
- Run `cargo doc --no-deps` when public APIs change

## Project Profiles

- Library crates: small stable APIs, typed errors, strong docs, additive features, no leaked internals
- Backend services: fail fast on invalid config, propagate cancellation and timeouts, keep transport separate from domain
- CLI apps: stable exit codes, clear stderr errors, no non-TTY assumptions unless designed
- Systems code: minimize allocation, document memory assumptions, keep `unsafe` and FFI boundaries narrow
