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
    exited: Option<u32>,
) {
    // Build display title, appending exit code when the process has terminated
    let display_title = if let Some(code) = exited {
        format!(" {title} [{harness_name}] [EXITED: {code}] ")
    } else {
        format!(" {title} [{harness_name}] ")
    };

    // Draw pane border — red when exited, cyan when focused, grey otherwise
    let border_style = if exited.is_some() {
        Style::default().fg(Color::Red)
    } else if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(display_title);

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

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use hom_core::{Cell, CellAttributes, CursorState, ScreenSnapshot, TermColor};

    #[derive(Clone)]
    struct FakeTerminal {
        snapshot: ScreenSnapshot,
    }

    impl TerminalBackend for FakeTerminal {
        fn new(cols: u16, rows: u16, _scrollback: usize) -> hom_core::HomResult<Self> {
            Ok(Self {
                snapshot: ScreenSnapshot {
                    rows: vec![vec![Cell::default(); cols as usize]; rows as usize],
                    cols,
                    num_rows: rows,
                    cursor: CursorState::default(),
                },
            })
        }

        fn process(&mut self, _bytes: &[u8]) {}

        fn resize(&mut self, cols: u16, rows: u16) {
            self.snapshot.cols = cols;
            self.snapshot.num_rows = rows;
        }

        fn screen_snapshot(&self) -> ScreenSnapshot {
            self.snapshot.clone()
        }

        fn cursor(&self) -> CursorState {
            self.snapshot.cursor.clone()
        }

        fn title(&self) -> Option<&str> {
            None
        }
    }

    #[test]
    fn render_pane_draws_title_content_and_cursor() {
        let mut terminal = Terminal::new(TestBackend::new(50, 8)).unwrap();
        let fake = FakeTerminal {
            snapshot: ScreenSnapshot {
                rows: vec![
                    vec![
                        Cell {
                            character: 'A',
                            fg: TermColor::Red,
                            bg: TermColor::Blue,
                            attrs: CellAttributes {
                                bold: true,
                                underline: true,
                                ..CellAttributes::default()
                            },
                        },
                        Cell {
                            character: 'B',
                            fg: TermColor::Green,
                            bg: TermColor::Default,
                            attrs: CellAttributes::default(),
                        },
                    ],
                    vec![Cell::default(); 2],
                ],
                cols: 2,
                num_rows: 2,
                cursor: CursorState {
                    row: 0,
                    col: 1,
                    visible: true,
                },
            },
        };

        terminal
            .draw(|frame| {
                render_pane(
                    frame,
                    Rect::new(0, 0, 50, 8),
                    &fake,
                    "Demo",
                    "Claude Code",
                    true,
                    Some(7),
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Demo"));
        assert!(content.contains("EXITED: 7"));

        let cell = buffer.cell((1, 1)).unwrap();
        assert_eq!(cell.symbol(), "A");
        assert_eq!(cell.fg, Color::Red);
        assert_eq!(cell.bg, Color::Blue);
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert!(cell.modifier.contains(Modifier::UNDERLINED));

        let cursor = terminal.get_cursor_position().unwrap();
        assert_eq!((cursor.x, cursor.y), (2, 1));
    }
}
