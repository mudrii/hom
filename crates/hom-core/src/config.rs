use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{HomError, HomResult};
use crate::types::LayoutKind;

/// Top-level HOM configuration, loaded from `~/.config/hom/config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HomConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub harnesses: HashMap<String, HarnessEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_layout")]
    pub default_layout: LayoutKind,
    #[serde(default = "default_scrollback")]
    pub max_scrollback: usize,
    #[serde(default = "default_fps")]
    pub render_fps: u32,
    #[serde(default = "default_max_panes")]
    pub max_panes: usize,
    #[serde(default)]
    pub workflow_dir: Option<PathBuf>,
    #[serde(default)]
    pub db_path: Option<PathBuf>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_layout: default_layout(),
            max_scrollback: default_scrollback(),
            render_fps: default_fps(),
            max_panes: default_max_panes(),
            workflow_dir: None,
            db_path: None,
        }
    }
}

fn default_layout() -> LayoutKind {
    LayoutKind::HSplit
}
fn default_scrollback() -> usize {
    10_000
}
fn default_fps() -> u32 {
    30
}
fn default_max_panes() -> usize {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingsConfig {
    #[serde(default = "default_toggle_cmd")]
    pub toggle_command_bar: String,
    #[serde(default = "default_next_pane")]
    pub next_pane: String,
    #[serde(default = "default_prev_pane")]
    pub prev_pane: String,
    #[serde(default = "default_kill_pane")]
    pub kill_pane: String,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            toggle_command_bar: default_toggle_cmd(),
            next_pane: default_next_pane(),
            prev_pane: default_prev_pane(),
            kill_pane: default_kill_pane(),
        }
    }
}

fn default_toggle_cmd() -> String {
    "ctrl-`".into()
}
fn default_next_pane() -> String {
    "ctrl-tab".into()
}
fn default_prev_pane() -> String {
    "ctrl-shift-tab".into()
}
fn default_kill_pane() -> String {
    "ctrl-w".into()
}

/// Per-harness configuration entry in `[harnesses.<name>]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessEntry {
    pub command: String,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub sideband: Option<String>,
    #[serde(default)]
    pub sideband_url: Option<String>,
}

/// Bundled default configuration (compiled into the binary).
const BUNDLED_DEFAULT_TOML: &str = include_str!("../../../config/default.toml");

impl HomConfig {
    /// Load configuration from the default path (`~/.config/hom/config.toml`),
    /// falling back to the bundled `config/default.toml` if the user file doesn't exist.
    pub fn load() -> HomResult<Self> {
        let config_path = Self::default_path();
        if config_path.exists() {
            Self::load_from(&config_path)
        } else {
            Self::load_bundled_default()
        }
    }

    /// Load the bundled default configuration (compiled into the binary from config/default.toml).
    fn load_bundled_default() -> HomResult<Self> {
        toml::from_str(BUNDLED_DEFAULT_TOML)
            .map_err(|e| HomError::ConfigError(format!("bundled default.toml parse error: {e}")))
    }

    /// Load from a specific file path. Env vars (`${VAR}`) are expanded after parsing.
    pub fn load_from(path: &Path) -> HomResult<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            HomError::ConfigError(format!("failed to read {}: {}", path.display(), e))
        })?;
        let expanded = expand_env_vars(&content);
        toml::from_str(&expanded).map_err(|e| HomError::ConfigError(format!("invalid TOML: {e}")))
    }

    /// Default config file path.
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hom")
            .join("config.toml")
    }

    /// Workflow directory (user-configured or default).
    pub fn workflow_dir(&self) -> PathBuf {
        self.general.workflow_dir.clone().unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("hom")
                .join("workflows")
        })
    }

    /// Database path (user-configured or default).
    pub fn db_path(&self) -> PathBuf {
        self.general.db_path.clone().unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("hom")
                .join("hom.db")
        })
    }
}

/// Expand `${VAR}` patterns in a string using environment variables.
/// Unknown variables and unterminated `${` are left as-is.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut var_name = String::new();
            let mut found_close = false;
            for ch in chars.by_ref() {
                if ch == '}' {
                    found_close = true;
                    break;
                }
                var_name.push(ch);
            }
            if !found_close {
                // Unterminated ${...  — preserve literal text
                result.push_str("${");
                result.push_str(&var_name);
            } else {
                match std::env::var(&var_name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        result.push_str("${");
                        result.push_str(&var_name);
                        result.push('}');
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_vars_basic() {
        // SAFETY: single-threaded test; no other threads reading this var
        unsafe { std::env::set_var("HOM_TEST_VAR", "hello") };
        let result = expand_env_vars("prefix ${HOM_TEST_VAR} suffix");
        assert_eq!(result, "prefix hello suffix");
        unsafe { std::env::remove_var("HOM_TEST_VAR") };
    }

    #[test]
    fn test_expand_env_vars_unknown() {
        let result = expand_env_vars("${UNLIKELY_VAR_12345}");
        assert_eq!(result, "${UNLIKELY_VAR_12345}");
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        let result = expand_env_vars("plain text");
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_expand_env_vars_dollar_without_brace() {
        let result = expand_env_vars("$HOME is not expanded");
        assert_eq!(result, "$HOME is not expanded");
    }

    #[test]
    fn test_expand_env_vars_unterminated() {
        let result = expand_env_vars("prefix ${UNTERMINATED");
        assert_eq!(result, "prefix ${UNTERMINATED");
    }

    #[test]
    fn test_bundled_default_loads() {
        let config = HomConfig::load_bundled_default().unwrap();
        assert!(config.general.render_fps > 0);
        assert!(config.general.max_panes > 0);
    }
}
