//! Benchmarks for terminal emulation and screen snapshot rendering.
//!
//! Targets NFR requirements:
//!   NF1: < 16ms render (60fps)
//!   NF3: < 30MB per pane memory

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use hom_core::TerminalBackend;

/// Benchmark the cost of processing PTY output through the vt100 backend.
fn bench_terminal_process(c: &mut Criterion) {
    let mut terminal = hom_terminal::create_terminal(120, 40, 10_000);

    // Simulate a typical PTY output burst — mixed text and ANSI escapes
    let data: Vec<u8> = (0..4096)
        .map(|i| match i % 50 {
            0 => b'\n',
            1..=3 => b'\x1b', // ESC sequences scattered in
            _ => b'A' + (i % 26) as u8,
        })
        .collect();

    c.bench_function("terminal_process_4kb", |b| {
        b.iter(|| {
            terminal.process(black_box(&data));
        });
    });
}

/// Benchmark taking a screen snapshot (the operation done every render frame).
fn bench_screen_snapshot(c: &mut Criterion) {
    let mut terminal = hom_terminal::create_terminal(120, 40, 10_000);

    // Fill the terminal with some content
    let fill: Vec<u8> = (0..4800)
        .map(|i| if i % 120 == 119 { b'\n' } else { b'X' })
        .collect();
    terminal.process(&fill);

    c.bench_function("screen_snapshot_120x40", |b| {
        b.iter(|| {
            let snap = terminal.screen_snapshot();
            black_box(snap);
        });
    });
}

/// Benchmark the full render cycle: process + snapshot + text extraction.
fn bench_render_cycle(c: &mut Criterion) {
    let mut terminal = hom_terminal::create_terminal(120, 40, 10_000);

    let data: Vec<u8> = (0..2048)
        .map(|i| if i % 120 == 119 { b'\n' } else { b'Z' })
        .collect();

    c.bench_function("render_cycle_process_and_snapshot", |b| {
        b.iter(|| {
            terminal.process(black_box(&data));
            let snap = terminal.screen_snapshot();
            let text = snap.text();
            black_box(text);
        });
    });
}

/// Benchmark simulated startup: config load + terminal creation.
/// Target: NF4 < 500ms startup time.
fn bench_startup(c: &mut Criterion) {
    c.bench_function("startup_config_and_terminal", |b| {
        b.iter(|| {
            let config = hom_core::HomConfig::load().unwrap_or_default();
            let terminal = hom_terminal::create_terminal(120, 40, config.general.max_scrollback);
            black_box((config, terminal));
        });
    });
}

/// Benchmark memory estimation for a pane: create terminal + fill with content.
/// Target: NF3 < 30MB per pane.
fn bench_memory_per_pane(c: &mut Criterion) {
    c.bench_function("pane_memory_120x40_10k_scrollback", |b| {
        b.iter(|| {
            let mut terminal = hom_terminal::create_terminal(120, 40, 10_000);
            // Fill terminal with a full screen of content
            let data: Vec<u8> = (0..4800)
                .map(|i| if i % 120 == 119 { b'\n' } else { b'A' })
                .collect();
            terminal.process(&data);
            let snap = terminal.screen_snapshot();
            black_box(snap);
        });
    });
}

/// Benchmark input encoding latency.
/// Target: NF2 < 50ms keystroke delivery.
fn bench_input_encoding(c: &mut Criterion) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    c.bench_function("input_encode_1000_keystrokes", |b| {
        let keys: Vec<KeyEvent> = (b'a'..=b'z')
            .cycle()
            .take(1000)
            .map(|c| KeyEvent::new(KeyCode::Char(c as char), KeyModifiers::empty()))
            .collect();
        b.iter(|| {
            for key in &keys {
                let bytes = hom_tui::input::encode_key_event(key);
                black_box(bytes);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_terminal_process,
    bench_screen_snapshot,
    bench_render_cycle,
    bench_startup,
    bench_memory_per_pane,
    bench_input_encoding,
);
criterion_main!(benches);
