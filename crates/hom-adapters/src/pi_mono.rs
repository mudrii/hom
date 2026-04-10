//! Adapter for pi-mono.
//!
//! **Tier 1** — Full orchestration/steering via RPC stdin/stdout + steering queue.
//!
//! Binary: `pi`
//! Models: Minimax 2.7, GLM 5.1, Kimi 2.5, DeepSeek, Nvidia (via NIM)
//! Sideband: RPC over stdin/stdout in a second process

use hom_core::*;

pub struct PiMonoAdapter;

impl PiMonoAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PiMonoAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl HarnessAdapter for PiMonoAdapter {
    fn harness_type(&self) -> HarnessType {
        HarnessType::PiMono
    }

    fn display_name(&self) -> &str {
        "pi-mono"
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
            if let Some(path) = line.strip_prefix("Created ") {
                events.push(HarnessEvent::FileChanged {
                    path: path.trim().into(),
                    change_type: ChangeType::Created,
                });
            } else if let Some(path) = line.strip_prefix("Modified ") {
                events.push(HarnessEvent::FileChanged {
                    path: path.trim().into(),
                    change_type: ChangeType::Modified,
                });
            } else if line.starts_with("Error:") || line.starts_with("error:") {
                events.push(HarnessEvent::Error {
                    message: line.to_string(),
                });
            }
        }

        events
    }

    fn detect_completion(&self, screen: &ScreenSnapshot) -> CompletionStatus {
        let last_line = screen.last_non_empty_line();
        let last_lines = screen.last_n_lines(5);

        if last_line.starts_with('❯') || last_line.starts_with("pi>") {
            CompletionStatus::WaitingForInput
        } else if last_lines.contains("Error:") || last_lines.contains("error:") {
            CompletionStatus::Failed { error: last_lines }
        } else {
            CompletionStatus::Running
        }
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            supports_steering: true,
            supports_json_output: false,
            supports_session_resume: false,
            supports_mcp: false,
            headless_command: None,
            sideband_type: Some(SidebandType::Rpc),
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
        let adapter = PiMonoAdapter::new();
        let screen = make_screen(&["some output", "pi> "]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::WaitingForInput
        ));
    }

    #[test]
    fn test_detect_running() {
        let adapter = PiMonoAdapter::new();
        let screen = make_screen(&["Working on task...", "Processing..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }

    #[test]
    fn test_detect_error() {
        let adapter = PiMonoAdapter::new();
        let screen = make_screen(&["Error: something failed"]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Failed { .. }
        ));
    }

    #[test]
    fn test_no_false_positive_on_code_with_angle_bracket() {
        let adapter = PiMonoAdapter::new();
        let screen = make_screen(&["if x > 0 {", "    println!(\"hi\");", "}", "compiling..."]);
        assert!(matches!(
            adapter.detect_completion(&screen),
            CompletionStatus::Running
        ));
    }
}
