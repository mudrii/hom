//! Main application state — ties together panes, input, commands, and rendering.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hom_adapters::AdapterRegistry;
use hom_core::{
    HarnessConfig, HarnessType, HomConfig, HomResult, LayoutKind, PaneId, TerminalBackend,
};
use hom_core::types::{McpCommand, McpRequest, McpResponse, PaneSummary};
use hom_pty::{AsyncPtyReader, PtyManager};
use hom_terminal::ActiveBackend;
use hom_web::{WebCell, WebFrame, WebInput, WebPane};
use tokio::sync::{broadcast, mpsc, mpsc as tokio_mpsc, oneshot};
use tracing::info;

use crate::command_bar::CommandBar;
use crate::input::InputRouter;
use crate::workflow_progress::WorkflowProgress;

/// A single pane in the TUI — holds a PTY, terminal emulator, and adapter reference.
pub struct Pane {
    pub id: PaneId,
    pub harness_type: HarnessType,
    pub model: Option<String>,
    pub title: String,
    pub terminal: ActiveBackend,
    pub pty_reader: Option<AsyncPtyReader>,
    /// Sideband channel for out-of-band communication (e.g. OpenCode HTTP API).
    /// Wrapped in Arc so it can be shared with spawned async tasks.
    pub sideband: Option<Arc<dyn hom_core::SidebandChannel>>,
    /// Exit code if the process has terminated. None while running.
    pub exited: Option<u32>,
}

/// A pending workflow completion — stored while waiting for a harness to finish.
pub struct PendingCompletion {
    pub pane_id: PaneId,
    pub reply: oneshot::Sender<HomResult<String>>,
    pub started: Instant,
    pub timeout: Duration,
}

/// The top-level application state.
pub struct App {
    pub config: HomConfig,
    pub panes: HashMap<PaneId, Pane>,
    pub pane_order: Vec<PaneId>,
    pub focused_pane: Option<PaneId>,
    pub layout: LayoutKind,
    pub input_router: InputRouter,
    pub command_bar: CommandBar,
    pub adapter_registry: AdapterRegistry,
    pub pty_manager: PtyManager,
    pub should_quit: bool,
    pub workflow_progress: Option<WorkflowProgress>,
    /// Optional database handle — opened at startup when available.
    pub db: Option<std::sync::Arc<hom_db::HomDb>>,
    /// Pending completions waiting for harness detect_completion().
    pub pending_completions: Vec<PendingCompletion>,
    /// Running total cost in USD, polled from the database.
    pub total_cost: f64,
    /// Receives MCP requests from the McpServer task. None when not in MCP mode.
    pub mcp_rx: Option<mpsc::Receiver<McpRequest>>,
    /// Broadcast channel for pushing WebFrame snapshots to WebSocket clients. None when --web is not set.
    pub web_tx: Option<broadcast::Sender<WebFrame>>,
    /// Receives browser keystrokes forwarded by the WebSocket server. None when --web is not set.
    pub web_input_rx: Option<tokio_mpsc::Receiver<WebInput>>,
}

impl App {
    pub fn new(config: HomConfig) -> Self {
        let layout = config.general.default_layout.clone();
        let input_router = InputRouter::from_config(&config.keybindings);

        Self {
            config,
            panes: HashMap::new(),
            pane_order: Vec::new(),
            focused_pane: None,
            layout,
            input_router,
            command_bar: CommandBar::new(),
            adapter_registry: AdapterRegistry::new(),
            pty_manager: PtyManager::new(),
            should_quit: false,
            workflow_progress: None,
            db: None,
            pending_completions: Vec::new(),
            total_cost: 0.0,
            mcp_rx: None,
            web_tx: None,
            web_input_rx: None,
        }
    }

    /// Spawn a new harness pane with additional options.
    pub fn spawn_pane_with_opts(
        &mut self,
        harness_type: HarnessType,
        model: Option<String>,
        working_dir: Option<std::path::PathBuf>,
        extra_args: Vec<String>,
        cols: u16,
        rows: u16,
    ) -> HomResult<PaneId> {
        self.spawn_pane_inner(harness_type, model, working_dir, extra_args, cols, rows)
    }

