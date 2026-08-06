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

/// Rewriting a read-only asset in place.
///
/// `retain_permissions` defaults on, so a downsync of an asset recorded `0444`
/// leaves a read-only file on disk. The next version's write has to truncate-open
/// that file, which the owner write bit forbids — the download failed with EACCES
/// and no way around it, since the CLI accepts `--use-legacy-write` (the escape
/// golongtail offers) but the library rejects it.
///
/// Both versions record the *same* `0444`, so the asset is content-modified and
/// not permissions-modified (`diff.rs:75-82` tests those independently). Step 7
/// therefore never visits it, and the mode below is the one apply put back rather
/// than one it reassigned — which is the half of the fix a fixture with differing
/// modes would not reach.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_read_only_asset_can_be_rewritten_and_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;

    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, src) = (tmp.path().join("store"), tmp.path().join("src"));
    let (v1, v2) = (tmp.path().join("v1.lvi"), tmp.path().join("v2.lvi"));

    let asset = src.join("config.ini");
    let publish = |body: &str, lvi: &std::path::Path| {
        std::fs::create_dir_all(&src).unwrap();
        // 0444 refuses our own rewrite too, so relax, write, then re-lock.
        if asset.exists() {
            std::fs::set_permissions(&asset, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        std::fs::write(&asset, body).unwrap();
        std::fs::set_permissions(&asset, std::fs::Permissions::from_mode(0o444)).unwrap();
        longtail::UpsyncOptions::new(
            src.to_string_lossy().into_owned(),
            store.to_string_lossy().into_owned(),
            lvi.to_string_lossy().into_owned(),
        )
    };

    longtail::upsync(publish("the first revision\n", &v1))
        .await
        .expect("upsync v1");
    longtail::upsync(publish("the second revision, which is longer\n", &v2))
        .await
        .expect("upsync v2");

    let target = tmp.path().join("out");
    run_downsync(v1, store.clone(), target.clone()).await;
    let landed = target.join("config.ini");
    assert_eq!(
        std::fs::metadata(&landed).unwrap().permissions().mode() & 0o777,
        0o444,
        "v1 must land read-only, or the rewrite below proves nothing"
    );

    run_downsync(v2, store, target.clone()).await;
    assert_eq!(
        std::fs::read_to_string(&landed).unwrap(),
        "the second revision, which is longer\n",
        "the read-only asset must be rewritten with v2's content"
    );
    assert_eq!(
        std::fs::metadata(&landed).unwrap().permissions().mode() & 0o777,
        0o444,
        "the mode relaxed to allow the write must be put back"
    );
}

/// `retain_permissions = false` means apply does not touch modes — including the
/// one it relaxes to get the write through. Without the restore the asset would
/// be left at whatever the unlock made it, which is a permission change made by
/// the flag that exists to avoid making permission changes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rewriting_a_read_only_asset_leaves_its_mode_alone_when_not_retaining() {
    use std::os::unix::fs::PermissionsExt;

    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let fx = fixtures_dir();
    let target = tmp.path().join("out");

    let run = |lvi: PathBuf| {
        let mut opts = DownsyncOptions::new(
            vec![lvi.to_string_lossy().into_owned()],
            fx.join("stores/default/store")
                .to_string_lossy()
                .into_owned(),
            target.to_string_lossy().into_owned(),
        );
        opts.cache_target_index = false;
        opts.retain_permissions = false;
        opts
    };

    downsync(run(fx.join("stores/default/chain-v1.lvi")))
        .await
        .expect("v1");

    // The mode is the operator's here, not the index's: with retaining off the
    // v1 landing used the umask. Lock it by hand to set up the rewrite.
    let landed = target.join("abitoftext.txt");
    std::fs::set_permissions(&landed, std::fs::Permissions::from_mode(0o444)).unwrap();

    downsync(run(fx.join("stores/default/chain-v2.lvi")))
        .await
        .expect("v2 over a read-only asset");
    assert_eq!(
        std::fs::metadata(&landed).unwrap().permissions().mode() & 0o777,
        0o444,
        "with retaining off, a mode apply relaxed must be put back exactly"
    );
}

