//! CLI progress rendering for `get`/`downsync`.
//!
//! Implements [`longtail::ProgressSink`]. On an interactive stderr it drives an
//! `indicatif` bar with a steady tick — so the pre-download indexing/diff phases
//! (which emit `on_phase` but no `on_progress`) still animate a spinner rather
//! than sitting silent. When stderr is not a terminal (piped / CI) it falls back
//! to throttled `eprintln!` lines so logs stay readable.
//!
//! The library already reports incrementally (per completed block in
//! `apply.rs`, plus phase markers) through a facade-side rate limiter; the CLI
//! previously attached no sink, so all of that was discarded to `NullProgress`.

use std::io::IsTerminal;
use std::sync::Mutex;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use longtail::ProgressSink;

/// A terminal-aware progress sink for the CLI.
pub enum CliProgress {
    /// Interactive: an indicatif bar drawn on stderr with a steady tick.
    Bar(ProgressBar),
    /// Non-interactive: throttled plain-text lines on stderr.
    Plain(Mutex<PlainState>),
}

/// State for the non-TTY fallback (guarded by a `Mutex` since `ProgressSink`
/// takes `&self`).
pub struct PlainState {
    phase: String,
    /// Last printed decile (`done*10/total`), or `-1` before the first print.
    last_decile: i32,
}

impl CliProgress {
    /// Build a sink, choosing the bar or plain renderer from whether stderr is a
    /// terminal.
    pub fn new() -> CliProgress {
        if std::io::stderr().is_terminal() {
            // Length unknown until the first `on_progress`; start as a spinner so
            // the indexing/diff phases animate. `{wide_bar}` renders empty until a
            // length is set, then fills.
            let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} {msg:20} [{wide_bar:.cyan/blue}] {pos}/{len} ({elapsed})",
                )
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=>-"),
            );
            bar.enable_steady_tick(Duration::from_millis(120));
            CliProgress::Bar(bar)
        } else {
            CliProgress::Plain(Mutex::new(PlainState {
                phase: String::new(),
                last_decile: -1,
            }))
        }
    }

    /// Finish and clear any live bar. Call before printing stats or an error so
    /// the leftover bar line does not clash with subsequent output.
    pub fn finish(&self) {
        if let CliProgress::Bar(bar) = self {
            bar.finish_and_clear();
        }
    }
}

impl ProgressSink for CliProgress {
    fn on_progress(&self, done: u32, total: u32) {
        match self {
            CliProgress::Bar(bar) => {
                // `total` may grow as phases are entered; keep the length in sync.
                bar.set_length(total as u64);
                bar.set_position(done as u64);
            }
            CliProgress::Plain(state) => {
                let mut s = state.lock().expect("progress mutex poisoned");
                let terminal = total != 0 && done >= total;
                // Coalesce to ~decile steps (plus the terminal report) so a large
                // block count does not flood the log — the facade forwards every
                // block, `RateLimited` uses step = 1.
                let decile = if total == 0 {
                    0
                } else {
                    (done as u64 * 10 / total as u64) as i32
                };
                if s.last_decile != decile || terminal {
                    s.last_decile = decile;
                    let pct = (done as u64 * 100).checked_div(total as u64).unwrap_or(100);
                    eprintln!("  {}: {done}/{total} ({pct}%)", s.phase);
                }
            }
        }
    }

    fn on_phase(&self, phase: &str) {
        match self {
            CliProgress::Bar(bar) => {
                // Reset for the new phase; the next `on_progress` sets the length.
                bar.set_position(0);
                bar.set_length(0);
                bar.set_message(phase.to_string());
            }
            CliProgress::Plain(state) => {
                let mut s = state.lock().expect("progress mutex poisoned");
                s.phase = phase.to_string();
                s.last_decile = -1;
                eprintln!("{phase}...");
            }
        }
    }
}
