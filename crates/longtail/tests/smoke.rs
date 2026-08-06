//! Tauri-shaped smoke test: caller-owned runtime, progress monotonicity,
//! mid-transfer cancellation → resumable, and the three runtime-discipline
//! assertions. Linux-only; skipped under miri.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use longtail::{
    DownsyncOptions, LongtailError, Progress, ProgressSink, downsync, downsync_blocking,
};
use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
fn pin_umask() {
    // SAFETY: `umask` is a simple libc call with no memory-safety concerns; one
    // test binary is one process, so this cannot race another thread's view.
    unsafe {
        libc::umask(0o022);
    }
}

/// No umask to pin: permissions are synthesized rather than read from the
/// filesystem (format-spec §7), so nothing here depends on the process umask.
#[cfg(not(unix))]
fn pin_umask() {}

fn store() -> PathBuf {
    fixtures_dir().join("stores/default/store")
}
fn zoo_lvi() -> PathBuf {
    fixtures_dir().join("stores/default/zoo.lvi")
}
fn zoo_manifest() -> TreeManifest {
    let p = fixtures_dir().join("manifests/zoo.json");
    TreeManifest::from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn base_opts(target: &Path) -> DownsyncOptions {
    let mut o = DownsyncOptions::new(
        vec![zoo_lvi().to_string_lossy().into_owned()],
        store().to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    o.cache_target_index = false;
    o
}

/// Records every progress update and asserts non-decreasing `done`.
#[derive(Default)]
struct MonotonicProgress {
    last: AtomicU64,
    violations: AtomicU32,
    updates: AtomicU32,
}

impl ProgressSink for MonotonicProgress {
    fn on_progress(&self, p: Progress) {
        self.updates.fetch_add(1, Ordering::Relaxed);
        let prev = self.last.swap(p.done_items, Ordering::Relaxed);
        if p.done_items < prev {
            self.violations.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn on_phase(&self, _phase: &str) {
        // A phase boundary resets the running counter.
        self.last.store(0, Ordering::Relaxed);
    }
}

/// (a) Two concurrent downsyncs on one caller runtime → both complete + match.
#[test]
fn two_concurrent_downsyncs() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let t1 = tmp.path().join("a");
    let t2 = tmp.path().join("b");
    rt.block_on(async {
        let f1 = downsync(base_opts(&t1));
        let f2 = downsync(base_opts(&t2));
        let (r1, r2) = tokio::join!(f1, f2);
        r1.expect("downsync a");
        r2.expect("downsync b");
    });
    let m = zoo_manifest();
    TreeManifest::capture(&t1)
        .unwrap()
        .compare(&m, cfg!(windows))
        .unwrap();
    TreeManifest::capture(&t2)
        .unwrap()
        .compare(&m, cfg!(windows))
        .unwrap();
}

/// (b) `downsync_blocking` from a plain thread while a runtime exists elsewhere.
#[test]
fn blocking_from_plain_thread_with_runtime_elsewhere() {
    pin_umask();
    // A runtime living on another thread in the process.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter(); // an ambient runtime exists on THIS thread…
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    // …but downsync_blocking runs on a fresh plain thread with its own runtime.
    let opts = base_opts(&target);
    let handle = std::thread::spawn(move || downsync_blocking(opts));
    handle.join().unwrap().expect("blocking downsync");
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&zoo_manifest(), cfg!(windows))
        .unwrap();
}

/// (c) `downsync()` awaited inside `tokio::spawn` must not create a runtime
/// (no "Cannot start a runtime from within a runtime" panic).
#[test]
fn downsync_inside_tokio_spawn() {
    pin_umask();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    rt.block_on(async move {
        let opts = base_opts(&target);
        let handle = tokio::spawn(async move { downsync(opts).await });
        handle.await.unwrap().expect("spawned downsync");
        TreeManifest::capture(&target)
            .unwrap()
            .compare(&zoo_manifest(), cfg!(windows))
            .unwrap();
    });
}

/// Progress is observed and monotonic across the transfer.
#[test]
fn progress_is_monotonic() {
    pin_umask();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let progress = Arc::new(MonotonicProgress::default());
    let mut opts = base_opts(&target);
    opts.progress = Some(progress.clone());
    rt.block_on(downsync(opts)).expect("downsync");
    assert_eq!(
        progress.violations.load(Ordering::Relaxed),
        0,
        "progress must be non-decreasing"
    );
    assert!(
        progress.updates.load(Ordering::Relaxed) > 0,
        "progress should be reported at least once"
    );
}

/// Cancel mid-transfer → typed `Cancelled`, target left resumable (a follow-up
/// downsync completes and matches the manifest).
#[test]
fn cancel_mid_transfer_then_resume() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");

    // A progress sink that cancels after the first block completes.
    struct CancelAfterFirst {
        token: CancellationToken,
        fired: Mutex<bool>,
    }
    impl ProgressSink for CancelAfterFirst {
        fn on_progress(&self, p: Progress) {
            if p.done_items >= 1 {
                let mut f = self.fired.lock().unwrap();
                if !*f {
                    *f = true;
                    self.token.cancel();
                }
            }
        }
    }

    let token = CancellationToken::new();
    let sink = Arc::new(CancelAfterFirst {
        token: token.clone(),
        fired: Mutex::new(false),
    });

    let mut opts = base_opts(&target);
    opts.progress = Some(sink);
    opts.cancel = Some(token.clone());
    let result = rt.block_on(downsync(opts));
    assert!(
        matches!(result, Err(LongtailError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );

    // Resume: a fresh downsync (scan-target heals the partial state) completes.
    rt.block_on(downsync(base_opts(&target)))
        .expect("resume downsync");
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&zoo_manifest(), cfg!(windows))
        .expect("resumed tree matches manifest");
}

/// A cancelled download must still sweep the block cache to its budget.
///
/// The store's end-of-run work — warm-cache write-backs and the LRU sweep that
/// enforces `cache_size_limit` — lives behind `close()`. Reaching it only on the
/// success path meant a cancel left the cache oversized until some later run
/// happened to finish, and for a launcher that cancels routinely "some later
/// run" may never come.
#[test]
fn cancel_still_sweeps_the_block_cache_to_its_budget() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let cache = tmp.path().join("cache");

    struct CancelAfterFirst {
        token: CancellationToken,
        fired: Mutex<bool>,
    }
    impl ProgressSink for CancelAfterFirst {
        fn on_progress(&self, p: Progress) {
            if p.done_items > 0 {
                let mut f = self.fired.lock().unwrap();
                if !*f {
                    *f = true;
                    self.token.cancel();
                }
            }
        }
    }

    let token = CancellationToken::new();
    let mut opts = base_opts(&target);
    opts.cache_path = Some(cache.clone());
    // Budget of zero: whatever the cancelled run cached must be swept away.
    opts.cache_size_limit = Some(0);
    opts.progress = Some(Arc::new(CancelAfterFirst {
        token: token.clone(),
        fired: Mutex::new(false),
    }));
    opts.cancel = Some(token.clone());

    let result = rt.block_on(downsync(opts));
    assert!(
        matches!(result, Err(LongtailError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );

    let cached_bytes = cache_bytes(&cache);
    assert_eq!(
        cached_bytes, 0,
        "cancel left {cached_bytes} bytes in the cache against a budget of 0; \
         the close-path sweep did not run"
    );

    // Control: the same cancelled run with no budget configured leaves blocks
    // behind. Without this the assertion above would also hold if a cancelled
    // run simply never cached anything, which would prove nothing.
    let target2 = tmp.path().join("target2");
    let cache2 = tmp.path().join("cache2");
    let token2 = CancellationToken::new();
    let mut opts2 = base_opts(&target2);
    opts2.cache_path = Some(cache2.clone());
    opts2.cache_size_limit = None;
    opts2.progress = Some(Arc::new(CancelAfterFirst {
        token: token2.clone(),
        fired: Mutex::new(false),
    }));
    opts2.cancel = Some(token2.clone());
    let r2 = rt.block_on(downsync(opts2));
    assert!(
        matches!(r2, Err(LongtailError::Cancelled)),
        "expected Cancelled, got {r2:?}"
    );
    assert!(
        cache_bytes(&cache2) > 0,
        "control failed: a cancelled run cached nothing, so the budget assertion \
         above cannot distinguish a working sweep from an empty cache"
    );
}

/// Total bytes of cached `.lrb` blocks under `dir` (absent dir = 0).
fn cache_bytes(dir: &Path) -> u64 {
    fn walk(d: &Path, acc: &mut u64) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, acc);
                } else if p.extension().and_then(|x| x.to_str()) == Some("lrb") {
                    *acc += e.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }
    }
    let mut acc = 0;
    walk(dir, &mut acc);
    acc
}

