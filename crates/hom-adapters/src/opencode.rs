//! Adapter for OpenCode.
//!
//! **Tier 1** — Full orchestration via HTTP REST API sideband.
//!
//! Binary: `opencode`
//! Models: anthropic/claude-sonnet-4-5, Minimax 2.7, GLM 5.1, Kimi 2.5, DeepSeek, Nvidia NIM
//! Sideband: HTTP REST API on localhost:4096

use hom_core::*;

use crate::screen_has_error_line;

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
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        if last_line.starts_with('❯') || last_line.starts_with("> ") || last_line == ">" {
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
        let adapter = OpenCodeAdapter::new();
        let screen = make_screen(&["some output", "❯ "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = OpenCodeAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = OpenCodeAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_parse_screen_extracts_created_and_updated_files() {
        let adapter = OpenCodeAdapter::new();
        let screen = make_screen(&["Created src/main.rs", "Updated src/lib.rs"]);
        let events = adapter.parse_screen(&screen);
        assert_eq!(events.len(), 2);
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
    }

    #[test]
    fn test_no_false_positive_on_code_with_angle_bracket() {
        let adapter = OpenCodeAdapter::new();
        let screen = make_screen(&["if x > 0 {", "    println!(\"hi\");", "}", "compiling..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    // ── build_command ─────────────────────────────────────

    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::OpenCode, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = OpenCodeAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "opencode");
        assert!(spec.args.is_empty(), "no args when no model or extra_args");
    }

    #[test]
    fn test_build_command_with_model() {
        let adapter = OpenCodeAdapter::new();
        let config = default_config().with_model("claude-sonnet-4-5");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "claude-sonnet-4-5"]);
    }

    #[test]
    fn test_build_command_extra_args() {
        let adapter = OpenCodeAdapter::new();
        let mut config = default_config();
        config.extra_args = vec!["--port".to_string(), "4096".to_string()];
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--port", "4096"]);
    }

    #[test]
    fn test_build_command_unaffected_by_sideband_url() {
        // sideband_url configures the HTTP client, not the spawned process
        let adapter = OpenCodeAdapter::new().with_url("http://localhost:9999");
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "opencode");
        assert!(spec.args.is_empty());
    }

    // ── translate_input ───────────────────────────────────

    #[test]
    fn test_translate_prompt() {
        let adapter = OpenCodeAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("fix it".to_string()));
        assert_eq!(bytes, b"fix it\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = OpenCodeAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Cancel),
            vec![0x03]
        );
    }

    #[test]
    fn test_translate_accept() {
        let adapter = OpenCodeAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Accept),
            b"y\n"
        );
    }

    #[test]
    fn test_translate_reject() {
        let adapter = OpenCodeAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Reject),
            b"n\n"
        );
    }
}
