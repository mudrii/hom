//! Adapter for GitHub Copilot CLI.
//!
//! **Tier 1** — Full orchestration via JSON-RPC 2.0 / ACP server.
//!
//! Binary: `copilot`
//! Agentic coding tool (GA Feb 2026)
//! Sideband: JSON-RPC 2.0

use hom_core::*;

pub struct CopilotAdapter {
    acp_mode: bool,
}

impl CopilotAdapter {
    pub fn new() -> Self {
        Self { acp_mode: false }
    }

    pub fn with_acp(mut self) -> Self {
        self.acp_mode = true;
        self
    }
}

impl Default for CopilotAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for CopilotAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::CopilotCli
    }

    fn display_name(&self) -> &str {
        "GitHub Copilot CLI"
    }

    fn build_command(&self, config: &HarnessConfig) -> CommandSpec {
        let mut args = Vec::new();

        // ACP mode: --acp --stdio for JSON-RPC communication
        if self.acp_mode {
            args.extend(["--acp".to_string(), "--stdio".to_string()]);
        }

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
            } else if let Some(cmd) = line.strip_prefix("Running: ") {
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
        if last_lines.contains('>') || last_lines.contains("$") {
            CompletionStatus::WaitingForInput
        } else {
            CompletionStatus::Running
        }
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            supports_steering: true,
            supports_json_output: true,
            supports_session_resume: false,
            supports_mcp: true,
            headless_command: None,
            sideband_type: if self.acp_mode {
                Some(SidebandType::JsonRpc)
            } else {
                None
            },
        }
    }

    // Sideband is constructed from config at spawn time in App::spawn_pane_inner(),
    // not from this trait method. The config entry specifies the correct program
    // and sideband_url for the ACP subprocess.
}
