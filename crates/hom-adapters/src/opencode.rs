//! Adapter for OpenCode.
//!
//! **Tier 1** — Full orchestration via HTTP REST API sideband.
//!
//! Binary: `opencode`
//! Models: anthropic/claude-sonnet-4-5, Minimax 2.7, GLM 5.1, Kimi 2.5, DeepSeek, Nvidia NIM
//! Sideband: HTTP REST API on localhost:4096

use hom_core::*;

pub struct OpenCodeAdapter {
    pub sideband_url: String,
}

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self {
            sideband_url: "http://localhost:4096".to_string(),
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.sideband_url = url.into();
        self
    }
}

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for OpenCodeAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::OpenCode
    }

    fn display_name(&self) -> &str {
        "OpenCode"
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
        // Primary data comes via HTTP sideband; screen parsing is a fallback
        let mut events = Vec::new();
        let text = screen.text();

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
            supports_steering: true,
            supports_json_output: true,
            supports_session_resume: true,
            supports_mcp: false,
            headless_command: None,
            sideband_type: Some(SidebandType::Http),
        }
    }

    fn sideband(&self) -> Option<Box<dyn SidebandChannel>> {
        Some(Box::new(super::sideband::http::HttpSideband::new(
            self.sideband_url.clone(),
        )))
    }
}
