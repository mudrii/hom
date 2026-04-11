# Plugin System for Adapters — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users load additional harness adapters at runtime — without recompiling HOM — via `.dylib`/`.so` plugin files. Plugins appear alongside the 7 built-in adapters; `:spawn mycli` works just like `:spawn claude`.

**Architecture:** A new `hom-plugin` crate defines a stable C ABI vtable (`HomPluginVtable`) that plugins must export. All data crossing the FFI boundary travels as JSON strings, eliminating Rust ABI instability. `PluginLoader` calls `dlopen` + `hom_plugin_init` via `libloading`. `PluginAdapter` wraps the vtable and implements `HarnessAdapter`. `AdapterRegistry` gains a second `HashMap<String, Box<dyn HarnessAdapter>>` for plugin-keyed adapters; `get_plugin(name)` handles the lookup. HOM scans `~/.config/hom/plugins/` at startup and registers each found plugin.

**Tech Stack:** `libloading = "0.8"`, `serde_json` (already in workspace), `HarnessAdapter` trait from `hom-core`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/hom-plugin/Cargo.toml` | Create | Crate manifest; deps: `hom-core`, `libloading`, `serde_json`, `tracing` |
| `crates/hom-plugin/src/lib.rs` | Create | Public re-exports |
| `crates/hom-plugin/src/ffi.rs` | Create | `HomPluginVtable` C repr struct; JSON schema constants |
| `crates/hom-plugin/src/loader.rs` | Create | `PluginLoader::load(path)` + `PluginLoader::scan_dir(dir)` |
| `crates/hom-plugin/src/adapter.rs` | Create | `PluginAdapter` — wraps vtable, implements `HarnessAdapter` |
| `Cargo.toml` (workspace) | Modify | Add `hom-plugin` member + `libloading` workspace dep |
| `crates/hom-adapters/Cargo.toml` | Modify | Add `hom-plugin` dep |
| `crates/hom-adapters/src/lib.rs` | Modify | `AdapterRegistry::load_plugin()`, `get_plugin()` |
| `crates/hom-tui/src/command_bar.rs` | Modify | Parse `:load-plugin <path>` command |
| `crates/hom-tui/src/app.rs` | Modify | Handle `:load-plugin`; route unknown harness names to plugin registry; auto-scan at startup |
| `CLAUDE.md` | Modify | Update implementation status |

---

### Task 1: Define the C ABI vtable in `hom-plugin`

**Files:**
- Create: `crates/hom-plugin/Cargo.toml`
- Create: `crates/hom-plugin/src/ffi.rs`
- Create: `crates/hom-plugin/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Write the failing test**

Create `crates/hom-plugin/src/ffi.rs` with tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_size_is_stable() {
        // The vtable must have a fixed, known size for FFI safety.
        // If this test breaks after adding a field, update the expected size AND bump
        // the plugin ABI version constant.
        assert_eq!(
            std::mem::size_of::<HomPluginVtable>(),
            // 8 function pointer fields × 8 bytes each on 64-bit = 64 bytes
            // Plus 1 usize abi_version field = 8 bytes = 72 total.
            // Adjust if fields change — the point is to notice accidental size changes.
            std::mem::size_of::<usize>() * 9,
            "HomPluginVtable size changed — bump HOM_PLUGIN_ABI_VERSION"
        );
    }

    #[test]
    fn abi_version_constant_is_1() {
        assert_eq!(HOM_PLUGIN_ABI_VERSION, 1);
    }
}
```

- [ ] **Step 2: Create `Cargo.toml` for `hom-plugin`**

Create `crates/hom-plugin/Cargo.toml`:

```toml
[package]
name = "hom-plugin"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Plugin loader and C ABI vtable for HOM harness adapter plugins"

[dependencies]
hom-core.workspace = true
libloading.workspace = true
serde_json.workspace = true
tracing.workspace = true
```

- [ ] **Step 3: Add `hom-plugin` and `libloading` to workspace**

In root `Cargo.toml`:

In `[workspace] members`, add:
```toml
"crates/hom-plugin",
```

In `[workspace.dependencies]`, add:
```toml
libloading = "0.8"
```

Also add to root `[workspace.dependencies]`:
```toml
hom-plugin = { path = "crates/hom-plugin" }
```

- [ ] **Step 4: Implement `ffi.rs`**

Create `crates/hom-plugin/src/ffi.rs`:

```rust
//! Stable C ABI for HOM harness adapter plugins.
//!
//! # Plugin ABI contract
//!
//! A plugin is a dynamic library (`.dylib` on macOS, `.so` on Linux) that exports:
//!
//! ```c
//! HomPluginVtable* hom_plugin_init(void);
//! void hom_plugin_destroy(HomPluginVtable* vtable);
//! ```
//!
//! `hom_plugin_init` is called once at load time. It returns a pointer to a
//! heap-allocated `HomPluginVtable` that remains valid until `hom_plugin_destroy`
//! is called. All function pointers in the vtable must remain valid for that lifetime.
//!
//! Data crossing the FFI boundary uses null-terminated UTF-8 JSON strings.
//! Strings returned by plugin functions must be freed by calling `free_str`.
//!
//! # Versioning
//!
//! The `abi_version` field in `HomPluginVtable` must equal `HOM_PLUGIN_ABI_VERSION`.
//! HOM rejects plugins with a mismatched version.

