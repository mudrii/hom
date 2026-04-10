# Plan: Wire :save/:restore to hom-db

**Date:** 2026-04-10
**TODO Item:** P1-3

## Changes

1. `src/main.rs` Command::Save handler: serialize layout + pane configs → JSON, call save_session()
2. `src/main.rs` Command::Restore handler: call load_session(), deserialize, respawn panes
3. Add serializable `SessionData` type to `hom-tui/src/app.rs`
4. Tests for session serialization round-trip
