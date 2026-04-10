# Plan: Workflow Parallel Execution

**Date:** 2026-04-10
**TODO Item:** P1-1
**File:** `crates/hom-workflow/src/executor.rs`

## Problem

Ready DAG steps run sequentially within each batch. The executor takes `&dyn WorkflowRuntime` which cannot be shared across tokio tasks because it's not `'static`.

## Solution

1. Change `WorkflowExecutor::execute*` methods to take `Arc<dyn WorkflowRuntime>` instead of `&dyn WorkflowRuntime`
2. Update `WorkflowBridge` usage in `src/main.rs` (already passes `Arc<WorkflowBridge>`)
3. Use `tokio::task::JoinSet` to run ready steps concurrently within each batch
4. Collect results from the JoinSet and process them (checkpoint, failure handling)

## Changes

- `crates/hom-workflow/src/executor.rs`: Change runtime parameter to `Arc<dyn WorkflowRuntime>`, use JoinSet
- `crates/hom-tui/src/workflow_bridge.rs`: No change needed (already `Send + Sync`)
- `src/main.rs`: Update call sites to pass `Arc` directly (already does `bridge.as_ref()` → just pass `bridge.clone()`)

## Tests

- Test that independent steps in a batch execute concurrently
- Test that dependent steps still execute in order
- Test failure handling with parallel steps (one fails, others complete)
