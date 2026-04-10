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
    handle: tokio::task::JoinHandle<()>,
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
            handle,
        }
    }

    /// Signal the background reader task to stop.
    ///
    /// For `spawn_blocking` tasks, this detaches the task rather than
    /// immediately terminating it — the blocking thread exits when its
    /// next `read()` call returns (which happens when the PTY fd closes).
    /// Calling `abort()` is still useful to reduce the detach window.
    pub fn abort(&self) {
        self.handle.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_abort_does_not_panic() {
        // Create a reader over an in-memory cursor — it exhausts quickly.
        // abort() should be safe to call at any point, even after task exit.
        let cursor = Box::new(Cursor::new(b"hello".to_vec())) as Box<dyn Read + Send>;
        let reader = AsyncPtyReader::start(99, cursor);
        reader.abort();
    }
}
