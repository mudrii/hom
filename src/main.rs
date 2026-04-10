//! HOM — AI Harness Orchestration Management TUI
//!
//! A Rust-based terminal multiplexer that spawns real AI coding harness TUIs
//! in visual panes, coordinates inputs/outputs, and executes workflows.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use hom_core::{HarnessType, HomConfig, LayoutKind, TerminalBackend};
use hom_tui::app::App;
use hom_tui::input::Action;
use hom_tui::render::render;
use hom_tui::workflow_bridge::{WorkflowBridge, WorkflowRequest, WorkflowRequestRx};
use hom_tui::workflow_progress::WorkflowProgress;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Create app
    let mut app = App::new(config);

    // Open database
    let db_path = app.config.db_path();
    match hom_db::HomDb::open(db_path.to_str().unwrap_or("hom.db")).await {
        Ok(db) => {
            let db = std::sync::Arc::new(db);
            app.db = Some(db.clone());
            info!(path = %db_path.display(), "database opened");
        }
        Err(e) => {
            // Non-fatal — run without persistence
            tracing::warn!(error = %e, "failed to open database, running without persistence");
        }
    }

    // Use render FPS from config
    let fps = app.config.general.render_fps.max(1);
    let tick_rate = Duration::from_millis(1000 / fps as u64);

    // Create workflow bridge channel for executor ↔ TUI communication
    let (bridge, workflow_rx) = WorkflowBridge::new();
    let bridge = Arc::new(bridge);

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
                let db = app.db.clone();
                let bridge_clone = bridge.clone();
                let wf_name = workflow_name.clone();
                tokio::spawn(async move {
                    run_workflow_task(def, bridge_clone, variables, db, &wf_name).await;
                });
                info!(workflow = %workflow_name, vars = ?cli.vars, "workflow launched via CLI");
            }
            Err(e) => {
                warn!(workflow = %workflow_name, error = %e, "failed to load CLI workflow");
                app.command_bar.last_error = Some(format!("workflow load error: {e}"));
            }
        }
    }

    let result = run_app(&mut terminal, &mut app, tick_rate, workflow_rx, bridge).await;

    // Clean up all PTY processes before restoring the terminal
    app.shutdown();

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }

    info!("HOM exited");
    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    tick_rate: Duration,
    mut workflow_rx: WorkflowRequestRx,
    bridge: Arc<WorkflowBridge>,
) -> anyhow::Result<()> {
    // Cost polling: query total_cost from DB roughly every second.
    let (cost_tx, mut cost_rx) = tokio::sync::mpsc::unbounded_channel::<f64>();
    let fps = app.config.general.render_fps.max(1) as u64;
    let cost_poll_interval = fps; // poll every `fps` ticks ≈ 1 second
    let mut cost_tick_counter: u64 = 0;

    loop {
        // Draw
        terminal.draw(|frame| {
            render(frame, app);
        })?;

        // Drain workflow bridge requests (non-blocking)
        while let Ok(req) = workflow_rx.try_recv() {
            handle_workflow_request(app, req, terminal.size()?.into());
        }

        // Poll for events
        if event::poll(tick_rate)? {
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
                    let inner_w = area.width.saturating_sub(2);
                    let inner_h = area.height.saturating_sub(2);
                    let _ = app.pty_manager.resize(*pane_id, inner_w, inner_h);
                    if let Some(pane) = app.panes.get_mut(pane_id) {
                        pane.terminal.resize(inner_w, inner_h);
                    }
                }
            }

            let action = app.input_router.handle_event(evt, &pane_areas);

            match action {
                Action::Quit => break,
                Action::WriteToPty(pane_id, bytes) => {
                    let _ = app.pty_manager.write_to(pane_id, &bytes);
                }
                Action::FocusPane(pane_id) => {
                    app.focused_pane = Some(pane_id);
                }
                Action::FocusCommandBar => {
                    // Already handled by input router
                }
                Action::CommandBarInput(key) => {
                    if let Some(cmd) = app.command_bar.handle_key(key) {
                        handle_command(app, cmd, size, &bridge)?;
                    }
                }
                Action::NextPane => app.focus_next(),
                Action::PrevPane => app.focus_prev(),
                Action::KillPane(pane_id) => {
                    let _ = app.kill_pane(pane_id);
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
            for (pane_id, harness_type, event) in &token_events {
                if let hom_core::HarnessEvent::TokenUsage { input, output } = event {
                    let db = db.clone();
                    let harness = harness_type.display_name().to_string();
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

        // Check for exited processes
        let pane_ids: Vec<_> = app.pane_order.clone();
        for pane_id in pane_ids {
            if let Ok(Some(_exit_code)) = app.pty_manager.try_wait(pane_id) {
                // Process exited — could mark pane or clean up
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
    bridge: &Arc<WorkflowBridge>,
) -> anyhow::Result<()> {
    use hom_tui::command_bar::Command;

    match cmd {
        Command::Spawn {
            harness,
            model,
            working_dir,
            extra_args,
        } => {
            let cols = terminal_size.width.saturating_sub(2);
            let rows = terminal_size.height.saturating_sub(6);
            match app.spawn_pane_with_opts(harness, model, working_dir, extra_args, cols, rows) {
                Ok(id) => info!(pane_id = id, "spawned pane"),
                Err(e) => {
                    app.command_bar.last_error = Some(format!("{e}"));
                }
            }
        }
        Command::Kill(selector) => {
            if let Some(id) = resolve_selector(&selector, app) {
                let _ = app.kill_pane(id);
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
                let inner_w = area.width.saturating_sub(2);
                let inner_h = area.height.saturating_sub(2);
                let _ = app.pty_manager.resize(*pane_id, inner_w, inner_h);
                if let Some(pane) = app.panes.get_mut(pane_id) {
                    use hom_core::TerminalBackend;
                    pane.terminal.resize(inner_w, inner_h);
                }
            }
        }
        Command::Quit => {
            app.should_quit = true;
        }
        Command::Help => {
            app.command_bar.last_error = Some(
                "commands: :spawn :kill :focus :send :pipe :broadcast :run :layout :save :restore :quit".to_string()
            );
        }
        Command::Send { target, input } => {
            if let Some(id) = resolve_selector(&target, app) {
                // Use adapter translation so the prompt is formatted correctly
                // for the target harness (e.g. proper escaping, newline appended).
                let bytes = if let Some(pane) = app.panes.get(&id) {
                    let adapter = app.adapter_registry.get(&pane.harness_type);
                    adapter
                        .map(|a| {
                            a.translate_input(&hom_core::OrchestratorCommand::Prompt(input.clone()))
                        })
                        .unwrap_or_else(|| format!("{input}\n").into_bytes())
                } else {
                    format!("{input}\n").into_bytes()
                };
                let _ = app.pty_manager.write_to(id, &bytes);
                info!(pane_id = id, "sent input to pane");
            } else {
                app.command_bar.last_error = Some("pane not found".to_string());
            }
        }
        Command::Pipe { source, target } => handle_pipe(app, source, target)?,
        Command::Broadcast(msg) => {
            for pane_id in &app.pane_order {
                // Use adapter translation per-pane so each harness gets correctly formatted input
                let bytes = if let Some(pane) = app.panes.get(pane_id) {
                    let adapter = app.adapter_registry.get(&pane.harness_type);
                    adapter
                        .map(|a| {
                            a.translate_input(&hom_core::OrchestratorCommand::Prompt(msg.clone()))
                        })
                        .unwrap_or_else(|| format!("{msg}\n").into_bytes())
                } else {
                    format!("{msg}\n").into_bytes()
                };
                let _ = app.pty_manager.write_to(*pane_id, &bytes);
            }
            info!(
                pane_count = app.pane_order.len(),
                "broadcast sent to all panes"
            );
        }
        Command::Run {
            workflow,
            variables,
        } => handle_run(app, workflow, variables, bridge)?,
        Command::Save(name) => handle_save(app, name),
        Command::Restore(name) => handle_restore(app, name, terminal_size),
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
                let snapshot = pane.terminal.screen_snapshot();
                // Try adapter's parse_screen() for structured output
                let events = app
                    .adapter_registry
                    .get(&pane.harness_type)
                    .map(|a| a.parse_screen(&snapshot))
                    .unwrap_or_default();
                if events.is_empty() {
                    // Fallback: use last N lines of raw screen text
                    // (avoids sending blank padding and scroll history)
                    snapshot.last_n_lines(20)
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

            // Use adapter translation for the target pane
            let bytes = if let Some(tgt_pane) = app.panes.get(&tgt) {
                let adapter = app.adapter_registry.get(&tgt_pane.harness_type);
                adapter
                    .map(|a| {
                        a.translate_input(&hom_core::OrchestratorCommand::Prompt(output.clone()))
                    })
                    .unwrap_or_else(|| format!("{output}\n").into_bytes())
            } else {
                format!("{output}\n").into_bytes()
            };
            let _ = app.pty_manager.write_to(tgt, &bytes);
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
    bridge: &Arc<WorkflowBridge>,
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
                // Spawn the workflow executor in a background task
                let bridge_clone = bridge.clone();
                let db = app.db.clone();
                let wf_name = workflow.clone();
                tokio::spawn(async move {
                    run_workflow_task(def, bridge_clone, variables, db, &wf_name).await;
                });
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
        let (layout_json, panes_json) = app.session_snapshot();
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

fn handle_restore(app: &mut App, name: String, terminal_size: ratatui::layout::Rect) {
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
                    let cols = terminal_size.width.saturating_sub(2);
                    let rows = terminal_size.height.saturating_sub(6);
                    for pc in &pane_configs {
                        if let Err(e) =
                            app.spawn_pane(pc.harness_type, pc.model.clone(), cols, rows)
                        {
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
                    let cols = terminal_size.width.saturating_sub(2);
                    let rows = terminal_size.height.saturating_sub(6);
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
                    let adapter = app.adapter_registry.get(&pane.harness_type);
                    let bytes = adapter
                        .map(|a| {
                            a.translate_input(&hom_core::OrchestratorCommand::Prompt(
                                prompt.clone(),
                            ))
                        })
                        .unwrap_or_else(|| format!("{prompt}\n").into_bytes());
                    match app.pty_manager.write_to(pane_id, &bytes) {
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
    workflow_name: &str,
) {
    let executor = hom_workflow::WorkflowExecutor::new();

    // Build checkpoint store if DB is available
    let checkpoint_store = db
        .as_ref()
        .map(|db| hom_tui::db_checkpoint::DbCheckpointStore::new(db.clone()));

    // Generate a single workflow ID used by both the DB row and the executor,
    // so that update_workflow_status targets the correct row.
    let wf_id = uuid::Uuid::new_v4().to_string();

    // Persist workflow start to DB
    if let Some(ref db) = db {
        let vars_json = serde_json::to_string(&variables).unwrap_or_default();
        if let Err(e) = hom_db::workflow::save_workflow(
            db.pool(),
            &wf_id,
            &def.name,
            workflow_name,
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
                .execute_with_id(&def, bridge, variables, Some(store), wf_id)
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
                // Update workflow status
                let status_str = format!("{:?}", wf_result.status);
                let _ = hom_db::workflow::update_workflow_status(
                    db.pool(),
                    &wf_result.workflow_id,
                    &status_str,
                    None,
                )
                .await;

                // Log cost for each completed step
                for (step_id, step_result) in &wf_result.step_results {
                    if step_result.status == hom_workflow::executor::StepStatus::Completed
                        && let Err(e) =
                            hom_db::cost::log_cost(db.pool(), 0, step_id, None, 0, 0, 0.0).await
                    {
                        warn!(step = %step_id, error = %e, "cost logging failed");
                    }
                }
            }
        }
        Err(e) => {
            error!(workflow = %workflow_name, error = %e, "workflow execution failed");
        }
    }
}
