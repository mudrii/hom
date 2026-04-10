use serde::{Deserialize, Serialize};

/// A single cell in a pane's screen buffer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebCell {
    pub ch: char,
    pub fg: u32,   // RRGGBB packed, 0xFFFFFF = terminal default
    pub bg: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for WebCell {
    fn default() -> Self {
        WebCell { ch: ' ', fg: 0xFF_FF_FF, bg: 0x00_00_00, bold: false, italic: false, underline: false }
    }
}

/// One pane's screen state as a flat cell grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebPane {
    pub pane_id: String,
    pub title: String,
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    /// Row-major flat vec: `cells[row * cols + col]`
    pub cells: Vec<WebCell>,
    pub focused: bool,
}

/// A full frame pushed to all WebSocket clients after each render tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFrame {
    pub ts: u64,   // Unix ms
    pub panes: Vec<WebPane>,
}

impl WebFrame {
    pub fn new(panes: Vec<WebPane>) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        WebFrame { ts, panes }
    }
}

/// A keystroke message sent from the browser to the server.
#[derive(Debug, Deserialize)]
pub struct WebInput {
    pub pane_id: String,
    /// UTF-8 text to send (newline appended by the server).
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcell_default_is_space() {
        let cell = WebCell::default();
        assert_eq!(cell.ch, ' ');
        assert!(!cell.bold);
    }

    #[test]
    fn webframe_serialises_to_json() {
        let pane = WebPane {
            pane_id: "p0".into(),
            title: "claude".into(),
            cols: 80,
            rows: 24,
            cursor_col: 0,
            cursor_row: 0,
            cells: vec![WebCell::default(); 80 * 24],
            focused: true,
        };
        let frame = WebFrame::new(vec![pane]);
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains("\"panes\""));
        assert!(json.contains("\"cells\""));
        assert!(json.contains("\"ts\""));
    }

    #[test]
    fn webinput_deserialises_from_json() {
        let raw = r#"{"pane_id":"p0","text":"hello"}"#;
        let input: WebInput = serde_json::from_str(raw).unwrap();
        assert_eq!(input.pane_id, "p0");
        assert_eq!(input.text, "hello");
    }

    #[test]
    fn webframe_cell_grid_is_row_major() {
        let pane = WebPane {
            pane_id: "p0".into(), title: "test".into(),
            cols: 10, rows: 5, cursor_col: 3, cursor_row: 1,
            cells: vec![WebCell::default(); 10 * 5],
            focused: false,
        };
        assert_eq!(pane.cells.len(), 50);
    }
}