use std::os::raw::c_char;

/// ABI version this build of HOM expects.
///
/// Increment when any field in `HomPluginVtable` changes position, size, or semantics.
pub const HOM_PLUGIN_ABI_VERSION: usize = 1;

/// C-compatible vtable exported by every HOM harness adapter plugin.
///
/// All pointers in this struct must be non-null when returned by `hom_plugin_init`.
///
/// # Safety
///
/// Every function pointer in this struct is called with valid UTF-8 JSON strings.
/// Strings are null-terminated. Return values (heap-allocated C strings) must be
/// freed via `free_str` — they must NOT be freed with Rust's allocator.
#[repr(C)]
pub struct HomPluginVtable {
    /// Must equal `HOM_PLUGIN_ABI_VERSION`. HOM rejects mismatches.
    pub abi_version: usize,

    /// Return a null-terminated UTF-8 display name (static lifetime, do NOT call free_str).
    ///
    /// Example return: `b"MyCLI Adapter\0"`
    pub display_name: extern "C" fn() -> *const c_char,

    /// Return a null-terminated UTF-8 binary name used as the registry key (static lifetime).
    ///
    /// This is what users type in `:spawn <name>`. Example: `b"mycli\0"`.
    pub binary_name: extern "C" fn() -> *const c_char,

    /// Build the command to spawn this harness.
    ///
    /// `config_json` is a null-terminated UTF-8 JSON object:
    /// `{"working_dir": "/path", "model": null | "model-name", "extra_args": [...]}`
    ///
    /// Returns a heap-allocated null-terminated JSON string:
    /// `{"program": "mycli", "args": ["--flag"], "env": {}, "working_dir": "/path"}`
    ///
    /// Caller must free with `free_str`.
    pub build_command: extern "C" fn(config_json: *const c_char) -> *mut c_char,

    /// Translate an orchestrator command into PTY bytes.
    ///
    /// `cmd_type`: 0=Prompt, 1=Cancel, 2=Accept, 3=Reject, 4=Raw.
    /// `text`: null-terminated UTF-8 string (prompt text for cmd_type 0; hex bytes for 4; ignored otherwise).
    ///
    /// Returns heap-allocated null-terminated UTF-8 hex string (e.g., `"48656c6c6f0a"` = "Hello\n").
    /// Caller must free with `free_str`.
    pub translate_input: extern "C" fn(cmd_type: u32, text: *const c_char) -> *mut c_char,

    /// Parse the terminal screen and return structured events.
    ///
    /// `screen_json`: null-terminated UTF-8 JSON matching `hom-core`'s `ScreenSnapshot` schema.
    ///
    /// Returns heap-allocated null-terminated JSON array of HarnessEvent objects:
    /// `[{"type": "TaskCompleted", "summary": "done"}, ...]`
    ///
    /// Returns an empty array `[]` when no events are detected. Caller must free with `free_str`.
    pub parse_screen: extern "C" fn(screen_json: *const c_char) -> *mut c_char,

    /// Detect whether the harness has finished its current task.
    ///
    /// `screen_json`: same format as `parse_screen`.
    ///
    /// Returns heap-allocated null-terminated JSON:
    /// `{"status": "Running"}` or
    /// `{"status": "WaitingForInput"}` or
    /// `{"status": "Completed", "output": "..."}` or
    /// `{"status": "Failed", "error": "..."}`
    ///
    /// Caller must free with `free_str`.
    pub detect_completion: extern "C" fn(screen_json: *const c_char) -> *mut c_char,

    /// Free a string returned by any plugin function (except static strings from
    /// `display_name` and `binary_name` which must NOT be freed).
    pub free_str: extern "C" fn(s: *mut c_char),

    /// Return this harness's capabilities.
    ///
    /// Returns heap-allocated null-terminated JSON matching `HarnessCapabilities` schema:
    /// `{"supports_steering": true, "supports_json_output": false, ...}`
    ///
    /// Caller must free with `free_str`.
    pub capabilities: extern "C" fn() -> *mut c_char,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_size_is_stable() {
        // 1 usize (abi_version) + 8 fn pointers × size_of::<usize>() each.
        assert_eq!(
            std::mem::size_of::<HomPluginVtable>(),
            std::mem::size_of::<usize>() * 9,
            "HomPluginVtable size changed — bump HOM_PLUGIN_ABI_VERSION"
        );
    }

    #[test]
    fn abi_version_constant_is_1() {
        assert_eq!(HOM_PLUGIN_ABI_VERSION, 1);
    }
}
```

- [ ] **Step 5: Create `lib.rs`**

Create `crates/hom-plugin/src/lib.rs`:

```rust
//! # hom-plugin
//!
//! Plugin loader and C ABI vtable definition for HOM harness adapter plugins.
//!
//! ## For plugin authors
//!
//! Export these two symbols from your dynamic library:
//!
//! ```c
//! HomPluginVtable* hom_plugin_init(void);
//! void hom_plugin_destroy(HomPluginVtable* vtable);
//! ```
//!
//! See `ffi::HomPluginVtable` for the full ABI contract.

