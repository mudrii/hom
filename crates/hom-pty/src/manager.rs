//! PTY lifecycle management: spawn, read, write, resize, kill.

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, PtySystem, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use tracing::{debug, info};

use hom_core::{CommandSpec, HomError, HomResult, PaneId};

/// Holds a spawned PTY process and its master handle.
pub struct PtyInstance {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub writer: Box<dyn Write + Send>,
    pub reader: Box<dyn Read + Send>,
}

/// Manages all active PTY instances.
pub struct PtyManager {
    instances: HashMap<PaneId, PtyInstance>,
    next_id: PaneId,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
            next_id: 1,
        }
    }

    /// Spawn a new PTY with the given command and dimensions.
    /// Returns the pane ID assigned to this PTY.
    pub fn spawn(&mut self, spec: &CommandSpec, cols: u16, rows: u16) -> HomResult<PaneId> {
        let pty_system = native_pty_system();
        self.spawn_with_system(pty_system.as_ref(), spec, cols, rows)
    }

    fn spawn_with_system(
        &mut self,
        pty_system: &dyn PtySystem,
        spec: &CommandSpec,
        cols: u16,
        rows: u16,
    ) -> HomResult<PaneId> {
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| HomError::PtyError(format!("openpty failed: {e}")))?;

        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        cmd.cwd(&spec.working_dir);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| HomError::PtyError(format!("spawn failed: {e}")))?;

        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(e) => {
                cleanup_failed_child(child);
                return Err(HomError::PtyError(format!("take_writer: {e}")));
            }
        };

        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(e) => {
                cleanup_failed_child(child);
                return Err(HomError::PtyError(format!("clone_reader: {e}")));
            }
        };

        let id = self.next_id;
        self.next_id += 1;

        info!(pane_id = id, program = %spec.program, "spawned PTY");

        self.instances.insert(
            id,
            PtyInstance {
                master: pair.master,
                child,
                writer,
                reader,
            },
        );

        Ok(id)
    }

    /// Write bytes to a PTY's stdin.
    pub fn write_to(&mut self, pane_id: PaneId, data: &[u8]) -> HomResult<()> {
        let instance = self
            .instances
            .get_mut(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        instance
            .writer
            .write_all(data)
            .map_err(|e| HomError::PtyError(format!("write failed: {e}")))?;

        instance
            .writer
            .flush()
            .map_err(|e| HomError::PtyError(format!("flush failed: {e}")))?;

        Ok(())
    }

    /// Resize a PTY.
    pub fn resize(&mut self, pane_id: PaneId, cols: u16, rows: u16) -> HomResult<()> {
        let instance = self
            .instances
            .get_mut(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        instance
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| HomError::PtyError(format!("resize failed: {e}")))?;

        debug!(pane_id, cols, rows, "resized PTY");
        Ok(())
    }

    /// Kill a PTY process and remove it.
    pub fn kill(&mut self, pane_id: PaneId) -> HomResult<()> {
        let mut instance = self
            .instances
            .remove(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        instance
            .child
            .kill()
            .map_err(|e| HomError::PtyError(format!("kill failed: {e}")))?;

        info!(pane_id, "killed PTY");
        Ok(())
    }

    /// Check if a child process has exited.
    pub fn try_wait(&mut self, pane_id: PaneId) -> HomResult<Option<u32>> {
        let instance = self
            .instances
            .get_mut(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        match instance.child.try_wait() {
            Ok(Some(status)) => Ok(Some(status.exit_code())),
            Ok(None) => Ok(None),
            Err(e) => Err(HomError::PtyError(format!("try_wait: {e}"))),
        }
    }

    /// Take the reader for a pane (consumes it — use for async reader setup).
    pub fn take_reader(&mut self, pane_id: PaneId) -> HomResult<Box<dyn Read + Send>> {
        let instance = self
            .instances
            .get_mut(&pane_id)
            .ok_or(HomError::PaneNotFound(pane_id))?;

        // Swap in a dummy reader
        let reader = std::mem::replace(&mut instance.reader, Box::new(std::io::empty()));
        Ok(reader)
    }

    /// Get all active pane IDs.
    pub fn active_panes(&self) -> Vec<PaneId> {
        self.instances.keys().copied().collect()
    }

    /// Check if a pane exists.
    pub fn has_pane(&self, pane_id: PaneId) -> bool {
        self.instances.contains_key(&pane_id)
    }

    /// Kill all active PTY processes. Used during shutdown cleanup.
    pub fn kill_all(&mut self) {
        let pane_ids: Vec<PaneId> = self.instances.keys().copied().collect();
        for pane_id in pane_ids {
            if let Some(mut instance) = self.instances.remove(&pane_id)
                && let Err(e) = instance.child.kill()
            {
                debug!(pane_id, error = %e, "failed to kill PTY during shutdown");
            }
        }
        info!("all PTY processes killed");
    }
}

fn cleanup_failed_child(mut child: Box<dyn Child + Send + Sync>) {
    if let Err(e) = child.kill() {
        debug!(error = %e, "failed to kill child after PTY setup error");
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    #[cfg(unix)]
    use libc::pid_t;
    use std::io::Read as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use portable_pty::{ChildKiller, ExitStatus, PtyPair, SlavePty};

    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn read_once_with_timeout(
        mut reader: Box<dyn std::io::Read + Send>,
        timeout: Duration,
    ) -> Vec<u8> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            let n = reader.read(&mut buf).unwrap_or(0);
            let _ = tx.send(buf[..n].to_vec());
        });

        rx.recv_timeout(timeout)
            .expect("PTY read timed out before output arrived")
    }

    #[test]
    fn test_kill_all_empties_instances() {
        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id1 = mgr.spawn(&spec, 80, 24).unwrap();
        let id2 = mgr.spawn(&spec, 80, 24).unwrap();
        assert_eq!(mgr.active_panes().len(), 2);

        mgr.kill_all();
        assert!(mgr.active_panes().is_empty());
        assert!(!mgr.has_pane(id1));
        assert!(!mgr.has_pane(id2));
    }

    #[test]
    fn test_kill_all_on_empty_manager() {
        let mut mgr = PtyManager::new();
        mgr.kill_all();
        assert!(mgr.active_panes().is_empty());
    }

    #[test]
    fn test_spawn_and_read_output() {
        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "echo hello_from_pty".to_string()],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id = mgr.spawn(&spec, 80, 24).unwrap();
        let reader = mgr.take_reader(id).unwrap();
        let output = read_once_with_timeout(reader, Duration::from_secs(2));
        let output = String::from_utf8_lossy(&output);

        assert!(
            output.contains("hello_from_pty"),
            "expected 'hello_from_pty' in PTY output, got: {output}"
        );

        mgr.kill_all();
    }

    #[test]
    fn test_spawn_and_write_input() {
        let mut mgr = PtyManager::new();
        let spec = CommandSpec {
            program: "cat".to_string(),
            args: vec![],
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let id = mgr.spawn(&spec, 80, 24).unwrap();

        mgr.write_to(id, b"test_input\n").unwrap();
        let reader = mgr.take_reader(id).unwrap();
        let output = read_once_with_timeout(reader, Duration::from_secs(2));
        let output = String::from_utf8_lossy(&output);

        assert!(
            output.contains("test_input"),
            "expected echo of 'test_input', got: {output}"
        );

        mgr.kill_all();
    }

    #[derive(Debug)]
    struct FakeChild {
        killed: Arc<AtomicUsize>,
    }

    impl ChildKiller for FakeChild {
        fn kill(&mut self) -> std::io::Result<()> {
            self.killed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(Self {
                killed: self.killed.clone(),
            })
        }
    }

    impl portable_pty::Child for FakeChild {
        fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
            Ok(None)
        }

        fn wait(&mut self) -> std::io::Result<ExitStatus> {
            Ok(ExitStatus::with_exit_code(0))
        }

        fn process_id(&self) -> Option<u32> {
            Some(42)
        }
    }

    struct FakeMaster {
        fail_writer: bool,
    }

    impl portable_pty::MasterPty for FakeMaster {
        fn resize(&self, _size: portable_pty::PtySize) -> Result<()> {
            Ok(())
        }

        fn get_size(&self) -> Result<portable_pty::PtySize> {
            Ok(portable_pty::PtySize::default())
        }

        fn try_clone_reader(&self) -> Result<Box<dyn std::io::Read + Send>> {
            Ok(Box::new(std::io::Cursor::new(Vec::<u8>::new())))
        }

        fn take_writer(&self) -> Result<Box<dyn std::io::Write + Send>> {
            if self.fail_writer {
                Err(std::io::Error::other("boom").into())
            } else {
                Ok(Box::new(std::io::sink()))
            }
        }

        #[cfg(unix)]
        fn process_group_leader(&self) -> Option<pid_t> {
            None
        }

        #[cfg(unix)]
        fn as_raw_fd(&self) -> Option<std::os::fd::RawFd> {
            None
        }

        #[cfg(unix)]
        fn tty_name(&self) -> Option<std::path::PathBuf> {
            None
        }
    }

    struct FakeSlave {
        killed: Arc<AtomicUsize>,
    }

    impl SlavePty for FakeSlave {
        fn spawn_command(
            &self,
            _cmd: portable_pty::CommandBuilder,
        ) -> Result<Box<dyn portable_pty::Child + Send + Sync>> {
            Ok(Box::new(FakeChild {
                killed: self.killed.clone(),
            }))
        }
    }

    struct FakePtySystem {
        killed: Arc<AtomicUsize>,
        fail_writer: bool,
    }

    impl portable_pty::PtySystem for FakePtySystem {
        fn openpty(&self, _size: portable_pty::PtySize) -> Result<PtyPair> {
            Ok(PtyPair {
                slave: Box::new(FakeSlave {
                    killed: self.killed.clone(),
                }),
                master: Box::new(FakeMaster {
                    fail_writer: self.fail_writer,
                }),
            })
        }
    }

    #[test]
    fn spawn_kills_child_if_take_writer_fails() {
        let killed = Arc::new(AtomicUsize::new(0));
        let pty_system = FakePtySystem {
            killed: killed.clone(),
            fail_writer: true,
        };
        let spec = CommandSpec {
            program: "fake".to_string(),
            args: Vec::new(),
            env: std::collections::HashMap::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        };
        let mut mgr = PtyManager::new();

        let err = mgr
            .spawn_with_system(&pty_system, &spec, 80, 24)
            .unwrap_err();

        assert!(err.to_string().contains("take_writer"));
        assert_eq!(killed.load(Ordering::SeqCst), 1);
        assert!(mgr.active_panes().is_empty());
    }
}
