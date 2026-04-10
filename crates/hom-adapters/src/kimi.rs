//! Adapter for kimi-cli.
//!
//! **Tier 2** — Headless with stream-json output, ACP server support.
//!
//! Binary: `kimi`
//! Structured output: stream-json

use hom_core::*;

pub struct KimiAdapter;

impl KimiAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for KimiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for KimiAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::KimiCli
    }

    fn display_name(&self) -> &str {
        "kimi-cli"
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
            // kimi-cli uses stream-json format
            if line.starts_with('{')
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(event_type) = val.get("type").and_then(|t| t.as_str())
            {
                match event_type {
                    "file_change" => {
                        if let Some(path) = val.get("path").and_then(|p| p.as_str()) {
                            events.push(HarnessEvent::FileChanged {
                                path: path.into(),
                                change_type: ChangeType::Modified,
                            });
                        }
                    }
                    "output" => {
                        if let Some(content) = val.get("content").and_then(|c| c.as_str()) {
                            events.push(HarnessEvent::OutputChunk {
                                content: content.to_string(),
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
        if last_lines.contains('>') || last_lines.contains("❯") {
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
