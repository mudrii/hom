//! HOM — AI Harness Orchestration Management TUI
//!
//! A Rust-based terminal multiplexer that spawns real AI coding harness TUIs
//! in visual panes, coordinates inputs/outputs, and executes workflows.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use hom_core::{HarnessType, HomConfig, LayoutKind, TerminalBackend};
use hom_mcp::McpServer;
use hom_pty::{ConnectedRemotePty, RemotePtyManager};
use hom_tui::app::{App, PaneSpawnRequest, PreparedRemotePaneSpawn, RemotePaneSpawnRequest};
use hom_tui::input::Action;
use hom_tui::render::render;
use hom_tui::workflow_bridge::{
    WorkflowBridge, WorkflowLaunchRx, WorkflowLauncher, WorkflowRequest, WorkflowRequestRx,
};
use hom_tui::workflow_progress::WorkflowProgress;

struct RunAppContext {
    allow_terminal_input: bool,
    tick_rate: Duration,
    workflow_rx: WorkflowRequestRx,
    workflow_launch_rx: WorkflowLaunchRx,
    bridge: Arc<WorkflowBridge>,
    workflow_launcher: WorkflowLauncher,
    remote_spawn_tx: mpsc::UnboundedSender<RemoteSpawnCompletion>,
    remote_spawn_rx: mpsc::UnboundedReceiver<RemoteSpawnCompletion>,
    remote_spawn_tasks: Vec<tokio::task::JoinHandle<()>>,
}

struct RemoteSpawnCompletion {
    prepared: PreparedRemotePaneSpawn,
    result: hom_core::HomResult<ConnectedRemotePty>,
}

struct BackgroundTask {
    name: &'static str,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: tokio::task::JoinHandle<()>,
}

impl BackgroundTask {
    async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        match tokio::time::timeout(Duration::from_secs(2), &mut self.handle).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!(task = self.name, error = %err, "background task exited with error");
            }
            Err(_) => {
                warn!(
                    task = self.name,
                    "background task did not exit after shutdown signal; aborting"
                );
                self.handle.abort();
                let _ = self.handle.await;
            }
        }
    }
}

async fn shutdown_background_tasks(tasks: Vec<BackgroundTask>) {
    for task in tasks {
        task.shutdown().await;
    }
}

#[derive(Parser)]
#[command(name = "hom", version, about = "AI Harness Orchestrator TUI")]
struct Cli {
    /// Path to config file (default: ~/.config/hom/config.toml)
    #[arg(short, long)]
    config: Option<String>,

    /// Run a workflow immediately
    #[arg(short, long)]
    run: Option<String>,

    /// Set workflow variables (key=value)
    #[arg(long = "var", value_parser = parse_var)]
    vars: Vec<(String, String)>,

    /// Run without database (disables session save, cost tracking, workflow checkpoints)
    #[arg(long)]
    no_db: bool,

    /// Run as an MCP server (JSON-RPC 2.0 over stdin/stdout).
    /// When enabled, the TUI renders on stderr so stdout stays protocol-clean.
    #[arg(long)]
    mcp: bool,

    /// Serve a live browser view at http://localhost:<web-port>
    #[arg(long)]
    web: bool,

    /// Port for the web viewer (default 4242)
    #[arg(long, default_value_t = 4242)]
    web_port: u16,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

fn parse_var(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("invalid variable format: {s} (expected key=value)"));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn clamp_terminal_dims(cols: u16, rows: u16) -> (u16, u16) {
    (cols.max(1), rows.max(1))
}

fn pane_inner_dims(area: ratatui::layout::Rect) -> (u16, u16) {
    clamp_terminal_dims(area.width.saturating_sub(2), area.height.saturating_sub(2))
}

fn queue_remote_spawn(
    app: &mut App,
    request: RemotePaneSpawnRequest,
    remote_spawn_tx: &mpsc::UnboundedSender<RemoteSpawnCompletion>,
    remote_spawn_tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) -> hom_core::HomResult<()> {
    let prepared = app.prepare_remote_pane_spawn(request)?;
    let tx = remote_spawn_tx.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let result = RemotePtyManager::connect(
            prepared.target.clone(),
            prepared.command.clone(),
            prepared.env.clone(),
            prepared.cols,
            prepared.rows,
            prepared.auth_methods.clone(),
        );
        let _ = tx.send(RemoteSpawnCompletion { prepared, result });
    });
    remote_spawn_tasks.push(handle);
    Ok(())
}

