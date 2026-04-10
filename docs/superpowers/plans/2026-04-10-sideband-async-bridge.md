# Plan: Full Sideband Async Bridge

**Date:** 2026-04-10
**TODO Item:** P1-2
**File:** `src/main.rs` (handle_workflow_request, SendAndWait handler)

## Problem

When a pane has a sideband channel (e.g. OpenCode HTTP API), `SendAndWait` should use `sideband.send_prompt()` for direct prompt delivery and response. Currently it falls through to PTY write + completion polling because calling async from the sync handler was not wired.

## Solution

1. When `pane.sideband` is `Some`, clone the `Box<dyn SidebandChannel>` and spawn a tokio task
2. The task calls `sideband.send_prompt(prompt)` and sends the result via the `reply` oneshot
3. No `PendingCompletion` polling needed — sideband provides the response directly

## Key Design Decision

`SidebandChannel` is `Send + Sync` (trait bound). To move it into a spawned task, we need to extract it from the pane. Since `Box<dyn SidebandChannel>` can't be cloned, we'll take ownership temporarily via `Option::take()` and put it back after spawning. Actually, since the task needs to own the sideband, and the pane might be reused, we should use `Arc<dyn SidebandChannel>` in the Pane struct.

## Changes

- `crates/hom-tui/src/app.rs`: Change `Pane.sideband` from `Option<Box<dyn SidebandChannel>>` to `Option<Arc<dyn SidebandChannel>>`
- `src/main.rs`: In SendAndWait handler, spawn async task with sideband when available
- Update sideband construction in `spawn_pane_inner()` to use `Arc`
