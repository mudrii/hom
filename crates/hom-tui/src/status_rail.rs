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
    total_cost: f64,
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

    if total_cost > 0.0 {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            format!("${total_cost:.2}"),
            Style::default().fg(Color::Magenta),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn cost_appears_when_positive() {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 80, 1);
                render_status_rail(frame, area, 2, Some(1), None, 3.14);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            line.contains("$3.14"),
            "expected '$3.14' in status rail, got: {line:?}"
        );
    }

    #[test]
    fn cost_hidden_when_zero() {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 80, 1);
                render_status_rail(frame, area, 1, None, None, 0.0);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            !line.contains('$'),
            "expected no '$' in status rail when cost is 0, got: {line:?}"
        );
    }
}
