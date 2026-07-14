//! Progress reporting — a plain callback trait (`ratelimitedprogress`'s role).
//! The facade rate-limits emission; the exact cadence is not
//! compat-relevant.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A progress sample carrying two independent dimensions: an **item** count
/// (blocks for the download phase, files for the indexing phase) and a **byte**
/// count (data handled). Either dimension is "unknown/indeterminate" when its
/// `total` is `0`. Both are reported together so a consumer can render an item
/// bar and a data-rate bar concurrently (and a GUI a double progress bar).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Items completed in the current phase (blocks or files).
    pub done_items: u64,
    /// Total items in the current phase (`0` = unknown).
    pub total_items: u64,
    /// Bytes handled in the current phase.
    pub done_bytes: u64,
    /// Total bytes in the current phase (`0` = unknown).
    pub total_bytes: u64,
}

/// A progress sink. [`on_progress`](ProgressSink::on_progress) is called as work
/// completes; a dimension's `total` may grow as phases are entered. `on_phase`
/// names the current phase.
///
/// Implementations must be cheap and non-blocking (they run on the async task,
/// and — for the indexing scan — on rayon workers via a throttled forwarder).
pub trait ProgressSink: Send + Sync {
    /// Report progress within the current phase (both dimensions at once).
    fn on_progress(&self, p: Progress);

    /// Called when a named phase begins (default: no-op). Phase-aware sinks can
    /// use this to segment; simple sinks ignore it.
    fn on_phase(&self, _phase: &str) {}
}

/// A no-op sink (the default when the caller supplies none).
#[derive(Debug, Clone, Copy, Default)]
pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn on_progress(&self, _p: Progress) {}
}

/// A facade-side rate limiter around a [`ProgressSink`]: forwards the first and
/// last update and coalesces the rest so a caller sink is not spammed. Cadence:
/// forward at most once per `step` completed items, plus always the terminal
/// `done_items == total_items`. (Byte-only phases still advance `done_items`
/// via the file counter, so item-keyed coalescing covers both dimensions.)
pub(crate) struct RateLimited {
    inner: Arc<dyn ProgressSink>,
    last_reported: AtomicU64,
    step: u64,
}

impl RateLimited {
    pub(crate) fn new(inner: Arc<dyn ProgressSink>) -> RateLimited {
        RateLimited {
            inner,
            last_reported: AtomicU64::new(u64::MAX),
            step: 1,
        }
    }

    pub(crate) fn phase(&self, phase: &str) {
        self.last_reported.store(u64::MAX, Ordering::Relaxed);
        self.inner.on_phase(phase);
    }

    pub(crate) fn report(&self, p: Progress) {
        let done = p.done_items;
        let last = self.last_reported.load(Ordering::Relaxed);
        let terminal = p.total_items != 0 && done >= p.total_items;
        let advanced = last == u64::MAX || done >= last.saturating_add(self.step) || done < last;
        if terminal || advanced {
            self.last_reported.store(done, Ordering::Relaxed);
            self.inner.on_progress(p);
        }
    }
}