    /// Spawn a new harness pane with defaults.
    pub fn spawn_pane(
        &mut self,
        harness_type: HarnessType,
        model: Option<String>,
        cols: u16,
        rows: u16,
    ) -> HomResult<PaneId> {
        self.spawn_pane_inner(harness_type, model, None, Vec::new(), cols, rows)
    }

    fn spawn_pane_inner(
        &mut self,
        harness_type: HarnessType,
        model: Option<String>,
        working_dir: Option<std::path::PathBuf>,
        extra_args: Vec<String>,
        cols: u16,
        rows: u16,
    ) -> HomResult<PaneId> {
        // Enforce max_panes limit
        if self.panes.len() >= self.config.general.max_panes {
            return Err(hom_core::HomError::MaxPanesReached(
                self.config.general.max_panes,
            ));
        }

        let adapter = self
            .adapter_registry
            .get(&harness_type)
            .ok_or(hom_core::HomError::UnsupportedHarness(harness_type))?;

        // Look up harness config from config.toml [harnesses.<name>] entries
        // Uses the canonical config key (e.g. "claude-code", "pi-mono") that matches
        // the keys in config/default.toml, falling back to default binary name.
        let config_entry = self
            .config
            .harnesses
            .get(harness_type.config_key())
            .or_else(|| self.config.harnesses.get(harness_type.default_binary()));

        // Use explicit working_dir, or fall back to current dir
        let effective_dir =
            working_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

        let mut harness_config = HarnessConfig::new(harness_type, effective_dir);

        // Apply config.toml overrides
        if let Some(entry) = config_entry {
            harness_config.binary_override = Some(entry.command.clone());
            if model.is_none() {
                // Use default_model from config if no explicit model given
                if let Some(ref default_model) = entry.default_model {
                    harness_config.model = Some(default_model.clone());
                }
            }
            harness_config.env_vars.extend(entry.env.clone());
        }

        // Explicit model from command bar overrides config default
        if let Some(m) = &model {
            harness_config = harness_config.with_model(m.clone());
        }

        // Apply extra args from command bar
        harness_config.extra_args.extend(extra_args);

        let cmd_spec = adapter.build_command(&harness_config);
        let pane_id = self.pty_manager.spawn(&cmd_spec, cols, rows)?;

        // Set up async reader
        let reader = self.pty_manager.take_reader(pane_id)?;
        let async_reader = AsyncPtyReader::start(pane_id, reader);

        let scrollback = self.config.general.max_scrollback;
        let terminal = hom_terminal::create_terminal(cols, rows, scrollback);

        // Build title showing the effective model (explicit or config default)
        let effective_model = harness_config.model.as_deref().unwrap_or("");
        let title = format!("{} {}", adapter.display_name(), effective_model,)
            .trim()
            .to_string();

        // Construct sideband channel from config if specified.
        // For HTTP sidebands (OpenCode), bind the session to the pane_id so
        // that send_prompt targets `/session/<pane_id>/prompt_async` rather
        // than the "default" fallback.
        let sideband: Option<Arc<dyn hom_core::SidebandChannel>> = config_entry.and_then(|entry| {
            let sideband_type = entry.sideband.as_deref()?;
            let url = entry.sideband_url.as_deref()?;
            match sideband_type {
                "http" => {
                    let http = hom_adapters::sideband::http::HttpSideband::new(url.to_string())
                        .with_session(pane_id.to_string());
                    Some(Arc::new(http) as Arc<dyn hom_core::SidebandChannel>)
                }
                "rpc" => Some(Arc::new(hom_adapters::sideband::rpc::RpcSideband::new(
                    url.to_string(),
                )) as Arc<dyn hom_core::SidebandChannel>),
                _ => None,
            }
        });

        let pane = Pane {
            id: pane_id,
            harness_type,
            model: model.clone(),
            title,
            terminal,
            pty_reader: Some(async_reader),
            sideband,
            exited: None,
        };

        self.panes.insert(pane_id, pane);
        self.pane_order.push(pane_id);

        // Auto-focus the new pane
        self.focused_pane = Some(pane_id);
        self.input_router.focus_pane(pane_id);

        Ok(pane_id)
    }