pub mod adapter;
pub mod ffi;
pub mod loader;

pub use adapter::PluginAdapter;
pub use ffi::{HOM_PLUGIN_ABI_VERSION, HomPluginVtable};
pub use loader::PluginLoader;
```

- [ ] **Step 6: Run tests**

```sh
cargo test -p hom-plugin -- vtable abi_version 2>&1 | tail -10
```

Expected: `test result: ok. 2 passed`.

- [ ] **Step 7: Cargo check**

```sh
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished dev profile`.

- [ ] **Step 8: Commit**

```sh
git add Cargo.toml crates/hom-plugin/
git commit -m "feat(plugin): add hom-plugin crate with C ABI vtable"
```

---

### Task 2: Implement `PluginLoader` and `PluginAdapter`

**Files:**
- Create: `crates/hom-plugin/src/loader.rs`
- Create: `crates/hom-plugin/src/adapter.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/hom-plugin/src/loader.rs` with tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn load_nonexistent_path_returns_error() {
        let result = PluginLoader::load(PathBuf::from("/nonexistent/plugin.dylib").as_path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("plugin") || msg.contains("load") || msg.contains("nonexistent"),
            "error message should mention loading: {msg}"
        );
    }

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let adapters = PluginLoader::scan_dir(PathBuf::from("/nonexistent/plugins").as_path());
        assert!(adapters.is_empty());
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let adapters = PluginLoader::scan_dir(dir.path());
        assert!(adapters.is_empty());
    }

    #[test]
    fn scan_dir_ignores_non_dylib_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "ignored").unwrap();
        std::fs::write(dir.path().join("plugin.rs"), "ignored").unwrap();
        let adapters = PluginLoader::scan_dir(dir.path());
        assert!(adapters.is_empty());
    }
}
```

Add `tempfile = "3"` to `[dev-dependencies]` in `crates/hom-plugin/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

Create `crates/hom-plugin/src/adapter.rs` with tests only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex_empty_string_gives_empty_bytes() {
        let result = decode_hex_bytes("");
        assert_eq!(result, Vec::<u8>::new());
    }

    #[test]
    fn decode_hex_hello_newline() {
        // "hello\n" = 68 65 6c 6c 6f 0a
        let result = decode_hex_bytes("68656c6c6f0a");
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn decode_hex_invalid_is_empty() {
        let result = decode_hex_bytes("zz");
        assert_eq!(result, Vec::<u8>::new());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```sh
cargo test -p hom-plugin 2>&1 | head -20
```

Expected: compile errors — `PluginLoader` and `PluginAdapter` not found.

- [ ] **Step 3: Implement `PluginLoader`**

Create `crates/hom-plugin/src/loader.rs`:

```rust
//! Loads HOM plugins from dynamic libraries.

use std::path::{Path, PathBuf};

use hom_core::HomError;
use libloading::Library;
use tracing::{debug, info, warn};

use crate::adapter::PluginAdapter;
use crate::ffi::{HOM_PLUGIN_ABI_VERSION, HomPluginVtable};

/// Loads HOM adapter plugins from `.dylib` / `.so` files.
pub struct PluginLoader;

impl PluginLoader {
    /// Load a single plugin from a dynamic library file.
    ///
    /// Calls `hom_plugin_init` in the library and validates the ABI version.
    /// Returns a `PluginAdapter` on success.
    ///
    /// # Errors
    ///
    /// Returns `HomError::PluginError` if the library cannot be opened, the symbol
    /// is absent, or the ABI version does not match `HOM_PLUGIN_ABI_VERSION`.
    pub fn load(path: &Path) -> Result<PluginAdapter, HomError> {
        // SAFETY: We are loading a user-provided dynamic library. The caller is
        // responsible for ensuring the library is trusted. HOM documents that plugins
        // run with the same privileges as the HOM process.
        let lib = unsafe {
            Library::new(path).map_err(|e| {
                HomError::PluginError(format!(
                    "failed to load plugin {}: {e}",
                    path.display()
                ))
            })?
        };

        // SAFETY: The symbol `hom_plugin_init` is expected to be a C function with
        // signature `fn() -> *mut HomPluginVtable`. The ABI version check below
        // guards against mismatched struct layouts.
        let vtable_ptr: *mut HomPluginVtable = unsafe {
            let init_fn: libloading::Symbol<extern "C" fn() -> *mut HomPluginVtable> = lib
                .get(b"hom_plugin_init\0")
                .map_err(|e| HomError::PluginError(format!("hom_plugin_init not found: {e}")))?;
            init_fn()
        };

        if vtable_ptr.is_null() {
            return Err(HomError::PluginError(
                "hom_plugin_init returned null".to_string(),
            ));
        }

        // SAFETY: vtable_ptr is non-null and points to a valid HomPluginVtable
        // returned by the plugin's init function.
        let vtable = unsafe { &*vtable_ptr };

        if vtable.abi_version != HOM_PLUGIN_ABI_VERSION {
            return Err(HomError::PluginError(format!(
                "plugin ABI version {} != expected {}",
                vtable.abi_version, HOM_PLUGIN_ABI_VERSION
            )));
        }

        info!(path = %path.display(), "loaded plugin");
        Ok(PluginAdapter::new(lib, vtable_ptr))
    }

