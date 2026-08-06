//! CLI progress rendering for `get`/`downsync` (and `upsync`/`put`/`clone-store`).
//!
//! Implements [`longtail::ProgressSink`]. On an interactive stderr it drives a
//! single `indicatif` bar that shows both dimensions on one line: the phase name
//! (prefix), the item count `XXX/YYY` (blocks for the download apply loop, files
//! for the target scan) as the message, and — driving the bar fill — the byte
//! dimension with a live data rate + ETA. The bar is byte-driven so indicatif's
//! smoothed rate applies to the actual data; the item count rides along as text.
//! Phases that report no progress (reading indexes, validating) show a plain
//! spinner + phase name. When stderr is not a terminal (piped / CI) it falls back
//! to a single throttled line carrying both metrics.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytesize::ByteSize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use longtail::{Progress, ProgressSink};
use tracing_subscriber::fmt::MakeWriter;

/// Fixed phase-label column width (the longest label, "Reading version index"),
/// so the bar starts at the same column regardless of phase.
const MSG_WIDTH: usize = 21;

/// The process-wide owner of anything drawn to stderr.
///
/// The bar and the `tracing` subscriber both write to stderr. Left uncoordinated
/// they interleave: the library emits warnings during a transfer (a store-index
/// fallback, an undeleted block), and each one lands on top of the live bar,
/// leaving a half-drawn frame and a log line spliced into it. Registering the bar
/// here and routing subscriber output through [`BarAwareStderr`] makes indicatif
/// clear the bar, let the line through, and redraw beneath it.
fn bars() -> &'static MultiProgress {
    static BARS: OnceLock<MultiProgress> = OnceLock::new();
    BARS.get_or_init(MultiProgress::new)
}

/// A `tracing` writer that keeps log output from corrupting the progress bar.
///
/// Install with `tracing_subscriber::fmt().with_writer(BarAwareStderr)`; without
/// it, every event emitted while the bar is live scribbles over it. Writes go to
/// stderr, matching the bar's own draw target, so ordinary stdout piping is
/// unaffected.
pub struct BarAwareStderr;

impl MakeWriter<'_> for BarAwareStderr {
    type Writer = SuspendedStderr;

    fn make_writer(&self) -> SuspendedStderr {
        SuspendedStderr
    }
}

/// Stderr, but each write is bracketed by an indicatif suspend.
pub struct SuspendedStderr;

impl Write for SuspendedStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // `suspend` clears the bar, runs the write, and redraws. Cheap when no
        // bar is registered (the non-TTY path), so this needs no terminal check.
        bars().suspend(|| io::stderr().write(buf))
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

/// A terminal-aware progress sink for the CLI.
pub enum CliProgress {
    /// Interactive: one indicatif bar carrying phase + item count + byte rate.
    Bar {
        bar: ProgressBar,
        /// Whether the byte/count ("full") style is installed for the current
        /// phase (vs the plain spinner used until real progress arrives).
        full: AtomicBool,
    },
    /// Non-interactive: throttled plain-text lines on stderr.
    Plain(Mutex<PlainState>),
}

/// State for the non-TTY fallback (guarded by a `Mutex` since `ProgressSink`
/// takes `&self`).
pub struct PlainState {
    phase: String,
    /// Last printed decile of the driving dimension, or `-1` before the first
    /// print.
    last_decile: i32,
    /// When the current phase began (for the average byte rate).
    phase_start: Instant,
}

/// Plain spinner + phase name, for phases that report no per-item progress.
fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {prefix}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
}

/// The single combined bar: phase, item count (`{msg}`), then the byte-driven
/// bar/size/rate/ETA.
fn full_style() -> ProgressStyle {
    ProgressStyle::with_template(&format!(
        "{{spinner:.green}} {{prefix:{MSG_WIDTH}}} [{{bar:30.cyan/blue}}] {{msg}}  {{bytes}}/{{total_bytes}} ({{binary_bytes_per_sec}}, ETA {{eta}})"
    ))
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=>-")
}

