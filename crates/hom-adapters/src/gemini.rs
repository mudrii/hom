//! Adapter for Google Gemini CLI.
//!
//! **Tier 2** — Headless with JSON output.
//!
//! Binary: `gemini`
//! Structured output: JSON mode

use hom_core::*;

pub struct GeminiAdapter;

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for GeminiAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::GeminiCli
    }

    fn display_name(&self) -> &str {
        "Gemini CLI"
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
            OrchestratorCommand::Cancel => vec![0x03],
            OrchestratorCommand::Accept => b"y\n".to_vec(),
            OrchestratorCommand::Reject => b"n\n".to_vec(),
            OrchestratorCommand::Raw(bytes) => bytes.clone(),
        }
    }

    fn parse_screen(&self, screen: &ScreenSnapshot) -> Vec<HarnessEvent> {
        let mut events = Vec::new();
        let text = screen.text();

        for line in text.lines() {
            let line = line.trim();
            if let Some(path) = line.strip_prefix("Created file: ") {
                events.push(HarnessEvent::FileChanged {
                    path: path.trim().into(),
                    change_type: ChangeType::Created,
                });
            } else if let Some(path) = line.strip_prefix("Updated file: ") {
                events.push(HarnessEvent::FileChanged {
                    path: path.trim().into(),
                    change_type: ChangeType::Modified,
                });
            } else if let Some(cmd) = line.strip_prefix("$ ") {
                events.push(HarnessEvent::CommandExecuted {
                    command: cmd.to_string(),
                    exit_code: None,
                });
            }
        }

        events
    }

    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_lines = screen.last_n_lines(3);
        if last_lines.contains("❯") || last_lines.contains(">") {
            CompletionStatus::WaitingForInput
        } else {
            CompletionStatus::Running
        }
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            supports_steering: false,
            supports_json_output: true,
            supports_session_resume: false,
            supports_mcp: true,
            headless_command: None,
            sideband_type: None,
        }
    }
}