/// Creating an asset inside a read-only directory.
///
/// The same defect one level up: creating a file needs write on the *parent*, so
/// a directory at a mode without the owner write bit blocks a new asset beneath
/// it. Reachable through the tool's own behaviour — step 7's first loop chmods
/// permissions-modified assets without skipping directories, so one version can
/// leave the target in this state for the next.
///
/// Both versions record `sub/` at the same `0555`, so the directory is not
/// permissions-modified and step 7 never visits it. The mode asserted at the end
/// is therefore the one apply put back, not one it reassigned.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_asset_can_be_created_inside_a_read_only_directory() {
    use std::os::unix::fs::PermissionsExt;

    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, src) = (tmp.path().join("store"), tmp.path().join("src"));
    let (v1, v2) = (tmp.path().join("v1.lvi"), tmp.path().join("v2.lvi"));
    let sub = src.join("sub");
    let chmod = |p: &std::path::Path, m: u32| {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    };

    let publish = |lvi: &std::path::Path| {
        longtail::UpsyncOptions::new(
            src.to_string_lossy().into_owned(),
            store.to_string_lossy().into_owned(),
            lvi.to_string_lossy().into_owned(),
        )
    };

    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("first.bin"), "the asset v1 already had\n").unwrap();
    chmod(&sub, 0o555);
    longtail::upsync(publish(&v1)).await.expect("upsync v1");

    // v2 adds a second asset *inside* the subdirectory — the write that needs the
    // directory writable. 0555 blocks our own setup too, hence the unlock/relock.
    chmod(&sub, 0o755);
    std::fs::write(sub.join("second.bin"), "the asset v2 adds inside sub/\n").unwrap();
    chmod(&sub, 0o555);
    longtail::upsync(publish(&v2)).await.expect("upsync v2");

    let target = tmp.path().join("out");
    run_downsync(v1, store.clone(), target.clone()).await;

    // An added directory is not permission-set (longtail.c:7995), so v1 leaves it
    // at the umask default; lock it the way a permissions-modified version would.
    let landed_dir = target.join("sub");
    chmod(&landed_dir, 0o555);

    run_downsync(v2, store, target.clone()).await;

    assert_eq!(
        std::fs::read_to_string(landed_dir.join("second.bin")).unwrap(),
        "the asset v2 adds inside sub/\n",
        "v2's asset must be created inside the read-only directory"
    );
    assert_eq!(
        std::fs::metadata(&landed_dir).unwrap().permissions().mode() & 0o777,
        0o555,
        "a directory mode relaxed to create an asset must be put back"
    );
    // Unlock before the tempdir drop, which cannot recurse into a 0555 dir.
    chmod(&landed_dir, 0o755);
}

/// Files the target has and the version does not are deleted — and an exclude
/// filter is what keeps user data out of that set.
///
/// The delete phase is driven by `source_removed_asset_indexes`, which is
/// "scanned in the target, absent from the source". A save-game directory sitting
/// in the install folder qualifies, so a plain downsync removes it. Excluding it
/// keeps it out of the target scan, so it is never in `current`, never in the
/// removed set, and never touched — no new option needed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_exclude_filter_keeps_user_files_out_of_the_delete_set() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let store = fx.join("stores/default/store");
    let lvi = fx.join("stores/default/chain-v2.lvi");

    run_downsync(lvi.clone(), store.clone(), target.clone()).await;

    let saved = target.join("Saved");
    std::fs::create_dir_all(&saved).unwrap();
    let slot = saved.join("player.sav");
    std::fs::write(&slot, b"hours of progress").unwrap();

    // Unfiltered: the file is not in the version, so it is removed.
    run_downsync(lvi.clone(), store.clone(), target.clone()).await;
    assert!(
        !slot.exists(),
        "without a filter the delete phase claims anything not in the version"
    );

    // Filtered: the scan never sees it, so the diff never proposes it.
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::write(&slot, b"hours of progress").unwrap();
    let mut opts = DownsyncOptions::new(
        vec![lvi.to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false;
    opts.exclude_filter_regex = Some("^Saved".to_string());
    downsync(opts).await.expect("filtered downsync");
    assert!(
        slot.exists(),
        "an excluded path must survive the delete phase"
    );
    assert_eq!(
        std::fs::read(&slot).unwrap(),
        b"hours of progress",
        "and must be left byte-identical"
    );
}

/// `delete_removed = false` — the repair shape.
///
/// Everything the version names is still checked against its content hash and
/// rewritten when wrong; everything else in the target is left alone. This is the
/// alternative to naming user directories in an exclude regex, which has to be
/// kept correct and deletes user data silently when it is not.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_repair_run_fixes_the_version_without_deleting_anything_else() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let store = fx.join("stores/default/store");
    let lvi = fx.join("stores/default/chain-v2.lvi");

    run_downsync(lvi.clone(), store.clone(), target.clone()).await;

    // User data the version knows nothing about, in two shapes: a nested file
    // and one directly in the target root.
    let saved = target.join("Saved");
    std::fs::create_dir_all(&saved).unwrap();
    std::fs::write(saved.join("player.sav"), b"hours of progress").unwrap();
    std::fs::write(target.join("settings.ini"), b"volume=11").unwrap();

    // Damage an asset the version *does* own, so the run has real repair work.
    let owned = target.join("abitoftext.txt");
    let good = std::fs::read(&owned).unwrap();
    std::fs::write(&owned, b"corrupted").unwrap();

    let mut opts = DownsyncOptions::new(
        vec![lvi.to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false; // force the content-hash scan
    opts.delete_removed = false;
    downsync(opts).await.expect("repair downsync");

    assert_eq!(
        std::fs::read(&owned).unwrap(),
        good,
        "the damaged asset must be repaired"
    );
    assert_eq!(
        std::fs::read(saved.join("player.sav")).unwrap(),
        b"hours of progress",
        "user data must survive a repair"
    );
    assert_eq!(
        std::fs::read(target.join("settings.ini")).unwrap(),
        b"volume=11",
        "user data in the target root must survive too"
    );

    // And the default still deletes, so the flag is doing the work rather than
    // the delete phase having quietly stopped firing.
    run_downsync(lvi, store, target.clone()).await;
    assert!(
        !saved.join("player.sav").exists() && !target.join("settings.ini").exists(),
        "the default must still remove what the version does not contain"
    );
}
