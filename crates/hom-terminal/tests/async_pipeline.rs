//! Integration tests for the AsyncPtyReader → TerminalBackend pipeline.
//!
//! These tests verify the full byte-flow path that the production event loop
//! uses: PTY spawns a real process, bytes travel through the tokio channel
//! bridge (AsyncPtyReader), and land in the terminal emulator.
//!
//! Covers the gap left by the synchronous pipeline test in fallback_vt100.rs,
//! which bypasses AsyncPtyReader and reads directly from the PTY.

use std::collections::HashMap;
use std::time::Duration;

use hom_core::{CommandSpec, TerminalBackend};
use hom_pty::{AsyncPtyReader, PtyManager};
use hom_terminal::create_terminal;

/// Drain all pending messages from an AsyncPtyReader channel with a timeout.
///
/// Returns the accumulated bytes. Stops when the channel is empty for
/// `quiet_ms` milliseconds in a row, or when `max_ms` total time elapses.
async fn drain_channel(
    rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    max_ms: u64,
    quiet_ms: u64,
) -> Vec<u8> {
    let mut buf = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(max_ms);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(quiet_ms), rx.recv()).await {
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk),
            Ok(None) => break, // channel closed
            Err(_) => break,   // quiet_ms elapsed with no new data
        }
    }
    buf
}

fn echo_spec(message: &str) -> CommandSpec {
    CommandSpec {
        program: "sh".to_string(),
        args: vec!["-c".to_string(), format!("echo {message}")],
        env: HashMap::new(),
        working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
    }
}

/// Spawn a PTY, pipe its output through AsyncPtyReader, feed bytes into the
/// terminal emulator, and assert the text appears in the screen snapshot.
///
/// This is the exact path used by `App::poll_pty_output()` in the main loop.
#[tokio::test]
async fn test_async_reader_feeds_terminal() {
    let mut mgr = PtyManager::new();
    let id = mgr.spawn(&echo_spec("ASYNC_PIPELINE_OK"), 80, 24).unwrap();
    let raw_reader = mgr.take_reader(id).unwrap();

    let mut async_reader = AsyncPtyReader::start(id, raw_reader);
    let bytes = drain_channel(&mut async_reader.rx, 2000, 200).await;
    async_reader.abort();
    mgr.kill_all();

    assert!(!bytes.is_empty(), "expected bytes from PTY, got none");

    let mut terminal = create_terminal(80, 24, 500);
    terminal.process(&bytes);

    let snap = terminal.screen_snapshot();
    let text = snap.text();
    assert!(
        text.contains("ASYNC_PIPELINE_OK"),
        "expected 'ASYNC_PIPELINE_OK' in snapshot, got: {text}"
    );
}

/// Write input to a PTY running `cat`, read the echo back through
/// AsyncPtyReader, and verify the terminal emulator sees the echoed text.
///
/// This covers the write→PTY→read half of the pipeline.
#[tokio::test]
async fn test_write_read_roundtrip_through_terminal() {
    let mut mgr = PtyManager::new();
    let spec = CommandSpec {
        program: "cat".to_string(),
        args: vec![],
        env: HashMap::new(),
        working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
    };
    let id = mgr.spawn(&spec, 80, 24).unwrap();
    let raw_reader = mgr.take_reader(id).unwrap();

    let mut async_reader = AsyncPtyReader::start(id, raw_reader);

    // Let `cat` start up before writing.
    tokio::time::sleep(Duration::from_millis(50)).await;
    mgr.write_to(id, b"ROUNDTRIP_CHECK\n").unwrap();

    let bytes = drain_channel(&mut async_reader.rx, 2000, 300).await;
    async_reader.abort();
    mgr.kill_all();

    assert!(!bytes.is_empty(), "expected bytes from PTY, got none");

    let mut terminal = create_terminal(80, 24, 500);
    terminal.process(&bytes);

    let snap = terminal.screen_snapshot();
    let text = snap.text();
    assert!(
        text.contains("ROUNDTRIP_CHECK"),
        "expected 'ROUNDTRIP_CHECK' in snapshot after write→read roundtrip, got: {text}"
    );
}

/// Verify that ANSI color escape sequences survive the full PTY pipeline
/// and are correctly parsed into cell attributes by the terminal emulator.
///
/// Uses `printf` to emit ESC[31m (red) around a sentinel string.
#[tokio::test]
async fn test_ansi_color_survives_pipeline() {
    let mut mgr = PtyManager::new();
    let spec = CommandSpec {
        program: "sh".to_string(),
        args: vec![
            "-c".to_string(),
            r#"printf '\033[31mCOLOR_TEST\033[0m\n'"#.to_string(),
        ],
        env: HashMap::new(),
        working_dir: std::env::current_dir().unwrap_or_else(|_| ".".into()),
    };
    let id = mgr.spawn(&spec, 80, 24).unwrap();
    let raw_reader = mgr.take_reader(id).unwrap();

    let mut async_reader = AsyncPtyReader::start(id, raw_reader);
    let bytes = drain_channel(&mut async_reader.rx, 2000, 200).await;
    async_reader.abort();
    mgr.kill_all();

    assert!(!bytes.is_empty(), "expected bytes from PTY, got none");

    let mut terminal = create_terminal(80, 24, 500);
    terminal.process(&bytes);

    let snap = terminal.screen_snapshot();

    // Find "COLOR_TEST" in the snapshot and verify cell fg color is Red.
    let text = snap.text();
    assert!(
        text.contains("COLOR_TEST"),
        "expected 'COLOR_TEST' in snapshot, got: {text}"
    );

    // Locate the 'C' of COLOR_TEST and check its fg color.
    'outer: for row in &snap.rows {
        for (col_idx, cell) in row.iter().enumerate() {
            if cell.character == 'C'
                && row.get(col_idx + 1).map(|c| c.character) == Some('O')
                && row.get(col_idx + 2).map(|c| c.character) == Some('L')
            {
                assert_eq!(
                    cell.fg,
                    hom_core::TermColor::Red,
                    "expected Red fg on 'C' of COLOR_TEST, got: {:?}",
                    cell.fg
                );
                break 'outer;
            }
        }
    }
}

/// Verify that abort() is safe to call after the reader has already finished.
///
/// Guard against a regression where abort() on a completed JoinHandle panics.
#[tokio::test]
async fn test_abort_after_completion_is_safe() {
    let mut mgr = PtyManager::new();
    let id = mgr.spawn(&echo_spec("ABORT_SAFE"), 80, 24).unwrap();
    let raw_reader = mgr.take_reader(id).unwrap();

    let mut async_reader = AsyncPtyReader::start(id, raw_reader);
    // Drain fully — the task exits naturally after EOF.
    let _ = drain_channel(&mut async_reader.rx, 1000, 300).await;

    // abort() after natural completion must not panic.
    async_reader.abort();
    mgr.kill_all();
}
