//! Pane-specific rendering — maps terminal emulator screen to ratatui buffer.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders};

use hom_core::TerminalBackend;
use hom_terminal::color_map::term_color_to_ratatui;

/// Render a pane's terminal state into a ratatui frame.
pub fn render_pane<B: TerminalBackend>(
    frame: &mut Frame,
    area: Rect,
    terminal: &B,
    title: &str,
    harness_name: &str,
    focused: bool,
) {
    // Draw pane border
    let border_style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(" {title} [{harness_name}] "));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Get screen state from terminal emulator
    let screen = terminal.screen_snapshot();

    // Map each cell to the ratatui buffer
    let buf = frame.buffer_mut();
    for (row_idx, row) in screen.rows.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let x = inner.x + col_idx as u16;
            let y = inner.y + row_idx as u16;

            if x < inner.right()
                && y < inner.bottom()
                && let Some(buf_cell) = buf.cell_mut((x, y))
            {
                buf_cell.set_char(cell.character);
                buf_cell.set_fg(term_color_to_ratatui(cell.fg));
                buf_cell.set_bg(term_color_to_ratatui(cell.bg));

                let mut modifier = Modifier::empty();
                if cell.attrs.bold {
                    modifier |= Modifier::BOLD;
                }
                if cell.attrs.italic {
                    modifier |= Modifier::ITALIC;
                }
                if cell.attrs.underline {
                    modifier |= Modifier::UNDERLINED;
                }
                if cell.attrs.dim {
                    modifier |= Modifier::DIM;
                }
                if cell.attrs.strikethrough {
                    modifier |= Modifier::CROSSED_OUT;
                }
                buf_cell.set_style(Style::default().add_modifier(modifier));
            }
        }
    }

    // Set cursor position if this pane is focused
    if focused {
        let cursor = screen.cursor;
        if cursor.visible {
            frame.set_cursor_position((inner.x + cursor.col, inner.y + cursor.row));
        }
    }
}
