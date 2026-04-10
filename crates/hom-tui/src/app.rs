//! Main application state — ties together panes, input, commands, and rendering.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hom_adapters::AdapterRegistry;
use hom_core::{
    HarnessConfig, HarnessType, HomConfig, HomResult, LayoutKind, PaneId, TerminalBackend,
};
use hom_pty::{AsyncPtyReader, PtyManager};
use hom_terminal::ActiveBackend;
use tokio::sync::oneshot;

use crate::command_bar::CommandBar;
use crate::input::InputRouter;

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
    pub workflow_status: Option<String>,
    /// Optional database handle — opened at startup when available.
    pub db: Option<std::sync::Arc<hom_db::HomDb>>,
    /// Pending completions waiting for harness detect_completion().
    pub pending_completions: Vec<PendingCompletion>,
    /// Running total cost in USD, polled from the database.
    pub total_cost: f64,
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
            workflow_status: None,
            db: None,
            pending_completions: Vec::new(),
            total_cost: 0.0,
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
}
