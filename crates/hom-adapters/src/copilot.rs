//! Adapter for GitHub Copilot CLI.
//!
//! **Tier 1** — Full orchestration via JSON-RPC 2.0 / ACP server.
//!
//! Binary: `copilot`
//! Agentic coding tool (GA Feb 2026)
//! Sideband: JSON-RPC 2.0

use hom_core::*;

use crate::screen_has_error_line;

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
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        if last_line.starts_with("$ ") || last_line == "$" || last_line.starts_with("copilot>") {
            CompletionStatus::WaitingForInput
        } else if screen_has_error_line(&last_lines) {
            CompletionStatus::Failed { error: last_lines }
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

#[cfg(test)]
mod tests {
    use super::*;
    use hom_core::traits::{Cell, CursorState, ScreenSnapshot};

    fn make_screen(lines: &[&str]) -> ScreenSnapshot {
        let rows: Vec<Vec<Cell>> = lines
            .iter()
            .map(|line| {
                let mut row: Vec<Cell> = line
                    .chars()
                    .map(|c| Cell {
                        character: c,
                        ..Cell::default()
                    })
                    .collect();
                while row.len() < 80 {
                    row.push(Cell::default());
                }
                row
            })
            .collect();
        let mut all_rows = rows;
        while all_rows.len() < 5 {
            all_rows.push(vec![Cell::default(); 80]);
        }
        ScreenSnapshot {
            cols: 80,
            num_rows: all_rows.len() as u16,
            rows: all_rows,
            cursor: CursorState::default(),
        }
    }

    #[test]
    fn test_detect_waiting_for_input() {
        let adapter = CopilotAdapter::new();
        let screen = make_screen(&["some output", "$ "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = CopilotAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = CopilotAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_parse_screen_extracts_file_and_command_events() {
        let adapter = CopilotAdapter::new();
        let screen = make_screen(&[
            "Created src/main.rs",
            "Updated src/lib.rs",
            "Running: cargo test",
        ]);
        let events = adapter.parse_screen(&screen);
        assert_eq!(events.len(), 3);
        assert!(matches!(
            &events[0],
            HarnessEvent::FileChanged { path, change_type }
                if path == &std::path::PathBuf::from("src/main.rs")
                    && *change_type == ChangeType::Created
        ));
        assert!(matches!(
            &events[1],
            HarnessEvent::FileChanged { path, change_type }
                if path == &std::path::PathBuf::from("src/lib.rs")
                    && *change_type == ChangeType::Modified
        ));
        assert!(matches!(
            &events[2],
            HarnessEvent::CommandExecuted { command, exit_code }
                if command == "cargo test" && exit_code.is_none()
        ));
    }

    #[test]
    fn test_no_false_positive_on_code_with_angle_bracket() {
        let adapter = CopilotAdapter::new();
        let screen = make_screen(&["if x > 0 {", "    println!(\"hi\");", "}", "compiling..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    // ── build_command ─────────────────────────────────────

    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::CopilotCli, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = CopilotAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "copilot");
        assert!(
            spec.args.is_empty(),
            "no args when no acp_mode, model, or extra_args"
        );
    }

    #[test]
    fn test_build_command_acp_mode_prepends_flags() {
        let adapter = CopilotAdapter::new().with_acp();
        let spec = adapter.build_command(&default_config());
        assert_eq!(&spec.args[..2], &["--acp", "--stdio"]);
    }

    #[test]
    fn test_build_command_acp_with_model() {
        let adapter = CopilotAdapter::new().with_acp();
        let config = default_config().with_model("gpt-4o");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--acp", "--stdio", "--model", "gpt-4o"]);
    }

    #[test]
    fn test_build_command_with_model_no_acp() {
        let adapter = CopilotAdapter::new();
        let config = default_config().with_model("gpt-4o");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "gpt-4o"]);
    }

    // ── translate_input ───────────────────────────────────

    #[test]
    fn test_translate_prompt() {
        let adapter = CopilotAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("fix it".to_string()));
        assert_eq!(bytes, b"fix it\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = CopilotAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Cancel),
            vec![0x03]
        );
    }

    #[test]
    fn test_translate_accept() {
        let adapter = CopilotAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Accept),
            b"y\n"
        );
    }

    #[test]
    fn test_translate_reject() {
        let adapter = CopilotAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Reject),
            b"n\n"
        );
    }
}
