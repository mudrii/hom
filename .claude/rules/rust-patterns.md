# Rust Patterns and Style Reference

Detailed conventions that complement `CLAUDE.md`. The rust-rig skill owns process discipline (ATDD/TDD, DI, review workflow). This file owns language-specific style, API, and testing patterns.

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
- Keep functions short, explicit, and focused on one job
- Let `rustfmt` define layout; use whitespace to separate ideas, not decorate
- Prefer explicit imports over glob imports outside tests or prelude modules
- Comments explain intent, invariants, tradeoffs, or non-obvious constraints
- Do not write comments that merely restate the code
- Document every `unsafe` block with a `SAFETY:` comment

## Public API

- Keep the public surface minimal and intentional
- Prefer private fields on public structs unless field access is deliberately part of the API
- Implement standard traits when meaningful: `Debug`, `Clone`, `Eq`, `PartialEq`, `Hash`, `Default`, `Ord`, `PartialOrd`, `Display`
- Use `#[non_exhaustive]` for public enums and structs expected to grow
- Do not implement `Deref` for wrapper types unless genuinely pointer-like
- Avoid exposing unstable dependency types in public APIs

## Types and Safety

- Prefer compile-time guarantees over runtime checks whenever practical
- Make invalid states unrepresentable with enums, newtypes, constructors, and validated inputs
- Keep types strict; avoid stringly typed state and loosely shaped maps where a real type belongs
- Prefer immutable bindings; use `mut` only when it improves clarity
- Isolate `unsafe`, minimize it, and wrap it in safe APIs

## Tooling

- Use linting and static analysis as part of normal development
- Do not silence lints casually; fix the code or document why the lint is wrong
- Prefer narrowly scoped `#[expect(...)]` or `#[allow(...)]` with rationale over broad suppression
- Match existing project choices for logging, tracing, config, serialization, database, and CLI libraries

## Project Profiles

- **Library crates**: small stable APIs, typed errors, strong rustdoc, additive features, no leaked internals
- **Backend services**: fail fast on invalid config, propagate cancellation and timeouts, structured logs and metrics, transport ≠ domain
- **CLI apps**: stable exit codes, clear stderr errors, no non-TTY assumptions, machine-readable output only when intentionally designed
- **Embedded / systems**: minimize allocation, document memory and interrupt assumptions, keep `unsafe` and FFI boundaries narrow, prefer `core`/`alloc`-friendly designs
