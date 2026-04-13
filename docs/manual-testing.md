# HOM Manual Testing Guide

This guide is for running HOM manually from a local build and smoke-testing the current features before wider testing.

## Build Artifact

Default release build:

```sh
cargo build --release
```

Default artifact path:

```sh
./target/release/hom
```

For the broadest local compatibility during manual testing, build the vt100 fallback artifact:

```sh
cargo build --release --no-default-features --features vt100-backend
```

That command also writes the runnable binary to:

```sh
./target/release/hom
```

If you specifically want the default Ghostty-backed release build on macOS, you may need to launch it with Cargo's built Ghostty library on `DYLD_LIBRARY_PATH`, for example:

```sh
DYLD_LIBRARY_PATH=target/release/build/libghostty-vt-sys-*/out/ghostty-install/lib ./target/release/hom
```

## Prerequisites

- Install the harness CLIs you want to exercise, for example `claude`, `codex`, `gemini`, `pi`, `kimi`, `opencode`, or `copilot`
- If you want workflow tests, ensure you have workflow YAML files available under the configured workflow directory
- If you want web-view testing, make sure the chosen web port is available
- If you want remote-pane testing, ensure SSH access works to the target host
- If you want plugin testing, have a compiled plugin `.dylib` or `.so` ready

## Basic Launch

Recommended manual-testing launch:

```sh
./target/release/hom
```

Useful startup variants:

```sh
./target/release/hom --no-db
./target/release/hom --web
./target/release/hom --web --web-port 8080
./target/release/hom --mcp
./target/release/hom --run code-review --var task="audit auth flow"
```

Notes:

- `--run` expects a workflow name, not an arbitrary relative path. HOM resolves it as `<workflow_dir>/<name>.yaml`.
- `--mcp` keeps protocol traffic on `stdout` and renders the TUI on `stderr`.
- `--web` serves the viewer at `http://localhost:<port>`.

## Command Bar Quick Reference

Open the command bar with `:`.

Common commands:

```text
:spawn claude
:spawn codex --model gpt-5.4
:spawn claude --remote user@example.com
:send 1 "summarize the repo layout"
:broadcast "list the top risks in this codebase"
:pipe 1 -> 2
:run code-review --var task="review parser"
:layout grid
:save smoke
:restore smoke
:load-plugin "/absolute/path/to/my-plugin.dylib"
:kill 2
:quit
```

## Manual Smoke Tests

### 1. Startup and navigation

1. Launch `./target/release/hom`
2. Confirm the app opens without crashing
3. Press `:` and verify the command bar appears
4. Press `Tab` to cycle focus after spawning more than one pane
5. Press `Ctrl-Q` and confirm HOM exits cleanly

### 2. Local pane lifecycle

1. Run `:spawn claude`
2. Run `:spawn codex`
3. Verify both panes render and can receive focus
4. Run `:send 1 "print a short status line"`
5. Run `:kill 2`
6. Confirm pane 2 is removed and focus recovers correctly

### 3. Layout changes

1. Spawn at least 3 panes
2. Run `:layout single`
3. Run `:layout hsplit`
4. Run `:layout vsplit`
5. Run `:layout grid`
6. Run `:layout tabbed`
7. Confirm rendering stays stable and panes resize correctly

### 4. Save and restore

1. Arrange 2 or more panes
2. Run `:save smoke`
3. Quit HOM
4. Relaunch HOM
5. Run `:restore smoke`
6. Confirm pane metadata and layout come back as expected

### 5. Workflow launch

1. Ensure the configured workflow directory contains `code-review.yaml` or another known workflow
2. Launch with:

```sh
./target/release/hom --run code-review --var task="inspect session restore"
```

3. Confirm workflow progress appears
4. Confirm pane creation and step updates happen
5. Confirm workflow errors are surfaced in the command bar if the workflow or harness fails

### 6. Web viewer

1. Launch:

```sh
./target/release/hom --web
```

2. Open `http://localhost:4242`
3. Confirm the browser shows live pane content
4. Type into the browser view and confirm input reaches the focused pane
5. Quit HOM and confirm the browser stops receiving updates

### 7. MCP mode

1. Launch:

```sh
./target/release/hom --mcp
```

2. Confirm JSON-RPC traffic is clean on `stdout`
3. Confirm the TUI remains visible on `stderr`
4. Exercise at least:
   - `tools/list`
   - `spawn_pane`
   - `send_to_pane`
   - `get_pane_output`
   - `kill_pane`

### 8. Remote pane

1. Run:

```text
:spawn claude --remote user@example.com
```

2. Confirm the pane appears after SSH setup completes
3. Send a prompt and confirm output streams back
4. Resize the terminal and confirm the remote pane keeps rendering
5. Kill the pane and confirm it disappears cleanly

### 9. Plugin load

1. Run:

```text
:load-plugin "/absolute/path/to/my-plugin.dylib"
```

2. Confirm no load error is shown
3. Spawn the plugin harness by name
4. Verify input, rendering, and completion detection behave as expected

## Troubleshooting

- If the default release build fails in the terminal backend, use the `vt100-backend` fallback build
- If `--run` says the workflow cannot be loaded, check the configured workflow directory and use the workflow name without `../` or path traversal components
- If a harness pane does not start, verify the corresponding CLI is installed and available on `PATH`
- If the web viewer is blank, confirm HOM was started with `--web` and that the port is reachable
- If remote spawn stalls, verify SSH connectivity outside HOM first

## Recommended Test Record

Capture these items during manual testing:

- binary path tested
- build command used
- config file path used
- harnesses exercised
- workflows exercised
- whether `--web`, `--mcp`, remote panes, and plugins were tested
- any command-bar errors shown
