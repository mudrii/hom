---
name: hom-adapter-development
description: Use when adding a new harness adapter or modifying an existing one in hom-adapters
---

# HOM Adapter Development

## When to Use

Invoke this skill when:
- Adding a new AI harness adapter to `crates/hom-adapters/src/`
- Modifying an existing adapter's `build_command`, `translate_input`, `parse_screen`, or `detect_completion`
- Adding or modifying a sideband channel (HTTP, RPC, JSON-RPC)
- Changing the `HarnessAdapter` trait in `hom-core`

## The Adapter Contract

Every adapter implements `HarnessAdapter` from `crates/hom-core/src/traits.rs`. This trait is the **single integration point** between HOM and a harness. Breaking it breaks everything.

### Required Methods

| Method | Purpose | Criticality |
|--------|---------|-------------|
| `harness_type()` | Return the `HarnessType` enum variant | Must match registry |
| `display_name()` | Human-readable name for status rail | Cosmetic |
| `build_command()` | Construct the spawn command + args + env + cwd | **Critical** — wrong command = harness won't start |
| `translate_input()` | Convert `OrchestratorCommand` → raw PTY bytes | **Critical** — wrong encoding = harness gets garbage |
| `parse_screen()` | Extract `HarnessEvent`s from terminal screen | Best-effort — used by workflow engine |
| `detect_completion()` | Determine if harness is done/waiting/failed | **Critical** — wrong detection = workflow hangs |
| `capabilities()` | Report what this harness supports | Informational |
| `sideband()` | Optional out-of-band channel | Only for Tier 1 harnesses with API access |

## Process: Adding a New Adapter

### Step 1: Research the Harness

Before writing any code, answer these questions:

1. **Binary name and install method** — What command spawns it? Is it `npm`, `cargo`, `go install`?
2. **CLI flags** — How do you set model, working directory, output format?
3. **Interactive mode** — What does the prompt look like when waiting for input? (exact characters)
4. **Structured output** — Does it support JSON/JSONL output mode?
5. **Completion signals** — What appears on screen when a task finishes?
6. **Error patterns** — What do errors look like on the terminal?
7. **Sideband** — Does it expose an HTTP API, RPC interface, or ACP server?

### Step 2: Write Tests First (TDD)

Create `crates/hom-adapters/tests/<harness_name>_test.rs`.

The snippet below is a template, not copy-paste-ready code. Replace placeholder names like `MyAdapter`, `MyHarness`, and helper builders with the real adapter and test helpers for your harness:

```rust
#[test]
fn test_build_command_default() {
    let adapter = MyAdapter::new();
    let config = HarnessConfig::new(HarnessType::MyHarness, PathBuf::from("/tmp"));
    let spec = adapter.build_command(&config);
    assert_eq!(spec.program, "my-harness-binary");
    assert!(spec.args.is_empty());
}

#[test]
fn test_build_command_with_model() {
    let adapter = MyAdapter::new();
    let config = HarnessConfig::new(HarnessType::MyHarness, PathBuf::from("/tmp"))
        .with_model("gpt-4");
    let spec = adapter.build_command(&config);
    assert!(spec.args.contains(&"--model".to_string()));
    assert!(spec.args.contains(&"gpt-4".to_string()));
}

#[test]
fn test_detect_completion_waiting() {
    let adapter = MyAdapter::new();
    let screen = make_screen_with_last_line("❯ ");
    assert!(matches!(adapter.detect_completion(&screen), CompletionStatus::WaitingForInput));
}

#[test]
fn test_translate_prompt() {
    let adapter = MyAdapter::new();
    let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("hello".into()));
    assert_eq!(bytes, b"hello\n");
}
```

Run tests: `cargo test -p hom-adapters`. They must **fail** before you write the implementation.

### Step 3: Implement the Adapter

Create `crates/hom-adapters/src/<harness_name>.rs`. Follow the pattern of existing adapters (see `claude_code.rs` as reference).

### Step 4: Register in AdapterRegistry

Add to `crates/hom-adapters/src/lib.rs`:
1. `pub mod <harness_name>;`
2. Insert into `AdapterRegistry::new()`

### Step 5: Add HarnessType Variant

In `crates/hom-core/src/types.rs`:
1. Add variant to `HarnessType` enum
2. Add `display_name()` match arm
3. Add `default_binary()` match arm
4. Add `from_str_loose()` match arm(s)

### Step 6: Add Default Config

In `config/default.toml`, add a `[harnesses.<name>]` section.

### Step 7: Verify

```bash
cargo check           # Must pass
cargo test -p hom-adapters  # All new tests green
cargo clippy          # No warnings
```

## Red Flags — STOP and Rethink

- **Adding a dependency to hom-core for one adapter** — Adapter-specific deps go in hom-adapters only
- **Hardcoding paths or API keys** — Use `HarnessConfig.env_vars` and `config.toml`
- **Modifying `HarnessAdapter` trait for one harness** — The trait serves ALL 7 harnesses. Use `sideband()` for harness-specific features
- **Skipping `detect_completion` tests** — This is the #1 source of workflow hangs
- **Parsing screen output with regex alone** — Screen state includes colors, cursor position, line wrapping. Use `ScreenSnapshot` methods, not raw string matching

## Checklist Before Committing

- [ ] `HarnessType` variant added with all match arms
- [ ] Adapter struct created implementing full `HarnessAdapter` trait
- [ ] Registered in `AdapterRegistry::new()`
- [ ] `config/default.toml` entry added
- [ ] `from_str_loose()` handles common aliases
- [ ] Tests for `build_command`, `translate_input`, `detect_completion`
- [ ] `cargo check && cargo test -p hom-adapters && cargo clippy` all pass
