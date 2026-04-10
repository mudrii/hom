//! # hom-adapters
//!
//! Harness adapter implementations for all 7 supported AI coding agents.
//!
//! Each adapter implements `HarnessAdapter` from `hom-core` and knows how
//! to spawn, drive, and interpret its corresponding harness CLI.
//!
//! ## Tier 1 (full orchestration/steering)
//! - **Claude Code** — client mode stdin/stdout
//! - **pi-mono** — RPC stdin/stdout, steering queue
//! - **OpenCode** — HTTP REST API sideband
//! - **GitHub Copilot CLI** — JSON-RPC 2.0
//!
//! ## Tier 2 (headless, limited steering)
//! - **Codex CLI** — JSONL events
//! - **Gemini CLI** — JSON output
//! - **kimi-cli** — stream-json, ACP server

pub mod claude_code;
pub mod codex;
pub mod copilot;
pub mod gemini;
pub mod kimi;
pub mod opencode;
pub mod pi_mono;
pub mod sideband;

use std::collections::HashMap;

use hom_core::{HarnessAdapter, HarnessType};

/// Registry of all available harness adapters.
pub struct AdapterRegistry {
    adapters: HashMap<HarnessType, Box<dyn HarnessAdapter>>,
}

impl AdapterRegistry {
    /// Create a registry with all built-in adapters.
    pub fn new() -> Self {
        let mut adapters: HashMap<HarnessType, Box<dyn HarnessAdapter>> = HashMap::new();

        adapters.insert(
            HarnessType::ClaudeCode,
            Box::new(claude_code::ClaudeCodeAdapter::new()),
        );
        adapters.insert(HarnessType::CodexCli, Box::new(codex::CodexAdapter::new()));
        adapters.insert(
            HarnessType::GeminiCli,
            Box::new(gemini::GeminiAdapter::new()),
        );
        adapters.insert(HarnessType::PiMono, Box::new(pi_mono::PiMonoAdapter::new()));
        adapters.insert(HarnessType::KimiCli, Box::new(kimi::KimiAdapter::new()));
        adapters.insert(
            HarnessType::OpenCode,
            Box::new(opencode::OpenCodeAdapter::new()),
        );
        adapters.insert(
            HarnessType::CopilotCli,
            Box::new(copilot::CopilotAdapter::new()),
        );

        Self { adapters }
    }

    /// Get an adapter by harness type.
    pub fn get(&self, harness: &HarnessType) -> Option<&dyn HarnessAdapter> {
        self.adapters.get(harness).map(|a| a.as_ref())
    }

    /// List all registered harness types.
    pub fn available(&self) -> Vec<HarnessType> {
        self.adapters.keys().copied().collect()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_all_seven_harnesses() {
        let registry = AdapterRegistry::new();
        let expected = [
            HarnessType::ClaudeCode,
            HarnessType::CodexCli,
            HarnessType::GeminiCli,
            HarnessType::PiMono,
            HarnessType::KimiCli,
            HarnessType::OpenCode,
            HarnessType::CopilotCli,
        ];
        for harness in &expected {
            assert!(
                registry.get(harness).is_some(),
                "AdapterRegistry missing adapter for {harness:?}"
            );
        }
    }

    #[test]
    fn test_registry_available_returns_seven() {
        let registry = AdapterRegistry::new();
        assert_eq!(registry.available().len(), 7, "expected 7 registered adapters");
    }

    #[test]
    fn test_registry_adapter_display_name_non_empty() {
        let registry = AdapterRegistry::new();
        for harness in registry.available() {
            let adapter = registry.get(&harness).unwrap();
            assert!(
                !adapter.display_name().is_empty(),
                "display_name() is empty for {harness:?}"
            );
        }
    }

    #[test]
    fn test_registry_adapter_harness_type_matches_key() {
        let registry = AdapterRegistry::new();
        for harness in registry.available() {
            let adapter = registry.get(&harness).unwrap();
            assert_eq!(
                adapter.harness_type(),
                harness,
                "adapter registered under {harness:?} returns wrong harness_type()"
            );
        }
    }
}
