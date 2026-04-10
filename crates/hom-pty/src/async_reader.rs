//! Async wrapper around blocking PTY reads using tokio::task::spawn_blocking.

use std::io::Read;

use tokio::sync::mpsc;
use tracing::{debug, error, trace};

use hom_core::PaneId;

/// Reads from a PTY in a background thread and sends chunks to a channel.
///
/// The channel receiver should be used to feed data into the terminal emulator.
pub struct AsyncPtyReader {
    pub pane_id: PaneId,
    pub rx: mpsc::Receiver<Vec<u8>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl AsyncPtyReader {
    /// Start reading from a PTY reader in a background thread.
    ///
    /// Returns an `AsyncPtyReader` whose `.rx` yields chunks of bytes.
    /// The reader stops when the PTY is closed (read returns 0 or error).
    pub fn start(pane_id: PaneId, mut reader: Box<dyn Read + Send>) -> Self {
        let (tx, rx) = mpsc::channel(256);

        let handle = tokio::task::spawn_blocking(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!(pane_id, "PTY reader got EOF");
                        break;
                    }
                    Ok(n) => {
                        trace!(pane_id, bytes = n, "PTY read");
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            debug!(pane_id, "PTY reader channel closed");
                            break;
                        }
                    }
                    Err(e) => {
                        error!(pane_id, error = %e, "PTY read error");
                        break;
                    }
                }
            }
        });

        Self {
            pane_id,
            rx,
            _handle: handle,
        }
    }
}
