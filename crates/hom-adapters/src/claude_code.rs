//! Adapter for Claude Code CLI.
//!
//! **Tier 1** — Full orchestration/steering via client mode (stdin/stdout).
//!
//! Binary: `claude`
//! Structured output: `--output-format stream-json`
//! Client mode: `--client-mode` for stdin/stdout IPC
//! Known issue: Ink/React renderer causes flickering in multiplexers.
//! Mitigation: Use headless mode for automated workflow steps.

use hom_core::*;

pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for ClaudeCodeAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::ClaudeCode
    }

    fn display_name(&self) -> &str {
        "Claude Code"
    }

    fn build_command(&self, config: &HarnessConfig) -> CommandSpec {
        let mut args = Vec::new();

        if let Some(model) = &config.model {
            args.extend(["--model".to_string(), model.clone()]);
        }

        args.extend(config.extra_args.clone());

        CommandSpec {
            program: config.binary().to_string(),
            args,
            env: config.env_vars.clone(),
            working_dir: config.working_dir.clone(),
        }
    }

    fn translate_input(&self, command: &OrchestratorCommand) -> Vec<u8> {
        match command {
            OrchestratorCommand::Prompt(text) => format!("{text}\n").into_bytes(),
            OrchestratorCommand::Cancel => vec![0x03], // Ctrl-C
            OrchestratorCommand::Accept => b"y\n".to_vec(),
            OrchestratorCommand::Reject => b"n\n".to_vec(),
            OrchestratorCommand::Raw(bytes) => bytes.clone(),
        }
    }

    fn parse_screen(&self, screen: &ScreenSnapshot) -> Vec<HarnessEvent> {
        let mut events = Vec::new();
        let text = screen.text();

        // Detect file changes from Claude Code's output
        if text.contains("Created ") || text.contains("Updated ") {
            for line in text.lines() {
                let line = line.trim();
                if let Some(path) = line.strip_prefix("Created ") {
                    events.push(HarnessEvent::FileChanged {
                        path: path.trim().into(),
                        change_type: ChangeType::Created,
                    });
                } else if let Some(path) = line.strip_prefix("Updated ") {
                    events.push(HarnessEvent::FileChanged {
                        path: path.trim().into(),
                        change_type: ChangeType::Modified,
                    });
                }
            }
        }

        events
    }

    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_lines = screen.last_n_lines(5);

        // Claude Code shows a prompt marker when waiting for input
        if last_lines.contains('❯') || last_lines.contains("> ") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error:") || last_lines.contains("error:") {
            CompletionStatus::Failed {
                error: last_lines.to_string(),
            }
        } else {
            CompletionStatus::Running
        }
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            supports_steering: true,
            supports_json_output: true,
            supports_session_resume: true,
            supports_mcp: true,
            headless_command: Some("claude --output-format stream-json -p".to_string()),
            sideband_type: None,
        }
    }
}
