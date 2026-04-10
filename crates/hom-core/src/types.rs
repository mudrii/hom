use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Unique identifier for a pane within a session.
pub type PaneId = u32;

/// Supported AI coding harnesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessType {
    ClaudeCode,
    CodexCli,
    GeminiCli,
    PiMono,
    KimiCli,
    OpenCode,
    CopilotCli,
}

impl HarnessType {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::CodexCli => "Codex CLI",
            Self::GeminiCli => "Gemini CLI",
            Self::PiMono => "pi-mono",
            Self::KimiCli => "kimi-cli",
            Self::OpenCode => "OpenCode",
            Self::CopilotCli => "GitHub Copilot CLI",
        }
    }

    pub fn default_binary(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::CodexCli => "codex",
            Self::GeminiCli => "gemini",
            Self::PiMono => "pi",
            Self::KimiCli => "kimi",
            Self::OpenCode => "opencode",
            Self::CopilotCli => "copilot",
        }
    }

    /// Canonical config key matching `[harnesses.<key>]` in config.toml.
    pub fn config_key(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::CodexCli => "codex",
            Self::GeminiCli => "gemini",
            Self::PiMono => "pi-mono",
            Self::KimiCli => "kimi",
            Self::OpenCode => "opencode",
            Self::CopilotCli => "copilot",
        }
    }

    /// Parse from a user-typed string (command bar input).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" | "claude-code" | "claudecode" => Some(Self::ClaudeCode),
            "codex" | "codex-cli" => Some(Self::CodexCli),
            "gemini" | "gemini-cli" => Some(Self::GeminiCli),
            "pi" | "pi-mono" | "pimono" => Some(Self::PiMono),
            "kimi" | "kimi-cli" | "kimicli" => Some(Self::KimiCli),
            "opencode" | "open-code" => Some(Self::OpenCode),
            "copilot" | "copilot-cli" | "gh-copilot" => Some(Self::CopilotCli),
            _ => None,
        }
    }
}

impl std::fmt::Display for HarnessType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Configuration for spawning a harness instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessConfig {
    pub harness_type: HarnessType,
    pub model: Option<String>,
    pub working_dir: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub extra_args: Vec<String>,
    /// Override the default binary path.
    pub binary_override: Option<String>,
}

impl HarnessConfig {
    pub fn new(harness_type: HarnessType, working_dir: PathBuf) -> Self {
        Self {
            harness_type,
            model: None,
            working_dir,
            env_vars: HashMap::new(),
            extra_args: Vec::new(),
            binary_override: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn binary(&self) -> &str {
        self.binary_override
            .as_deref()
            .unwrap_or_else(|| self.harness_type.default_binary())
    }
}

/// What a harness adapter can do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessCapabilities {
    /// Can accept programmatic steering (prompt injection, mode switching).
    pub supports_steering: bool,
    /// Can produce structured JSON output.
    pub supports_json_output: bool,
    /// Can resume a previous session.
    pub supports_session_resume: bool,
    /// Has MCP server/client support.
    pub supports_mcp: bool,
    /// Command for headless (non-TUI) mode, if available.
    pub headless_command: Option<String>,
    /// Sideband channel type (HTTP, RPC, etc.).
    pub sideband_type: Option<SidebandType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SidebandType {
    Http,
    Rpc,
    JsonRpc,
}

/// Events emitted by a harness, extracted via screen parsing or sideband.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HarnessEvent {
    TaskStarted {
        description: String,
    },
    FileChanged {
        path: PathBuf,
        change_type: ChangeType,
    },
    CommandExecuted {
        command: String,
        exit_code: Option<i32>,
    },
    TokenUsage {
        input: u64,
        output: u64,
    },
    Error {
        message: String,
    },
    TaskCompleted {
        summary: String,
    },
    OutputChunk {
        content: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
}

/// Whether a harness has finished its current task.
#[derive(Debug, Clone)]
pub enum CompletionStatus {
    /// Still working.
    Running,
    /// Waiting for user input (prompt visible).
    WaitingForInput,
    /// Task completed successfully.
    Completed { output: String },
    /// Task failed.
    Failed { error: String },
}

/// High-level commands the orchestrator can send to a harness.
#[derive(Debug, Clone)]
pub enum OrchestratorCommand {
    /// Send a prompt/instruction.
    Prompt(String),
    /// Cancel the current operation (Ctrl-C).
    Cancel,
    /// Accept a confirmation prompt (y/yes).
    Accept,
    /// Reject a confirmation prompt (n/no).
    Reject,
    /// Send raw bytes to the PTY.
    Raw(Vec<u8>),
}

/// Pane layout variants.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutKind {
    Single,
    #[default]
    HSplit,
    VSplit,
    Grid,
    Tabbed,
}

/// SSH connection target for remote panes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTarget {
    pub user: String,
    pub host: String,
    /// SSH port. Defaults to 22.
    pub port: u16,
}

