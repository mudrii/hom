//! Input routing — directs keyboard/mouse events to the right target.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use hom_core::PaneId;

/// Where input is currently directed.
#[derive(Debug, Clone)]
pub enum InputMode {
    /// All input goes to the focused pane's PTY.
    PaneInput { focused: PaneId },
    /// Input goes to the command bar.
    CommandBar,
    /// Workflow is running — input restricted to control commands.
    WorkflowControl,
}

/// Actions the input router can produce.
#[derive(Debug)]
pub enum Action {
    /// Write raw bytes to a pane's PTY.
    WriteToPty(PaneId, Vec<u8>),
    /// Focus a specific pane.
    FocusPane(PaneId),
    /// Switch to command bar mode.
    FocusCommandBar,
    /// Send a character to the command bar.
    CommandBarInput(KeyEvent),
    /// Quit the application.
    Quit,
    /// Focus the next pane.
    NextPane,
    /// Focus the previous pane.
    PrevPane,
    /// Kill the focused pane.
    KillPane(PaneId),
    /// No action needed.
    None,
}

/// Parsed keybinding: a key code + required modifiers.
#[derive(Debug, Clone)]
struct Keybinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

/// Routes input events to the appropriate destination.
pub struct InputRouter {
    pub mode: InputMode,
    toggle_command_bar: Keybinding,
    next_pane: Keybinding,
    prev_pane: Keybinding,
    kill_pane: Keybinding,
}

impl InputRouter {
    pub fn new() -> Self {
        Self {
            mode: InputMode::CommandBar,
            toggle_command_bar: Keybinding {
                code: KeyCode::Char('`'),
                modifiers: KeyModifiers::CONTROL,
            },
            next_pane: Keybinding {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::CONTROL,
            },
            prev_pane: Keybinding {
                code: KeyCode::Tab,
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            },
            kill_pane: Keybinding {
                code: KeyCode::Char('w'),
                modifiers: KeyModifiers::CONTROL,
            },
        }
    }

    /// Create an InputRouter with keybindings from config.
    pub fn from_config(config: &hom_core::KeybindingsConfig) -> Self {
        let mut router = Self::new();
        if let Some(kb) = parse_keybinding(&config.toggle_command_bar) {
            router.toggle_command_bar = kb;
        }
        if let Some(kb) = parse_keybinding(&config.next_pane) {
            router.next_pane = kb;
        }
        if let Some(kb) = parse_keybinding(&config.prev_pane) {
            router.prev_pane = kb;
        }
        if let Some(kb) = parse_keybinding(&config.kill_pane) {
            router.kill_pane = kb;
        }
        router
    }

    pub fn handle_event(
        &mut self,
        event: Event,
        pane_areas: &[(PaneId, ratatui::layout::Rect)],
    ) -> Action {
        match (&self.mode, &event) {
            // ── Global: Ctrl-Q quits ──────────────────────────────
            (
                _,
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }),
            ) => Action::Quit,

            // ── Toggle command bar (configurable) ─────────────────
            (InputMode::PaneInput { .. }, Event::Key(ke))
                if matches_keybinding(ke, &self.toggle_command_bar) =>
            {
                self.mode = InputMode::CommandBar;
                Action::FocusCommandBar
            }

