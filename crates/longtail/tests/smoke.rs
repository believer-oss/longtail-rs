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
/// the store's own index — every `store*.lsi` shard listed and merged, covering
/// the whole store rather than this version's blocks — which produces no progress
/// of its own, so the UI simply stops moving. The event naming the offending URI
/// is the only thing that separates "slow store" from "your override path is
/// wrong".
#[test]
fn a_fallback_to_the_whole_store_index_says_so() {
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

    /// Records the phase labels in order, which is the channel a GUI renders.
    #[derive(Default)]
    struct Phases(StdMutex<Vec<String>>);
    impl ProgressSink for Phases {
        fn on_progress(&self, _p: Progress) {}
        fn on_phase(&self, phase: &str) {
            self.0.lock().unwrap().push(phase.to_string());
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

    let phases = StdArc::new(Phases::default());
    let mut opts = base_opts(&target);
    opts.progress = Some(phases.clone());
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
        captured.contains("whole store index"),
        "no warning about the fallback; captured:\n{captured}"
    );
    assert!(
        captured.contains("nonexistent.lsi"),
        "the warning must name the offending uri; captured:\n{captured}"
    );

    // The log says it to an operator; the phase says it to whoever is watching a
    // progress bar, which on the desktop path is the only surface there is.
    let seen = phases.0.lock().unwrap().clone();
    assert!(
        seen.iter().any(|p| p == "Reading full store index"),
        "the fallback must re-phase so a stalled bar names it; phases: {seen:?}"
    );
}

fn chain_opts(target: &Path, lvi: &str) -> DownsyncOptions {
    let mut o = DownsyncOptions::new(
        vec![
            fixtures_dir()
                .join("stores/default")
                .join(lvi)
                .to_string_lossy()
                .into_owned(),
        ],
        store().to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    o.cache_target_index = true;
    o
}

fn chain_manifest(name: &str) -> TreeManifest {
    let p = fixtures_dir().join("manifests").join(name);
    TreeManifest::from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// Cancels the run when a named phase begins.
struct CancelOnPhase {
    phase: &'static str,
    token: CancellationToken,
}
impl ProgressSink for CancelOnPhase {
    fn on_progress(&self, _p: Progress) {}
    fn on_phase(&self, phase: &str) {
        if phase == self.phase {
            self.token.cancel();
        }
    }
}

/// Resume with the target-index cache on — the library and CLI default.
///
/// Every other test in this file sets `cache_target_index = false`, so the
/// invariant the default rests on has no coverage: a cached target index
/// short-circuits the target scan entirely, so a cache file surviving a
/// cancelled run would make the next run believe the target is already the
/// desired version, write nothing, and exit 0 over a torn tree.
///
/// What prevents that is ordering — `downsync` deletes the cache index before
/// anything mutates the target and rewrites it only after a successful apply.
/// Moving the write earlier, or making the delete non-fatal, passes every other
/// test here and fails this one.
///
/// Cancellation is keyed on the apply phase rather than a block count: the
/// v1 → v2 diff is small, and "after the first block" may be after the last one.
#[test]
fn resume_with_the_target_index_cache_enabled() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache_index = target.join(".longtail.index.cache.lvi");

    rt.block_on(downsync(chain_opts(&target, "chain-v1.lvi")))
        .expect("v1 downsync");
    assert!(
        cache_index.exists(),
        "a completed run must leave the cache index, or the rest proves nothing"
    );

    let token = CancellationToken::new();
    let mut opts = chain_opts(&target, "chain-v2.lvi");
    opts.progress = Some(Arc::new(CancelOnPhase {
        phase: "Updating version",
        token: token.clone(),
    }));
    opts.cancel = Some(token.clone());
    let result = rt.block_on(downsync(opts));
    assert!(
        matches!(result, Err(LongtailError::Cancelled)),
        "expected Cancelled, got {result:?}"
    );
    assert!(
        !cache_index.exists(),
        "a cancelled run must not leave a cache index claiming the target is current"
    );

    rt.block_on(downsync(chain_opts(&target, "chain-v2.lvi")))
        .expect("resume downsync");
    // The cache index lives inside the target, so it is an extra tree entry the
    // manifest does not carry.
    std::fs::remove_file(&cache_index).unwrap();
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&chain_manifest("chain-v2.json"), cfg!(windows))
        .expect("resumed tree matches manifest");
}

/// An unreadable cache index costs a scan, not the download.
///
/// The cache file sits inside the target, so a folder that was downsynced and
/// then upsynced carries it into the version index as an asset — golongtail
/// indexes it the same way, so existing stores already hold such versions. Apply
/// then re-creates it like any other asset: sized up front by `create_file_sized`
/// and zero-filled until its blocks arrive. A run that dies in between leaves the
/// zeros, and the post-apply cache write never replaces them. Reading that back
/// as a version index yields `unsupported format version: found 0x00000000`, and
/// the download used to stop there — on a target that was only stale, and whose
/// cure was deleting a file no user should have to know about.
///
/// Zeros at the full size are the exact residue that path leaves, so the test
/// writes those rather than arbitrary junk.
#[test]
fn a_corrupt_target_index_cache_falls_back_to_scanning() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache_index = target.join(".longtail.index.cache.lvi");

    rt.block_on(downsync(chain_opts(&target, "chain-v1.lvi")))
        .expect("v1 downsync");
    let good_len = std::fs::metadata(&cache_index).unwrap().len();
    assert!(good_len > 0, "the cache index must exist to be corrupted");

    std::fs::write(&cache_index, vec![0u8; good_len as usize]).unwrap();

    // The run must succeed *and* do real work: falling back means scanning the
    // target, so the v1 -> v2 diff is computed from what is on disk.
    rt.block_on(downsync(chain_opts(&target, "chain-v2.lvi")))
        .expect("a zero-filled cache index must not fail the download");

    assert!(
        std::fs::read(&cache_index).unwrap().iter().any(|&b| b != 0),
        "a successful run must replace the rejected cache, not leave the zeros behind"
    );

    std::fs::remove_file(&cache_index).unwrap();
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&chain_manifest("chain-v2.json"), cfg!(windows))
        .expect("tree matches v2 despite the corrupt cache");
}

