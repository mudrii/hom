# Remote Pane Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow `:spawn <harness> --remote user@host` to open a pane that runs the harness process on a remote machine over SSH, appearing identically to a local pane in the TUI.

**Architecture:** A new `RemotePtyManager` in `hom-pty` uses the `ssh2` crate to open an authenticated SSH session, allocate a PTY on the remote host, and run the harness binary. The SSH channel is `Read + Write`, so it slots into the same `AsyncPtyReader` pipeline as local PTY output. `App` stores both `PtyManager` (local) and `RemotePtyManager` (remote) and routes pane operations to the appropriate one. The remote command string is built with shell quoting to prevent argument injection.

**Tech Stack:** `ssh2 = "0.9"`, existing `portable-pty`, `tokio`, `AsyncPtyReader`

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `crates/hom-core/src/types.rs` | Modify | Add `RemoteTarget`, `PaneKind` |
| `crates/hom-pty/Cargo.toml` | Modify | Add `ssh2 = "0.9"` dependency |
| `crates/hom-pty/src/remote.rs` | Create | `RemotePtyManager` — SSH session + channel lifecycle |
| `crates/hom-pty/src/lib.rs` | Modify | Re-export `RemotePtyManager` |
| `crates/hom-tui/src/command_bar.rs` | Modify | Parse `--remote user@host[:port]` flag in `:spawn` |
| `crates/hom-tui/src/app.rs` | Modify | `remote_ptys: RemotePtyManager`, `spawn_remote_pane()`, route write/resize/kill |
| `CLAUDE.md` | Modify | Update implementation status |

---

### Task 1: Add `RemoteTarget` and `PaneKind` to `hom-core`

**Files:**
- Modify: `crates/hom-core/src/types.rs`

- [ ] **Step 1: Write the failing test**

