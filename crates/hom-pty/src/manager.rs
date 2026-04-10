//! PTY lifecycle management: spawn, read, write, resize, kill.

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
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

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| HomError::PtyError(format!("take_writer: {e}")))?;

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| HomError::PtyError(format!("clone_reader: {e}")))?;

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
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}
