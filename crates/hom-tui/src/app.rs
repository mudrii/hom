//! Main application state — ties together panes, input, commands, and rendering.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hom_adapters::AdapterRegistry;
use hom_core::types::{McpCommand, McpRequest, McpResponse, PaneSummary};
use hom_core::{
    HarnessConfig, HarnessType, HomConfig, HomResult, LayoutKind, OrchestratorCommand, PaneId,
    PaneKind, RemoteTarget, TerminalBackend,
};
use hom_pty::{AsyncPtyReader, ConnectedRemotePty, PtyManager, RemotePtyManager, SshAuthMethod};
use hom_terminal::ActiveBackend;
use hom_web::{WebCell, WebFrame, WebInput, WebPane};
use tokio::sync::{broadcast, mpsc, mpsc as tokio_mpsc, oneshot};
use tracing::{debug, info};

use crate::command_bar::CommandBar;
use crate::input::InputRouter;
use crate::workflow_bridge::WorkflowLauncher;
use crate::workflow_progress::WorkflowProgress;

/// A single pane in the TUI — holds a PTY, terminal emulator, and adapter reference.
pub struct Pane {
    pub id: PaneId,
    pub harness_type: HarnessType,
    pub pane_kind: PaneKind,
    /// Set for plugin-backed panes; None for built-in harness types.
    /// Used to look up the correct plugin adapter in poll_pending_completions and poll_pty_output.
    pub plugin_name: Option<String>,
    pub model: Option<String>,
    pub working_dir: PathBuf,
    pub extra_args: Vec<String>,
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

/// Request to spawn a local pane.
pub struct PaneSpawnRequest {
    pub harness: Option<HarnessType>,
    pub harness_name: String,
    pub model: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub extra_args: Vec<String>,
    pub cols: u16,
    pub rows: u16,
}

/// Request to spawn a remote pane via SSH.
pub struct RemotePaneSpawnRequest {
    pub harness_type: HarnessType,
    pub model: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub extra_args: Vec<String>,
    pub target: RemoteTarget,
    pub cols: u16,
    pub rows: u16,
}

/// Prepared remote spawn metadata that can be sent to a blocking worker.
pub struct PreparedRemotePaneSpawn {
    pub pane_id: PaneId,
    pub harness_type: HarnessType,
    pub model: Option<String>,
    pub working_dir: PathBuf,
    pub extra_args: Vec<String>,
    pub title: String,
    pub target: RemoteTarget,
    pub cols: u16,
    pub rows: u16,
    pub command: String,
    pub env: HashMap<String, String>,
    pub auth_methods: Vec<SshAuthMethod>,
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
    /// Manages SSH-backed remote pane sessions.
    pub remote_ptys: RemotePtyManager,
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
    /// Queues workflow launches requested by MCP or the TUI command bar.
    pub workflow_launcher: Option<WorkflowLauncher>,
    /// Broadcast channel for pushing WebFrame snapshots to WebSocket clients. None when --web is not set.
    pub web_tx: Option<broadcast::Sender<WebFrame>>,
    /// Receives browser keystrokes forwarded by the WebSocket server. None when --web is not set.
    pub web_input_rx: Option<tokio_mpsc::Receiver<WebInput>>,
    /// Remote panes reserved but still connecting over SSH.
    pub pending_remote_spawns: usize,
}

impl App {
    pub fn new(config: HomConfig) -> Self {
        let layout = config.general.default_layout.clone();
        let input_router = InputRouter::from_config(&config.keybindings);

        let mut adapter_registry = AdapterRegistry::new();

        // Auto-load any plugins from ~/.config/hom/plugins/ at startup.
        let loaded = adapter_registry.scan_default_plugin_dir();
        if !loaded.is_empty() {
            tracing::info!(plugins = ?loaded, "auto-loaded plugins at startup");
        }

        Self {
            config,
            panes: HashMap::new(),
            pane_order: Vec::new(),
            focused_pane: None,
            layout,
            input_router,
            command_bar: CommandBar::new(),
            adapter_registry,
            pty_manager: PtyManager::new(),
            remote_ptys: RemotePtyManager::new(),
            should_quit: false,
            workflow_progress: None,
            db: None,
            pending_completions: Vec::new(),
            total_cost: 0.0,
            mcp_rx: None,
            workflow_launcher: None,
            web_tx: None,
            web_input_rx: None,
            pending_remote_spawns: 0,
        }
    }