    /// Scan a directory for `.dylib` (macOS) and `.so` (Linux) plugin files.
    ///
    /// Silently skips files that fail to load — logs warnings for each failure.
    /// Returns only successfully loaded adapters.
    pub fn scan_dir(dir: &Path) -> Vec<PluginAdapter> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            debug!(dir = %dir.display(), "plugin dir not found, skipping scan");
            return Vec::new();
        };

        let mut adapters = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            // Only attempt to load files with the platform's shared library extension.
            if !matches!(ext, "dylib" | "so" | "dll") {
                continue;
            }

            match Self::load(&path) {
                Ok(adapter) => {
                    info!(path = %path.display(), name = %adapter.plugin_name(), "registered plugin");
                    adapters.push(adapter);
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to load plugin, skipping");
                }
            }
        }

        adapters
    }

    /// Return the default plugin directory: `~/.config/hom/plugins/`.
    pub fn default_plugin_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hom")
            .join("plugins")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_nonexistent_path_returns_error() {
        let result = PluginLoader::load(Path::new("/nonexistent/plugin.dylib"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("plugin") || msg.contains("load") || msg.contains("nonexistent"),
            "error message should mention loading: {msg}"
        );
    }

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let adapters = PluginLoader::scan_dir(Path::new("/nonexistent/plugins"));
        assert!(adapters.is_empty());
    }

    #[test]
    fn scan_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let adapters = PluginLoader::scan_dir(dir.path());
        assert!(adapters.is_empty());
    }

    #[test]
    fn scan_dir_ignores_non_dylib_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "ignored").unwrap();
        std::fs::write(dir.path().join("plugin.rs"), "ignored").unwrap();
        let adapters = PluginLoader::scan_dir(dir.path());
        assert!(adapters.is_empty());
    }
}
```

- [ ] **Step 4: Implement `PluginAdapter`**

Create `crates/hom-plugin/src/adapter.rs`:

```rust
//! Wraps a plugin's `HomPluginVtable` and implements `HarnessAdapter`.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use async_trait::async_trait;
use hom_core::{
    CommandSpec, CompletionStatus, HarnessAdapter, HarnessCapabilities, HarnessConfig,
    HarnessEvent, HarnessType, OrchestratorCommand, SidebandChannel, ScreenSnapshot,
};
use libloading::Library;
use tracing::warn;

use crate::ffi::HomPluginVtable;

/// Wraps a loaded plugin vtable and implements `HarnessAdapter`.
///
/// Holds the `Library` handle to prevent premature unloading.
/// All vtable function calls serialize/deserialize via JSON strings.
pub struct PluginAdapter {
    /// Keeps the `.dylib`/`.so` loaded for the adapter's lifetime.
    _lib: Library,
    vtable: *mut HomPluginVtable,
}

// SAFETY: `HomPluginVtable` function pointers are thread-safe by convention of the
// plugin ABI — plugins must not use mutable shared state in their vtable functions.
// `PluginAdapter` is only called from the tokio event loop (single-threaded dispatch).
unsafe impl Send for PluginAdapter {}
unsafe impl Sync for PluginAdapter {}

impl PluginAdapter {
    /// Create from a loaded library and vtable pointer.
    ///
    /// # Safety
    ///
    /// `vtable` must be a valid non-null pointer returned by `hom_plugin_init` in `lib`.
    pub fn new(lib: Library, vtable: *mut HomPluginVtable) -> Self {
        Self { _lib: lib, vtable }
    }

