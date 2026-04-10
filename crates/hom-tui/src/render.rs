//! Top-level frame rendering — composites status rail, pane grid, and command bar.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::input::InputMode;
use crate::layout::compute_pane_areas;
use crate::pane_render::render_pane;
use crate::status_rail::render_status_rail;

/// Render the entire application frame.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Status rail
            Constraint::Min(10),   // Pane grid
            Constraint::Length(3), // Command bar
        ])
        .split(frame.area());

    // ── Status rail ──────────────────────────────────────────────
    render_status_rail(
        frame,
        chunks[0],
        app.panes.len(),
        app.focused_pane,
        app.workflow_status.as_deref(),
        app.total_cost,
    );

    // ── Pane grid ────────────────────────────────────────────────
    if app.panes.is_empty() {
        // Empty state
        let welcome = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Welcome to HOM",
                Style::default().fg(Color::Cyan),
            )),
            Line::from(""),
            Line::from("  Type :spawn <harness> [model] to start."),
            Line::from(""),
            Line::from("  Examples:"),
            Line::from("    :spawn claude opus"),
            Line::from("    :spawn codex 5.4"),
            Line::from("    :spawn pi minimax-2.7"),
            Line::from(""),
            Line::from("  Press Ctrl-` to toggle the command bar."),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        frame.render_widget(welcome, chunks[1]);
    } else {
        let pane_areas = compute_pane_areas(chunks[1], &app.pane_order, &app.layout);

        for (pane_id, area) in &pane_areas {
            if let Some(pane) = app.panes.get(pane_id) {
                let is_focused = app.focused_pane == Some(*pane_id);
                render_pane(
                    frame,
                    *area,
                    &pane.terminal,
                    &pane.title,
                    pane.harness_type.display_name(),
                    is_focused,
                );
            }
        }
    }

    // ── Command bar ──────────────────────────────────────────────
    render_command_bar(frame, chunks[2], app);
}

fn render_command_bar(frame: &mut Frame, area: Rect, app: &App) {
    let is_active = matches!(app.input_router.mode, InputMode::CommandBar);

    let border_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Command ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(err) = &app.command_bar.last_error {
        let err_line = Paragraph::new(Line::from(Span::styled(
            format!("Error: {err}"),
            Style::default().fg(Color::Red),
        )));
        frame.render_widget(err_line, inner);
    } else {
        let display = format!(":{}", app.command_bar.input);
        let input_line = Paragraph::new(Line::from(Span::styled(
            display,
            if is_active {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )));
        frame.render_widget(input_line, inner);

        // Show cursor in command bar when active
        if is_active {
            frame.set_cursor_position((inner.x + 1 + app.command_bar.cursor_pos as u16, inner.y));
        }
    }
}