    /// Load a plugin at runtime and register it in the adapter registry.
    pub fn handle_load_plugin(&mut self, path: &std::path::Path) {
        match self.adapter_registry.load_plugin(path) {
            Ok(name) => {
                // Success is logged; no last_error update because the render prefixes
                // last_error with "Error:" in red, which is confusing for a success message.
                tracing::info!(plugin = %name, "loaded plugin adapter");
            }
            Err(e) => {
                self.command_bar.last_error = Some(format!("plugin load failed: {e}"));
            }
        }
    }

    /// Spawn a new harness pane with additional options.
    ///
    /// `harness` is `Some` for built-in harness types and `None` for plugin-backed harnesses.
    /// When `harness` is `None`, `harness_name` is used to look up a loaded plugin adapter.
    pub fn spawn_pane_with_opts(&mut self, request: PaneSpawnRequest) -> HomResult<PaneId> {
        self.spawn_pane_inner(request)
    }

    /// Spawn a new harness pane with defaults.
    ///
    /// Convenience wrapper — uses the built-in `harness_type` as both the
    /// lookup key and the harness name.
    pub fn spawn_pane(
        &mut self,
        harness_type: HarnessType,
        model: Option<String>,
        cols: u16,
        rows: u16,
    ) -> HomResult<PaneId> {
        self.spawn_pane_inner(PaneSpawnRequest {
            harness: Some(harness_type),
            harness_name: harness_type.default_binary().to_string(),
            model,
            working_dir: None,
            extra_args: Vec::new(),
            cols,
            rows,
        })
    }