    /// Kill a pane and remove it.
    pub fn kill_pane(&mut self, pane_id: PaneId) -> HomResult<()> {
        // Abort the async reader task before killing the PTY process.
        // This reduces the window between kill and task exit.
        if let Some(pane) = self.panes.get(&pane_id)
            && let Some(reader) = &pane.pty_reader
        {
            reader.abort();
        }

        self.pty_manager.kill(pane_id)?;
        self.panes.remove(&pane_id);
        self.pane_order.retain(|&id| id != pane_id);

        // Refocus if needed
        if self.focused_pane == Some(pane_id) {
            self.focused_pane = self.pane_order.last().copied();
            if let Some(new_focus) = self.focused_pane {
                self.input_router.focus_pane(new_focus);
            }
        }

        Ok(())
    }

    /// Focus the next pane in order.
    pub fn focus_next(&mut self) {
        if self.pane_order.is_empty() {
            return;
        }
        let current_idx = self
            .focused_pane
            .and_then(|id| self.pane_order.iter().position(|&p| p == id))
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.pane_order.len();
        let next_id = self.pane_order[next_idx];
        self.focused_pane = Some(next_id);
        self.input_router.focus_pane(next_id);
    }

    /// Focus the previous pane in order.
    pub fn focus_prev(&mut self) {
        if self.pane_order.is_empty() {
            return;
        }
        let current_idx = self
            .focused_pane
            .and_then(|id| self.pane_order.iter().position(|&p| p == id))
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            self.pane_order.len() - 1
        } else {
            current_idx - 1
        };
        let prev_id = self.pane_order[prev_idx];
        self.focused_pane = Some(prev_id);
        self.input_router.focus_pane(prev_id);
    }

    /// Check pending workflow completions — detect_completion() on each pending pane.
    ///
    /// When a pane reports `WaitingForInput` (finished) or `Failed`, the pending
    /// completion is resolved with the screen text. Timed-out completions are
    /// resolved with an error.
    pub fn poll_pending_completions(&mut self) {
        let mut resolved_indices = Vec::new();

        for (i, pending) in self.pending_completions.iter().enumerate() {
            // Check timeout first
            if pending.started.elapsed() >= pending.timeout {
                resolved_indices.push((
                    i,
                    Err(hom_core::HomError::WorkflowTimeout(
                        pending.timeout.as_secs(),
                    )),
                ));
                continue;
            }

            if let Some(pane) = self.panes.get(&pending.pane_id) {
                let snapshot = pane.terminal.screen_snapshot();
                let adapter = self.adapter_registry.get(&pane.harness_type);

                let status = adapter
                    .map(|a| a.detect_completion(&snapshot))
                    .unwrap_or(hom_core::CompletionStatus::Running);

                match status {
                    hom_core::CompletionStatus::WaitingForInput => {
                        // Harness is done — return screen text as output
                        let output = snapshot.last_n_lines(50);
                        resolved_indices.push((i, Ok(output)));
                    }
                    hom_core::CompletionStatus::Completed { output } => {
                        resolved_indices.push((i, Ok(output)));
                    }
                    hom_core::CompletionStatus::Failed { error } => {
                        resolved_indices.push((
                            i,
                            Err(hom_core::HomError::WorkflowStepFailed {
                                step: format!("pane-{}", pending.pane_id),
                                reason: error,
                            }),
                        ));
                    }
                    hom_core::CompletionStatus::Running => {
                        // Still running, check again next tick
                    }
                }
            } else {
                // Pane no longer exists
                resolved_indices.push((i, Err(hom_core::HomError::PaneNotFound(pending.pane_id))));
            }
        }

        // Drain resolved completions in reverse order to preserve indices
        for (i, result) in resolved_indices.into_iter().rev() {
            let pending = self.pending_completions.remove(i);
            let _ = pending.reply.send(result);
        }
    }

    /// Check for processes that have exited and mark their panes.
    /// Returns a list of (pane_id, exit_code) for newly exited panes.
    pub fn handle_exited_panes(&mut self) -> Vec<(PaneId, u32)> {
        let mut newly_exited = Vec::new();
        let pane_ids: Vec<PaneId> = self.pane_order.clone();

        for pane_id in pane_ids {
            if let Some(pane) = self.panes.get(&pane_id)
                && pane.exited.is_some()
            {
                continue;
            }

            if let Ok(Some(exit_code)) = self.pty_manager.try_wait(pane_id) {
                if let Some(pane) = self.panes.get_mut(&pane_id) {
                    pane.exited = Some(exit_code);
                }
                newly_exited.push((pane_id, exit_code));
            }
        }

        newly_exited
    }

    /// Process PTY output for all panes — feed bytes into terminal emulators.
    /// Returns any token usage events detected via adapter parse_screen().
    pub fn poll_pty_output(&mut self) -> Vec<(PaneId, HarnessType, hom_core::HarnessEvent)> {
        let mut token_events = Vec::new();
        let pane_ids: Vec<PaneId> = self.panes.keys().copied().collect();
        for pane_id in pane_ids {
            let mut had_data = false;
            if let Some(pane) = self.panes.get_mut(&pane_id)
                && let Some(reader) = &mut pane.pty_reader
            {
                while let Ok(data) = reader.rx.try_recv() {
                    pane.terminal.process(&data);
                    had_data = true;
                }
            }

            // After processing new data, scan for token usage events
            if had_data && let Some(pane) = self.panes.get(&pane_id) {
                let snapshot = pane.terminal.screen_snapshot();
                if let Some(adapter) = self.adapter_registry.get(&pane.harness_type) {
                    for event in adapter.parse_screen(&snapshot) {
                        if matches!(event, hom_core::HarnessEvent::TokenUsage { .. }) {
                            token_events.push((pane_id, pane.harness_type, event));
                        }
                    }
                }
            }
        }
        token_events
    }

    /// Serialize the current session (layout + pane configs) for persistence.
    pub fn session_snapshot(&self) -> (String, String) {
        let layout_json = serde_json::to_string(&self.layout).unwrap_or_default();
        let pane_configs: Vec<SessionPaneConfig> = self
            .pane_order
            .iter()
            .filter_map(|id| {
                self.panes.get(id).map(|pane| SessionPaneConfig {
                    harness_type: pane.harness_type,
                    model: pane.model.clone(),
                })
            })
            .collect();
        let panes_json = serde_json::to_string(&pane_configs).unwrap_or_default();
        (layout_json, panes_json)
    }

    /// Process up to 16 pending MCP requests per tick to avoid starving the render loop.
    pub fn handle_mcp_requests(&mut self) {
        // Take the receiver out of self so we can call &mut self methods without
        // holding a simultaneous borrow on self.mcp_rx.
        let mut rx = match self.mcp_rx.take() {
            Some(r) => r,
            None => return,
        };

        let mut pending: Vec<McpRequest> = Vec::with_capacity(16);
        for _ in 0..16 {
            match rx.try_recv() {
                Ok(req) => pending.push(req),
                Err(_) => break,
            }
        }

        // Put the receiver back before processing so callers can re-enter.
        self.mcp_rx = Some(rx);

        for McpRequest { command, reply } in pending {
            let response = self.execute_mcp_command(command);
            let _ = reply.send(response);
        }
    }

    fn execute_mcp_command(&mut self, command: McpCommand) -> McpResponse {
        match command {
            McpCommand::ListPanes => {
                let panes = self
                    .panes
                    .iter()
                    .map(|(id, pane)| PaneSummary {
                        pane_id: id.to_string(),
                        harness: pane.harness_type.to_string(),
                        status: if pane.exited.is_some() {
                            "exited".into()
                        } else {
                            "running".into()
                        },
                    })
                    .collect();
                McpResponse::ListPanes { panes }
            }
            McpCommand::SpawnPane { harness, model } => {
                match HarnessType::from_str_loose(&harness) {
                    Some(harness_type) => {
                        // Use a modest default size; actual resize happens on first render.
                        match self.spawn_pane(harness_type, model, 80, 24) {
                            Ok(pane_id) => McpResponse::SpawnPane {
                                pane_id: pane_id.to_string(),
                            },
                            Err(e) => McpResponse::Error {
                                error: e.to_string(),
                            },
                        }
                    }
                    None => McpResponse::Error {
                        error: format!("unknown harness: {harness}"),
                    },
                }
            }
            McpCommand::SendToPane { pane_id, text } => {
                match pane_id.parse::<PaneId>() {
                    Ok(id) => {
                        if let Some(pane) = self.panes.get(&id) {
                            let adapter = self.adapter_registry.get(&pane.harness_type);
                            let bytes = adapter
                                .map(|a| {
                                    a.translate_input(&hom_core::OrchestratorCommand::Prompt(
                                        text.clone(),
                                    ))
                                })
                                .unwrap_or_else(|| format!("{text}\n").into_bytes());
                            match self.pty_manager.write_to(id, &bytes) {
                                Ok(()) => McpResponse::SendToPane { ok: true },
                                Err(e) => McpResponse::Error {
                                    error: e.to_string(),
                                },
                            }
                        } else {
                            McpResponse::Error {
                                error: format!("pane not found: {pane_id}"),
                            }
                        }
                    }
                    Err(_) => McpResponse::Error {
                        error: format!("invalid pane_id: {pane_id}"),
                    },
                }
            }
            McpCommand::RunWorkflow { path, vars } => {
                // The workflow executor lives in main.rs (handle_run). We cannot call it
                // directly from App without pulling in the WorkflowBridge dependency.
                // Instead we return an error directing the caller to use :run via the
                // command bar, which is the correct orchestration path.
                // A future refactor can move the bridge into App.
                let _ = (path, vars);
                McpResponse::Error {
                    error: "run_workflow via MCP is not yet wired; use :run in the TUI command bar".into(),
                }
            }
            McpCommand::GetPaneOutput { pane_id, lines } => {
                match pane_id.parse::<PaneId>() {
                    Ok(id) => {
                        if let Some(pane) = self.panes.get(&id) {
                            let snapshot = pane.terminal.screen_snapshot();
                            let start = snapshot.rows.len().saturating_sub(lines);
                            let output_lines: Vec<String> = snapshot.rows[start..]
                                .iter()
                                .map(|row| {
                                    row.iter()
                                        .map(|c| c.character)
                                        .collect::<String>()
                                        .trim_end()
                                        .to_string()
                                })
                                .collect();
                            McpResponse::GetPaneOutput {
                                lines: output_lines,
                            }
                        } else {
                            McpResponse::Error {
                                error: format!("pane not found: {pane_id}"),
                            }
                        }
                    }
                    Err(_) => McpResponse::Error {
                        error: format!("invalid pane_id: {pane_id}"),
                    },
                }
            }
            McpCommand::KillPane { pane_id } => {
                match pane_id.parse::<PaneId>() {
                    Ok(id) => match self.kill_pane(id) {
                        Ok(()) => McpResponse::KillPane { ok: true },
                        Err(e) => McpResponse::Error {
                            error: e.to_string(),
                        },
                    },
                    Err(_) => McpResponse::Error {
                        error: format!("invalid pane_id: {pane_id}"),
                    },
                }
            }
        }
    }

    /// Serialize current pane state and broadcast to WebSocket clients.
    /// No-op when --web is not active or no clients are connected.
    pub fn publish_web_frame(&self) {
        let Some(tx) = self.web_tx.as_ref() else {
            return;
        };
        if tx.receiver_count() == 0 {
            return;
        }

        let panes: Vec<WebPane> = self
            .panes
            .iter()
            .map(|(id, pane)| {
                let snap = pane.terminal.screen_snapshot();
                let cells: Vec<WebCell> = snap
                    .rows
                    .iter()
                    .flat_map(|row| {
                        row.iter().map(|cell| {
                            use hom_core::TermColor;
                            let to_rgb = |c: &TermColor| -> u32 {
                                match c {
                                    TermColor::Rgb(r, g, b) => {
                                        (*r as u32) << 16 | (*g as u32) << 8 | *b as u32
                                    }
                                    _ => 0xFF_FF_FF,
                                }
                            };
                            WebCell {
                                ch: cell.character,
                                fg: to_rgb(&cell.fg),
                                bg: to_rgb(&cell.bg),
                                bold: cell.attrs.bold,
                                italic: cell.attrs.italic,
                                underline: cell.attrs.underline,
                            }
                        })
                    })
                    .collect();
                WebPane {
                    pane_id: id.to_string(),
                    title: pane.title.clone(),
                    cols: snap.cols,
                    rows: snap.num_rows,
                    cursor_col: snap.cursor.col,
                    cursor_row: snap.cursor.row,
                    cells,
                    focused: self.focused_pane == Some(*id),
                }
            })
            .collect();

        let _ = tx.send(WebFrame::new(panes));
    }

    /// Forward browser keystrokes to the targeted pane's PTY.
    /// No-op when --web is not active.
    ///
    /// Uses the same take/put-back pattern as handle_mcp_requests() to avoid
    /// holding a borrow on self.web_input_rx while calling &mut self methods.
    pub fn handle_web_input(&mut self) {
        let mut rx = match self.web_input_rx.take() {
            Some(r) => r,
            None => return,
        };

        let mut inputs: Vec<WebInput> = Vec::with_capacity(16);
        for _ in 0..16 {
            match rx.try_recv() {
                Ok(input) => inputs.push(input),
                Err(_) => break,
            }
        }

        // Restore the receiver before processing inputs.
        self.web_input_rx = Some(rx);

        for input in inputs {
            match input.pane_id.parse::<PaneId>() {
                Ok(id) => {
                    if let Some(pane) = self.panes.get(&id) {
                        let adapter = self.adapter_registry.get(&pane.harness_type);
                        let bytes = adapter
                            .map(|a| {
                                a.translate_input(&hom_core::OrchestratorCommand::Prompt(
                                    input.text.clone(),
                                ))
                            })
                            .unwrap_or_else(|| format!("{}\n", input.text).into_bytes());
                        let _ = self.pty_manager.write_to(id, &bytes);
                    }
                }
                Err(_) => {
                    tracing::warn!(pane_id = %input.pane_id, "web input: invalid pane_id");
                }
            }
        }
    }

    /// Clean shutdown: kill all PTY processes and drain pending completions.
    pub fn shutdown(&mut self) {
        for pending in self.pending_completions.drain(..) {
            let _ = pending
                .reply
                .send(Err(hom_core::HomError::Other("shutting down".to_string())));
        }
        self.pty_manager.kill_all();
        self.panes.clear();
        self.pane_order.clear();
        info!("app shutdown complete");
    }
}

