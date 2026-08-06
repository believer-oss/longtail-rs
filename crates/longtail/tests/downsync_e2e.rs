//! Pure-Rust facade downsync end-to-end against committed fixture stores,
//! compared to the committed tree manifests. Linux-only; skipped under miri.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use longtail::{DownsyncOptions, Progress, ProgressSink, downsync};
use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;

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

fn manifest(name: &str) -> TreeManifest {
    let p = fixtures_dir().join("manifests").join(name);
    TreeManifest::from_json(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

async fn run_downsync(source_lvi: PathBuf, store: PathBuf, target: PathBuf) {
    let mut opts = DownsyncOptions::new(
        vec![source_lvi.to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false;
    downsync(opts).await.expect("downsync");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_downsync_zoo_matches_manifest() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_downsync(
        fx.join("stores/default/zoo.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;
    let got = TreeManifest::capture(&target).unwrap();
    got.compare(&manifest("zoo.json"), cfg!(windows))
        .expect("zoo tree matches manifest");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_v1_then_v2() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    // v1 into empty target, then v2 over it (resume/upgrade).
    run_downsync(
        fx.join("stores/default/chain-v1.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&manifest("chain-v1.json"), cfg!(windows))
        .expect("v1 tree");
    run_downsync(
        fx.join("stores/default/chain-v2.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&manifest("chain-v2.json"), cfg!(windows))
        .expect("v2 tree after resume");
}

/// Records every `(phase, Progress)` the facade emits, so tests can assert the
/// item + byte dimensions.
#[derive(Default)]
struct Recorder {
    phase: Mutex<String>,
    events: Mutex<Vec<(String, Progress)>>,
}

impl ProgressSink for Recorder {
    fn on_phase(&self, phase: &str) {
        *self.phase.lock().unwrap() = phase.to_string();
    }
    fn on_progress(&self, p: Progress) {
        let phase = self.phase.lock().unwrap().clone();
        self.events.lock().unwrap().push((phase, p));
    }
}

/// The dual-dimension progress feed: a v2-over-v1 downsync scans the populated
/// v1 tree (indexing bytes) AND applies v2's new blocks (download bytes), so one
/// run exercises both. Assert each phase's dimensions are populated, monotonic,
/// non-overshooting, and terminate at 100%.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_reports_item_and_byte_dimensions() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");

    // v1 first (populates the target so v2's target scan has files to index).
    run_downsync(
        fx.join("stores/default/chain-v1.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;

    // v2 over v1, recording progress.
    let rec = Arc::new(Recorder::default());
    let mut opts = DownsyncOptions::new(
        vec![
            fx.join("stores/default/chain-v2.lvi")
                .to_string_lossy()
                .into_owned(),
        ],
        fx.join("stores/default/store")
            .to_string_lossy()
            .into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false; // force the target scan (indexing phase)
    opts.progress = Some(rec.clone() as Arc<dyn ProgressSink>);
    downsync(opts).await.expect("v2 downsync");

    let events = rec.events.lock().unwrap();
    let assert_phase = |name: &str, want_bytes: bool| {
        let samples: Vec<Progress> = events
            .iter()
            .filter(|(ph, _)| ph == name)
            .map(|(_, p)| *p)
            .collect();
        assert!(!samples.is_empty(), "phase `{name}` reported no progress");
        // Monotonic + non-overshoot on both dimensions.
        let mut prev = Progress::default();
        for p in &samples {
            assert!(p.done_items >= prev.done_items, "{name}: items regressed");
            assert!(p.done_bytes >= prev.done_bytes, "{name}: bytes regressed");
            if p.total_items != 0 {
                assert!(p.done_items <= p.total_items, "{name}: items overshoot");
            }
            if p.total_bytes != 0 {
                assert!(p.done_bytes <= p.total_bytes, "{name}: bytes overshoot");
            }
            prev = *p;
        }
        let last = *samples.last().unwrap();
        assert!(last.total_items > 0, "{name}: item total never known");
        assert_eq!(last.done_items, last.total_items, "{name}: items != 100%");
        if want_bytes {
            assert!(last.total_bytes > 0, "{name}: byte total never known");
            assert_eq!(last.done_bytes, last.total_bytes, "{name}: bytes != 100%");
        }
    };

    // Indexing item dim = files; download item dim = blocks. Both carry bytes.
    assert_phase("Indexing version", true);
    assert_phase("Updating version", true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sharded_store_merge_on_read() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_downsync(
        fx.join("stores/sharded/version.lvi"),
        fx.join("stores/sharded"),
        target.clone(),
    )
    .await;
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&manifest("sharded-union.json"), cfg!(windows))
        .expect("sharded union tree");
}
