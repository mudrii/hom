//! Status rail — top bar showing active harnesses, workflow status, and cost.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Render the status rail at the top of the screen.
pub fn render_status_rail(
    frame: &mut Frame,
    area: Rect,
    pane_count: usize,
    focused_pane: Option<u32>,
    workflow_status: Option<&str>,
) {
    let mut spans = vec![
        Span::styled(
            " HOM ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!(
                "{pane_count} pane{}",
                if pane_count != 1 { "s" } else { "" }
            ),
            Style::default().fg(Color::White),
        ),
    ];

    if let Some(focused) = focused_pane {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("focused: #{focused}"),
            Style::default().fg(Color::Yellow),
        ));
    }

    if let Some(status) = workflow_status {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("workflow: {status}"),
            Style::default().fg(Color::Green),
        ));
    }

    spans.push(Span::raw(" | "));
    spans.push(Span::styled(
        "Ctrl-` cmd | Ctrl-Tab pane | Ctrl-Q quit",
        Style::default().fg(Color::DarkGray),
    ));

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