    fn spawn_pane_inner(&mut self, request: PaneSpawnRequest) -> HomResult<PaneId> {
        let PaneSpawnRequest {
            harness,
            harness_name,
            model,
            working_dir,
            extra_args,
            cols,
            rows,
        } = request;
        // Enforce max_panes limit
        if self.panes.len() + self.pending_remote_spawns >= self.config.general.max_panes {
            return Err(hom_core::HomError::MaxPanesReached(
                self.config.general.max_panes,
            ));
        }

        // Extract all config data we need up front so the borrow on self.config
        // ends before the mutable PTY/adapter operations below.
        let (binary_override, config_default_model, env_vars, sideband_type, sideband_url) = {
            let config_entry = if let Some(ht) = harness {
                self.config
                    .harnesses
                    .get(ht.config_key())
                    .or_else(|| self.config.harnesses.get(ht.default_binary()))
            } else {
                self.config.harnesses.get(&harness_name)
            };
            match config_entry {
                Some(entry) => (
                    Some(entry.command.clone()),
                    entry.default_model.clone(),
                    entry.env.clone(),
                    entry.sideband.clone(),
                    entry.sideband_url.clone(),
                ),
                None => (None, None, std::collections::HashMap::new(), None, None),
            }
        };
        let max_scrollback = self.config.general.max_scrollback;

        // Use explicit working_dir, or fall back to current dir
        let effective_dir =
            working_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

        // For plugin harnesses we still need a HarnessType for HarnessConfig.
        // Use ClaudeCode as a neutral placeholder — the plugin's build_command()
        // overrides the binary and arguments completely.
        let config_harness_type = harness.unwrap_or(HarnessType::ClaudeCode);
        let mut harness_config = HarnessConfig::new(config_harness_type, effective_dir);

        // Apply config.toml overrides
        if let Some(bin) = binary_override {
            harness_config.binary_override = Some(bin);
        }
        if model.is_none()
            && let Some(default_model) = config_default_model
        {
            harness_config.model = Some(default_model);
        }
        harness_config.env_vars.extend(env_vars);

        // Explicit model from command bar overrides config default
        if let Some(m) = &model {
            harness_config = harness_config.with_model(m.clone());
        }

        // Apply extra args from command bar
        harness_config.extra_args.extend(extra_args);

        // Build CommandSpec from the adapter (immutable borrow of self.adapter_registry).
        // The borrow ends at the semicolon so subsequent mutable borrows are safe.
        let (cmd_spec, display_name) = if let Some(ht) = harness {
            let adapter = self
                .adapter_registry
                .get(&ht)
                .ok_or(hom_core::HomError::UnsupportedHarness(ht))?;
            (
                adapter.build_command(&harness_config),
                adapter.display_name().to_string(),
            )
        } else {
            let adapter = self
                .adapter_registry
                .get_plugin(&harness_name)
                .ok_or_else(|| {
                    hom_core::HomError::Other(format!(
                        "unknown harness '{harness_name}' — is the plugin loaded?"
                    ))
                })?;
            (
                adapter.build_command(&harness_config),
                adapter.display_name().to_string(),
            )
        };

        let pane_id = self.pty_manager.spawn(&cmd_spec, cols, rows)?;

        // Set up async reader
        let reader = self.pty_manager.take_reader(pane_id)?;
        let async_reader = AsyncPtyReader::start(pane_id, reader);

        let terminal = hom_terminal::create_terminal(cols, rows, max_scrollback)?;

        // Build title showing the effective model (explicit or config default)
        let effective_model = harness_config.model.as_deref().unwrap_or("");
        let title = format!("{} {}", display_name, effective_model)
            .trim()
            .to_string();

        // Construct sideband channel from config if specified.
        // For HTTP sidebands (OpenCode), bind the session to the pane_id so
        // that send_prompt targets `/session/<pane_id>/prompt_async` rather
        // than the "default" fallback.
        let sideband: Option<Arc<dyn hom_core::SidebandChannel>> =
            match (sideband_type.as_deref(), sideband_url.as_deref()) {
                (Some("http"), Some(url)) => {
                    let http = hom_adapters::sideband::http::HttpSideband::new(url.to_string())
                        .with_session(pane_id.to_string());
                    Some(Arc::new(http) as Arc<dyn hom_core::SidebandChannel>)
                }
                (Some("rpc"), Some(url)) => Some(Arc::new(
                    hom_adapters::sideband::rpc::RpcSideband::new(url.to_string()),
                )
                    as Arc<dyn hom_core::SidebandChannel>),
                _ => None,
            };

        // Use the resolved harness type for the Pane, falling back to ClaudeCode
        // for plugin-backed panes (plugin adapters don't map to a built-in HarnessType).
        let pane_harness_type = harness.unwrap_or(HarnessType::ClaudeCode);

        let pane = Pane {
            id: pane_id,
            harness_type: pane_harness_type,
            pane_kind: PaneKind::Local,
            plugin_name: if harness.is_none() {
                Some(harness_name)
            } else {
                None
            },
            model: model.clone(),
            working_dir: harness_config.working_dir.clone(),
            extra_args: harness_config.extra_args.clone(),
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

    /// Return a (cols, rows) size for the focused pane, or a sensible default.
    ///
    /// Used when spawning remote panes from the command bar before the first
    /// render has produced real pane areas. The actual size is corrected on the
    /// next resize event.
    pub fn focused_pane_dimensions(&self) -> (u16, u16) {
        (80, 24)
    }

    /// Prepare a remote pane spawn for execution on a blocking worker thread.
    pub fn prepare_remote_pane_spawn(
        &mut self,
        request: RemotePaneSpawnRequest,
    ) -> HomResult<PreparedRemotePaneSpawn> {
        let RemotePaneSpawnRequest {
            harness_type,
            model,
            working_dir,
            extra_args,
            target,
            cols,
            rows,
        } = request;
        if self.panes.len() + self.pending_remote_spawns >= self.config.general.max_panes {
            return Err(hom_core::HomError::MaxPanesReached(
                self.config.general.max_panes,
            ));
        }

        let adapter = self
            .adapter_registry
            .get(&harness_type)
            .ok_or(hom_core::HomError::UnsupportedHarness(harness_type))?;

        let effective_dir =
            working_dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

        let mut harness_config = HarnessConfig::new(harness_type, effective_dir);

        let config_entry = self
            .config
            .harnesses
            .get(harness_type.config_key())
            .or_else(|| self.config.harnesses.get(harness_type.default_binary()));

        if let Some(entry) = config_entry {
            harness_config.binary_override = Some(entry.command.clone());
            if model.is_none()
                && let Some(ref default_model) = entry.default_model
            {
                harness_config.model = Some(default_model.clone());
            }
            harness_config.env_vars.extend(entry.env.clone());
        }

        if let Some(m) = &model {
            harness_config = harness_config.with_model(m.clone());
        }

        harness_config.extra_args.extend(extra_args);

        let cmd_spec = adapter.build_command(&harness_config);
        let remote_cmd = RemoteTarget::build_remote_command(&cmd_spec);
        let pane_id = self.remote_ptys.reserve_pane_id();
        let auth_methods = SshAuthMethod::defaults();
        let effective_model = harness_config.model.as_deref().unwrap_or("");
        let title = format!(
            "{} {} [{}]",
            adapter.display_name(),
            effective_model,
            target
        )
        .trim()
        .to_string();

        self.pending_remote_spawns += 1;

        Ok(PreparedRemotePaneSpawn {
            pane_id,
            harness_type,
            model,
            working_dir: harness_config.working_dir.clone(),
            extra_args: harness_config.extra_args.clone(),
            title,
            target,
            cols,
            rows,
            command: remote_cmd,
            env: cmd_spec.env,
            auth_methods,
        })
    }

    /// Complete a previously prepared remote pane spawn on the app thread.
    pub fn complete_remote_pane_spawn(
        &mut self,
        prepared: PreparedRemotePaneSpawn,
        connected: ConnectedRemotePty,
    ) -> HomResult<PaneId> {
        self.pending_remote_spawns = self.pending_remote_spawns.saturating_sub(1);

        let terminal = hom_terminal::create_terminal(
            prepared.cols,
            prepared.rows,
            self.config.general.max_scrollback,
        )?;
        let async_reader = AsyncPtyReader::start(prepared.pane_id, connected.reader);

        self.remote_ptys
            .insert_session(prepared.pane_id, connected.session);

        let pane = Pane {
            id: prepared.pane_id,
            harness_type: prepared.harness_type,
            pane_kind: PaneKind::Remote(prepared.target.clone()),
            plugin_name: None,
            model: prepared.model.clone(),
            working_dir: prepared.working_dir,
            extra_args: prepared.extra_args,
            title: prepared.title,
            terminal,
            pty_reader: Some(async_reader),
            sideband: None,
            exited: None,
        };

        self.panes.insert(prepared.pane_id, pane);
        self.pane_order.push(prepared.pane_id);
        self.focused_pane = Some(prepared.pane_id);
        self.input_router.focus_pane(prepared.pane_id);

        Ok(prepared.pane_id)
    }

    /// Release a reserved remote spawn slot after a failed connection attempt.
    pub fn remote_spawn_failed(&mut self) {
        self.pending_remote_spawns = self.pending_remote_spawns.saturating_sub(1);
    }

    /// Synchronous helper for tests and non-interactive callers.
    pub fn spawn_remote_pane_with_opts(
        &mut self,
        request: RemotePaneSpawnRequest,
    ) -> HomResult<PaneId> {
        let prepared = self.prepare_remote_pane_spawn(request)?;
        match RemotePtyManager::connect(
            prepared.target.clone(),
            prepared.command.clone(),
            prepared.env.clone(),
            prepared.cols,
            prepared.rows,
            prepared.auth_methods.clone(),
        ) {
            Ok(connected) => self.complete_remote_pane_spawn(prepared, connected),
            Err(err) => {
                self.remote_spawn_failed();
                Err(err)
            }
        }
    }

    /// Synchronous helper for tests and non-interactive callers.
    pub fn spawn_remote_pane(
        &mut self,
        harness_type: HarnessType,
        model: Option<String>,
        target: RemoteTarget,
        cols: u16,
        rows: u16,
    ) -> HomResult<PaneId> {
        self.spawn_remote_pane_with_opts(RemotePaneSpawnRequest {
            harness_type,
            model,
            working_dir: None,
            extra_args: Vec::new(),
            target,
            cols,
            rows,
        })
    }

    fn fallback_input_bytes(command: &OrchestratorCommand) -> Vec<u8> {
        match command {
            OrchestratorCommand::Prompt(text) => format!("{text}\n").into_bytes(),
            OrchestratorCommand::Raw(bytes) => bytes.clone(),
            OrchestratorCommand::Cancel
            | OrchestratorCommand::Accept
            | OrchestratorCommand::Reject => Vec::new(),
        }
    }

    fn adapter_for_pane(&self, pane: &Pane) -> Option<&dyn hom_core::HarnessAdapter> {
        if let Some(ref plugin_name) = pane.plugin_name {
            self.adapter_registry.get_plugin(plugin_name)
        } else {
            self.adapter_registry.get(&pane.harness_type)
        }
    }

    pub fn pane_harness_key(&self, pane_id: PaneId) -> Option<String> {
        self.panes.get(&pane_id).map(|pane| {
            pane.plugin_name
                .clone()
                .unwrap_or_else(|| pane.harness_type.default_binary().to_string())
        })
    }

    pub fn pane_display_name(&self, pane_id: PaneId) -> Option<String> {
        self.panes.get(&pane_id).map(|pane| {
            self.adapter_for_pane(pane)
                .map(|adapter| adapter.display_name().to_string())
                .unwrap_or_else(|| {
                    pane.plugin_name
                        .clone()
                        .unwrap_or_else(|| pane.harness_type.display_name().to_string())
                })
        })
    }

    pub fn translate_input_for_pane(
        &self,
        pane_id: PaneId,
        command: &OrchestratorCommand,
    ) -> Option<Vec<u8>> {
        self.panes.get(&pane_id).map(|pane| {
            self.adapter_for_pane(pane)
                .map(|adapter| adapter.translate_input(command))
                .unwrap_or_else(|| Self::fallback_input_bytes(command))
        })
    }

    pub fn parse_screen_for_pane(&self, pane_id: PaneId) -> Vec<hom_core::HarnessEvent> {
        let Some(pane) = self.panes.get(&pane_id) else {
            return Vec::new();
        };
        let snapshot = pane.terminal.screen_snapshot();
        self.adapter_for_pane(pane)
            .map(|adapter| adapter.parse_screen(&snapshot))
            .unwrap_or_default()
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

        // Remote panes are tracked by remote_ptys; local panes by pty_manager.
        if self.remote_ptys.has_pane(pane_id) {
            self.remote_ptys.kill(pane_id)?;
        } else {
            self.pty_manager.kill(pane_id)?;
        }
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

    /// Write bytes to a pane's PTY, dispatching to the correct manager.
    ///
    /// Remote panes (IDs ≥ 1000) are served by `remote_ptys`; local panes by `pty_manager`.
    pub fn pty_write(&mut self, pane_id: PaneId, bytes: &[u8]) -> HomResult<()> {
        if self.remote_ptys.has_pane(pane_id) {
            self.remote_ptys.write_to(pane_id, bytes)
        } else {
            self.pty_manager.write_to(pane_id, bytes)
        }
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

                // Plugin panes must use the plugin adapter, not the ClaudeCode placeholder.
                let status = if let Some(ref plugin_name) = pane.plugin_name {
                    self.adapter_registry
                        .get_plugin(plugin_name)
                        .map(|a| a.detect_completion(&snapshot))
                        .unwrap_or(hom_core::CompletionStatus::Running)
                } else {
                    self.adapter_registry
                        .get(&pane.harness_type)
                        .map(|a| a.detect_completion(&snapshot))
                        .unwrap_or(hom_core::CompletionStatus::Running)
                };

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

            let maybe_exit = if self.remote_ptys.has_pane(pane_id) {
                self.remote_ptys.try_wait(pane_id).ok().flatten()
            } else {
                self.pty_manager.try_wait(pane_id).ok().flatten()
            };

            if let Some(exit_code) = maybe_exit {
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
    pub fn poll_pty_output(&mut self) -> Vec<(PaneId, String, hom_core::HarnessEvent)> {
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

            // After processing new data, scan for token usage events.
            // Plugin panes must use the plugin adapter for screen parsing.
            if had_data && let Some(pane) = self.panes.get(&pane_id) {
                let snapshot = pane.terminal.screen_snapshot();
                let events = if let Some(ref plugin_name) = pane.plugin_name {
                    self.adapter_registry
                        .get_plugin(plugin_name)
                        .map(|a| a.parse_screen(&snapshot))
                        .unwrap_or_default()
                } else {
                    self.adapter_registry
                        .get(&pane.harness_type)
                        .map(|a| a.parse_screen(&snapshot))
                        .unwrap_or_default()
                };
                for event in events {
                    if matches!(event, hom_core::HarnessEvent::TokenUsage { .. }) {
                        let harness = pane
                            .plugin_name
                            .clone()
                            .unwrap_or_else(|| pane.harness_type.default_binary().to_string());
                        token_events.push((pane_id, harness, event));
                    }
                }
            }
        }
        token_events
    }

    /// Serialize the current session (layout + pane configs) for persistence.
    pub fn session_snapshot(&self) -> HomResult<(String, String)> {
        let layout_json = serde_json::to_string(&self.layout)
            .map_err(|e| hom_core::HomError::Other(format!("serialize layout: {e}")))?;
        let pane_configs: Vec<SessionPaneConfig> = self
            .pane_order
            .iter()
            .filter_map(|id| {
                self.panes.get(id).map(|pane| SessionPaneConfig {
                    harness_type: pane.harness_type,
                    plugin_name: pane.plugin_name.clone(),
                    model: pane.model.clone(),
                    pane_kind: pane.pane_kind.clone(),
                    working_dir: pane.working_dir.clone(),
                    extra_args: pane.extra_args.clone(),
                })
            })
            .collect();
        let panes_json = serde_json::to_string(&pane_configs)
            .map_err(|e| hom_core::HomError::Other(format!("serialize panes: {e}")))?;
        Ok((layout_json, panes_json))
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
                        harness: pane
                            .plugin_name
                            .clone()
                            .unwrap_or_else(|| pane.harness_type.default_binary().to_string()),
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
            McpCommand::SendToPane { pane_id, text } => match pane_id.parse::<PaneId>() {
                Ok(id) => {
                    if self.panes.contains_key(&id) {
                        let bytes = self
                            .translate_input_for_pane(id, &OrchestratorCommand::Prompt(text))
                            .unwrap_or_default();
                        match self.pty_write(id, &bytes) {
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
            },
            McpCommand::RunWorkflow { path, vars } => {
                let Some(ref launcher) = self.workflow_launcher else {
                    return McpResponse::Error {
                        error: "workflow launcher is not available".into(),
                    };
                };
                let workflow_path = PathBuf::from(&path);
                match hom_workflow::WorkflowDef::from_file(&workflow_path) {
                    Ok(def) => {
                        self.workflow_progress = Some(WorkflowProgress::new(
                            def.name.clone(),
                            def.steps.iter().map(|step| step.id.clone()).collect(),
                        ));
                        match launcher.launch(def, vars, path) {
                            Ok(workflow_id) => McpResponse::RunWorkflow { workflow_id },
                            Err(e) => McpResponse::Error {
                                error: e.to_string(),
                            },
                        }
                    }
                    Err(e) => McpResponse::Error {
                        error: e.to_string(),
                    },
                }
            }
            McpCommand::GetPaneOutput { pane_id, lines } => match pane_id.parse::<PaneId>() {
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
            },
            McpCommand::KillPane { pane_id } => match pane_id.parse::<PaneId>() {
                Ok(id) => match self.kill_pane(id) {
                    Ok(()) => McpResponse::KillPane { ok: true },
                    Err(e) => McpResponse::Error {
                        error: e.to_string(),
                    },
                },
                Err(_) => McpResponse::Error {
                    error: format!("invalid pane_id: {pane_id}"),
                },
            },
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

        fn xterm256_to_rgb(i: u8) -> u32 {
            // Standard xterm 256-color palette
            match i {
                0..=15 => [
                    0x00_00_00, 0xCC_00_00, 0x00_CC_00, 0xCC_CC_00, 0x00_00_CC, 0xCC_00_CC,
                    0x00_CC_CC, 0xCC_CC_CC, 0x55_55_55, 0xFF_55_55, 0x55_FF_55, 0xFF_FF_55,
                    0x55_55_FF, 0xFF_55_FF, 0x55_FF_FF, 0xFF_FF_FF,
                ][i as usize],
                16..=231 => {
                    let v = i - 16;
                    let b = (v % 6) * 51;
                    let g = ((v / 6) % 6) * 51;
                    let r = (v / 36) * 51;
                    (r as u32) << 16 | (g as u32) << 8 | b as u32
                }
                232..=255 => {
                    let gray = 8 + (i - 232) * 10;
                    (gray as u32) << 16 | (gray as u32) << 8 | gray as u32
                }
            }
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
                            let to_rgb = |c: &TermColor, default: u32| -> u32 {
                                match c {
                                    TermColor::Rgb(r, g, b) => {
                                        (*r as u32) << 16 | (*g as u32) << 8 | *b as u32
                                    }
                                    TermColor::Black => 0x00_00_00,
                                    TermColor::Red => 0xCC_00_00,
                                    TermColor::Green => 0x00_CC_00,
                                    TermColor::Yellow => 0xCC_CC_00,
                                    TermColor::Blue => 0x00_00_CC,
                                    TermColor::Magenta => 0xCC_00_CC,
                                    TermColor::Cyan => 0x00_CC_CC,
                                    TermColor::White => 0xCC_CC_CC,
                                    TermColor::BrightBlack => 0x55_55_55,
                                    TermColor::BrightRed => 0xFF_55_55,
                                    TermColor::BrightGreen => 0x55_FF_55,
                                    TermColor::BrightYellow => 0xFF_FF_55,
                                    TermColor::BrightBlue => 0x55_55_FF,
                                    TermColor::BrightMagenta => 0xFF_55_FF,
                                    TermColor::BrightCyan => 0x55_FF_FF,
                                    TermColor::BrightWhite => 0xFF_FF_FF,
                                    TermColor::Indexed(i) => xterm256_to_rgb(*i),
                                    TermColor::Default => default,
                                }
                            };
                            WebCell {
                                ch: cell.character,
                                fg: to_rgb(&cell.fg, hom_web::DEFAULT_FG_SENTINEL),
                                bg: to_rgb(&cell.bg, hom_web::DEFAULT_BG_SENTINEL),
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

        if let Err(e) = tx.send(WebFrame::new(panes)) {
            debug!(error = %e, "web frame dropped because there are no active receivers");
        }
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
                    if self.panes.contains_key(&id) {
                        let bytes = self
                            .translate_input_for_pane(id, &OrchestratorCommand::Prompt(input.text))
                            .unwrap_or_default();
                        if let Err(e) = self.pty_write(id, &bytes) {
                            tracing::warn!(pane_id = %id, "web input: PTY write failed: {e}");
                        }
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
        self.remote_ptys.kill_all();
        self.panes.clear();
        self.pane_order.clear();
        info!("app shutdown complete");
    }
}

/// Serializable pane configuration for session save/restore.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionPaneConfig {
    pub harness_type: HarnessType,
    #[serde(default)]
    pub plugin_name: Option<String>,
    pub model: Option<String>,
    #[serde(default = "default_session_pane_kind")]
    pub pane_kind: PaneKind,
    #[serde(default = "default_session_working_dir")]
    pub working_dir: PathBuf,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_session_pane_kind() -> PaneKind {
    PaneKind::Local
}

fn default_session_working_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| ".".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_bridge::WorkflowLauncher;

    #[test]
    fn app_has_remote_pty_manager() {
        let cfg = hom_core::HomConfig::default();
        let app = App::new(cfg);
        assert_eq!(app.remote_ptys.active_panes().len(), 0);
    }

    #[test]
    fn test_session_pane_config_roundtrip() {
        let configs = vec![
            SessionPaneConfig {
                harness_type: HarnessType::ClaudeCode,
                plugin_name: None,
                model: Some("opus".to_string()),
                pane_kind: PaneKind::Local,
                working_dir: PathBuf::from("/tmp/one"),
                extra_args: vec!["--verbose".to_string()],
            },
            SessionPaneConfig {
                harness_type: HarnessType::CodexCli,
                plugin_name: Some("custom-plugin".to_string()),
                model: None,
                pane_kind: PaneKind::Remote(RemoteTarget {
                    user: "alice".into(),
                    host: "example.com".into(),
                    port: 22,
                }),
                working_dir: PathBuf::from("/tmp/two"),
                extra_args: Vec::new(),
            },
        ];
        let json = serde_json::to_string(&configs).unwrap();
        let parsed: Vec<SessionPaneConfig> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].harness_type, HarnessType::ClaudeCode);
        assert_eq!(parsed[0].working_dir, PathBuf::from("/tmp/one"));
        assert_eq!(parsed[0].extra_args, vec!["--verbose".to_string()]);
        assert_eq!(parsed[0].model, Some("opus".to_string()));
        assert_eq!(parsed[1].plugin_name, Some("custom-plugin".to_string()));
        assert_eq!(parsed[1].model, None);
        assert!(matches!(parsed[1].pane_kind, PaneKind::Remote(_)));
    }

    #[test]
    fn test_session_snapshot_empty_app() {
        let app = App::new(HomConfig::default());
        let (layout, panes) = app.session_snapshot().unwrap();
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

    #[test]
    fn spawn_pane_with_opts_rejects_when_max_panes_reached() {
        let mut config = HomConfig::default();
        config.general.max_panes = 0;
        let mut app = App::new(config);

        let err = app
            .spawn_pane_with_opts(PaneSpawnRequest {
                harness: Some(HarnessType::ClaudeCode),
                harness_name: "claude".to_string(),
                model: None,
                working_dir: None,
                extra_args: Vec::new(),
                cols: 80,
                rows: 24,
            })
            .unwrap_err();

        assert!(matches!(err, hom_core::HomError::MaxPanesReached(0)));
    }

    #[test]
    fn kill_pane_returns_not_found_for_unknown_id() {
        let mut app = App::new(HomConfig::default());
        let err = app.kill_pane(999).unwrap_err();
        assert!(matches!(err, hom_core::HomError::PaneNotFound(999)));
    }

    #[test]
    fn handle_load_plugin_nonexistent_sets_error() {
        let cfg = hom_core::HomConfig::default();
        let mut app = App::new(cfg);
        app.handle_load_plugin(std::path::Path::new("/nonexistent.dylib"));
        assert!(app.command_bar.last_error.is_some());
        let err = app.command_bar.last_error.as_ref().unwrap();
        assert!(
            err.contains("plugin") || err.contains("load") || err.contains("nonexistent"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn list_panes_reports_plugin_identity() {
        let mut app = App::new(HomConfig::default());
        app.panes.insert(
            7,
            Pane {
                id: 7,
                harness_type: HarnessType::ClaudeCode,
                pane_kind: PaneKind::Local,
                plugin_name: Some("demo-plugin".to_string()),
                model: None,
                working_dir: PathBuf::from("."),
                extra_args: Vec::new(),
                title: "Demo Plugin".to_string(),
                terminal: hom_terminal::create_terminal(80, 24, 100).unwrap(),
                pty_reader: None,
                sideband: None,
                exited: None,
            },
        );
        let response = app.execute_mcp_command(McpCommand::ListPanes);
        match response {
            McpResponse::ListPanes { panes } => {
                assert_eq!(panes.len(), 1);
                assert_eq!(panes[0].harness, "demo-plugin");
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    fn run_workflow_mcp_queues_launch_request() {
        let dir = tempfile::tempdir().unwrap();
        let workflow_path = dir.path().join("review.yaml");
        std::fs::write(
            &workflow_path,
            r#"
name: review
steps:
  - id: s1
    harness: claude
    prompt: "hello"
"#,
        )
        .unwrap();

        let mut app = App::new(HomConfig::default());
        let (launcher, mut rx) = WorkflowLauncher::new();
        app.workflow_launcher = Some(launcher);

        let response = app.execute_mcp_command(McpCommand::RunWorkflow {
            path: workflow_path.display().to_string(),
            vars: HashMap::from([(String::from("task"), String::from("demo"))]),
        });

        let workflow_id = match response {
            McpResponse::RunWorkflow { workflow_id } => workflow_id,
            other => panic!("unexpected response: {other:?}"),
        };

        let request = rx.try_recv().unwrap();
        assert_eq!(request.workflow_id, workflow_id);
        assert_eq!(request.definition_path, workflow_path.display().to_string());
        assert_eq!(request.def.name, "review");
        assert_eq!(
            request.variables.get("task").map(String::as_str),
            Some("demo")
        );
    }

    #[test]
    fn session_snapshot_preserves_plugin_and_remote_metadata() {
        let mut app = App::new(HomConfig::default());
        app.panes.insert(
            1,
            Pane {
                id: 1,
                harness_type: HarnessType::ClaudeCode,
                pane_kind: PaneKind::Local,
                plugin_name: Some("demo-plugin".to_string()),
                model: Some("v1".to_string()),
                working_dir: PathBuf::from("/tmp/plugin"),
                extra_args: vec!["--fast".to_string()],
                title: "Plugin".to_string(),
                terminal: hom_terminal::create_terminal(80, 24, 100).unwrap(),
                pty_reader: None,
                sideband: None,
                exited: None,
            },
        );
        app.panes.insert(
            2,
            Pane {
                id: 2,
                harness_type: HarnessType::CodexCli,
                pane_kind: PaneKind::Remote(RemoteTarget {
                    user: "alice".into(),
                    host: "example.com".into(),
                    port: 2200,
                }),
                plugin_name: None,
                model: None,
                working_dir: PathBuf::from("/tmp/remote"),
                extra_args: vec!["--json".to_string()],
                title: "Remote".to_string(),
                terminal: hom_terminal::create_terminal(80, 24, 100).unwrap(),
                pty_reader: None,
                sideband: None,
                exited: None,
            },
        );
        app.pane_order = vec![1, 2];

        let (_, panes_json) = app.session_snapshot().unwrap();
        let configs: Vec<SessionPaneConfig> = serde_json::from_str(&panes_json).unwrap();

        assert_eq!(configs[0].plugin_name, Some("demo-plugin".to_string()));
        assert_eq!(configs[0].working_dir, PathBuf::from("/tmp/plugin"));
        assert_eq!(configs[0].extra_args, vec!["--fast".to_string()]);
        assert!(matches!(configs[1].pane_kind, PaneKind::Remote(_)));
        assert_eq!(configs[1].working_dir, PathBuf::from("/tmp/remote"));
    }

    #[test]
    fn publish_web_frame_uses_distinct_default_color_sentinels() {
        let mut app = App::new(HomConfig::default());
        let mut terminal = hom_terminal::create_terminal(1, 1, 10).unwrap();
        terminal.process(b"A");
        app.panes.insert(
            1,
            Pane {
                id: 1,
                harness_type: HarnessType::ClaudeCode,
                pane_kind: PaneKind::Local,
                plugin_name: None,
                model: None,
                working_dir: PathBuf::from("."),
                extra_args: Vec::new(),
                title: "One".to_string(),
                terminal,
                pty_reader: None,
                sideband: None,
                exited: None,
            },
        );
        let (tx, mut rx) = tokio::sync::broadcast::channel(1);
        app.web_tx = Some(tx);

        if let Some(pane) = app.panes.get_mut(&1) {
            pane.terminal = hom_terminal::create_terminal(1, 1, 10).unwrap();
            pane.terminal.process(b"A");
        }

        app.publish_web_frame();
        let frame = rx.try_recv().unwrap();
        assert_eq!(frame.panes[0].cells[0].fg, hom_web::DEFAULT_FG_SENTINEL);
        assert_eq!(frame.panes[0].cells[0].bg, hom_web::DEFAULT_BG_SENTINEL);
    }
}