/// A silent fallback to the slow path is indistinguishable from a hang.
///
/// When a version-local store index cannot be read, the download falls back to
/// scanning the whole store — orders of magnitude slower, and producing no
/// progress of its own, so the UI simply stops moving. The event naming the
/// offending URI is the only thing that separates "slow store" from "your
/// override path is wrong".
#[test]
fn a_fallback_to_the_full_store_scan_says_so() {
    use std::sync::{Arc as StdArc, Mutex as StdMutex};
    use tracing_subscriber::prelude::*;

    #[derive(Clone, Default)]
    struct Capture(StdArc<StdMutex<Vec<String>>>);
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            struct V(String);
            impl tracing::field::Visit for V {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    self.0.push_str(&format!(" {}={:?}", f.name(), v));
                }
            }
            let mut v = V(format!("{}", event.metadata().level()));
            event.record(&mut v);
            self.0.lock().unwrap().push(v.0);
        }
    }

    pin_umask();
    let events = Capture::default();
    let sink = events.clone();
    let subscriber = tracing_subscriber::registry().with(sink);
    let _guard = tracing::subscriber::set_default(subscriber);

    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let missing_lsi = tmp.path().join("nonexistent.lsi");

    let mut opts = base_opts(&target);
    opts.version_local_store_index_paths = vec![missing_lsi.to_string_lossy().into_owned()];
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    // The download itself still succeeds — that is the point: it silently got
    // slower, which is exactly why it needs to be audible.
    rt.block_on(downsync(opts))
        .expect("fallback still downloads");

    let captured = events.0.lock().unwrap().join("\n");
    assert!(
        captured.contains("full store scan"),
        "no warning about the fallback; captured:\n{captured}"
    );
    assert!(
        captured.contains("nonexistent.lsi"),
        "the warning must name the offending uri; captured:\n{captured}"
    );
}