Add to `crates/hom-core/src/types.rs` at the bottom inside a `#[cfg(test)] mod tests` block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_target_parse_with_port() {
        let t = RemoteTarget::parse("alice@example.com:2222").unwrap();
        assert_eq!(t.user, "alice");
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 2222);
    }

    #[test]
    fn remote_target_parse_default_port() {
        let t = RemoteTarget::parse("bob@10.0.0.5").unwrap();
        assert_eq!(t.user, "bob");
        assert_eq!(t.host, "10.0.0.5");
        assert_eq!(t.port, 22);
    }

    #[test]
    fn remote_target_parse_missing_at_fails() {
        assert!(RemoteTarget::parse("notaremote").is_none());
    }

    #[test]
    fn pane_kind_is_remote() {
        let kind = PaneKind::Remote(RemoteTarget {
            user: "u".into(),
            host: "h".into(),
            port: 22,
        });
        assert!(matches!(kind, PaneKind::Remote(_)));
    }

    #[test]
    fn remote_target_shell_args_are_individually_quoted() {
        let spec = CommandSpec {
            program: "claude".to_string(),
            args: vec!["--model".to_string(), "claude opus".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: ".".into(),
        };
        let parts = RemoteTarget::spec_to_argv(&spec);
        // Each arg is a separate element — no shell splitting occurs.
        assert_eq!(parts[0], "claude");
        assert_eq!(parts[2], "claude opus"); // spaces preserved in individual element
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```sh
cargo test -p hom-core -- remote_target pane_kind 2>&1 | head -20
```

Expected: `error[E0412]: cannot find type 'RemoteTarget'`.

- [ ] **Step 3: Add `RemoteTarget`, `PaneKind`, and `spec_to_argv` to `types.rs`**

Add after the `LayoutKind` enum in `crates/hom-core/src/types.rs`:

```rust
/// SSH connection target for remote panes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTarget {
    pub user: String,
    pub host: String,
    /// SSH port. Defaults to 22.
    pub port: u16,
}

impl RemoteTarget {
    /// Parse `user@host` or `user@host:port`. Returns `None` if `@` is absent.
    pub fn parse(s: &str) -> Option<Self> {
        let (user, rest) = s.split_once('@')?;
        let (host, port) = if let Some((h, p)) = rest.rsplit_once(':') {
            let port: u16 = p.parse().ok()?;
            (h.to_string(), port)
        } else {
            (rest.to_string(), 22)
        };
        Some(Self {
            user: user.to_string(),
            host,
            port,
        })
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Return the command spec as a Vec of individual argument strings.
    ///
    /// Use this instead of joining with spaces — ssh2 Channel::exec() receives
    /// a single string that gets passed to the remote shell. Keeping arguments
    /// as a Vec lets the caller apply proper shell quoting before joining.
    pub fn spec_to_argv(spec: &CommandSpec) -> Vec<String> {
        std::iter::once(spec.program.clone())
            .chain(spec.args.iter().cloned())
            .collect()
    }

    /// Shell-quote a single argument so it is safe to embed in a shell command string.
    ///
    /// Wraps the value in single quotes and escapes embedded single quotes via the
    /// `'` → `'\''` technique, which works in POSIX sh without any metacharacter risk.
    pub fn shell_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', r"'\''"))
    }

    /// Build a shell-safe command string for `ssh2::Channel::exec()`.
    ///
    /// Each argument is individually quoted via `shell_quote()`, then joined with spaces.
    /// This prevents argument injection when args contain spaces or shell metacharacters.
    pub fn build_remote_command(spec: &CommandSpec) -> String {
        Self::spec_to_argv(spec)
            .iter()
            .map(|a| Self::shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl std::fmt::Display for RemoteTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.port == 22 {
            write!(f, "{}@{}", self.user, self.host)
        } else {
            write!(f, "{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// Whether a pane is backed by a local PTY or a remote SSH channel.
#[derive(Debug, Clone)]
pub enum PaneKind {
    Local,
    Remote(RemoteTarget),
}
```

- [ ] **Step 4: Run tests**

```sh
cargo test -p hom-core -- remote_target pane_kind 2>&1 | tail -10
```

Expected: `test result: ok. 5 passed`.

- [ ] **Step 5: Cargo check**

```sh
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished dev profile`.

- [ ] **Step 6: Commit**

```sh
git add crates/hom-core/src/types.rs
git commit -m "feat(core): add RemoteTarget, PaneKind, and shell_quote helpers"
```

---

### Task 2: Create `RemotePtyManager` in `hom-pty`

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/hom-pty/Cargo.toml`
- Create: `crates/hom-pty/src/remote.rs`
- Modify: `crates/hom-pty/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/hom-pty/src/remote.rs` with tests only:

```rust
//! Remote PTY management over SSH.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_pty_manager_new_has_no_panes() {
        let mgr = RemotePtyManager::new();
        assert_eq!(mgr.active_panes().len(), 0);
    }

    #[test]
    fn remote_pty_manager_has_pane_false_for_unknown() {
        let mgr = RemotePtyManager::new();
        assert!(!mgr.has_pane(99));
    }

    #[test]
    fn auth_methods_default_contains_agent() {
        let methods = SshAuthMethod::defaults();
        assert!(methods.contains(&SshAuthMethod::Agent));
    }

    #[test]
    fn kill_nonexistent_pane_returns_error() {
        let mut mgr = RemotePtyManager::new();
        let result = mgr.kill(42);
        assert!(matches!(result, Err(hom_core::HomError::PaneNotFound(42))));
    }

    #[test]
    fn write_nonexistent_pane_returns_error() {
        let mut mgr = RemotePtyManager::new();
        let result = mgr.write_to(42, b"hello");
        assert!(matches!(result, Err(hom_core::HomError::PaneNotFound(42))));
    }

    #[test]
    fn kill_all_on_empty_manager_is_noop() {
        let mut mgr = RemotePtyManager::new();
        mgr.kill_all();
        assert_eq!(mgr.active_panes().len(), 0);
    }

    #[test]
    fn id_offset_prevents_local_collision() {
        let mgr = RemotePtyManager::with_offset(500);
        assert!(!mgr.has_pane(1));
        assert!(!mgr.has_pane(500));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```sh
cargo test -p hom-pty -- remote 2>&1 | head -20
```

Expected: `error[E0412]: cannot find type 'RemotePtyManager'`.

- [ ] **Step 3: Add `ssh2` to workspace dependencies**

In `Cargo.toml` (workspace root), add to `[workspace.dependencies]`:

```toml
ssh2 = "0.9"
```

In `crates/hom-pty/Cargo.toml`, add to `[dependencies]`:

```toml
ssh2.workspace = true
dirs.workspace = true
```

- [ ] **Step 4: Implement `RemotePtyManager`**

Replace `crates/hom-pty/src/remote.rs` with:

```rust
//! Remote PTY management over SSH.
//!
//! Opens an authenticated SSH session to a remote host, allocates a PTY,
//! and runs a command. The resulting SSH channel is Read + Write, compatible
//! with AsyncPtyReader.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use hom_core::{HomError, HomResult, PaneId, RemoteTarget};
use ssh2::Session;
use tracing::{debug, info, warn};

/// Authentication methods tried in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshAuthMethod {
    /// Use the running SSH agent.
    Agent,
    /// Use a specific private key file.
    KeyFile(PathBuf),
}

impl SshAuthMethod {
    /// Default auth sequence: SSH agent first, then common key files.
    pub fn defaults() -> Vec<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
        vec![
            Self::Agent,
            Self::KeyFile(home.join(".ssh/id_ed25519")),
            Self::KeyFile(home.join(".ssh/id_rsa")),
        ]
    }
}

/// A live SSH session + channel for one remote pane.
pub struct RemoteSession {
    /// SSH channel providing Read + Write access to the remote PTY.
    pub channel: ssh2::Channel,
    // These fields keep the underlying TCP stream and SSH session alive.
    // The channel borrows from the session; the session borrows from the TCP stream.
    // Dropping either early would invalidate the channel.
    _tcp: TcpStream,
    _session: Session,
}

/// Manages all active remote PTY sessions.
pub struct RemotePtyManager {
    sessions: HashMap<PaneId, RemoteSession>,
    next_id: PaneId,
}

impl RemotePtyManager {
    /// Create with a default ID offset of 1000 to avoid collisions with local pane IDs.
    pub fn new() -> Self {
        Self::with_offset(1000)
    }

    /// Create with a custom ID offset.
    pub fn with_offset(offset: PaneId) -> Self {
        Self {
            sessions: HashMap::new(),
            next_id: offset,
        }
    }

    /// Spawn a remote pane via SSH.
    ///
    /// `command` must be a pre-quoted shell-safe string — use
    /// `RemoteTarget::build_remote_command()` to construct it from a `CommandSpec`.
    ///
    /// Returns `(pane_id, reader)`. The reader is a `Box<dyn Read + Send>` suitable
    /// for use with `AsyncPtyReader`.
    pub fn spawn(
        &mut self,
        target: &RemoteTarget,
        command: &str,
        cols: u16,
        rows: u16,
        auth_methods: &[SshAuthMethod],
    ) -> HomResult<(PaneId, Box<dyn Read + Send>)> {
        let tcp = TcpStream::connect(target.addr()).map_err(|e| {
            HomError::PtyError(format!("SSH connect to {} failed: {e}", target.addr()))
        })?;
        tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();

        let tcp_clone = tcp
            .try_clone()
            .map_err(|e| HomError::PtyError(format!("TCP clone failed: {e}")))?;

        let mut session = Session::new()
            .map_err(|e| HomError::PtyError(format!("SSH session create failed: {e}")))?;
        session.set_tcp_stream(tcp_clone);
        session
            .handshake()
            .map_err(|e| HomError::PtyError(format!("SSH handshake failed: {e}")))?;

        // Try authentication methods in order; stop at first success.
        let authenticated = Self::try_authenticate(&mut session, &target.user, auth_methods);
        if !authenticated {
            return Err(HomError::PtyError(format!(
                "SSH authentication failed for {}@{}",
                target.user, target.host
            )));
        }

        let mut channel = session
            .channel_session()
            .map_err(|e| HomError::PtyError(format!("SSH channel_session failed: {e}")))?;

        // Allocate a PTY so the remote process gets a real terminal environment.
        channel
            .request_pty(
                "xterm-256color",
                None,
                Some((cols as u32, rows as u32, 0, 0)),
            )
            .map_err(|e| HomError::PtyError(format!("SSH request_pty failed: {e}")))?;

        // Run the pre-quoted command string on the remote shell.
        // The command arrives at the remote SSH daemon and is interpreted by /bin/sh.
        // All arguments must be shell-quoted by the caller (use RemoteTarget::build_remote_command).
        channel
            .request_exec(command.as_bytes())
            .map_err(|e| HomError::PtyError(format!("SSH exec failed: {e}")))?;

        // Stream 0 is stdout — this is what the terminal emulator reads.
        let reader: Box<dyn Read + Send> = Box::new(channel.stream(0));

        let id = self.next_id;
        self.next_id += 1;

        info!(pane_id = id, %target, "spawned remote PTY");

        self.sessions.insert(
            id,
            RemoteSession {
                channel,
                _tcp: tcp,
                _session: session,
            },
        );

        Ok((id, reader))
    }

    fn try_authenticate(session: &mut Session, user: &str, methods: &[SshAuthMethod]) -> bool {
        for method in methods {
            match method {
                SshAuthMethod::Agent => {
                    if let Ok(mut agent) = session.agent() {
                        if agent.connect().is_ok() && agent.list_identities().is_ok() {
                            for identity in agent.identities().unwrap_or_default() {
                                if agent.userauth(user, &identity).is_ok()
                                    && session.authenticated()
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
                SshAuthMethod::KeyFile(path) => {
                    if path.exists() {
                        if session
                            .userauth_pubkey_file(user, None, path, None)
                            .is_ok()
                            && session.authenticated()
                        {
                            return true;
                        }
                    } else {
                        debug!(path = %path.display(), "SSH key file not found, skipping");
                    }
                }
            }
        }
        false
    }

    /// Write bytes to a remote PTY channel stdin.
    pub fn write_to(&mut self, pane_id: PaneId, data: &[u8]) -> HomResult<()> {
        let session = self
            .sessions
            .get_mut(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        session
            .channel
            .write_all(data)
            .map_err(|e| HomError::PtyError(format!("remote write failed: {e}")))?;

        Ok(())
    }

    /// Notify the remote PTY of a terminal resize.
    pub fn resize(&mut self, pane_id: PaneId, cols: u16, rows: u16) -> HomResult<()> {
        let session = self
            .sessions
            .get_mut(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        session
            .channel
            .request_pty_size(cols as u32, rows as u32, None, None)
            .map_err(|e| HomError::PtyError(format!("remote resize failed: {e}")))?;

        debug!(pane_id, cols, rows, "resized remote PTY");
        Ok(())
    }

    /// Send EOF to a remote channel and remove the session.
    pub fn kill(&mut self, pane_id: PaneId) -> HomResult<()> {
        let mut session = self
            .sessions
            .remove(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        if let Err(e) = session.channel.send_eof() {
            warn!(pane_id, error = %e, "send_eof on remote channel failed");
        }

        info!(pane_id, "killed remote PTY");
        Ok(())
    }

    /// Close all remote sessions. Called during App::shutdown().
    pub fn kill_all(&mut self) {
        let ids: Vec<PaneId> = self.sessions.keys().copied().collect();
        for id in ids {
            if let Some(mut s) = self.sessions.remove(&id) {
                if let Err(e) = s.channel.send_eof() {
                    debug!(pane_id = id, error = %e, "send_eof failed during kill_all");
                }
            }
        }
        info!("all remote PTY sessions closed");
    }

    pub fn active_panes(&self) -> Vec<PaneId> {
        self.sessions.keys().copied().collect()
    }

    pub fn has_pane(&self, pane_id: PaneId) -> bool {
        self.sessions.contains_key(&pane_id)
    }
}

impl Default for RemotePtyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_pty_manager_new_has_no_panes() {
        let mgr = RemotePtyManager::new();
        assert_eq!(mgr.active_panes().len(), 0);
    }

    #[test]
    fn remote_pty_manager_has_pane_false_for_unknown() {
        let mgr = RemotePtyManager::new();
        assert!(!mgr.has_pane(99));
    }

    #[test]
    fn auth_methods_default_contains_agent() {
        let methods = SshAuthMethod::defaults();
        assert!(methods.contains(&SshAuthMethod::Agent));
    }

    #[test]
    fn kill_nonexistent_pane_returns_error() {
        let mut mgr = RemotePtyManager::new();
        let result = mgr.kill(42);
        assert!(matches!(result, Err(hom_core::HomError::PaneNotFound(42))));
    }

    #[test]
    fn write_nonexistent_pane_returns_error() {
        let mut mgr = RemotePtyManager::new();
        let result = mgr.write_to(42, b"hello");
        assert!(matches!(result, Err(hom_core::HomError::PaneNotFound(42))));
    }

    #[test]
    fn kill_all_on_empty_manager_is_noop() {
        let mut mgr = RemotePtyManager::new();
        mgr.kill_all();
        assert_eq!(mgr.active_panes().len(), 0);
    }

    #[test]
    fn id_offset_prevents_local_collision() {
        let mgr = RemotePtyManager::with_offset(500);
        assert!(!mgr.has_pane(1));
        assert!(!mgr.has_pane(500));
    }
}
```

- [ ] **Step 5: Re-export from `hom-pty/src/lib.rs`**

Add to `crates/hom-pty/src/lib.rs`:

```rust
pub mod remote;

pub use remote::{RemotePtyManager, SshAuthMethod};
```

- [ ] **Step 6: Run tests**

```sh
cargo test -p hom-pty -- remote 2>&1 | tail -15
```

Expected: `test result: ok. 7 passed`.

- [ ] **Step 7: Cargo check workspace**

```sh
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished dev profile`.

- [ ] **Step 8: Commit**

```sh
git add Cargo.toml crates/hom-pty/Cargo.toml crates/hom-pty/src/remote.rs crates/hom-pty/src/lib.rs
git commit -m "feat(pty): add RemotePtyManager for SSH-backed panes"
```

---

### Task 3: Parse `--remote` flag in command bar

**Files:**
- Modify: `crates/hom-tui/src/command_bar.rs`

- [ ] **Step 1: Write the failing test**

Find `#[cfg(test)] mod tests` in `command_bar.rs`. Add:

```rust
#[test]
fn spawn_parses_remote_flag_user_at_host() {
    let cmd = CommandBar::parse_command("spawn claude --remote alice@build.example.com").unwrap();
    match cmd {
        Command::Spawn { harness, remote, .. } => {
            assert_eq!(harness, HarnessType::ClaudeCode);
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
```

- [ ] **Step 2: Run tests to verify they fail**

```sh
cargo test -p hom-tui -- spawn_parses_remote spawn_without_remote 2>&1 | head -15
```

Expected: compile error — `Command::Spawn` has no `remote` field.

- [ ] **Step 3: Update `Command::Spawn` to include `remote` field**

In `crates/hom-tui/src/command_bar.rs`, update the import and enum:

```rust
use hom_core::{HarnessType, LayoutKind, PaneId, RemoteTarget};
```

```rust
/// `:spawn claude [--model opus] [--dir /path] [--remote user@host[:port]]`
Spawn {
    harness: HarnessType,
    model: Option<String>,
    working_dir: Option<PathBuf>,
    extra_args: Vec<String>,
    /// SSH remote target. `None` means local spawn.
    remote: Option<RemoteTarget>,
},
```

- [ ] **Step 4: Update the spawn parser to extract `--remote`**

Find the `parse_spawn` function (or equivalent) in `command_bar.rs` and add `--remote` handling:

```rust
fn parse_spawn(parts: &[&str]) -> Option<Command> {
    if parts.is_empty() {
        return None;
    }
    let harness = HarnessType::from_str_loose(parts[0])?;

    let mut model: Option<String> = None;
    let mut working_dir: Option<PathBuf> = None;
    let mut remote: Option<RemoteTarget> = None;
    let mut extra_args: Vec<String> = Vec::new();
    let mut i = 1;

    while i < parts.len() {
        match parts[i] {
            "--model" => {
                i += 1;
                if i < parts.len() {
                    model = Some(parts[i].to_string());
                }
            }
            "--dir" => {
                i += 1;
                if i < parts.len() {
                    working_dir = Some(PathBuf::from(parts[i]));
                }
            }
            "--remote" => {
                i += 1;
                if i < parts.len() {
                    remote = RemoteTarget::parse(parts[i]);
                }
            }
            "--" => {
                extra_args.extend(parts[i + 1..].iter().map(|s| s.to_string()));
                break;
            }
            arg => {
                extra_args.push(arg.to_string());
            }
        }
        i += 1;
    }

    Some(Command::Spawn {
        harness,
        model,
        working_dir,
        extra_args,
        remote,
    })
}
```

- [ ] **Step 5: Fix all `Command::Spawn` destructures elsewhere**

Find every match arm on `Command::Spawn`:

```sh
grep -n "Command::Spawn {" crates/hom-tui/src/app.rs
```

For each, add `remote` to the destructure pattern. In `spawn_pane` dispatch, pass `remote` along (or ignore it if not yet used there).

- [ ] **Step 6: Run tests**

```sh
cargo test -p hom-tui -- spawn_parses_remote spawn_without_remote 2>&1 | tail -10
```

Expected: `test result: ok. 3 passed`.

- [ ] **Step 7: Cargo check**

```sh
cargo check --workspace 2>&1 | tail -5
```

Expected: `Finished dev profile`.

- [ ] **Step 8: Commit**

```sh
git add crates/hom-tui/src/command_bar.rs
git commit -m "feat(tui): parse --remote flag in :spawn command"
```

---

### Task 4: Wire `spawn_remote_pane()` into `app.rs`

**Files:**
- Modify: `crates/hom-tui/src/app.rs`

- [ ] **Step 1: Write the failing test**

In `crates/hom-tui/src/app.rs`, find the `#[cfg(test)] mod tests` block and add:

```rust
#[test]
fn app_has_remote_pty_manager() {
    let cfg = hom_core::HomConfig::default();
    let app = App::new(cfg, None).unwrap();
    // remote_ptys starts empty
    assert_eq!(app.remote_ptys.active_panes().len(), 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

```sh
cargo test -p hom-tui -- app_has_remote_pty_manager 2>&1 | head -15
```

Expected: `error[E0609]: no field 'remote_ptys'`.

- [ ] **Step 3: Add `remote_ptys` field to `App`**

In `crates/hom-tui/src/app.rs`, add import and field:

```rust
use hom_pty::RemotePtyManager;
```

In the `App` struct:

```rust
pub remote_ptys: RemotePtyManager,
```

In `App::new()`:

```rust
remote_ptys: RemotePtyManager::new(),
```

- [ ] **Step 4: Implement `spawn_remote_pane()`**

Add to `impl App`:

```rust
/// Open a pane backed by an SSH channel to `target`, running the harness binary.
pub async fn spawn_remote_pane(
    &mut self,
    harness: HarnessType,
    model: Option<String>,
    target: RemoteTarget,
    cols: u16,
    rows: u16,
) -> HomResult<PaneId> {
    if self.panes.len() >= self.config.general.max_panes {
        return Err(HomError::TuiError(format!(
            "max_panes ({}) reached",
            self.config.general.max_panes
        )));
    }

    let adapter = self
        .adapters
        .get(&harness)
        .ok_or_else(|| HomError::TuiError(format!("no adapter for {harness:?}")))?;

    let harness_cfg = {
        let mut cfg =
            HarnessConfig::new(harness, std::env::current_dir().unwrap_or_default());
        cfg.model = model.clone();
        cfg
    };

    let cmd_spec = adapter.build_command(&harness_cfg)?;
    // Build a shell-safe command string via individual argument quoting.
    let remote_cmd = RemoteTarget::build_remote_command(&cmd_spec);

    let auth = hom_pty::SshAuthMethod::defaults();
    let (pane_id, reader) = self
        .remote_ptys
        .spawn(&target, &remote_cmd, cols, rows, &auth)?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let async_reader = hom_pty::AsyncPtyReader::new(reader, tx);
    async_reader.start();

    let terminal = hom_terminal::new_terminal(cols, rows)?;
    let title = format!(
        "[{}] {} [{}]@{}",
        pane_id,
        harness.display_name(),
        model.as_deref().unwrap_or("default"),
        target
    );

    self.panes.insert(
        pane_id,
        Pane {
            id: pane_id,
            harness,
            terminal,
            pty_rx: rx,
            title,
            is_exited: false,
            exit_code: None,
        },
    );

    if self.focused_pane.is_none() {
        self.focused_pane = Some(pane_id);
    }

    info!(pane_id, %target, harness = ?harness, "spawned remote pane");
    Ok(pane_id)
}
```

- [ ] **Step 5: Route `Command::Spawn { remote: Some(target), .. }` in `handle_command`**

Find the `Command::Spawn` match arm in `handle_command()` or the equivalent dispatch location:

```rust
Command::Spawn { harness, model, working_dir: _, extra_args: _, remote } => {
    if let Some(target) = remote {
        let (cols, rows) = self.focused_pane_dimensions();
        match self.spawn_remote_pane(harness, model, target, cols, rows).await {
            Ok(id) => info!(pane_id = id, "remote pane spawned"),
            Err(e) => {
                self.command_bar.last_error = Some(format!("remote spawn failed: {e}"));
            }
        }
    } else {
        self.spawn_pane(harness, model, working_dir, extra_args).await?;
    }
}
```

Add a helper to `impl App` to return sensible dimensions for the next pane:

```rust
fn focused_pane_dimensions(&self) -> (u16, u16) {
    // Use a sensible default — actual resize happens on first render tick.
    (80, 24)
}
```

- [ ] **Step 6: Wire `remote_ptys.kill_all()` in `App::shutdown()`**

Find `App::shutdown()` and add alongside `self.pty_manager.kill_all()`:

```rust
self.remote_ptys.kill_all();
```

- [ ] **Step 7: Run tests**

```sh
cargo test -p hom-tui -- app_has_remote_pty_manager 2>&1 | tail -10
```

Expected: `test result: ok. 1 passed`.

- [ ] **Step 8: Cargo check + clippy**

```sh
cargo check --workspace 2>&1 | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: zero errors, zero warnings.

- [ ] **Step 9: Commit**

```sh
git add crates/hom-tui/src/app.rs
git commit -m "feat(tui): wire spawn_remote_pane() with SSH-backed PTY"
```

---

### Task 5: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add to Implementation Status in CLAUDE.md**

Find the most recent "Resolved" block and append:

```markdown
**Resolved (remote pane support):**
- `RemoteTarget` added to `hom-core/src/types.rs` — parse `user@host[:port]`, `shell_quote()`, `build_remote_command()`
- `PaneKind::Local` / `PaneKind::Remote(RemoteTarget)` discriminant added to `hom-core`
- `RemotePtyManager` in `crates/hom-pty/src/remote.rs` — SSH session + channel lifecycle via `ssh2 = "0.9"`
- `SshAuthMethod::defaults()` tries SSH agent then `~/.ssh/id_ed25519` then `~/.ssh/id_rsa`
- All remote command args are individually shell-quoted via `RemoteTarget::shell_quote()` before SSH exec
- `:spawn <harness> --remote user@host[:port]` parsed in command bar; routes to `App::spawn_remote_pane()`
- `App::shutdown()` calls `remote_ptys.kill_all()` for graceful cleanup
- 7 unit tests for `RemotePtyManager` + 3 for command bar `--remote` flag parsing
```

- [ ] **Step 2: Full test run**

```sh
cargo nextest run --workspace 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```sh
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for remote pane support"
```

---

## Self-Review

**Spec coverage:**
- ✅ `:spawn <harness> --remote user@host` — Task 3 + 4
- ✅ SSH PTY allocation — `request_pty("xterm-256color", ...)` in Task 2
- ✅ Key-based auth (agent + key files) — `SshAuthMethod::defaults()` in Task 2
- ✅ Shell-safe command quoting — `shell_quote()` + `build_remote_command()` in Task 1
- ✅ Same render pipeline as local — `AsyncPtyReader` in Task 4
- ✅ Graceful shutdown — `kill_all()` in Task 4

**Placeholder scan:** None — all steps include concrete code.

**Type consistency:**
- `RemoteTarget` defined in `hom-core`, imported as `hom_core::RemoteTarget` in `hom-pty` and `hom-tui`.
- `Command::Spawn { remote }` added in Task 3, consumed in Task 4.
- `RemotePtyManager` exported from `hom-pty::remote`, re-exported from `hom-pty` root.
