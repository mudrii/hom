//! Command bar — parses orchestrator-level commands.

use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};

use hom_core::{HarnessType, LayoutKind, PaneId, RemoteTarget};

/// Parsed command from the command bar.
#[derive(Debug, Clone)]
pub enum Command {
    /// `:spawn claude [--model opus] [--dir /path] [--remote user@host[:port]]`
    Spawn {
        /// Built-in harness type if recognized, else `None` for plugin lookup.
        harness: Option<HarnessType>,
        /// Raw harness name from command bar (used for plugin lookup when `harness` is `None`).
        harness_name: String,
        model: Option<String>,
        working_dir: Option<PathBuf>,
        extra_args: Vec<String>,
        /// SSH remote target. `None` means local spawn.
        remote: Option<RemoteTarget>,
    },
    /// `:load-plugin /path/to/plugin.dylib`
    LoadPlugin { path: PathBuf },
    /// `:focus 1` or `:focus claude`
    Focus(PaneSelector),
    /// `:send 1 "analyze this codebase"`
    Send { target: PaneSelector, input: String },
    /// `:pipe 1 -> 2`
    Pipe {
        source: PaneSelector,
        target: PaneSelector,
    },
    /// `:broadcast "stop"`
    Broadcast(String),
    /// `:run code-review --var task="add auth"`
    Run {
        workflow: String,
        variables: HashMap<String, String>,
    },
    /// `:kill 1`
    Kill(PaneSelector),
    /// `:layout grid`
    Layout(LayoutKind),
    /// `:save my-session`
    Save(String),
    /// `:restore my-session`
    Restore(String),
    /// `:help`
    Help,
    /// `:quit`
    Quit,
}

#[derive(Debug, Clone)]
pub enum PaneSelector {
    Id(PaneId),
    Name(String),
}

/// The command bar widget state.
pub struct CommandBar {
    pub input: String,
    pub cursor_pos: usize,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub last_error: Option<String>,
}

impl CommandBar {
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_idx: None,
            last_error: None,
        }
    }

    /// Handle a key event in the command bar.
    /// Returns Some(Command) if the user presses Enter on a valid command.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        self.last_error = None;

        match key.code {
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                None
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                    self.input.remove(self.cursor_pos);
                }
                None
            }
            KeyCode::Delete => {
                if self.cursor_pos < self.input.len() {
                    self.input.remove(self.cursor_pos);
                }
                None
            }
            KeyCode::Left => {
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
                None
            }
            KeyCode::Right => {
                self.cursor_pos = (self.cursor_pos + 1).min(self.input.len());
                None
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
                None
            }
            KeyCode::End => {
                self.cursor_pos = self.input.len();
                None
            }
            KeyCode::Up => {
                // History navigation
                if !self.history.is_empty() {
                    let idx = match self.history_idx {
                        Some(i) => i.saturating_sub(1),
                        None => self.history.len() - 1,
                    };
                    self.history_idx = Some(idx);
                    self.input = self.history[idx].clone();
                    self.cursor_pos = self.input.len();
                }
                None
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 < self.history.len() {
                        self.history_idx = Some(idx + 1);
                        self.input = self.history[idx + 1].clone();
                    } else {
                        self.history_idx = None;
                        self.input.clear();
                    }
                    self.cursor_pos = self.input.len();
                }
                None
            }
            KeyCode::Enter => {
                let input = self.input.trim().to_string();
                if input.is_empty() {
                    return None;
                }

                self.history.push(input.clone());
                self.history_idx = None;
                self.input.clear();
                self.cursor_pos = 0;

                match parse_command(&input) {
                    Ok(cmd) => Some(cmd),
                    Err(e) => {
                        self.last_error = Some(e);
                        None
                    }
                }
            }
            _ => None,
        }
    }
}