async fn shutdown_remote_spawn_tasks(tasks: Vec<tokio::task::JoinHandle<()>>) {
    for mut task in tasks {
        match tokio::time::timeout(Duration::from_secs(1), &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!(error = %err, "remote spawn task exited with error");
            }
            Err(_) => {
                warn!("remote spawn task still running during shutdown");
                task.abort();
                let _ = task.await;
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let mut background_tasks = Vec::new();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cli.log_level)),
        )
        .with_writer(io::stderr)
        .init();

    info!("starting HOM");

    // Load configuration
    let config = if let Some(path) = &cli.config {
        HomConfig::load_from(std::path::Path::new(path))?
    } else {
        HomConfig::load()?
    };

    // Set up terminal
    enable_raw_mode()?;
    let mut ui_stream: Box<dyn io::Write> = if cli.mcp {
        Box::new(io::stderr())
    } else {
        Box::new(io::stdout())
    };
    execute!(ui_stream, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(ui_stream);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Create app
    let mut app = App::new(config);

    // Start MCP server if --mcp flag is set.
    // The server reads JSON-RPC 2.0 from stdin and writes responses to stdout.
    // It communicates with the TUI app via an mpsc channel.
    if cli.mcp {
        let (mcp_tx, mcp_rx) = tokio::sync::mpsc::channel(64);
        app.mcp_rx = Some(mcp_rx);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            McpServer::new(mcp_tx)
                .run_until_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        background_tasks.push(BackgroundTask {
            name: "mcp-server",
            shutdown_tx: Some(shutdown_tx),
            handle,
        });
        tracing::info!("MCP server started (JSON-RPC 2.0 on stdin/stdout)");
    }

    // Start web viewer if --web flag is set.
    // Serves a Canvas2D live view of all panes at http://localhost:<web-port>.
    if cli.web {
        // Initial receiver dropped — WebSocket clients subscribe via tx.subscribe() at connect time.
        let (web_tx, _) = tokio::sync::broadcast::channel(8);
        let (web_input_tx, web_input_rx) = tokio::sync::mpsc::channel(64);
        app.web_tx = Some(web_tx.clone());
        app.web_input_rx = Some(web_input_rx);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            if let Err(e) = hom_web::WebServer::new(cli.web_port, web_tx, web_input_tx)
                .run_until_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
            {
                tracing::error!("Web server failed: {e}");
            }
        });
        background_tasks.push(BackgroundTask {
            name: "web-server",
            shutdown_tx: Some(shutdown_tx),
            handle,
        });
        tracing::info!("Web view at http://localhost:{}", cli.web_port);
    }

    // Validate keybinding strings — warn the user about any invalid entries.
    // Invalid entries silently fall back to defaults, which is confusing.
    {
        let errors = hom_tui::input::validate_keybindings(&app.config.keybindings);
        if !errors.is_empty() {
            let msg = errors.join("; ");
            warn!(keybinding_errors = %msg, "invalid keybinding config");
            app.command_bar.last_error = Some(format!("keybinding config warning: {msg}"));
        }
    }

    // Open database (required unless --no-db)
    if cli.no_db {
        info!("running without database (--no-db)");
        app.command_bar.last_error = Some("running without database (--no-db)".to_string());
    } else {
        let db_path = app.config.db_path();
        let db = match hom_db::HomDb::open_path(&db_path).await {
            Ok(db) => db,
            Err(e) => {
                disable_raw_mode()?;
                execute!(
                    terminal.backend_mut(),
                    DisableMouseCapture,
                    LeaveAlternateScreen
                )?;
                terminal.show_cursor()?;
                return Err(anyhow::anyhow!(
                    "Failed to open database at {}: {e}\n\
                     Use --no-db to run without persistence.",
                    db_path.display()
                ));
            }
        };
        let db = std::sync::Arc::new(db);
        app.db = Some(db.clone());
        info!(path = %db_path.display(), "database opened");
    }

    // Use render FPS from config
    let fps = app.config.general.render_fps.max(1);
    let tick_rate = Duration::from_millis(1000 / fps as u64);

    // Create workflow bridge channel for executor ↔ TUI communication
    let (bridge, workflow_rx) = WorkflowBridge::new();
    let bridge = Arc::new(bridge);
    let (workflow_launcher, workflow_launch_rx) = WorkflowLauncher::new();
    app.workflow_launcher = Some(workflow_launcher.clone());
    let (remote_spawn_tx, remote_spawn_rx) = mpsc::unbounded_channel();

    // Wire CLI --run/--var: if a workflow was specified, launch it
    if let Some(workflow_name) = &cli.run {
        let workflow_dir = app.config.workflow_dir();
        let workflow_path = workflow_dir.join(format!("{workflow_name}.yaml"));
        match hom_workflow::WorkflowDef::from_file(&workflow_path) {
            Ok(def) => {
                app.workflow_progress = Some(WorkflowProgress::new(
                    workflow_name.to_string(),
                    def.steps.iter().map(|s| s.id.clone()).collect(),
                ));
                let variables: HashMap<String, String> = cli.vars.iter().cloned().collect();
                match workflow_launcher.launch(def, variables, workflow_path.display().to_string())
                {
                    Ok(workflow_id) => {
                        info!(
                            workflow = %workflow_name,
                            workflow_id,
                            vars = ?cli.vars,
                            "workflow launched via CLI"
                        );
                    }
                    Err(e) => {
                        app.command_bar.last_error = Some(format!("workflow launch error: {e}"));
                    }
                }
            }
            Err(e) => {
                warn!(workflow = %workflow_name, error = %e, "failed to load CLI workflow");
                app.command_bar.last_error = Some(format!("workflow load error: {e}"));
            }
        }
    }

    let mut ctx = RunAppContext {
        allow_terminal_input: !cli.mcp,
        tick_rate,
        workflow_rx,
        workflow_launch_rx,
        bridge,
        workflow_launcher,
        remote_spawn_tx,
        remote_spawn_rx,
        remote_spawn_tasks: Vec::new(),
    };
    let result = run_app(&mut terminal, &mut app, &mut ctx).await;

    shutdown_remote_spawn_tasks(std::mem::take(&mut ctx.remote_spawn_tasks)).await;

    shutdown_background_tasks(background_tasks).await;

    // Clean up all PTY processes before restoring the terminal
    app.shutdown();

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    info!("HOM exited");
    Ok(())
}

