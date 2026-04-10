//! Adapter for OpenAI Codex CLI.
//!
//! **Tier 2** — Headless with limited steering via JSONL events.
//!
//! Binary: `codex`
//! Structured output: JSONL events
//! Headless: `codex --quiet`

use hom_core::*;

pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for CodexAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::CodexCli
    }

    fn display_name(&self) -> &str {
        "Codex CLI"
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
            // Codex emits JSONL with event types
            if line.starts_with('{')
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(event_type) = val.get("type").and_then(|t| t.as_str())
            {
                match event_type {
                    "file_change" | "file_edit" => {
                        if let Some(path) = val.get("path").and_then(|p| p.as_str()) {
                            events.push(HarnessEvent::FileChanged {
                                path: path.into(),
                                change_type: ChangeType::Modified,
                            });
                        }
                    }
                    "command" => {
                        if let Some(cmd) = val.get("command").and_then(|c| c.as_str()) {
                            events.push(HarnessEvent::CommandExecuted {
                                command: cmd.to_string(),
                                exit_code: val
                                    .get("exit_code")
                                    .and_then(|e| e.as_i64())
                                    .map(|e| e as i32),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        events
    }

    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_lines = screen.last_n_lines(3);

        if last_lines.contains("$") || last_lines.contains(">") {
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
            supports_mcp: false,
            headless_command: Some("codex --quiet".to_string()),
            sideband_type: None,
        }
    }
}
