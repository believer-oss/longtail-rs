//! Progress reporting — a plain callback trait (`ratelimitedprogress`'s role).
//! The facade rate-limits emission; the exact cadence is not
//! compat-relevant.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// A progress sink. `on_progress(done, total)` is called as work completes;
/// `total` may grow as phases are entered. `phase` names the current phase.
///
/// Implementations must be cheap and non-blocking (they run on the async task).
pub trait ProgressSink: Send + Sync {
    /// Report progress within the current phase.
    fn on_progress(&self, done: u32, total: u32);

    /// Called when a named phase begins (default: no-op). Phase-aware sinks can
    /// use this to segment; simple sinks ignore it.
    fn on_phase(&self, _phase: &str) {}
}

/// A no-op sink (the default when the caller supplies none).
#[derive(Debug, Clone, Copy, Default)]
pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn on_progress(&self, _done: u32, _total: u32) {}
}

/// A facade-side rate limiter around a [`ProgressSink`]: forwards the first and
/// last update and coalesces the rest so a caller sink is not spammed. Cadence:
/// forward at most once per `step` completed items, plus always the terminal
/// `done == total`.
pub(crate) struct RateLimited {
    inner: Arc<dyn ProgressSink>,
    last_reported: AtomicU32,
    step: u32,
}

impl RateLimited {
    pub(crate) fn new(inner: Arc<dyn ProgressSink>) -> RateLimited {
        RateLimited {
            inner,
            last_reported: AtomicU32::new(u32::MAX),
            step: 1,
        }
    }

    pub(crate) fn phase(&self, phase: &str) {
        self.last_reported.store(u32::MAX, Ordering::Relaxed);
        self.inner.on_phase(phase);
    }

    pub(crate) fn report(&self, done: u32, total: u32) {
        let last = self.last_reported.load(Ordering::Relaxed);
        let terminal = total != 0 && done >= total;
        let advanced = last == u32::MAX || done >= last.saturating_add(self.step) || done < last;
        if terminal || advanced {
            self.last_reported.store(done, Ordering::Relaxed);
            self.inner.on_progress(done, total);
        }
    }
}
