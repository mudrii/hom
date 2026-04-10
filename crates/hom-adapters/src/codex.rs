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
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        if last_line.starts_with("$ ") || last_line == "$" || last_line.starts_with("codex>") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error") || last_lines.contains("error:") {
            CompletionStatus::Failed { error: last_lines }
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
        let adapter = CodexAdapter::new();
        let screen = make_screen(&["some output", "$ "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = CodexAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = CodexAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_no_false_positive_on_code_with_angle_bracket() {
        let adapter = CodexAdapter::new();
        let screen = make_screen(&["if x > 0 {", "    println!(\"hi\");", "}", "compiling..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    // ── build_command ─────────────────────────────────────────────────

    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::CodexCli, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = CodexAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "codex");
        assert!(spec.args.is_empty(), "no args when no model or extra_args");
    }

    #[test]
    fn test_build_command_with_model() {
        let adapter = CodexAdapter::new();
        let config = default_config().with_model("o3");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "o3"]);
    }

    #[test]
    fn test_build_command_extra_args() {
        let adapter = CodexAdapter::new();
        let mut config = default_config();
        config.extra_args = vec!["--quiet".to_string()];
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--quiet"]);
    }

    // ── translate_input ───────────────────────────────────────────────

    #[test]
    fn test_translate_prompt() {
        let adapter = CodexAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("fix it".to_string()));
        assert_eq!(bytes, b"fix it\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = CodexAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Cancel),
            vec![0x03]
        );
    }

    #[test]
    fn test_translate_accept() {
        let adapter = CodexAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Accept),
            b"y\n"
        );
    }

    #[test]
    fn test_translate_reject() {
        let adapter = CodexAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Reject),
            b"n\n"
        );
    }
}