            // ── Escape exits command bar back to pane ─────────────
            (
                InputMode::CommandBar,
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }),
            ) => {
                if let Some(&(pane_id, _)) = pane_areas.first() {
                    self.mode = InputMode::PaneInput { focused: pane_id };
                    Action::FocusPane(pane_id)
                } else {
                    Action::None
                }
            }

            // ── Next pane (configurable) ──────────────────────────
            (_, Event::Key(ke)) if matches_keybinding(ke, &self.next_pane) => Action::NextPane,

            // ── Previous pane (configurable) ─────────────────────
            (_, Event::Key(ke)) if matches_keybinding(ke, &self.prev_pane) => Action::PrevPane,

            // ── Kill pane (configurable) ──────────────────────────
            (InputMode::PaneInput { focused }, Event::Key(ke))
                if matches_keybinding(ke, &self.kill_pane) =>
            {
                Action::KillPane(*focused)
            }

            // ── In pane mode, forward all keys to PTY ─────────────
            (InputMode::PaneInput { focused }, Event::Key(key_event)) => {
                let bytes = encode_key_event(key_event);
                Action::WriteToPty(*focused, bytes)
            }

            // ── In command bar mode, send keys to command bar ─────
            (InputMode::CommandBar, Event::Key(key_event)) => Action::CommandBarInput(*key_event),

            // ── Mouse click focuses a pane ────────────────────────
            (
                _,
                Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    ..
                }),
            ) => {
                if let Some(pane_id) = super::layout::pane_at_position(pane_areas, *column, *row) {
                    self.mode = InputMode::PaneInput { focused: pane_id };
                    Action::FocusPane(pane_id)
                } else {
                    Action::None
                }
            }

            _ => Action::None,
        }
    }

    /// Set focus to a specific pane.
    pub fn focus_pane(&mut self, pane_id: PaneId) {
        self.mode = InputMode::PaneInput { focused: pane_id };
    }
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn matches_keybinding(ke: &KeyEvent, kb: &Keybinding) -> bool {
    ke.code == kb.code && ke.modifiers.contains(kb.modifiers)
}

/// Parse a keybinding string like "ctrl-`", "ctrl-tab", "ctrl-w".
fn parse_keybinding(s: &str) -> Option<Keybinding> {
    let s = s.trim().to_lowercase();
    let parts: Vec<&str> = s.split('-').collect();

    let (modifiers, key_part) = match parts.len() {
        1 => (KeyModifiers::empty(), parts[0]),
        2 => {
            let m = match parts[0] {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => return None,
            };
            (m, parts[1])
        }
        3 => {
            let mut m = KeyModifiers::empty();
            for &modifier in &parts[..2] {
                match modifier {
                    "ctrl" => m |= KeyModifiers::CONTROL,
                    "alt" => m |= KeyModifiers::ALT,
                    "shift" => m |= KeyModifiers::SHIFT,
                    _ => return None,
                }
            }
            (m, parts[2])
        }
        _ => return None,
    };

    let code = match key_part {
        "tab" => KeyCode::Tab,
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        s if s.len() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        s if s.starts_with('f') => {
            let n: u8 = s[1..].parse().ok()?;
            KeyCode::F(n)
        }
        _ => return None,
    };

    Some(Keybinding { code, modifiers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_key() {
        let kb = parse_keybinding("tab").unwrap();
        assert_eq!(kb.code, KeyCode::Tab);
        assert!(kb.modifiers.is_empty());
    }

    #[test]
    fn test_parse_ctrl_key() {
        let kb = parse_keybinding("ctrl-w").unwrap();
        assert_eq!(kb.code, KeyCode::Char('w'));
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn test_parse_ctrl_backtick() {
        let kb = parse_keybinding("ctrl-`").unwrap();
        assert_eq!(kb.code, KeyCode::Char('`'));
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn test_parse_ctrl_tab() {
        let kb = parse_keybinding("ctrl-tab").unwrap();
        assert_eq!(kb.code, KeyCode::Tab);
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn test_parse_ctrl_shift_tab() {
        let kb = parse_keybinding("ctrl-shift-tab").unwrap();
        assert_eq!(kb.code, KeyCode::Tab);
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
        assert!(kb.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_f_key() {
        let kb = parse_keybinding("ctrl-f1").unwrap();
        assert_eq!(kb.code, KeyCode::F(1));
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_keybinding("").is_none());
        assert!(parse_keybinding("mega-x").is_none());
    }

    #[test]
    fn test_encode_ctrl_c() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(encode_key_event(&key), vec![0x03]);
    }

    #[test]
    fn test_encode_enter() {
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
        assert_eq!(encode_key_event(&key), vec![b'\r']);
    }
}

/// Encode a crossterm key event into raw bytes for a PTY.
pub fn encode_key_event(key: &KeyEvent) -> Vec<u8> {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl-A = 0x01, Ctrl-B = 0x02, ..., Ctrl-Z = 0x1A
                if c.is_ascii_lowercase() {
                    vec![(c as u8) - b'a' + 1]
                } else {
                    format!("{c}").into_bytes()
                }
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf);
                buf[..c.len_utf8()].to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}