impl RemoteTarget {
    /// Parse `user@host` or `user@host:port`. Returns `None` if:
    /// - The string contains no `@` (not a remote target spec)
    /// - The port string is present but is not a valid u16 (0..=65535)
    ///
    /// Call sites should treat `None` as "invalid remote spec" and show the
    /// input string in the error message so the user knows what was rejected.
    pub fn parse(s: &str) -> Option<Self> {
        let (user, rest) = s.split_once('@')?;
        let (host, port) = if let Some((h, p)) = rest.rsplit_once(':') {
            let port: u16 = p.parse().ok()?;
            (h.to_string(), port)
        } else {
            (rest.to_string(), 22)
        };
        Some(Self {
            user: user.to_string(),
            host,
            port,
        })
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Return the command spec as a Vec of individual argument strings.
    pub fn spec_to_argv(spec: &crate::CommandSpec) -> Vec<String> {
        std::iter::once(spec.program.clone())
            .chain(spec.args.iter().cloned())
            .collect()
    }

    /// Shell-quote a single argument (POSIX single-quote wrapping).
    pub fn shell_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', r"'\''"))
    }

    /// Build a shell-safe command string for `ssh2::Channel::exec()`.
    pub fn build_remote_command(spec: &crate::CommandSpec) -> String {
        Self::spec_to_argv(spec)
            .iter()
            .map(|a| Self::shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl std::fmt::Display for RemoteTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.port == 22 {
            write!(f, "{}@{}", self.user, self.host)
        } else {
            write!(f, "{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// Whether a pane is backed by a local PTY or a remote SSH channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PaneKind {
    Local,
    Remote(RemoteTarget),
}

// ── MCP server ────────────────────────────────────────────────────────

/// A command sent from the MCP server to the TUI app, with a channel to receive
/// the result. The app processes this in its event loop and sends back a McpResponse.
#[derive(Debug)]
pub struct McpRequest {
    pub command: McpCommand,
    pub reply: tokio::sync::oneshot::Sender<McpResponse>,
}

/// The action the MCP server wants the app to perform.
#[derive(Debug)]
pub enum McpCommand {
    SpawnPane { harness: String, model: Option<String> },
    SendToPane { pane_id: String, text: String },
    RunWorkflow { path: String, vars: std::collections::HashMap<String, String> },
    ListPanes,
    GetPaneOutput { pane_id: String, lines: usize },
    KillPane { pane_id: String },
}

/// The result the app sends back to the MCP server.
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
pub enum McpResponse {
    SpawnPane { pane_id: String },
    SendToPane { ok: bool },
    RunWorkflow { workflow_id: String },
    ListPanes { panes: Vec<PaneSummary> },
    GetPaneOutput { lines: Vec<String> },
    KillPane { ok: bool },
    Error { error: String },
}

/// Summary of a single pane returned by list_panes.
#[derive(Debug, serde::Serialize)]
pub struct PaneSummary {
    pub pane_id: String,
    pub harness: String,
    pub status: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandSpec;

    #[test]
    fn remote_target_parse_with_port() {
        let t = RemoteTarget::parse("alice@example.com:2222").unwrap();
        assert_eq!(t.user, "alice");
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 2222);
    }

    #[test]
    fn remote_target_parse_default_port() {
        let t = RemoteTarget::parse("bob@10.0.0.5").unwrap();
        assert_eq!(t.user, "bob");
        assert_eq!(t.host, "10.0.0.5");
        assert_eq!(t.port, 22);
    }

    #[test]
    fn remote_target_parse_missing_at_fails() {
        assert!(RemoteTarget::parse("notaremote").is_none());
    }

    #[test]
    fn pane_kind_is_remote() {
        let kind = PaneKind::Remote(RemoteTarget {
            user: "u".into(),
            host: "h".into(),
            port: 22,
        });
        assert!(matches!(kind, PaneKind::Remote(_)));
    }

    #[test]
    fn remote_target_shell_args_are_individually_quoted() {
        let spec = CommandSpec {
            program: "claude".to_string(),
            args: vec!["--model".to_string(), "claude opus".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: ".".into(),
        };
        let parts = RemoteTarget::spec_to_argv(&spec);
        assert_eq!(parts[0], "claude");
        assert_eq!(parts[2], "claude opus");
    }

    #[test]
    fn remote_target_parse_invalid_port_fails() {
        // Port 99999 exceeds u16::MAX (65535) — parse returns None
        assert!(RemoteTarget::parse("user@host:99999").is_none());
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        // "it's" → 'it'\''s'
        assert_eq!(RemoteTarget::shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn build_remote_command_quotes_all_args() {
        let spec = CommandSpec {
            program: "claude".to_string(),
            args: vec!["--model".to_string(), "claude opus 4".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: ".".into(),
        };
        let cmd = RemoteTarget::build_remote_command(&spec);
        // Each arg individually quoted and joined with spaces
        assert_eq!(cmd, "'claude' '--model' 'claude opus 4'");
    }
}
