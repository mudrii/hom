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
    pub display_name: extern "C" fn() -> *const c_char,

    /// Return a null-terminated UTF-8 binary name used as the registry key (static lifetime).
    pub binary_name: extern "C" fn() -> *const c_char,

    /// Build the command to spawn this harness.
    ///
    /// `config_json`: `{"working_dir": "/path", "model": null | "model-name", "extra_args": [...]}`
    ///
    /// Returns heap-allocated JSON: `{"program": "mycli", "args": ["--flag"], "env": {}, "working_dir": "/path"}`
    /// Caller must free with `free_str`.
    pub build_command: extern "C" fn(config_json: *const c_char) -> *mut c_char,

    /// Translate an orchestrator command into PTY bytes.
    ///
    /// `cmd_type`: 0=Prompt, 1=Cancel, 2=Accept, 3=Reject, 4=Raw.
    /// `text`: null-terminated UTF-8 string.
    ///
    /// Returns heap-allocated hex string (e.g., `"48656c6c6f0a"` = "Hello\n").
    /// Caller must free with `free_str`.
    pub translate_input: extern "C" fn(cmd_type: u32, text: *const c_char) -> *mut c_char,

    /// Parse the terminal screen and return structured events.
    ///
    /// `screen_json`: null-terminated UTF-8 JSON matching `ScreenSnapshot` schema.
    ///
    /// Returns heap-allocated JSON array: `[{"type": "TaskCompleted", "summary": "done"}, ...]`
    /// Caller must free with `free_str`.
    pub parse_screen: extern "C" fn(screen_json: *const c_char) -> *mut c_char,

    /// Detect whether the harness has finished its current task.
    ///
    /// `screen_json`: same format as `parse_screen`.
    ///
    /// Returns heap-allocated JSON:
    /// `{"status": "Running"}` or `{"status": "WaitingForInput"}` or
    /// `{"status": "Completed", "output": "..."}` or `{"status": "Failed", "error": "..."}`
    /// Caller must free with `free_str`.
    pub detect_completion: extern "C" fn(screen_json: *const c_char) -> *mut c_char,

    /// Free a string returned by any plugin function (except static strings from
    /// `display_name` and `binary_name`).
    pub free_str: extern "C" fn(s: *mut c_char),

    /// Return this harness's capabilities as heap-allocated JSON.
    /// Caller must free with `free_str`.
    pub capabilities: extern "C" fn() -> *mut c_char,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_size_is_stable() {
        // 1 usize (abi_version) + 8 fn pointers × size_of::<usize>() each = 9 × 8 = 72 bytes.
        // If this fails after adding a field, bump HOM_PLUGIN_ABI_VERSION.
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