impl Default for CommandBar {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandBar {
    /// Parse a command string — public entry point used in tests and external callers.
    pub fn parse_command(input: &str) -> Result<Command, String> {
        parse_command(input)
    }
}

/// Split a string respecting single and double quotes.
///
/// `"hello world" foo 'bar baz'` → `["hello world", "foo", "bar baz"]`
fn shell_split(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape_next = false;

    for ch in input.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if !in_single => {
                escape_next = true;
            }
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Parse a command string into a Command.
fn parse_command(input: &str) -> Result<Command, String> {
    let input = input.strip_prefix(':').unwrap_or(input);
    let parts: Vec<&str> = input.splitn(3, ' ').collect();

    match parts.first().map(|s| s.to_lowercase()).as_deref() {
        Some("spawn") | Some("s") => {
            if parts.len() < 2 {
                return Err(
                    "usage: :spawn <harness> [model] [--dir path] [-- extra args]".to_string(),
                );
            }
            let harness_name = parts[1].to_string();
            let harness = HarnessType::from_str_loose(parts[1]);

            // Parse the rest (parts[2]) for model, --dir, --remote, and extra args
            let mut model: Option<String> = None;
            let mut working_dir: Option<PathBuf> = None;
            let mut extra_args: Vec<String> = Vec::new();
            let mut remote: Option<RemoteTarget> = None;

            if let Some(rest) = parts.get(2) {
                let tokens = shell_split(rest);
                let mut i = 0;
                let mut collecting_extra = false;

                while i < tokens.len() {
                    if tokens[i] == "--" {
                        collecting_extra = true;
                        i += 1;
                    } else if collecting_extra {
                        extra_args.push(tokens[i].clone());
                        i += 1;
                    } else if tokens[i] == "--dir" {
                        if i + 1 < tokens.len() {
                            working_dir = Some(PathBuf::from(&tokens[i + 1]));
                            i += 2;
                        } else {
                            return Err("--dir requires a path".to_string());
                        }
                    } else if tokens[i] == "--remote" {
                        if i + 1 < tokens.len() {
                            remote =
                                Some(RemoteTarget::parse(&tokens[i + 1]).ok_or_else(|| {
                                    format!("invalid remote target: {}", tokens[i + 1])
                                })?);
                            i += 2;
                        } else {
                            return Err("--remote requires user@host[:port]".to_string());
                        }
                    } else if model.is_none() {
                        model = Some(tokens[i].clone());
                        i += 1;
                    } else {
                        // Unknown token — treat as extra arg
                        extra_args.push(tokens[i].clone());
                        i += 1;
                    }
                }
            }

            Ok(Command::Spawn {
                harness,
                harness_name,
                model,
                working_dir,
                extra_args,
                remote,
            })
        }
        Some("load-plugin") => {
            let path = parts
                .get(1)
                .ok_or_else(|| "usage: :load-plugin <path>".to_string())?;
            Ok(Command::LoadPlugin {
                path: PathBuf::from(path),
            })
        }
        Some("focus") | Some("f") => {
            if parts.len() < 2 {
                return Err("usage: :focus <id|name>".to_string());
            }
            let selector = if let Ok(id) = parts[1].parse::<PaneId>() {
                PaneSelector::Id(id)
            } else {
                PaneSelector::Name(parts[1].to_string())
            };
            Ok(Command::Focus(selector))
        }
        Some("send") => {
            if parts.len() < 3 {
                return Err("usage: :send <id|name> <input>".to_string());
            }
            let target = if let Ok(id) = parts[1].parse::<PaneId>() {
                PaneSelector::Id(id)
            } else {
                PaneSelector::Name(parts[1].to_string())
            };
            // Strip surrounding quotes from the input if present
            let raw = parts[2].to_string();
            let input = if (raw.starts_with('"') && raw.ends_with('"'))
                || (raw.starts_with('\'') && raw.ends_with('\''))
            {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw
            };
            Ok(Command::Send { target, input })
        }
        Some("pipe") => {
            if parts.len() < 2 {
                return Err("usage: :pipe <source> -> <target>".to_string());
            }
            // Parse "1 -> 2" or "1 2"
            let rest = parts[1..].join(" ");
            let pipe_parts: Vec<&str> = rest.split("->").collect();
            if pipe_parts.len() != 2 {
                return Err("usage: :pipe <source> -> <target>".to_string());
            }
            let source = pipe_parts[0]
                .trim()
                .parse::<PaneId>()
                .map(PaneSelector::Id)
                .unwrap_or_else(|_| PaneSelector::Name(pipe_parts[0].trim().to_string()));
            let target = pipe_parts[1]
                .trim()
                .parse::<PaneId>()
                .map(PaneSelector::Id)
                .unwrap_or_else(|_| PaneSelector::Name(pipe_parts[1].trim().to_string()));
            Ok(Command::Pipe { source, target })
        }
        Some("broadcast") | Some("bc") => {
            let msg = parts[1..].join(" ");
            Ok(Command::Broadcast(msg))
        }
        Some("run") | Some("r") => {
            if parts.len() < 2 {
                return Err("usage: :run <workflow> [--var key=value ...]".to_string());
            }
            let workflow = parts[1].to_string();
            let mut variables = HashMap::new();

            // Parse --var key=value pairs from the rest of the input
            if parts.len() > 2 {
                let rest = parts[2];
                let tokens = shell_split(rest);
                let mut i = 0;
                while i < tokens.len() {
                    if tokens[i] == "--var" {
                        if i + 1 < tokens.len() {
                            if let Some((k, v)) = tokens[i + 1].split_once('=') {
                                variables.insert(k.to_string(), v.to_string());
                            }
                            i += 2;
                        } else {
                            i += 1;
                        }
                    } else if let Some(rest_kv) = tokens[i].strip_prefix("--var=") {
                        if let Some((k, v)) = rest_kv.split_once('=') {
                            variables.insert(k.to_string(), v.to_string());
                        }
                        i += 1;
                    } else {
                        i += 1;
                    }
                }
            }

            Ok(Command::Run {
                workflow,
                variables,
            })
        }
        Some("kill") | Some("k") => {
            if parts.len() < 2 {
                return Err("usage: :kill <id|name>".to_string());
            }
            let selector = if let Ok(id) = parts[1].parse::<PaneId>() {
                PaneSelector::Id(id)
            } else {
                PaneSelector::Name(parts[1].to_string())
            };
            Ok(Command::Kill(selector))
        }
        Some("layout") | Some("l") => {
            if parts.len() < 2 {
                return Err("usage: :layout <hsplit|vsplit|grid|tabs|single>".to_string());
            }
            let kind = match parts[1].to_lowercase().as_str() {
                "hsplit" | "h" => LayoutKind::HSplit,
                "vsplit" | "v" => LayoutKind::VSplit,
                "grid" | "g" => LayoutKind::Grid,
                "tabs" | "t" => LayoutKind::Tabbed,
                "single" | "s" => LayoutKind::Single,
                _ => return Err(format!("unknown layout: {}", parts[1])),
            };
            Ok(Command::Layout(kind))
        }
        Some("save") => {
            let name = parts.get(1).unwrap_or(&"default").to_string();
            Ok(Command::Save(name))
        }
        Some("restore") | Some("load") => {
            let name = parts.get(1).unwrap_or(&"default").to_string();
            Ok(Command::Restore(name))
        }
        Some("help") | Some("h") | Some("?") => Ok(Command::Help),
        Some("quit") | Some("q") => Ok(Command::Quit),
        _ => Err(format!("unknown command: {input}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_parses_remote_flag_user_at_host() {
        let cmd =
            CommandBar::parse_command("spawn claude --remote alice@build.example.com").unwrap();
        match cmd {
            Command::Spawn {
                harness,
                harness_name,
                remote,
                ..
            } => {
                assert_eq!(harness, Some(HarnessType::ClaudeCode));
                assert_eq!(harness_name, "claude");
                let t = remote.unwrap();
                assert_eq!(t.user, "alice");
                assert_eq!(t.host, "build.example.com");
                assert_eq!(t.port, 22);
            }
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn spawn_parses_remote_flag_with_port() {
        let cmd = CommandBar::parse_command("spawn claude --remote bob@10.0.0.5:2222").unwrap();
        match cmd {
            Command::Spawn { remote, .. } => {
                let t = remote.unwrap();
                assert_eq!(t.port, 2222);
            }
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn spawn_without_remote_has_none() {
        let cmd = CommandBar::parse_command("spawn claude").unwrap();
        match cmd {
            Command::Spawn { remote, .. } => assert!(remote.is_none()),
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn spawn_harness_name_is_set_for_known_harness() {
        let cmd = CommandBar::parse_command("spawn codex").unwrap();
        match cmd {
            Command::Spawn {
                harness,
                harness_name,
                ..
            } => {
                assert_eq!(harness, Some(HarnessType::CodexCli));
                assert_eq!(harness_name, "codex");
            }
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn spawn_unknown_harness_sets_harness_name_and_none() {
        let cmd = CommandBar::parse_command("spawn mycli").unwrap();
        match cmd {
            Command::Spawn {
                harness,
                harness_name,
                ..
            } => {
                assert!(harness.is_none(), "unknown harness should be None");
                assert_eq!(harness_name, "mycli");
            }
            _ => panic!("expected Spawn"),
        }
    }

    #[test]
    fn load_plugin_parses_absolute_path() {
        let cmd = CommandBar::parse_command(
            "load-plugin /home/user/.config/hom/plugins/mycli.dylib",
        )
        .unwrap();
        match cmd {
            Command::LoadPlugin { path } => {
                assert_eq!(
                    path.to_str().unwrap(),
                    "/home/user/.config/hom/plugins/mycli.dylib"
                );
            }
            _ => panic!("expected LoadPlugin"),
        }
    }

    #[test]
    fn load_plugin_requires_path() {
        let result = CommandBar::parse_command("load-plugin");
        assert!(result.is_err());
    }

    #[test]
    fn shell_split_basic() {
        assert_eq!(shell_split("hello world"), vec!["hello", "world"],);
    }

    #[test]
    fn shell_split_double_quotes() {
        assert_eq!(
            shell_split(r#""hello world" foo"#),
            vec!["hello world", "foo"],
        );
    }

    #[test]
    fn shell_split_single_quotes() {
        assert_eq!(shell_split("'hello world' foo"), vec!["hello world", "foo"],);
    }

    #[test]
    fn shell_split_mixed_quotes() {
        assert_eq!(
            shell_split(r#"--var task="add auth" --var lang='rust'"#),
            vec!["--var", "task=add auth", "--var", "lang=rust"],
        );
    }

    #[test]
    fn shell_split_escaped_space() {
        assert_eq!(shell_split(r"hello\ world foo"), vec!["hello world", "foo"],);
    }

    #[test]
    fn shell_split_empty() {
        assert_eq!(shell_split(""), Vec::<String>::new());
    }

    #[test]
    fn parse_run_with_quoted_var() {
        let cmd = parse_command("run code-review --var task=\"add auth middleware\"").unwrap();
        match cmd {
            Command::Run {
                workflow,
                variables,
            } => {
                assert_eq!(workflow, "code-review");
                assert_eq!(variables.get("task").unwrap(), "add auth middleware");
            }
            _ => panic!("expected Run command"),
        }
    }
}
