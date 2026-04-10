# Plan: Wire Cost Tracking

**Date:** 2026-04-10
**TODO Item:** P1-4

## Changes

1. After workflow step completion in run_workflow_task(), call log_cost() with step data
2. In the main event loop, parse HarnessEvent::TokenUsage from poll_pty_output and log costs
3. Display total cost in the status rail
