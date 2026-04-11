//! Wraps a plugin's `HomPluginVtable` and implements `HarnessAdapter`.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use hom_core::{
    Cell, CellAttributes, CommandSpec, CompletionStatus, CursorState, HarnessCapabilities,
    HarnessConfig, HarnessEvent, HarnessType, OrchestratorCommand, ScreenSnapshot, SidebandChannel,
    TermColor,
};
use libloading::Library;
use serde::Serialize;
use tracing::warn;

use crate::ffi::{HomInputKind, HomPluginVtable};

/// Wraps a loaded plugin vtable and implements `HarnessAdapter`.
///
/// Holds the `Library` handle to prevent premature unloading.
/// The `display_name` and `plugin_name` strings are cached at construction
/// time so that `HarnessAdapter::display_name()` can return a `&str` with
/// lifetime tied to `self`.
///
/// # Field declaration order
///
/// Fields drop in declaration order. `_lib` is declared last so the dynamic
/// library remains loaded when `Drop` calls `destroy_fn(vtable)`.
#[derive(Debug)]
pub struct PluginAdapter {
    /// Cached from `vtable.display_name()` at construction.
    display_name: String,
    /// Cached from `vtable.binary_name()` at construction; used as registry key.
    binary_name: String,
    /// Optional cleanup function called in `Drop` before the library unloads.
    destroy_fn: Option<extern "C" fn(*mut HomPluginVtable)>,
    vtable: *mut HomPluginVtable,
    /// Keeps the `.dylib`/`.so` loaded for the adapter's lifetime.
    /// Declared last so it drops after `destroy_fn` has been called.
    _lib: Library,
}

// SAFETY: `PluginAdapter` is only accessed from the single-threaded TUI event loop.
// The vtable function pointers are C function pointers (stateless code pointers)
// with no thread affinity. No concurrent access to the vtable ever occurs.
unsafe impl Send for PluginAdapter {}
unsafe impl Sync for PluginAdapter {}

impl Drop for PluginAdapter {
    fn drop(&mut self) {
        if let Some(destroy) = self.destroy_fn {
            // vtable is still valid because _lib has not been dropped yet —
            // struct fields drop in declaration order, and _lib is declared after
            // destroy_fn and vtable, so the library is still loaded here.
            destroy(self.vtable);
        }
    }
}

impl PluginAdapter {
    /// Create from a loaded library, vtable pointer, and optional destroy function.
    ///
    /// Eagerly caches `display_name` and `binary_name` from the vtable.
    ///
    /// # Safety
    ///
    /// `vtable` must be a valid non-null pointer returned by `hom_plugin_init` in `lib`.
    /// `destroy_fn`, if provided, must be the plugin's `hom_plugin_destroy` symbol.
    pub unsafe fn new(
        lib: Library,
        vtable: *mut HomPluginVtable,
        destroy_fn: Option<extern "C" fn(*mut HomPluginVtable)>,
    ) -> Self {
        // SAFETY: vtable is non-null and was validated by PluginLoader::load.
        // display_name and binary_name return static C strings — no free needed.
        let (dn_ptr, bn_ptr) = unsafe { (((*vtable).display_name)(), ((*vtable).binary_name)()) };

        let display_name = if dn_ptr.is_null() {
            "unknown plugin".to_string()
        } else {
            // SAFETY: dn_ptr is a valid, null-terminated C string with static lifetime.
            unsafe { CStr::from_ptr(dn_ptr) }
                .to_string_lossy()
                .into_owned()
        };

        let binary_name = if bn_ptr.is_null() {
            "unknown-plugin".to_string()
        } else {
            // SAFETY: bn_ptr is a valid, null-terminated C string with static lifetime.
            unsafe { CStr::from_ptr(bn_ptr) }
                .to_string_lossy()
                .into_owned()
        };

        Self {
            display_name,
            binary_name,
            destroy_fn,
            vtable,
            _lib: lib,
        }
    }

    /// Return the plugin's binary name (used as registry key).
    pub fn plugin_name(&self) -> String {
        self.binary_name.clone()
    }