/// An explicitly named `--target-index-path` keeps failing loudly. The fallback
/// above is for a file this code wrote for itself; a path the caller supplied is
/// part of the request, and quietly scanning instead would run an operation they
/// did not ask for.
#[test]
fn an_explicit_target_index_still_errors_when_unreadable() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let bogus = tmp.path().join("supplied.lvi");
    std::fs::write(&bogus, vec![0u8; 4096]).unwrap();

    let mut opts = chain_opts(&target, "chain-v1.lvi");
    opts.target_index_path = Some(bogus.to_string_lossy().into_owned());
    let result = rt.block_on(downsync(opts));
    assert!(
        matches!(result, Err(LongtailError::Format(_))),
        "expected a format error, got {result:?}"
    );
}

/// The one thing a resume cannot heal, recorded as a limitation rather than a bug.
///
/// The cached index is trusted as the target's state, so damage done to the tree
/// *behind* it is invisible: the next run diffs the cache against the source,
/// finds nothing to do, and exits 0. This is why the cache is deleted before
/// mutation rather than after — the window it leaves is the one below. Re-running
/// without the cache (a full scan) is the documented recovery, and it works.
#[test]
fn a_stale_cache_index_hides_damage_a_full_scan_finds() {
    pin_umask();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache_index = target.join(".longtail.index.cache.lvi");

    rt.block_on(downsync(chain_opts(&target, "chain-v2.lvi")))
        .expect("v2 downsync");
    assert!(cache_index.exists());

    // Damage an asset without touching the cache index — the shape a crash after
    // the cache was written would leave.
    let victim = target.join("abitoftext.txt");
    let good = std::fs::read(&victim).expect("fixture asset");
    std::fs::write(&victim, b"torn").unwrap();

    rt.block_on(downsync(chain_opts(&target, "chain-v2.lvi")))
        .expect("downsync over a stale cache");
    assert_ne!(
        std::fs::read(&victim).unwrap(),
        good,
        "if a cached run now heals a damaged target, the scan is no longer \
         short-circuited — re-read this test rather than deleting it"
    );

    // The documented recovery: without the cache the scan sees the truth.
    let mut healing = chain_opts(&target, "chain-v2.lvi");
    healing.cache_target_index = false;
    rt.block_on(downsync(healing)).expect("full-scan downsync");
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        good,
        "a full scan must heal what the cache hid"
    );
}
