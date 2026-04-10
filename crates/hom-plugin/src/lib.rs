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

pub mod ffi;

// loader and adapter are created in Task 2
pub use ffi::{HOM_PLUGIN_ABI_VERSION, HomInputKind, HomPluginVtable};