/// Like [`full_style`] but for the finished line left on screen: shows the
/// elapsed time (`in {elapsed}`) instead of a now-meaningless `ETA 0s`.
fn finished_style() -> ProgressStyle {
    ProgressStyle::with_template(&format!(
        "{{spinner:.green}} {{prefix:{MSG_WIDTH}}} [{{bar:30.cyan/blue}}] {{msg}}  {{bytes}}/{{total_bytes}} ({{binary_bytes_per_sec}}, in {{elapsed}})"
    ))
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=>-")
}

impl CliProgress {
    /// Build a sink, choosing the bar or plain renderer from whether stderr is a
    /// terminal.
    pub fn new() -> CliProgress {
        if std::io::stderr().is_terminal() {
            // Added to the registry rather than built with its own draw target,
            // so log output can suspend it (see `bars`).
            let bar = bars().add(ProgressBar::new_spinner());
            bar.set_style(spinner_style());
            bar.enable_steady_tick(Duration::from_millis(120));
            CliProgress::Bar {
                bar,
                full: AtomicBool::new(false),
            }
        } else {
            CliProgress::Plain(Mutex::new(PlainState {
                phase: String::new(),
                last_decile: -1,
                phase_start: Instant::now(),
            }))
        }
    }

    /// Freeze the bar and leave its final line on screen. On `completed` the bar
    /// snaps to 100% (the run finished); otherwise (cancel/error) it is abandoned
    /// at its actual position so the frozen line honestly shows how far it got.
    /// Either way the elapsed-time style replaces the now-meaningless ETA.
    pub fn finish(&self, completed: bool) {
        if let CliProgress::Bar { bar, full } = self {
            if full.load(Ordering::Relaxed) {
                bar.set_style(finished_style());
            }
            if completed {
                bar.finish();
            } else {
                bar.abandon();
            }
        }
    }
}

impl ProgressSink for CliProgress {
    fn on_progress(&self, p: Progress) {
        match self {
            CliProgress::Bar { bar, full } => {
                // First real sample of the phase → switch to the combined style.
                if !full.swap(true, Ordering::Relaxed) {
                    bar.set_style(full_style());
                }
                // Byte-driven fill (indicatif derives the rate); the item count
                // rides along as the message.
                bar.set_length(p.total_bytes);
                bar.set_position(p.done_bytes);
                if p.total_items != 0 {
                    bar.set_message(format!("{}/{}", p.done_items, p.total_items));
                }
            }
            CliProgress::Plain(state) => {
                let mut s = state.lock().expect("progress mutex poisoned");
                // Drive the decile off whichever dimension is known (prefer items).
                let (done, total) = if p.total_items != 0 {
                    (p.done_items, p.total_items)
                } else {
                    (p.done_bytes, p.total_bytes)
                };
                let terminal = total != 0 && done >= total;
                let decile = done
                    .checked_mul(10)
                    .and_then(|x| x.checked_div(total))
                    .unwrap_or(0) as i32;
                if s.last_decile != decile || terminal {
                    s.last_decile = decile;
                    let mut line = format!("  {}:", s.phase);
                    if p.total_items != 0 {
                        let pct = p
                            .done_items
                            .checked_mul(100)
                            .and_then(|x| x.checked_div(p.total_items))
                            .unwrap_or(0);
                        line.push_str(&format!(" {}/{} ({pct}%)", p.done_items, p.total_items));
                    }
                    if p.total_bytes != 0 {
                        let secs = s.phase_start.elapsed().as_secs_f64();
                        let rate = if secs > 0.0 {
                            (p.done_bytes as f64 / secs) as u64
                        } else {
                            0
                        };
                        line.push_str(&format!(
                            " {} ({}/s)",
                            ByteSize(p.done_bytes),
                            ByteSize(rate)
                        ));
                    }
                    eprintln!("{line}");
                }
            }
        }
    }

    fn on_phase(&self, phase: &str) {
        match self {
            CliProgress::Bar { bar, full } => {
                // Back to the plain spinner until this phase reports progress;
                // fresh per-phase elapsed/rate/ETA.
                bar.reset();
                bar.set_style(spinner_style());
                bar.set_message("");
                bar.set_prefix(phase.to_string());
                full.store(false, Ordering::Relaxed);
            }
            CliProgress::Plain(state) => {
                let mut s = state.lock().expect("progress mutex poisoned");
                s.phase = phase.to_string();
                s.last_decile = -1;
                s.phase_start = Instant::now();
                eprintln!("{phase}...");
            }
        }
    }
}
