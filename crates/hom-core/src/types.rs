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
