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
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        if last_line.starts_with('❯') || last_line.starts_with("kimi>") {
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
            supports_mcp: true,
            headless_command: None,
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
        let adapter = KimiAdapter::new();
        let screen = make_screen(&["some output", "kimi> "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = KimiAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = KimiAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_no_false_positive_on_code_with_angle_bracket() {
        let adapter = KimiAdapter::new();
        let screen = make_screen(&["if x > 0 {", "    println!(\"hi\");", "}", "compiling..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    // ── build_command ─────────────────────────────────────

    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::KimiCli, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = KimiAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "kimi");
        assert!(spec.args.is_empty(), "no args when no model or extra_args");
    }

    #[test]
    fn test_build_command_with_model() {
        let adapter = KimiAdapter::new();
        let config = default_config().with_model("k2");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "k2"]);
    }

    #[test]
    fn test_build_command_extra_args() {
        let adapter = KimiAdapter::new();
        let mut config = default_config();
        config.extra_args = vec!["--stream".to_string()];
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--stream"]);
    }

    // ── translate_input ───────────────────────────────────

    #[test]
    fn test_translate_prompt() {
        let adapter = KimiAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("fix it".to_string()));
        assert_eq!(bytes, b"fix it\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = KimiAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Cancel),
            vec![0x03]
        );
    }

    #[test]
    fn test_translate_accept() {
        let adapter = KimiAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Accept),
            b"y\n"
        );
    }

    #[test]
    fn test_translate_reject() {
        let adapter = KimiAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Reject),
            b"n\n"
        );
    }
}
