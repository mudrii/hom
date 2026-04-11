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
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        vec![
            Self::Agent,
            Self::KeyFile(home.join(".ssh/id_ed25519")),
            Self::KeyFile(home.join(".ssh/id_rsa")),
        ]
    }
}

/// A live SSH session + channel for one remote pane.
///
/// The `Session` and `TcpStream` are stored here to keep them alive for the
/// duration of the channel (libssh2 requires the underlying TCP connection to
/// remain open while a channel is in use).
pub struct RemoteSession {
    /// SSH channel providing Read + Write access to the remote PTY.
    pub(crate) channel: ssh2::Channel,
    // Keep the session and TCP stream alive alongside the channel.
    _session: Session,
    _tcp: TcpStream,
}

/// A fully connected remote PTY ready to be inserted into the manager.
pub struct ConnectedRemotePty {
    pub session: RemoteSession,
    pub reader: Box<dyn Read + Send>,
}

// SAFETY: `RemoteSession` is used from the single-threaded TUI event loop for writes
// and control (write_to, resize, send_eof). The cloned channel handle passed to
// `AsyncPtyReader` is used for reads only in a spawn_blocking task.
//
// `ssh2::Channel` is backed by an `Arc`-protected libssh2 channel handle. The read
// and write paths use separate libssh2 internal buffers. Concurrent read (reader task)
// and write (event loop) on separate halves is the intended usage pattern.
//
// Known limitation: libssh2 is not documented as fully thread-safe. If this causes
// issues in practice, move to a Mutex<ssh2::Channel> shared between reader and writer.
unsafe impl Send for RemoteSession {}

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
    /// `command` must be a shell-safe string — use `RemoteTarget::build_remote_command()`
    /// to construct it from a `CommandSpec`.
    ///
    /// Returns `(pane_id, reader)`. The reader is a `Box<dyn Read + Send>` suitable
    /// for `AsyncPtyReader`.
    pub fn spawn(
        &mut self,
        target: &RemoteTarget,
        command: &str,
        env: &std::collections::HashMap<String, String>,
        cols: u16,
        rows: u16,
        auth_methods: &[SshAuthMethod],
    ) -> HomResult<(PaneId, Box<dyn std::io::Read + Send>)> {
        let pane_id = self.reserve_pane_id();
        let connected = Self::connect(
            target.clone(),
            command.to_string(),
            env.clone(),
            cols,
            rows,
            auth_methods.to_vec(),
        )?;

        info!(pane_id, %target, "spawned remote PTY");
        self.insert_session(pane_id, connected.session);
        Ok((pane_id, connected.reader))
    }

    /// Reserve a pane ID for a remote pane before the blocking SSH setup starts.
    pub fn reserve_pane_id(&mut self) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Insert a connected session for a previously reserved pane ID.
    pub fn insert_session(&mut self, pane_id: PaneId, session: RemoteSession) {
        self.sessions.insert(pane_id, session);
    }

    /// Perform the blocking SSH setup required for a remote PTY.
    ///
    /// This is intended to run inside `tokio::task::spawn_blocking`.
    pub fn connect(
        target: RemoteTarget,
        command: String,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
        auth_methods: Vec<SshAuthMethod>,
    ) -> HomResult<ConnectedRemotePty> {
        let tcp = TcpStream::connect(target.addr()).map_err(|e| {
            HomError::PtyError(format!("SSH connect to {} failed: {e}", target.addr()))
        })?;
        tcp.set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| HomError::PtyError(format!("SSH read timeout setup failed: {e}")))?;

        let mut session = Session::new()
            .map_err(|e| HomError::PtyError(format!("SSH session create failed: {e}")))?;
        session.set_tcp_stream(
            tcp.try_clone()
                .map_err(|e| HomError::PtyError(format!("TCP clone failed: {e}")))?,
        );
        session
            .handshake()
            .map_err(|e| HomError::PtyError(format!("SSH handshake failed: {e}")))?;

        if !Self::try_authenticate(&mut session, &target.user, &auth_methods) {
            return Err(HomError::PtyError(format!(
                "SSH authentication failed for {}@{}",
                target.user, target.host
            )));
        }

        let mut channel = session
            .channel_session()
            .map_err(|e| HomError::PtyError(format!("SSH channel_session failed: {e}")))?;

        channel
            .request_pty(
                "xterm-256color",
                None,
                Some((cols as u32, rows as u32, 0, 0)),
            )
            .map_err(|e| HomError::PtyError(format!("SSH request_pty failed: {e}")))?;

        for (key, value) in &env {
            channel
                .setenv(key, value)
                .map_err(|e| HomError::PtyError(format!("SSH setenv {key} failed: {e}")))?;
        }

        channel
            .exec(&command)
            .map_err(|e| HomError::PtyError(format!("SSH channel exec failed: {e}")))?;

        Ok(ConnectedRemotePty {
            reader: Box::new(channel.clone()),
            session: RemoteSession {
                channel,
                _session: session,
                _tcp: tcp,
            },
        })
    }

    fn try_authenticate(session: &mut Session, user: &str, methods: &[SshAuthMethod]) -> bool {
        for method in methods {
            match method {
                SshAuthMethod::Agent => {
                    if let Ok(mut agent) = session.agent()
                        && agent.connect().is_ok()
                        && agent.list_identities().is_ok()
                    {
                        for identity in agent.identities().unwrap_or_else(|e| {
                            debug!("SSH agent identities() failed: {e}");
                            vec![]
                        }) {
                            if agent.userauth(user, &identity).is_ok() && session.authenticated() {
                                return true;
                            }
                        }
                    }
                }
                SshAuthMethod::KeyFile(path) => {
                    if path.exists() {
                        if session.userauth_pubkey_file(user, None, path, None).is_ok()
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

    /// Remove a pane by ID, sending EOF and dropping the session.
    ///
    /// Alias for [`kill`](Self::kill) — use whichever name reads better at the call site.
    pub fn kill_pane(&mut self, pane_id: PaneId) -> HomResult<()> {
        self.kill(pane_id)
    }

    /// Close all remote sessions. Called during App::shutdown().
    pub fn kill_all(&mut self) {
        let ids: Vec<PaneId> = self.sessions.keys().copied().collect();
        for id in &ids {
            if let Some(mut s) = self.sessions.remove(id)
                && let Err(e) = s.channel.send_eof()
            {
                debug!(pane_id = id, error = %e, "send_eof failed during kill_all");
            }
        }
        if !ids.is_empty() {
            info!("all remote PTY sessions closed");
        }
    }

    /// Returns all active remote pane IDs.
    pub fn active_panes(&self) -> Vec<PaneId> {
        self.sessions.keys().copied().collect()
    }

    /// Returns true if the given pane ID is managed by this manager.
    pub fn has_pane(&self, pane_id: PaneId) -> bool {
        self.sessions.contains_key(&pane_id)
    }

    /// Check whether the remote process has exited.
    ///
    /// Returns `Ok(Some(code))` when the channel has received EOF from the remote
    /// side (`channel.eof()` is true). Returns `Ok(None)` while the process is
    /// still running. The exit status is read from the SSH channel's cached value.
    pub fn try_wait(&self, pane_id: PaneId) -> HomResult<Option<u32>> {
        let session = self
            .sessions
            .get(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;
        if session.channel.eof() {
            let code = session.channel.exit_status().map(|c| c as u32).unwrap_or(1);
            Ok(Some(code))
        } else {
            Ok(None)
        }
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