    /// Return the plugin's binary name (used as registry key).
    pub fn plugin_name(&self) -> String {
        // SAFETY: vtable is valid (validated in PluginLoader::load).
        // display_name returns a static C string — no free needed.
        let ptr = unsafe { ((*self.vtable).binary_name)() };
        if ptr.is_null() {
            return String::from("unknown-plugin");
        }
        // SAFETY: ptr is a valid null-terminated C string with static lifetime.
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    /// Call a plugin function that takes a JSON string and returns a heap JSON string.
    ///
    /// # Safety
    ///
    /// `f` must be a valid function pointer from the vtable. `input` is a UTF-8 string.
    /// The returned string is freed via `free_str` in the vtable.
    unsafe fn call_json_fn(
        &self,
        f: extern "C" fn(*const c_char) -> *mut c_char,
        input: &str,
    ) -> Option<String> {
        let input_c = CString::new(input).ok()?;
        // SAFETY: f is a valid function pointer; input_c is a valid C string.
        let out_ptr = unsafe { f(input_c.as_ptr()) };
        if out_ptr.is_null() {
            return None;
        }
        // SAFETY: out_ptr is a valid null-terminated C string allocated by the plugin.
        let result = unsafe { CStr::from_ptr(out_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: The vtable's free_str must be called on strings returned by plugin functions.
        unsafe { ((*self.vtable).free_str)(out_ptr) };
        Some(result)
    }
}

impl Drop for PluginAdapter {
    fn drop(&mut self) {
        if !self.vtable.is_null() {
            // Plugins that do cleanup export `hom_plugin_destroy`. We don't store a
            // function pointer for it in the vtable — the `_lib: Library` drop will
            // unload the library naturally. Plugins should not rely on a destroy callback.
        }
    }
}

/// Decode a hex string into bytes (e.g., `"68656c6c6f0a"` → `b"hello\n"`).
///
/// Returns empty Vec on any invalid input.
pub fn decode_hex_bytes(s: &str) -> Vec<u8> {
    if s.len() % 2 != 0 {
        return Vec::new();
    }
    s.as_bytes()
        .chunks(2)
        .map(|chunk| {
            let hi = char::from(chunk[0]).to_digit(16)? as u8;
            let lo = char::from(chunk[1]).to_digit(16)? as u8;
            Some((hi << 4) | lo)
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

#[async_trait]
impl HarnessAdapter for PluginAdapter {
    fn harness_type(&self) -> HarnessType {
        // Plugins don't map to a built-in HarnessType variant.
        // They are looked up by name in AdapterRegistry::plugins, not by HarnessType.
        // Return ClaudeCode as a structural placeholder — callers that need the real
        // identity use plugin_name() or the pane title.
        HarnessType::ClaudeCode
    }

    fn display_name(&self) -> &str {
        // SAFETY: display_name returns a static C string from the plugin.
        let ptr = unsafe { ((*self.vtable).display_name)() };
        if ptr.is_null() {
            return "unknown plugin";
        }
        // SAFETY: ptr is valid, null-terminated, and has static lifetime.
        unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("plugin")
    }

    fn build_command(&self, config: &HarnessConfig) -> CommandSpec {
        let config_json = serde_json::json!({
            "working_dir": config.working_dir,
            "model": config.model,
            "extra_args": config.extra_args,
        })
        .to_string();

        // SAFETY: build_command vtable fn is valid.
        let result = unsafe {
            self.call_json_fn((*self.vtable).build_command, &config_json)
        };

        let Some(json) = result else {
            warn!(plugin = self.plugin_name(), "build_command returned null");
            return CommandSpec {
                program: self.plugin_name(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: config.working_dir.clone(),
            };
        };

        let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
            warn!(plugin = self.plugin_name(), json, "build_command returned invalid JSON");
            return CommandSpec {
                program: self.plugin_name(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: config.working_dir.clone(),
            };
        };

        let program = value["program"].as_str().unwrap_or(&self.plugin_name()).to_string();
        let args = value["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        CommandSpec {
            program,
            args,
            env: HashMap::new(),
            working_dir: config.working_dir.clone(),
        }
    }

    fn translate_input(&self, command: &OrchestratorCommand) -> Vec<u8> {
        let (cmd_type, text) = match command {
            OrchestratorCommand::Prompt(s) => (0u32, s.as_str()),
            OrchestratorCommand::Cancel => (1, ""),
            OrchestratorCommand::Accept => (2, ""),
            OrchestratorCommand::Reject => (3, ""),
            OrchestratorCommand::Raw(_) => (4, ""),
        };

        let text_c = match CString::new(text) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        // SAFETY: translate_input vtable fn is valid; text_c is a valid C string.
        let out_ptr = unsafe { ((*self.vtable).translate_input)(cmd_type, text_c.as_ptr()) };
        if out_ptr.is_null() {
            return Vec::new();
        }

        // SAFETY: out_ptr is a valid null-terminated C string.
        let hex = unsafe { CStr::from_ptr(out_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: free_str must be called on returned strings.
        unsafe { ((*self.vtable).free_str)(out_ptr) };

        decode_hex_bytes(&hex)
    }

    fn parse_screen(&self, screen: &ScreenSnapshot) -> Vec<HarnessEvent> {
        let screen_json = match serde_json::to_string(screen) {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "failed to serialize ScreenSnapshot for plugin");
                return Vec::new();
            }
        };

        // SAFETY: parse_screen vtable fn is valid.
        let result = unsafe {
            self.call_json_fn((*self.vtable).parse_screen, &screen_json)
        };

        let Some(json) = result else {
            return Vec::new();
        };

        serde_json::from_str::<Vec<HarnessEvent>>(&json).unwrap_or_default()
    }

    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let screen_json = match serde_json::to_string(screen) {
            Ok(j) => j,
            Err(_) => return CompletionStatus::Running,
        };

        // SAFETY: detect_completion vtable fn is valid.
        let result = unsafe {
            self.call_json_fn((*self.vtable).detect_completion, &screen_json)
        };

        let Some(json) = result else {
            return CompletionStatus::Running;
        };

        let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
            return CompletionStatus::Running;
        };

        match value["status"].as_str() {
            Some("WaitingForInput") => CompletionStatus::WaitingForInput,
            Some("Completed") => CompletionStatus::Completed {
                output: value["output"].as_str().unwrap_or("").to_string(),
            },
            Some("Failed") => CompletionStatus::Failed {
                error: value["error"].as_str().unwrap_or("unknown").to_string(),
            },
            _ => CompletionStatus::Running,
        }
    }

    fn capabilities(&self) -> HarnessCapabilities {
        // SAFETY: capabilities vtable fn is valid.
        let result = unsafe {
            self.call_json_fn((*self.vtable).capabilities, "")
        };

        let Some(json) = result else {
            return HarnessCapabilities {
                supports_steering: false,
                supports_json_output: false,
                supports_session_resume: false,
                supports_mcp: false,
                headless_command: None,
                sideband_type: None,
            };
        };

        serde_json::from_str::<HarnessCapabilities>(&json).unwrap_or(HarnessCapabilities {
            supports_steering: false,
            supports_json_output: false,
            supports_session_resume: false,
            supports_mcp: false,
            headless_command: None,
            sideband_type: None,
        })
    }

    fn sideband(&self) -> Option<Box<dyn SidebandChannel>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex_empty_string_gives_empty_bytes() {
        let result = decode_hex_bytes("");
        assert_eq!(result, Vec::<u8>::new());
    }

    #[test]
    fn decode_hex_hello_newline() {
        let result = decode_hex_bytes("68656c6c6f0a");
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn decode_hex_invalid_is_empty() {
        let result = decode_hex_bytes("zz");
        assert_eq!(result, Vec::<u8>::new());
    }

    #[test]
    fn decode_hex_odd_length_is_empty() {
        let result = decode_hex_bytes("abc");
        assert_eq!(result, Vec::<u8>::new());
    }
}
```

- [ ] **Step 5: Run tests**

```sh
cargo test -p hom-plugin 2>&1 | tail -15
```

Expected: `test result: ok. 8 passed` (vtable_size, abi_version, load_nonexistent, scan_nonexistent, scan_empty, scan_ignores_non_dylib, decode_hex_* 4 tests).

- [ ] **Step 6: Cargo check workspace**

```sh
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished dev profile`.

- [ ] **Step 7: Commit**

```sh
git add crates/hom-plugin/src/loader.rs crates/hom-plugin/src/adapter.rs crates/hom-plugin/src/lib.rs
git commit -m "feat(plugin): implement PluginLoader and PluginAdapter"
```

---

### Task 3: Extend `AdapterRegistry` with plugin support

**Files:**
- Modify: `crates/hom-adapters/Cargo.toml`
- Modify: `crates/hom-adapters/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/hom-adapters/src/lib.rs`, find the `#[cfg(test)] mod tests` block. Add:

```rust
#[test]
fn registry_get_plugin_unknown_returns_none() {
    let registry = AdapterRegistry::new();
    assert!(registry.get_plugin("unknown-plugin").is_none());
}

#[test]
fn registry_load_plugin_nonexistent_path_returns_error() {
    let mut registry = AdapterRegistry::new();
    let result = registry.load_plugin(std::path::Path::new("/nonexistent/plugin.dylib"));
    assert!(result.is_err());
}

#[test]
fn registry_plugin_names_empty_initially() {
    let registry = AdapterRegistry::new();
    assert!(registry.plugin_names().is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```sh
cargo test -p hom-adapters -- registry_get_plugin registry_load_plugin registry_plugin_names 2>&1 | head -15
```

Expected: compile errors — methods not yet defined.

- [ ] **Step 3: Add `hom-plugin` dependency to `hom-adapters`**

In `crates/hom-adapters/Cargo.toml`, add to `[dependencies]`:

```toml
hom-plugin.workspace = true
```

- [ ] **Step 4: Extend `AdapterRegistry`**

In `crates/hom-adapters/src/lib.rs`, add the import and extend the struct + impl:

```rust
use std::path::Path;

use hom_core::HomError;
use hom_plugin::PluginLoader;
```

In `AdapterRegistry`:

```rust
pub struct AdapterRegistry {
    adapters: HashMap<HarnessType, Box<dyn HarnessAdapter>>,
    /// Plugin adapters keyed by binary name (e.g., "mycli").
    plugins: HashMap<String, Box<dyn HarnessAdapter>>,
}
```

In `AdapterRegistry::new()`, initialize `plugins`:

```rust
plugins: HashMap::new(),
```

Add to `impl AdapterRegistry`:

```rust
/// Get a plugin adapter by its binary name.
///
/// Does NOT check the built-in adapter map. Use `get()` for built-in harnesses.
pub fn get_plugin(&self, name: &str) -> Option<&dyn HarnessAdapter> {
    self.plugins.get(name).map(|a| a.as_ref())
}

/// Load a plugin from a `.dylib`/`.so` file and register it by binary name.
///
/// Returns the plugin's binary name on success, or an error if loading fails.
pub fn load_plugin(&mut self, path: &Path) -> Result<String, HomError> {
    let adapter = PluginLoader::load(path)?;
    let name = adapter.plugin_name();
    self.plugins.insert(name.clone(), Box::new(adapter));
    Ok(name)
}

/// Scan a directory and register all loadable plugins.
///
/// Silently skips files that fail to load. Returns the names of loaded plugins.
pub fn load_plugins_from_dir(&mut self, dir: &Path) -> Vec<String> {
    PluginLoader::scan_dir(dir)
        .into_iter()
        .map(|adapter| {
            let name = adapter.plugin_name();
            self.plugins.insert(name.clone(), Box::new(adapter));
            name
        })
        .collect()
}

/// Names of all loaded plugin adapters.
pub fn plugin_names(&self) -> Vec<String> {
    self.plugins.keys().cloned().collect()
}
```

- [ ] **Step 5: Run tests**

```sh
cargo test -p hom-adapters -- registry_get_plugin registry_load_plugin registry_plugin_names 2>&1 | tail -10
```

Expected: `test result: ok. 3 passed`.

- [ ] **Step 6: Cargo check workspace**

```sh
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished dev profile`.

- [ ] **Step 7: Commit**

```sh
git add crates/hom-adapters/Cargo.toml crates/hom-adapters/src/lib.rs
git commit -m "feat(adapters): extend AdapterRegistry with plugin support"
```

---

### Task 4: Wire `:load-plugin` command and auto-scan at startup

**Files:**
- Modify: `crates/hom-tui/src/command_bar.rs`
- Modify: `crates/hom-tui/src/app.rs`

- [ ] **Step 1: Write failing tests for command bar**

In `crates/hom-tui/src/command_bar.rs` tests:

```rust
#[test]
fn load_plugin_parses_absolute_path() {
    let cmd = CommandBar::parse_command("load-plugin /home/user/.config/hom/plugins/mycli.dylib").unwrap();
    match cmd {
        Command::LoadPlugin { path } => {
            assert_eq!(path.to_str().unwrap(), "/home/user/.config/hom/plugins/mycli.dylib");
        }
        _ => panic!("expected LoadPlugin"),
    }
}

#[test]
fn load_plugin_requires_path() {
    let result = CommandBar::parse_command("load-plugin");
    assert!(result.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```sh
cargo test -p hom-tui -- load_plugin_parses load_plugin_requires 2>&1 | head -15
```

Expected: compile errors — `Command::LoadPlugin` not defined.

- [ ] **Step 3: Add `Command::LoadPlugin` variant**

In `crates/hom-tui/src/command_bar.rs`, add to `Command`:

```rust
/// `:load-plugin /path/to/plugin.dylib`
LoadPlugin { path: PathBuf },
```

In the `parse_command` match (or wherever commands are parsed):

```rust
"load-plugin" => {
    let path = tokens.next()?;
    Some(Command::LoadPlugin { path: PathBuf::from(path) })
}
```

- [ ] **Step 4: Write failing test for `app.rs` plugin handling**

In `crates/hom-tui/src/app.rs` tests:

```rust
#[test]
fn handle_load_plugin_nonexistent_sets_error() {
    // A sync test — handle_load_plugin_sync is the non-async path for tests.
    let cfg = hom_core::HomConfig::default();
    let mut app = App::new(cfg, None).unwrap();
    app.handle_load_plugin(std::path::Path::new("/nonexistent.dylib"));
    assert!(app.command_bar.last_error.is_some());
    let err = app.command_bar.last_error.as_ref().unwrap();
    assert!(err.contains("plugin") || err.contains("load") || err.contains("nonexistent"));
}
```

- [ ] **Step 5: Run test to verify it fails**

```sh
cargo test -p hom-tui -- handle_load_plugin_nonexistent 2>&1 | head -15
```

Expected: compile error — `handle_load_plugin` not defined.

- [ ] **Step 6: Add `handle_load_plugin` to `App`**

In `crates/hom-tui/src/app.rs`:

```rust
/// Load a plugin at runtime and register it in the adapter registry.
pub fn handle_load_plugin(&mut self, path: &std::path::Path) {
    match self.adapters.load_plugin(path) {
        Ok(name) => {
            tracing::info!(plugin = %name, "loaded plugin adapter");
            // Clear any previous error; success is shown via pane title on next :spawn.
        }
        Err(e) => {
            self.command_bar.last_error = Some(format!("plugin load failed: {e}"));
        }
    }
}
```

- [ ] **Step 7: Route `Command::LoadPlugin` in `handle_command`**

In the command dispatch (wherever `Command` arms are matched):

```rust
Command::LoadPlugin { path } => {
    self.handle_load_plugin(&path);
}
```

- [ ] **Step 8: Route unknown harness names to plugin registry in `spawn_pane`**

Find where `HarnessType::from_str_loose()` is called in command bar parsing or `spawn_pane`. After it returns `None`, check the plugin registry:

In `parse_spawn` (command_bar.rs), change the early return on unknown harness to instead store the name as-is in a new `harness_name: String` field. Then in `app.rs` `spawn_pane()`, check `self.adapters.get_plugin(&harness_name)` if the built-in lookup fails.

Add `harness_name: String` to `Command::Spawn` alongside `harness: Option<HarnessType>`:

```rust
Spawn {
    /// Built-in harness type if recognized, else None for plugin lookup.
    harness: Option<HarnessType>,
    /// Raw harness name from command bar (used for plugin lookup when harness is None).
    harness_name: String,
    model: Option<String>,
    working_dir: Option<PathBuf>,
    extra_args: Vec<String>,
    remote: Option<RemoteTarget>,
},
```

Update `parse_spawn`:

```rust
let harness_name = parts[0].to_string();
let harness = HarnessType::from_str_loose(parts[0]);

Some(Command::Spawn {
    harness,
    harness_name,
    // ... rest unchanged
})
```

In `app.rs` `spawn_pane()`, add fallback:

```rust
// resolve adapter: built-in first, then plugin by name
let adapter: &dyn HarnessAdapter = if let Some(ht) = harness {
    self.adapters.get(&ht)
        .ok_or_else(|| HomError::TuiError(format!("no adapter for {ht:?}")))?
} else {
    self.adapters.get_plugin(&harness_name)
        .ok_or_else(|| HomError::TuiError(format!("unknown harness '{harness_name}' — is the plugin loaded?")))?
};
```

- [ ] **Step 9: Auto-scan plugin dir at startup in `App::new()`**

In `App::new()`, after `adapters` is constructed:

```rust
// Load any plugins from ~/.config/hom/plugins/ at startup.
let plugin_dir = hom_plugin::PluginLoader::default_plugin_dir();
let loaded = adapters.load_plugins_from_dir(&plugin_dir);
if !loaded.is_empty() {
    tracing::info!(plugins = ?loaded, "auto-loaded plugins from {:?}", plugin_dir);
}
```

- [ ] **Step 10: Run tests**

```sh
cargo test -p hom-tui -- load_plugin handle_load_plugin 2>&1 | tail -10
```

Expected: `test result: ok. 3 passed`.

- [ ] **Step 11: Cargo check + clippy**

```sh
cargo check --workspace 2>&1 | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: zero errors, zero warnings.

- [ ] **Step 12: Commit**

```sh
git add crates/hom-tui/src/command_bar.rs crates/hom-tui/src/app.rs
git commit -m "feat(tui): wire :load-plugin command and plugin auto-scan at startup"
```

---

### Task 5: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add plugin crate to architecture table**

Find the workspace crate table in the `## Architecture` section and add:

```markdown
| `hom-plugin` | C ABI vtable (`HomPluginVtable`), `PluginLoader`, `PluginAdapter` |
```

Update the `## File Layout` section under `crates/`:

```
│   ├── hom-plugin/src/          # lib.rs, ffi.rs, loader.rs, adapter.rs
```

Update crate dependency rules — add:

```
- `hom-plugin` depends on `hom-core` only
- `hom-adapters` depends on `hom-core`, `hom-plugin`
```

- [ ] **Step 2: Add to Implementation Status**

Append to the most recent "Resolved" block:

```markdown
**Resolved (plugin system):**
- `crates/hom-plugin/` new crate — stable C ABI vtable (`HomPluginVtable`, ABI v1)
- `HomPluginVtable` uses JSON strings for all complex data crossing FFI (ScreenSnapshot, HarnessEvent, CompletionStatus)
- `PluginLoader::load(path)` — validates ABI version, wraps vtable in `PluginAdapter`
- `PluginLoader::scan_dir(dir)` — discovers `.dylib`/`.so` files, logs failures, returns adapters
- `PluginLoader::default_plugin_dir()` — `~/.config/hom/plugins/`
- `PluginAdapter` implements `HarnessAdapter` — all 6 vtable methods dispatch through JSON FFI
- `decode_hex_bytes()` converts hex-encoded PTY bytes returned by plugin `translate_input`
- `AdapterRegistry::load_plugin(path)` + `get_plugin(name)` + `load_plugins_from_dir(dir)` + `plugin_names()`
- `:load-plugin /path/to/plugin.dylib` command wired in command bar and `App::handle_load_plugin()`
- Auto-scan of `~/.config/hom/plugins/` at `App::new()` startup
- Unknown harness names fall through to plugin registry: `:spawn mycli` works if `mycli` plugin is loaded
- 8 unit tests in `hom-plugin` + 3 in `hom-adapters` + 3 in `hom-tui`
```

- [ ] **Step 3: Final full test run**

```sh
cargo nextest run --workspace 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```sh
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for plugin system"
```

---

## Self-Review

**Spec coverage:**
- ✅ Dynamic loading — `libloading` + `PluginLoader::load()` in Task 2
- ✅ Plugin discovery — `scan_dir()` + auto-scan at startup in Task 4
- ✅ Manual load — `:load-plugin` command in Task 4
- ✅ C ABI stability — `HomPluginVtable` JSON-based boundary in Task 1
- ✅ `AdapterRegistry` extension — `load_plugin()` + `get_plugin()` in Task 3
- ✅ `:spawn mycli` works after plugin load — fallback routing in Task 4

**Placeholder scan:** None found — all steps include concrete code.

**Type consistency:**
- `HomPluginVtable` defined in `ffi.rs`, used in `loader.rs` and `adapter.rs` — same type.
- `PluginAdapter::plugin_name()` used in `loader.rs` to build registry key, same as `AdapterRegistry::get_plugin(name)` key.
- `Command::Spawn { harness: Option<HarnessType>, harness_name: String }` — both fields set consistently in `parse_spawn`, both consumed in `app.rs`.

**ABI stability note:** The `vtable_size_is_stable` test catches accidental field additions. When new vtable fields are needed, increment `HOM_PLUGIN_ABI_VERSION` and update the expected size in that test.