    /// Call a plugin function that takes a JSON C string and returns a heap JSON C string.
    ///
    /// # Safety
    ///
    /// `f` must be a valid function pointer from the vtable.
    unsafe fn call_json_fn(
        &self,
        f: extern "C" fn(*const c_char) -> *mut c_char,
        input: &str,
    ) -> Option<String> {
        let input_c = CString::new(input).ok()?;
        let out_ptr = f(input_c.as_ptr());
        if out_ptr.is_null() {
            return None;
        }
        // SAFETY: out_ptr is a valid null-terminated C string allocated by the plugin.
        let result = unsafe { CStr::from_ptr(out_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: free_str must be called on heap strings returned by plugin functions.
        unsafe { ((*self.vtable).free_str)(out_ptr) };
        Some(result)
    }

    fn command_spec_from_json(
        plugin_name: &str,
        json: &str,
        fallback_working_dir: &Path,
    ) -> Option<CommandSpec> {
        let value = serde_json::from_str::<serde_json::Value>(json).ok()?;
        let program = value["program"]
            .as_str()
            .map(String::from)
            .unwrap_or_else(|| plugin_name.to_string());
        let args = value["args"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let env = value["env"]
            .as_object()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|(key, value)| value.as_str().map(|v| (key.clone(), v.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        let working_dir = value["working_dir"]
            .as_str()
            .map(PathBuf::from)
            .unwrap_or_else(|| fallback_working_dir.to_path_buf());

        Some(CommandSpec {
            program,
            args,
            env,
            working_dir,
        })
    }

    /// Serialize a `ScreenSnapshot` to JSON for the plugin boundary.
    fn snapshot_to_json(screen: &ScreenSnapshot) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct SerializableScreenSnapshot {
            rows: Vec<Vec<SerializableCell>>,
            cols: u16,
            num_rows: u16,
            cursor: SerializableCursorState,
        }

        #[derive(Serialize)]
        struct SerializableCell {
            character: char,
            fg: SerializableColor,
            bg: SerializableColor,
            attrs: SerializableCellAttributes,
        }

        #[derive(Serialize)]
        struct SerializableCursorState {
            row: u16,
            col: u16,
            visible: bool,
        }

        #[derive(Serialize)]
        struct SerializableCellAttributes {
            bold: bool,
            italic: bool,
            underline: bool,
            dim: bool,
            strikethrough: bool,
            inverse: bool,
            blink: bool,
        }

        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum SerializableColor {
            Default,
            Named { name: &'static str },
            Indexed { value: u8 },
            Rgb { r: u8, g: u8, b: u8 },
        }

        fn color_to_json(color: &TermColor) -> SerializableColor {
            match color {
                TermColor::Default => SerializableColor::Default,
                TermColor::Black => SerializableColor::Named { name: "black" },
                TermColor::Red => SerializableColor::Named { name: "red" },
                TermColor::Green => SerializableColor::Named { name: "green" },
                TermColor::Yellow => SerializableColor::Named { name: "yellow" },
                TermColor::Blue => SerializableColor::Named { name: "blue" },
                TermColor::Magenta => SerializableColor::Named { name: "magenta" },
                TermColor::Cyan => SerializableColor::Named { name: "cyan" },
                TermColor::White => SerializableColor::Named { name: "white" },
                TermColor::BrightBlack => SerializableColor::Named {
                    name: "bright_black",
                },
                TermColor::BrightRed => SerializableColor::Named { name: "bright_red" },
                TermColor::BrightGreen => SerializableColor::Named {
                    name: "bright_green",
                },
                TermColor::BrightYellow => SerializableColor::Named {
                    name: "bright_yellow",
                },
                TermColor::BrightBlue => SerializableColor::Named {
                    name: "bright_blue",
                },
                TermColor::BrightMagenta => SerializableColor::Named {
                    name: "bright_magenta",
                },
                TermColor::BrightCyan => SerializableColor::Named {
                    name: "bright_cyan",
                },
                TermColor::BrightWhite => SerializableColor::Named {
                    name: "bright_white",
                },
                TermColor::Indexed(value) => SerializableColor::Indexed { value: *value },
                TermColor::Rgb(r, g, b) => SerializableColor::Rgb {
                    r: *r,
                    g: *g,
                    b: *b,
                },
            }
        }

        fn attrs_to_json(attrs: &CellAttributes) -> SerializableCellAttributes {
            SerializableCellAttributes {
                bold: attrs.bold,
                italic: attrs.italic,
                underline: attrs.underline,
                dim: attrs.dim,
                strikethrough: attrs.strikethrough,
                inverse: attrs.inverse,
                blink: attrs.blink,
            }
        }

        fn cell_to_json(cell: &Cell) -> SerializableCell {
            SerializableCell {
                character: cell.character,
                fg: color_to_json(&cell.fg),
                bg: color_to_json(&cell.bg),
                attrs: attrs_to_json(&cell.attrs),
            }
        }

        fn cursor_to_json(cursor: &CursorState) -> SerializableCursorState {
            SerializableCursorState {
                row: cursor.row,
                col: cursor.col,
                visible: cursor.visible,
            }
        }

        serde_json::to_string(&SerializableScreenSnapshot {
            rows: screen
                .rows
                .iter()
                .map(|row| row.iter().map(cell_to_json).collect())
                .collect(),
            cols: screen.cols,
            num_rows: screen.num_rows,
            cursor: cursor_to_json(&screen.cursor),
        })
    }
}

/// Decode a hex string into bytes (e.g., `"68656c6c6f0a"` → `b"hello\n"`).
///
/// Returns empty Vec on any invalid input (odd length, non-hex characters).
pub fn decode_hex_bytes(s: &str) -> Vec<u8> {
    if !s.len().is_multiple_of(2) {
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

#[async_trait::async_trait]
impl hom_core::HarnessAdapter for PluginAdapter {
    fn harness_type(&self) -> HarnessType {
        // Plugins don't map to a built-in HarnessType.
        // They are keyed by plugin_name() in AdapterRegistry::plugins.
        // Callers that need the real identity use plugin_name() or the pane title.
        // This value is intentionally unused for plugin dispatch — plugins are keyed
        // by name in AdapterRegistry::plugins, never by HarnessType.
        HarnessType::ClaudeCode
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn build_command(&self, config: &HarnessConfig) -> CommandSpec {
        let config_json = serde_json::json!({
            "working_dir": config.working_dir,
            "model": config.model,
            "extra_args": config.extra_args,
        })
        .to_string();

        // SAFETY: build_command vtable fn is valid.
        let result = unsafe { self.call_json_fn((*self.vtable).build_command, &config_json) };

        let Some(json) = result else {
            warn!(plugin = %self.plugin_name(), "build_command returned null");
            return CommandSpec {
                program: self.plugin_name().to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: config.working_dir.clone(),
            };
        };

        Self::command_spec_from_json(&self.plugin_name(), &json, &config.working_dir)
            .unwrap_or_else(|| {
                warn!(plugin = %self.plugin_name(), json, "build_command returned invalid JSON");
                CommandSpec {
                    program: self.plugin_name().to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: config.working_dir.clone(),
                }
            })
    }

    fn translate_input(&self, command: &OrchestratorCommand) -> Vec<u8> {
        // `owned_hex` is declared here so it lives long enough for `owned_hex.as_str()`
        // inside the match arm. It is only initialized in the `Raw` branch.
        let owned_hex;
        let (kind, text) = match command {
            OrchestratorCommand::Prompt(s) => (HomInputKind::Prompt, s.as_str()),
            OrchestratorCommand::Cancel => (HomInputKind::Cancel, ""),
            OrchestratorCommand::Accept => (HomInputKind::Accept, ""),
            OrchestratorCommand::Reject => (HomInputKind::Reject, ""),
            OrchestratorCommand::Raw(bytes) => {
                // Hex-encode raw bytes so they survive the null-terminated C string boundary.
                owned_hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
                (HomInputKind::Raw, owned_hex.as_str())
            }
        };

        let text_c = match CString::new(text) {
            Ok(s) => s,
            Err(_) => {
                warn!(plugin = %self.plugin_name(), "translate_input: text contains null byte");
                return Vec::new();
            }
        };

        // SAFETY: translate_input is a valid function pointer from the vtable.
        // text_c is a valid null-terminated C string. out_ptr is heap-allocated by the plugin
        // and must be freed with free_str.
        let out_ptr = unsafe { ((*self.vtable).translate_input)(kind, text_c.as_ptr()) };
        if out_ptr.is_null() {
            return Vec::new();
        }

        // SAFETY: out_ptr is a valid null-terminated C string returned by the plugin.
        let hex = unsafe { CStr::from_ptr(out_ptr) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: free_str must be called on heap strings returned by plugin functions.
        unsafe { ((*self.vtable).free_str)(out_ptr) };

        decode_hex_bytes(&hex)
    }

    fn parse_screen(&self, screen: &ScreenSnapshot) -> Vec<HarnessEvent> {
        let screen_json = match Self::snapshot_to_json(screen) {
            Ok(json) => json,
            Err(e) => {
                warn!(plugin = %self.plugin_name(), error = %e, "failed to serialize screen snapshot for parse_screen");
                return Vec::new();
            }
        };

        // SAFETY: parse_screen vtable fn is valid.
        let result = unsafe { self.call_json_fn((*self.vtable).parse_screen, &screen_json) };

        result
            .and_then(|json| serde_json::from_str::<Vec<HarnessEvent>>(&json).ok())
            .unwrap_or_default()
    }

    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let screen_json = match Self::snapshot_to_json(screen) {
            Ok(json) => json,
            Err(e) => {
                warn!(plugin = %self.plugin_name(), error = %e, "failed to serialize screen snapshot for detect_completion");
                return CompletionStatus::Running;
            }
        };

        // SAFETY: detect_completion vtable fn is valid.
        let result = unsafe { self.call_json_fn((*self.vtable).detect_completion, &screen_json) };

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
        // SAFETY: capabilities vtable fn is valid; the return value is a heap-allocated
        // null-terminated C string that must be freed with free_str.
        let maybe_json: Option<String> = unsafe {
            let out_ptr = ((*self.vtable).capabilities)();
            if out_ptr.is_null() {
                None
            } else {
                let s = CStr::from_ptr(out_ptr).to_string_lossy().into_owned();
                ((*self.vtable).free_str)(out_ptr);
                Some(s)
            }
        };

        maybe_json
            .and_then(|json| serde_json::from_str::<HarnessCapabilities>(&json).ok())
            .unwrap_or(HarnessCapabilities {
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
    use hom_core::{Cell, CellAttributes, CursorState, TermColor};

    #[test]
    fn decode_hex_empty_string_gives_empty_bytes() {
        assert_eq!(decode_hex_bytes(""), Vec::<u8>::new());
    }

    #[test]
    fn decode_hex_hello_newline() {
        assert_eq!(decode_hex_bytes("68656c6c6f0a"), b"hello\n");
    }

    #[test]
    fn decode_hex_invalid_is_empty() {
        assert_eq!(decode_hex_bytes("zz"), Vec::<u8>::new());
    }

    #[test]
    fn decode_hex_odd_length_is_empty() {
        assert_eq!(decode_hex_bytes("abc"), Vec::<u8>::new());
    }

    #[test]
    fn command_spec_from_json_preserves_env_and_working_dir() {
        let json = r#"{
            "program": "demo",
            "args": ["--flag", "value"],
            "env": {"FOO": "bar", "BAZ": "qux"},
            "working_dir": "/tmp/plugin-cwd"
        }"#;

        let spec = PluginAdapter::command_spec_from_json("fallback", json, Path::new(".")).unwrap();
        assert_eq!(spec.program, "demo");
        assert_eq!(spec.args, vec!["--flag", "value"]);
        assert_eq!(spec.env.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(spec.working_dir, PathBuf::from("/tmp/plugin-cwd"));
    }

    #[test]
    fn snapshot_to_json_contains_full_screen_shape() {
        let snapshot = ScreenSnapshot {
            rows: vec![vec![Cell {
                character: 'A',
                fg: TermColor::BrightGreen,
                bg: TermColor::Rgb(1, 2, 3),
                attrs: CellAttributes {
                    bold: true,
                    italic: false,
                    underline: true,
                    dim: false,
                    strikethrough: false,
                    inverse: false,
                    blink: false,
                },
            }]],
            cols: 1,
            num_rows: 1,
            cursor: CursorState {
                row: 0,
                col: 0,
                visible: true,
            },
        };

        let json = PluginAdapter::snapshot_to_json(&snapshot).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["cols"], 1);
        assert_eq!(value["num_rows"], 1);
        assert_eq!(value["rows"][0][0]["character"], "A");
        assert_eq!(value["rows"][0][0]["fg"]["kind"], "named");
        assert_eq!(value["rows"][0][0]["fg"]["name"], "bright_green");
        assert_eq!(value["rows"][0][0]["bg"]["kind"], "rgb");
        assert_eq!(value["cursor"]["visible"], true);
    }
}