async fn run_app<W: io::Write>(
    terminal: &mut Terminal<CrosstermBackend<W>>,
    app: &mut App,
    ctx: &mut RunAppContext,
) -> anyhow::Result<()> {
    // Cost polling: query total_cost from DB roughly every second.
    let (cost_tx, mut cost_rx) = tokio::sync::mpsc::unbounded_channel::<f64>();
    let fps = app.config.general.render_fps.max(1) as u64;
    let cost_poll_interval = fps; // poll every `fps` ticks ≈ 1 second
    let mut cost_tick_counter: u64 = 0;

    // Sideband health polling: check every ~5 seconds (5 * fps ticks).
    let health_poll_interval = fps * 5;
    let mut health_tick_counter: u64 = 0;
    let (health_tx, mut health_rx) = tokio::sync::mpsc::unbounded_channel::<(u32, String, bool)>();

    loop {
        while let Ok(completion) = ctx.remote_spawn_rx.try_recv() {
            match completion.result {
                Ok(connected) => {
                    match app.complete_remote_pane_spawn(completion.prepared, connected) {
                        Ok(pane_id) => info!(pane_id, "remote pane spawned"),
                        Err(e) => {
                            app.command_bar.last_error = Some(format!("remote spawn failed: {e}"));
                            warn!(error = %e, "failed to finalize remote pane spawn");
                        }
                    }
                }
                Err(e) => {
                    app.remote_spawn_failed();
                    app.command_bar.last_error = Some(format!("remote spawn failed: {e}"));
                    warn!(error = %e, "remote pane connect failed");
                }
            }
        }
        ctx.remote_spawn_tasks.retain(|task| !task.is_finished());

        // Forward any browser keystrokes to the focused pane before rendering.
        app.handle_web_input();

        // Draw
        terminal.draw(|frame| {
            render(frame, app);
        })?;

        // Broadcast current pane state to WebSocket clients after rendering.
        app.publish_web_frame();

        // Drain workflow bridge requests (non-blocking)
        while let Ok(req) = ctx.workflow_rx.try_recv() {
            handle_workflow_request(app, req, terminal.size()?.into());
        }

        while let Ok(req) = ctx.workflow_launch_rx.try_recv() {
            let db = app.db.clone();
            let bridge = ctx.bridge.clone();
            tokio::spawn(async move {
                run_workflow_task(
                    req.def,
                    bridge,
                    req.variables,
                    db,
                    &req.definition_path,
                    Some(req.workflow_id),
                )
                .await;
            });
        }

        // Dispatch pending MCP requests (up to 16 per tick)
        app.handle_mcp_requests();

        // Poll for events
        if ctx.allow_terminal_input && event::poll(ctx.tick_rate)? {
            let evt = event::read()?;

            // Build pane areas for hit testing
            let term_size = terminal.size()?;
            let size = ratatui::layout::Rect::new(0, 0, term_size.width, term_size.height);
            let pane_areas: Vec<_> = hom_tui::layout::compute_pane_areas(
                ratatui::layout::Rect {
                    x: 0,
                    y: 1,
                    width: size.width,
                    height: size.height.saturating_sub(4),
                },
                &app.pane_order,
                &app.layout,
            );

            // Handle terminal resize events before routing
            if let Event::Resize(new_cols, new_rows) = &evt {
                // Recompute per-pane areas using the layout engine, then resize each
                let pane_area = ratatui::layout::Rect {
                    x: 0,
                    y: 1,
                    width: *new_cols,
                    height: new_rows.saturating_sub(4),
                };
                let new_areas =
                    hom_tui::layout::compute_pane_areas(pane_area, &app.pane_order, &app.layout);
                for (pane_id, area) in &new_areas {
                    let (inner_w, inner_h) = pane_inner_dims(*area);
                    if app.remote_ptys.has_pane(*pane_id) {
                        if let Err(e) = app.remote_ptys.resize(*pane_id, inner_w, inner_h) {
                            warn!(pane_id, error = %e, "failed to resize remote PTY");
                        }
                    } else {
                        if let Err(e) = app.pty_manager.resize(*pane_id, inner_w, inner_h) {
                            warn!(pane_id, error = %e, "failed to resize PTY");
                        }
                    }
                    if let Some(pane) = app.panes.get_mut(pane_id) {
                        pane.terminal.resize(inner_w, inner_h);
                    }
                }
            }

            let action = app.input_router.handle_event(evt, &pane_areas);

            match action {
                Action::Quit => break,
                Action::WriteToPty(pane_id, bytes) => {
                    let _ = app.pty_write(pane_id, &bytes);
                }
                Action::FocusPane(pane_id) => {
                    app.focused_pane = Some(pane_id);
                }
                Action::FocusCommandBar => {
                    // Already handled by input router
                }
                Action::CommandBarInput(key) => {
                    if let Some(cmd) = app.command_bar.handle_key(key) {
                        let remote_spawn_tx = ctx.remote_spawn_tx.clone();
                        handle_command(
                            app,
                            cmd,
                            size,
                            &ctx.workflow_launcher,
                            &remote_spawn_tx,
                            &mut ctx.remote_spawn_tasks,
                        )?;
                    }
                }
                Action::NextPane => app.focus_next(),
                Action::PrevPane => app.focus_prev(),
                Action::KillPane(pane_id) => {
                    if let Err(e) = app.kill_pane(pane_id) {
                        warn!(pane_id, error = %e, "failed to kill pane");
                        app.command_bar.last_error = Some(format!("kill pane failed: {e}"));
                    }
                }
                Action::None => {}
            }
        }

        // Poll PTY output and feed into terminal emulators
        let token_events = app.poll_pty_output();

        // Log any token usage events to the database
        if !token_events.is_empty()
            && let Some(ref db) = app.db
        {
            for (pane_id, harness, event) in &token_events {
                if let hom_core::HarnessEvent::TokenUsage { input, output } = event {
                    let db = db.clone();
                    let harness = harness.clone();
                    let pane = *pane_id;
                    let inp = *input as i64;
                    let out = *output as i64;
                    tokio::spawn(async move {
                        let _ =
                            hom_db::cost::log_cost(db.pool(), pane, &harness, None, inp, out, 0.0)
                                .await;
                    });
                }
            }
        }

        // Check pending workflow completions (detect_completion polling)
        app.poll_pending_completions();

        // Poll total cost from the database periodically (~1 second)
        cost_tick_counter += 1;
        if cost_tick_counter >= cost_poll_interval {
            cost_tick_counter = 0;
            if let Some(ref db) = app.db {
                let db = db.clone();
                let tx = cost_tx.clone();
                tokio::spawn(async move {
                    if let Ok(cost) = hom_db::cost::total_cost(db.pool()).await {
                        let _ = tx.send(cost);
                    }
                });
            }
        }
        // Drain cost results into app state
        while let Ok(cost) = cost_rx.try_recv() {
            app.total_cost = cost;
        }

        // Poll sideband health every ~5 seconds
        health_tick_counter += 1;
        if health_tick_counter >= health_poll_interval {
            health_tick_counter = 0;
            for pane_id in &app.pane_order {
                if let Some(pane) = app.panes.get(pane_id)
                    && let Some(sideband) = &pane.sideband
                {
                    let sideband = sideband.clone();
                    let harness_name = app
                        .pane_display_name(*pane_id)
                        .unwrap_or_else(|| format!("pane #{pane_id}"));
                    let pid = *pane_id;
                    let tx = health_tx.clone();
                    tokio::spawn(async move {
                        let healthy = matches!(
                            tokio::time::timeout(Duration::from_secs(3), sideband.health_check())
                                .await,
                            Ok(Ok(true))
                        );
                        let _ = tx.send((pid, harness_name, healthy));
                    });
                }
            }
        }
        // Drain health check results — notify on failure
        while let Ok((pane_id, harness_name, healthy)) = health_rx.try_recv() {
            if !healthy {
                warn!(pane_id, harness_name, "sideband health check failed");
                app.command_bar.last_error = Some(format!(
                    "sideband for pane #{pane_id} ({harness_name}) is not responding"
                ));
            }
        }

        // Check for exited processes and handle them
        let exited_panes = app.handle_exited_panes();
        for (pane_id, exit_code) in &exited_panes {
            warn!(pane_id, exit_code, "harness process exited");

            // Notify the user in the command bar so they see it immediately,
            // even if they are not looking at the affected pane.
            let harness_name = app
                .pane_display_name(*pane_id)
                .unwrap_or_else(|| format!("pane #{pane_id}"));
            app.command_bar.last_error = Some(format!(
                "pane #{pane_id} ({harness_name}) exited with code {exit_code}"
            ));

            // Resolve any pending workflow completions for this pane
            let mut resolved_indices = Vec::new();
            for (i, pending) in app.pending_completions.iter().enumerate() {
                if pending.pane_id == *pane_id {
                    resolved_indices.push(i);
                }
            }
            for i in resolved_indices.into_iter().rev() {
                let pending = app.pending_completions.remove(i);
                let _ = pending.reply.send(Err(hom_core::HomError::Other(format!(
                    "harness process exited with code {exit_code}"
                ))));
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn handle_command(
    app: &mut App,
    cmd: hom_tui::command_bar::Command,
    terminal_size: ratatui::layout::Rect,
    workflow_launcher: &WorkflowLauncher,
    remote_spawn_tx: &mpsc::UnboundedSender<RemoteSpawnCompletion>,
    remote_spawn_tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) -> anyhow::Result<()> {
    use hom_tui::command_bar::Command;

    match cmd {
        Command::Spawn {
            harness,
            harness_name,
            model,
            working_dir,
            extra_args,
            remote,
        } => {
            if let Some(target) = remote {
                // Remote spawn requires a known built-in harness type.
                match harness {
                    Some(ht) => {
                        let (cols, rows) = app.focused_pane_dimensions();
                        match queue_remote_spawn(
                            app,
                            RemotePaneSpawnRequest {
                                harness_type: ht,
                                model,
                                working_dir,
                                extra_args,
                                target,
                                cols,
                                rows,
                            },
                            remote_spawn_tx,
                            remote_spawn_tasks,
                        ) {
                            Ok(()) => info!("queued remote pane spawn"),
                            Err(e) => {
                                app.command_bar.last_error =
                                    Some(format!("remote spawn failed: {e}"));
                            }
                        }
                    }
                    None => {
                        app.command_bar.last_error = Some(format!(
                            "remote spawn only supports built-in harnesses; plugin '{harness_name}' cannot be spawned remotely"
                        ));
                    }
                }
            } else {
                let (cols, rows) = clamp_terminal_dims(
                    terminal_size.width.saturating_sub(2),
                    terminal_size.height.saturating_sub(6),
                );
                match app.spawn_pane_with_opts(PaneSpawnRequest {
                    harness,
                    harness_name,
                    model,
                    working_dir,
                    extra_args,
                    cols,
                    rows,
                }) {
                    Ok(id) => info!(pane_id = id, "spawned pane"),
                    Err(e) => {
                        app.command_bar.last_error = Some(format!("{e}"));
                    }
                }
            }
        }
        Command::LoadPlugin { path } => {
            app.handle_load_plugin(&path);
        }
        Command::Kill(selector) => {
            if let Some(id) = resolve_selector(&selector, app)
                && let Err(e) = app.kill_pane(id)
            {
                app.command_bar.last_error = Some(format!("kill pane failed: {e}"));
                warn!(pane_id = id, error = %e, "failed to kill pane");
            }
        }
        Command::Focus(selector) => {
            if let Some(id) = resolve_selector(&selector, app) {
                app.focused_pane = Some(id);
                app.input_router.focus_pane(id);
            }
        }
        Command::Layout(kind) => {
            app.layout = kind;
            // Recompute pane areas and resize PTYs to match new layout
            let pane_area = ratatui::layout::Rect {
                x: 0,
                y: 1,
                width: terminal_size.width,
                height: terminal_size.height.saturating_sub(4),
            };
            let new_areas =
                hom_tui::layout::compute_pane_areas(pane_area, &app.pane_order, &app.layout);
            for (pane_id, area) in &new_areas {
                let (inner_w, inner_h) = pane_inner_dims(*area);
                if app.remote_ptys.has_pane(*pane_id) {
                    if let Err(e) = app.remote_ptys.resize(*pane_id, inner_w, inner_h) {
                        warn!(pane_id, error = %e, "failed to resize remote PTY");
                    }
                } else {
                    if let Err(e) = app.pty_manager.resize(*pane_id, inner_w, inner_h) {
                        warn!(pane_id, error = %e, "failed to resize PTY");
                    }
                }
                if let Some(pane) = app.panes.get_mut(pane_id) {
                    pane.terminal.resize(inner_w, inner_h);
                }
            }
        }
        Command::Quit => {
            app.should_quit = true;
        }
        Command::Help => {
            app.command_bar.last_error = Some(
                "commands: :spawn :kill :focus :send :pipe :broadcast :run :layout :save :restore :load-plugin :quit".to_string()
            );
        }
        Command::Send { target, input } => {
            if let Some(id) = resolve_selector(&target, app) {
                let bytes = app
                    .translate_input_for_pane(id, &hom_core::OrchestratorCommand::Prompt(input))
                    .unwrap_or_default();
                let _ = app.pty_write(id, &bytes);
                info!(pane_id = id, "sent input to pane");
            } else {
                app.command_bar.last_error = Some("pane not found".to_string());
            }
        }
        Command::Pipe { source, target } => handle_pipe(app, source, target)?,
        Command::Broadcast(msg) => {
            let pane_ids: Vec<hom_core::PaneId> = app.pane_order.clone();
            for pane_id in &pane_ids {
                let bytes = app
                    .translate_input_for_pane(
                        *pane_id,
                        &hom_core::OrchestratorCommand::Prompt(msg.clone()),
                    )
                    .unwrap_or_default();
                let _ = app.pty_write(*pane_id, &bytes);
            }
            info!(
                pane_count = app.pane_order.len(),
                "broadcast sent to all panes"
            );
        }
        Command::Run {
            workflow,
            variables,
        } => handle_run(app, workflow, variables, workflow_launcher)?,
        Command::Save(name) => handle_save(app, name),
        Command::Restore(name) => handle_restore(
            app,
            name,
            terminal_size,
            remote_spawn_tx,
            remote_spawn_tasks,
        ),
    }

    Ok(())
}

fn resolve_selector(selector: &hom_tui::command_bar::PaneSelector, app: &App) -> Option<u32> {
    match selector {
        hom_tui::command_bar::PaneSelector::Id(id) => {
            if app.panes.contains_key(id) {
                Some(*id)
            } else {
                None
            }
        }
        hom_tui::command_bar::PaneSelector::Name(name) => app
            .panes
            .iter()
            .find(|(_, p)| p.title.to_lowercase().contains(&name.to_lowercase()))
            .map(|(id, _)| *id),
    }
}

fn handle_pipe(
    app: &mut App,
    source: hom_tui::command_bar::PaneSelector,
    target: hom_tui::command_bar::PaneSelector,
) -> anyhow::Result<()> {
    // Pipe: extract structured output from source pane, write to target PTY.
    // Uses adapter's parse_screen() for structured events when available,
    // falls back to raw screen text otherwise.
    let source_id = resolve_selector(&source, app);
    let target_id = resolve_selector(&target, app);
    match (source_id, target_id) {
        (Some(src), Some(tgt)) => {
            let output = if let Some(pane) = app.panes.get(&src) {
                let events = app.parse_screen_for_pane(src);
                if events.is_empty() {
                    // Fallback: use last N lines of raw screen text
                    // (avoids sending blank padding and scroll history)
                    pane.terminal.screen_snapshot().last_n_lines(20)
                } else {
                    // Format structured events as newline-separated text
                    events
                        .iter()
                        .map(|e| format!("{e:?}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            } else {
                String::new()
            };

            let bytes = app
                .translate_input_for_pane(tgt, &hom_core::OrchestratorCommand::Prompt(output))
                .unwrap_or_default();
            let _ = app.pty_write(tgt, &bytes);
            info!(source = src, target = tgt, "piped output between panes");
        }
        _ => {
            app.command_bar.last_error = Some("source or target pane not found".to_string());
        }
    }
    Ok(())
}

fn handle_run(
    app: &mut App,
    workflow: String,
    variables: HashMap<String, String>,
    workflow_launcher: &WorkflowLauncher,
) -> anyhow::Result<()> {
    // Load workflow from config workflow dir
    let workflow_dir = app.config.workflow_dir();
    let workflow_path = workflow_dir.join(format!("{workflow}.yaml"));
    if workflow_path.exists() {
        match hom_workflow::parser::WorkflowDef::from_file(&workflow_path) {
            Ok(def) => {
                app.workflow_progress = Some(WorkflowProgress::new(
                    workflow.clone(),
                    def.steps.iter().map(|s| s.id.clone()).collect(),
                ));
                info!(
                    workflow = %workflow,
                    steps = def.steps.len(),
                    vars = ?variables,
                    "workflow loaded, launching executor"
                );
                match workflow_launcher.launch(def, variables, workflow_path.display().to_string())
                {
                    Ok(workflow_id) => {
                        info!(workflow = %workflow, workflow_id, "workflow queued");
                    }
                    Err(e) => {
                        app.command_bar.last_error = Some(format!("workflow launch error: {e}"));
                    }
                }
            }
            Err(e) => {
                app.command_bar.last_error = Some(format!("workflow parse error: {e}"));
            }
        }
    } else {
        app.command_bar.last_error =
            Some(format!("workflow not found: {}", workflow_path.display()));
    }
    Ok(())
}

fn handle_save(app: &mut App, name: String) {
    if let Some(ref db) = app.db {
        let (layout_json, panes_json) = match app.session_snapshot() {
            Ok(snapshot) => snapshot,
            Err(e) => {
                app.command_bar.last_error = Some(format!("session snapshot failed: {e}"));
                warn!(error = %e, "session snapshot failed");
                return;
            }
        };
        let session_id = uuid::Uuid::new_v4().to_string();
        let db = db.clone();
        let name_clone = name.clone();
        tokio::spawn(async move {
            if let Err(e) = hom_db::session::save_session(
                db.pool(),
                &session_id,
                &name_clone,
                &layout_json,
                &panes_json,
            )
            .await
            {
                warn!(error = %e, "session save failed");
            }
        });
        app.command_bar.last_error = Some(format!("session '{name}' saved"));
        info!(session = %name, "session saved");
    } else {
        app.command_bar.last_error = Some("no database available for session save".to_string());
    }
}

fn handle_restore(
    app: &mut App,
    name: String,
    terminal_size: ratatui::layout::Rect,
    remote_spawn_tx: &mpsc::UnboundedSender<RemoteSpawnCompletion>,
    remote_spawn_tasks: &mut Vec<tokio::task::JoinHandle<()>>,
) {
    if let Some(ref db) = app.db {
        let db = db.clone();
        let name_clone = name.clone();
        // Load session synchronously enough to get pane configs.
        // We use block_in_place since we need the result immediately.
        let load_result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(hom_db::session::load_session(db.pool(), &name_clone))
        });
        match load_result {
            Ok(Some((layout_json, panes_json))) => {
                // Restore layout
                if let Ok(layout) = serde_json::from_str::<LayoutKind>(&layout_json) {
                    app.layout = layout;
                }
                // Restore panes
                if let Ok(pane_configs) =
                    serde_json::from_str::<Vec<hom_tui::app::SessionPaneConfig>>(&panes_json)
                {
                    let (cols, rows) = clamp_terminal_dims(
                        terminal_size.width.saturating_sub(2),
                        terminal_size.height.saturating_sub(6),
                    );
                    for pc in &pane_configs {
                        let result = match (&pc.plugin_name, &pc.pane_kind) {
                            (Some(plugin_name), hom_core::PaneKind::Local) => app
                                .spawn_pane_with_opts(PaneSpawnRequest {
                                    harness: None,
                                    harness_name: plugin_name.clone(),
                                    model: pc.model.clone(),
                                    working_dir: Some(pc.working_dir.clone()),
                                    extra_args: pc.extra_args.clone(),
                                    cols,
                                    rows,
                                }),
                            (None, hom_core::PaneKind::Local) => {
                                app.spawn_pane_with_opts(PaneSpawnRequest {
                                    harness: Some(pc.harness_type),
                                    harness_name: pc.harness_type.default_binary().to_string(),
                                    model: pc.model.clone(),
                                    working_dir: Some(pc.working_dir.clone()),
                                    extra_args: pc.extra_args.clone(),
                                    cols,
                                    rows,
                                })
                            }
                            (None, hom_core::PaneKind::Remote(target)) => queue_remote_spawn(
                                app,
                                RemotePaneSpawnRequest {
                                    harness_type: pc.harness_type,
                                    model: pc.model.clone(),
                                    working_dir: Some(pc.working_dir.clone()),
                                    extra_args: pc.extra_args.clone(),
                                    target: target.clone(),
                                    cols,
                                    rows,
                                },
                                remote_spawn_tx,
                                remote_spawn_tasks,
                            )
                            .map(|()| 0),
                            (Some(plugin_name), hom_core::PaneKind::Remote(_)) => {
                                Err(hom_core::HomError::Other(format!(
                                    "cannot restore remote plugin pane '{plugin_name}'"
                                )))
                            }
                        };
                        if let Err(e) = result {
                            warn!(error = %e, "failed to restore pane");
                        }
                    }
                    app.command_bar.last_error = Some(format!(
                        "session '{name}' restored ({} panes)",
                        pane_configs.len()
                    ));
                }
                info!(session = %name, "session restored");
            }
            Ok(None) => {
                app.command_bar.last_error = Some(format!("session '{name}' not found"));
            }
            Err(e) => {
                app.command_bar.last_error = Some(format!("session restore failed: {e}"));
            }
        }
    } else {
        app.command_bar.last_error = Some("no database available for session restore".to_string());
    }
}

/// Handle a workflow bridge request from the executor task.
fn handle_workflow_request(
    app: &mut App,
    req: WorkflowRequest,
    terminal_size: ratatui::layout::Rect,
) {
    match req {
        WorkflowRequest::SpawnPane {
            harness,
            model,
            reply,
        } => {
            let harness_type = HarnessType::from_str_loose(&harness);
            let result = match harness_type {
                Some(ht) => {
                    let (cols, rows) = clamp_terminal_dims(
                        terminal_size.width.saturating_sub(2),
                        terminal_size.height.saturating_sub(6),
                    );
                    app.spawn_pane(ht, model, cols, rows)
                }
                None => Err(hom_core::HomError::Other(format!(
                    "unknown harness: {harness}"
                ))),
            };
            let _ = reply.send(result);
        }
        WorkflowRequest::SendAndWait {
            pane_id,
            prompt,
            timeout,
            reply,
        } => {
            if let Some(pane) = app.panes.get(&pane_id) {
                if let Some(sideband) = pane.sideband.clone() {
                    // Sideband available — spawn async task for direct prompt/response.
                    // This bypasses PTY write and completion polling entirely.
                    tokio::spawn(async move {
                        let result =
                            tokio::time::timeout(timeout, sideband.send_prompt(&prompt)).await;
                        let response = match result {
                            Ok(Ok(output)) => Ok(output),
                            Ok(Err(e)) => Err(e),
                            Err(_) => Err(hom_core::HomError::WorkflowTimeout(timeout.as_secs())),
                        };
                        let _ = reply.send(response);
                    });
                } else {
                    // No sideband — write to PTY and register for completion polling.
                    let bytes = app
                        .translate_input_for_pane(
                            pane_id,
                            &hom_core::OrchestratorCommand::Prompt(prompt.clone()),
                        )
                        .unwrap_or_default();
                    match app.pty_write(pane_id, &bytes) {
                        Ok(()) => {
                            app.pending_completions
                                .push(hom_tui::app::PendingCompletion {
                                    pane_id,
                                    reply,
                                    started: std::time::Instant::now(),
                                    timeout,
                                });
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                        }
                    }
                }
            } else {
                let _ = reply.send(Err(hom_core::HomError::PaneNotFound(pane_id)));
            }
        }
        WorkflowRequest::KillPane { pane_id, reply } => {
            let result = app.kill_pane(pane_id);
            let _ = reply.send(result);
        }
        WorkflowRequest::StepUpdate { step_id, status } => {
            if let Some(ref mut progress) = app.workflow_progress {
                progress.update_step(&step_id, status);
            }
        }
    }
}

/// Run a workflow in a background task.
async fn run_workflow_task(
    def: hom_workflow::WorkflowDef,
    bridge: Arc<WorkflowBridge>,
    variables: HashMap<String, String>,
    db: Option<Arc<hom_db::HomDb>>,
    definition_path: &str,
    workflow_id: Option<String>,
) {
    let executor = hom_workflow::WorkflowExecutor::new();

    // Build checkpoint store if DB is available
    let checkpoint_store = db
        .as_ref()
        .map(|db| hom_tui::db_checkpoint::DbCheckpointStore::new(db.clone()));

    // Generate a single workflow ID used by both the DB row and the executor,
    // so that update_workflow_status targets the correct row.
    let wf_id = workflow_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Persist workflow start to DB
    if let Some(ref db) = db {
        let vars_json = serde_json::to_string(&variables).unwrap_or_default();
        if let Err(e) = hom_db::workflow::save_workflow(
            db.pool(),
            &wf_id,
            &def.name,
            definition_path,
            "running",
            &vars_json,
        )
        .await
        {
            warn!(error = %e, "failed to persist workflow start");
        }
    }

    let result = match &checkpoint_store {
        Some(store) => {
            executor
                .execute_with_id(&def, bridge, variables, Some(store), wf_id.clone())
                .await
        }
        None => executor.execute(&def, bridge, variables).await,
    };

    match result {
        Ok(wf_result) => {
            info!(
                workflow = %wf_result.name,
                status = ?wf_result.status,
                duration_ms = wf_result.duration.as_millis() as u64,
                "workflow completed"
            );
            if let Some(ref db) = db {
                let status_str = match wf_result.status {
                    hom_workflow::executor::WorkflowStatus::Completed => "completed",
                    hom_workflow::executor::WorkflowStatus::Aborted => "aborted",
                    hom_workflow::executor::WorkflowStatus::Failed { .. } => "failed",
                };
                let _ = hom_db::workflow::update_workflow_status(
                    db.pool(),
                    &wf_result.workflow_id,
                    status_str,
                    None,
                )
                .await;
            }
        }
        Err(e) => {
            if let Some(ref db) = db {
                let _ = hom_db::workflow::update_workflow_status(
                    db.pool(),
                    &wf_id,
                    "failed",
                    Some(&e.to_string()),
                )
                .await;
            }
            error!(workflow = %definition_path, error = %e, "workflow execution failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::sync::oneshot;

    use super::*;
    use hom_core::PaneKind;
    use hom_tui::command_bar::{Command, PaneSelector};

    fn test_app() -> App {
        App::new(HomConfig::default())
    }

    fn insert_test_pane(app: &mut App, id: u32, title: &str) {
        app.panes.insert(
            id,
            hom_tui::app::Pane {
                id,
                harness_type: HarnessType::ClaudeCode,
                pane_kind: PaneKind::Local,
                plugin_name: None,
                model: None,
                working_dir: PathBuf::from("."),
                extra_args: Vec::new(),
                title: title.to_string(),
                terminal: hom_terminal::create_terminal(20, 5, 10).unwrap(),
                pty_reader: None,
                sideband: None,
                exited: None,
            },
        );
        app.pane_order.push(id);
    }

    #[test]
    fn parse_var_accepts_values_containing_equals() {
        assert_eq!(
            parse_var("task=ship=it").unwrap(),
            ("task".to_string(), "ship=it".to_string())
        );
        assert!(parse_var("missing-delimiter").is_err());
    }

    #[test]
    fn clamp_terminal_dims_never_returns_zero() {
        assert_eq!(clamp_terminal_dims(0, 0), (1, 1));
        assert_eq!(clamp_terminal_dims(80, 0), (80, 1));
        assert_eq!(clamp_terminal_dims(0, 24), (1, 24));
    }

    #[test]
    fn pane_inner_dims_never_returns_zero() {
        assert_eq!(
            pane_inner_dims(ratatui::layout::Rect::new(0, 0, 1, 1)),
            (1, 1)
        );
        assert_eq!(
            pane_inner_dims(ratatui::layout::Rect::new(0, 0, 2, 2)),
            (1, 1)
        );
        assert_eq!(
            pane_inner_dims(ratatui::layout::Rect::new(0, 0, 10, 6)),
            (8, 4)
        );
    }

    #[test]
    fn resolve_selector_supports_id_and_case_insensitive_name() {
        let mut app = test_app();
        insert_test_pane(&mut app, 7, "Claude Review");

        assert_eq!(resolve_selector(&PaneSelector::Id(7), &app), Some(7));
        assert_eq!(
            resolve_selector(&PaneSelector::Name("review".to_string()), &app),
            Some(7)
        );
        assert_eq!(
            resolve_selector(&PaneSelector::Name("CLAUDE".to_string()), &app),
            Some(7)
        );
        assert_eq!(
            resolve_selector(&PaneSelector::Name("missing".to_string()), &app),
            None
        );
    }

    #[test]
    fn handle_command_sets_expected_local_error_states() {
        let mut app = test_app();
        let (launcher, _rx) = WorkflowLauncher::new();
        let (remote_spawn_tx, _remote_spawn_rx) = mpsc::unbounded_channel();
        let mut remote_spawn_tasks = Vec::new();
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);

        handle_command(
            &mut app,
            Command::Help,
            area,
            &launcher,
            &remote_spawn_tx,
            &mut remote_spawn_tasks,
        )
        .unwrap();
        assert!(
            app.command_bar
                .last_error
                .as_deref()
                .unwrap()
                .contains("commands:")
        );

        handle_command(
            &mut app,
            Command::Save("demo".to_string()),
            area,
            &launcher,
            &remote_spawn_tx,
            &mut remote_spawn_tasks,
        )
        .unwrap();
        assert_eq!(
            app.command_bar.last_error.as_deref(),
            Some("no database available for session save")
        );

        handle_command(
            &mut app,
            Command::Restore("demo".to_string()),
            area,
            &launcher,
            &remote_spawn_tx,
            &mut remote_spawn_tasks,
        )
        .unwrap();
        assert_eq!(
            app.command_bar.last_error.as_deref(),
            Some("no database available for session restore")
        );

        handle_command(
            &mut app,
            Command::Run {
                workflow: "missing-workflow".to_string(),
                variables: HashMap::new(),
            },
            area,
            &launcher,
            &remote_spawn_tx,
            &mut remote_spawn_tasks,
        )
        .unwrap();
        assert!(
            app.command_bar
                .last_error
                .as_deref()
                .unwrap()
                .contains("workflow not found:")
        );

        handle_command(
            &mut app,
            Command::Quit,
            area,
            &launcher,
            &remote_spawn_tx,
            &mut remote_spawn_tasks,
        )
        .unwrap();
        assert!(app.should_quit);
    }

    #[tokio::test]
    async fn handle_workflow_request_returns_errors_for_invalid_targets() {
        let mut app = test_app();
        let area = ratatui::layout::Rect::new(0, 0, 80, 24);

        let (unknown_tx, unknown_rx) = oneshot::channel();
        handle_workflow_request(
            &mut app,
            WorkflowRequest::SpawnPane {
                harness: "unknown-harness".to_string(),
                model: None,
                reply: unknown_tx,
            },
            area,
        );
        let err = unknown_rx.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("unknown harness"));

        let (missing_tx, missing_rx) = oneshot::channel();
        handle_workflow_request(
            &mut app,
            WorkflowRequest::SendAndWait {
                pane_id: 99,
                prompt: "hello".to_string(),
                timeout: Duration::from_secs(1),
                reply: missing_tx,
            },
            area,
        );
        let err = missing_rx.await.unwrap().unwrap_err().to_string();
        assert!(err.contains("pane") && err.contains("99"));
    }

    #[test]
    fn handle_workflow_request_updates_progress_state() {
        let mut app = test_app();
        app.workflow_progress = Some(WorkflowProgress::new(
            "demo".to_string(),
            vec!["plan".to_string(), "review".to_string()],
        ));

        handle_workflow_request(
            &mut app,
            WorkflowRequest::StepUpdate {
                step_id: "plan".to_string(),
                status: hom_tui::workflow_progress::StepProgress::Completed,
            },
            ratatui::layout::Rect::new(0, 0, 80, 24),
        );

        let summary = app.workflow_progress.as_ref().unwrap().summary();
        assert!(summary.contains("1/2"));
    }
}
