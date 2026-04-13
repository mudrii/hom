# Manual Testing Handoff Plan

## Goal

Prepare a current HOM binary for manual testing and add a tester-oriented usage guide that matches the current CLI and runtime behavior.

## Scope

- Build the default release binary
- Verify the binary artifact path
- Document launch commands and manual smoke-test flows
- Link the guide from the main README

## Deliverables

- `target/release/hom`
- `docs/manual-testing.md`
- README link to the manual testing guide

## Validation

- `cargo build --release`
- `target/release/hom --help`
- `cargo fmt --all`
