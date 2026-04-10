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
                HomError::PluginError(format!("failed to load plugin {}: {e}", path.display()))
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

        // SAFETY: hom_plugin_destroy is an optional C function with signature
        // `fn(*mut HomPluginVtable)`. If absent, the plugin has no cleanup to do.
        let destroy_fn: Option<extern "C" fn(*mut HomPluginVtable)> = unsafe {
            lib.get(b"hom_plugin_destroy\0")
                .ok()
                .map(|s: libloading::Symbol<extern "C" fn(*mut HomPluginVtable)>| *s)
        };

        info!(path = %path.display(), "loaded plugin");
        // SAFETY: vtable_ptr is non-null and points to a valid HomPluginVtable (checked above).
        // lib keeps the library loaded for the duration of PluginAdapter's lifetime.
        Ok(unsafe { PluginAdapter::new(lib, vtable_ptr, destroy_fn) })
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
