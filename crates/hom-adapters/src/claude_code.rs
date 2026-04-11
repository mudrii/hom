//! Adapter for Claude Code CLI.
//!
//! **Tier 1** — Full orchestration via stdin/stdout with stream-json output.
//!
//! Binary: `claude`
//! Sideband: None (uses PTY + stream-json output format)
//!
//! # Known Limitation: Terminal Flickering
//!
//! Claude Code's Ink/React-based TUI generates approximately 4,000–6,700
//! scroll events per second in any terminal multiplexer. This is upstream
//! behavior that cannot be mitigated in HOM.
//!
//! **Workaround for automated workflow steps:** Use headless mode by adding
//! `--output-format stream-json` to `extra_args` in the harness config.
//! Headless mode suppresses the TUI and outputs JSONL events instead, which
//! HOM's `parse_screen()` can parse directly. Example config:
//!
//! ```toml
//! [harnesses.claude-code]
//! command = "claude"
//! extra_args = ["--output-format", "stream-json"]
//! ```
//!
//! In this mode, the pane renders JSON lines rather than a TUI, but
//! completion detection and output parsing work correctly.

use hom_core::*;

use crate::screen_has_error_line;

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
            supports_mcp: true,
            headless_command: Some("claude --output-format stream-json -p".to_string()),
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
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["some output", "❯ "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_parse_screen_extracts_created_and_updated_files() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["Created src/main.rs", "Updated Cargo.toml"]);
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
                if path == &std::path::PathBuf::from("Cargo.toml")
                    && *change_type == ChangeType::Modified
        ));
    }

    #[test]
    fn test_no_false_positive_on_code_with_angle_bracket() {
        let adapter = ClaudeCodeAdapter::new();
        let screen = make_screen(&["if x > 0 {", "    println!(\"hi\");", "}", "compiling..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    // ── build_command ─────────────────────────────────────────────────

    fn default_config() -> HarnessConfig {
        HarnessConfig::new(HarnessType::ClaudeCode, ".".into())
    }

    #[test]
    fn test_build_command_default() {
        let adapter = ClaudeCodeAdapter::new();
        let spec = adapter.build_command(&default_config());
        assert_eq!(spec.program, "claude");
        assert!(spec.args.is_empty(), "no args when no model or extra_args");
    }

    #[test]
    fn test_build_command_with_model() {
        let adapter = ClaudeCodeAdapter::new();
        let config = default_config().with_model("claude-opus-4-6");
        let spec = adapter.build_command(&config);
        assert_eq!(spec.program, "claude");
        assert_eq!(spec.args, vec!["--model", "claude-opus-4-6"]);
    }

    #[test]
    fn test_build_command_with_binary_override() {
        let adapter = ClaudeCodeAdapter::new();
        let mut config = default_config();
        config.binary_override = Some("/usr/local/bin/claude".to_string());
        let spec = adapter.build_command(&config);
        assert_eq!(spec.program, "/usr/local/bin/claude");
    }

    #[test]
    fn test_build_command_extra_args_appended_after_model() {
        let adapter = ClaudeCodeAdapter::new();
        let mut config = default_config().with_model("opus");
        config.extra_args = vec!["--no-auto-update".to_string()];
        let spec = adapter.build_command(&config);
        assert_eq!(spec.args, vec!["--model", "opus", "--no-auto-update"]);
    }

    // ── translate_input ───────────────────────────────────────────────

    #[test]
    fn test_translate_prompt() {
        let adapter = ClaudeCodeAdapter::new();
        let bytes = adapter.translate_input(&OrchestratorCommand::Prompt("hello".to_string()));
        assert_eq!(bytes, b"hello\n");
    }

    #[test]
    fn test_translate_cancel() {
        let adapter = ClaudeCodeAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Cancel),
            vec![0x03]
        );
    }

    #[test]
    fn test_translate_accept() {
        let adapter = ClaudeCodeAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Accept),
            b"y\n"
        );
    }

    #[test]
    fn test_translate_reject() {
        let adapter = ClaudeCodeAdapter::new();
        assert_eq!(
            adapter.translate_input(&OrchestratorCommand::Reject),
            b"n\n"
        );
    }

    #[test]
    fn test_translate_raw_passthrough() {
        let adapter = ClaudeCodeAdapter::new();
        let payload = vec![0x1b, b'[', b'A'];
        let bytes = adapter.translate_input(&OrchestratorCommand::Raw(payload.clone()));
        assert_eq!(bytes, payload);
    }
}