/// Serializable pane configuration for session save/restore.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionPaneConfig {
    pub harness_type: HarnessType,
    pub model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_pane_config_roundtrip() {
        let configs = vec![
            SessionPaneConfig {
                harness_type: HarnessType::ClaudeCode,
                model: Some("opus".to_string()),
            },
            SessionPaneConfig {
                harness_type: HarnessType::CodexCli,
                model: None,
            },
        ];
        let json = serde_json::to_string(&configs).unwrap();
        let parsed: Vec<SessionPaneConfig> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].harness_type, HarnessType::ClaudeCode);
        assert_eq!(parsed[0].model, Some("opus".to_string()));
        assert_eq!(parsed[1].model, None);
    }

    #[test]
    fn test_session_snapshot_empty_app() {
        let app = App::new(HomConfig::default());
        let (layout, panes) = app.session_snapshot();
        assert!(!layout.is_empty());
        let parsed: Vec<SessionPaneConfig> = serde_json::from_str(&panes).unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_shutdown_clears_state() {
        let mut app = App::new(HomConfig::default());
        app.shutdown();
        assert!(app.panes.is_empty());
        assert!(app.pane_order.is_empty());
        assert!(app.pending_completions.is_empty());
    }

    #[test]
    fn test_handle_exited_panes_returns_empty_for_new_app() {
        let mut app = App::new(HomConfig::default());
        let newly_exited = app.handle_exited_panes();
        assert!(
            newly_exited.is_empty(),
            "expected no exited panes for empty app"
        );
    }
}
