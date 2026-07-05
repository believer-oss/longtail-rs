//! Tauri-shaped smoke test: caller-owned runtime, progress monotonicity,
//! mid-transfer cancellation → resumable, and the three runtime-discipline
//! assertions. Linux-only; skipped under miri.

#![cfg(unix)]
#![cfg_attr(miri, ignore)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use longtail::{DownsyncOptions, LongtailError, ProgressSink, downsync, downsync_blocking};
use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;
use tokio_util::sync::CancellationToken;

fn pin_umask() {
    unsafe {
        libc::umask(0o022);
    }
}

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
    last: AtomicU32,
    violations: AtomicU32,
    updates: AtomicU32,
}

impl ProgressSink for MonotonicProgress {
    fn on_progress(&self, done: u32, _total: u32) {
        self.updates.fetch_add(1, Ordering::Relaxed);
        let prev = self.last.swap(done, Ordering::Relaxed);
        if done < prev {
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
        .compare(&m, false)
        .unwrap();
    TreeManifest::capture(&t2)
        .unwrap()
        .compare(&m, false)
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
        .compare(&zoo_manifest(), false)
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
            .compare(&zoo_manifest(), false)
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
        fn on_progress(&self, done: u32, _total: u32) {
            if done >= 1 {
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
        .compare(&zoo_manifest(), false)
        .expect("resumed tree matches manifest");
}
