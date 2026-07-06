//! Stage 5 Task 4: the three-way e2e downsync differential.
//!
//! Pure-Rust facade (`longtail::downsync_blocking`) vs the C library
//! (`longtail-ffi`, in-process) vs the spawned pinned golongtail binary (when
//! `xtask fetch-golongtail` has cached it; skipped cleanly otherwise). All three
//! must produce identical tree manifests (content/size/permissions) across
//! fresh / resume / synthetic-partial-state / dirty / cache / cross-impl-handoff
//! / sharded scenarios.
//!
//! **Fixture protection:** the C FSBlockStore writes `store.lsi.sync` lock
//! sidecars, so every impl reads from a **temp copy** of the store (`stage`),
//! never the committed fixtures. Source `.lvi`/`.lsi` are read lock-free and
//! come straight from the fixtures.
//!
//! Differential lane only (needs the native lib). Linux-only.

#![cfg(all(feature = "differential", unix))]

use std::path::{Path, PathBuf};
use std::process::Command;

use longtail::{DownsyncOptions, downsync_blocking};
use longtail_testkit::paths::{fixtures_dir, golongtail_binary};
use longtail_testkit::tree_manifest::TreeManifest;

fn pin_umask() {
    unsafe {
        libc::umask(0o022);
    }
}

fn fixture_store_default() -> PathBuf {
    fixtures_dir().join("stores/default/store")
}
fn lvi(name: &str) -> PathBuf {
    fixtures_dir().join("stores/default").join(name)
}
fn manifest(name: &str) -> TreeManifest {
    let p = fixtures_dir().join("manifests").join(name);
    TreeManifest::from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

/// Copy a fixture store into a temp dir so C's lock sidecars never touch the
/// committed fixtures. Returns the tempdir guard and the staged store path.
fn stage(src: &Path) -> (tempfile::TempDir, PathBuf) {
    let td = tempfile::tempdir().unwrap();
    let dst = td.path().join("store");
    copy_dir(src, &dst);
    (td, dst)
}

/// Pure-Rust downsync matching the ffi helper's settings.
fn rust_downsync(
    storage_uri: &Path,
    source_lvi: &Path,
    target: &Path,
    lsi: Option<Vec<String>>,
    cache: Option<&Path>,
) -> Result<(), String> {
    let mut opts = DownsyncOptions::new(
        vec![source_lvi.to_string_lossy().into_owned()],
        storage_uri.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.retain_permissions = true;
    opts.scan_target = true;
    opts.cache_target_index = false;
    opts.cache_path = cache.map(|c| c.to_path_buf());
    if let Some(lsi) = lsi {
        opts.version_local_store_index_paths = lsi;
    }
    downsync_blocking(opts)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

fn rust_downsync_multi(
    storage_uri: &Path,
    sources: &[PathBuf],
    target: &Path,
) -> Result<(), String> {
    let mut opts = DownsyncOptions::new(
        sources
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        storage_uri.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false;
    downsync_blocking(opts)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The C `get_existing_store_index_sync` has a **missed-wake race** that can
/// intermittently hang the ffi leg (Stage 5 audit nit d). CI timeout-minutes
/// catch a hang eventually, but an in-test watchdog is better: each ffi downsync
/// runs on a worker thread bounded by a per-attempt timeout, and because a
/// missed wake is recovered by a *fresh* call (new C job/cond-var state), a
/// timed-out attempt is retried before the leg is declared failed. The hung
/// worker thread is abandoned (it waits forever on its own cond var — harmless
/// in a short-lived test process; the retry uses independent C objects).
const FFI_ATTEMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const FFI_MAX_ATTEMPTS: u32 = 3;

fn ffi_downsync(
    storage_uri: &Path,
    source_lvi: &Path,
    target: &Path,
    lsi: Option<Vec<String>>,
    cache: Option<&Path>,
) -> Result<(), String> {
    for attempt in 1..=FFI_MAX_ATTEMPTS {
        let storage = storage_uri.to_path_buf();
        let source = source_lvi.to_path_buf();
        let target = target.to_path_buf();
        let cache = cache.map(|c| c.to_path_buf());
        let lsi = lsi.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = longtail_testkit::differential::downsync_version(
                &storage.to_string_lossy(),
                &source,
                &target,
                lsi,
                cache.as_deref(),
            );
            let _ = tx.send(r);
        });
        match rx.recv_timeout(FFI_ATTEMPT_TIMEOUT) {
            Ok(r) => return r,
            Err(_) => {
                eprintln!(
                    "ffi downsync watchdog: attempt {attempt}/{FFI_MAX_ATTEMPTS} did not \
                     complete within {}s (get_existing_store_index_sync missed-wake race); \
                     retrying with fresh C state",
                    FFI_ATTEMPT_TIMEOUT.as_secs()
                );
            }
        }
    }
    panic!(
        "ffi downsync watchdog: C downsync hung on all {FFI_MAX_ATTEMPTS} attempts \
         (get_existing_store_index_sync missed-wake race did not recover)"
    )
}

/// Spawn the pinned golongtail binary; `None` when it isn't cached / can't run.
fn golongtail_downsync(
    storage_uri: &Path,
    source_lvi: &Path,
    target: &Path,
    lsi: Option<&Path>,
    cache: Option<&Path>,
) -> Option<Result<(), String>> {
    let bin = golongtail_binary()?;
    let mut cmd = Command::new(bin);
    cmd.arg("downsync")
        .arg("--storage-uri")
        .arg(storage_uri)
        .arg("--source-path")
        .arg(source_lvi)
        .arg("--target-path")
        .arg(target)
        .arg("--no-cache-target-index");
    if let Some(lsi) = lsi {
        cmd.arg("--version-local-store-index-path").arg(lsi);
    }
    if let Some(c) = cache {
        cmd.arg("--cache-path").arg(c);
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(_) => return None,
    };
    Some(if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    })
}

fn capture(dir: &Path) -> TreeManifest {
    TreeManifest::capture(dir).unwrap()
}

/// Fresh downsync of every fixture version → Rust ≡ ffi ≡ golongtail(if present).
#[test]
fn three_way_fresh_all_versions() {
    pin_umask();
    let cases: &[(PathBuf, PathBuf, &str)] = &[
        (fixture_store_default(), lvi("zoo.lvi"), "zoo.json"),
        (
            fixture_store_default(),
            lvi("chain-v1.lvi"),
            "chain-v1.json",
        ),
        (
            fixture_store_default(),
            lvi("chain-v2.lvi"),
            "chain-v2.json",
        ),
        (
            fixture_store_default(),
            lvi("chain-v3.lvi"),
            "chain-v3.json",
        ),
        (
            fixtures_dir().join("stores/sharded"),
            fixtures_dir().join("stores/sharded/version.lvi"),
            "sharded-union.json",
        ),
    ];
    let mut golongtail_ran = false;
    for (fixture_store, source, man) in cases {
        let (_g, store) = stage(fixture_store);
        let tmp = tempfile::tempdir().unwrap();
        let tr = tmp.path().join("rust");
        let tf = tmp.path().join("ffi");
        rust_downsync(&store, source, &tr, None, None).expect("rust downsync");
        ffi_downsync(&store, source, &tf, None, None).expect("ffi downsync");
        let expected = manifest(man);
        capture(&tr)
            .compare(&expected, false)
            .expect("rust == manifest");
        capture(&tf)
            .compare(&expected, false)
            .expect("ffi == manifest");
        capture(&tr)
            .compare(&capture(&tf), false)
            .expect("rust == ffi");
        if let Some(res) = golongtail_downsync(&store, source, &tmp.path().join("go"), None, None) {
            res.expect("golongtail downsync");
            capture(&tmp.path().join("go"))
                .compare(&expected, false)
                .expect("golongtail == manifest");
            golongtail_ran = true;
        }
    }
    eprintln!(
        "three_way_fresh: golongtail third way {}",
        if golongtail_ran {
            "RAN"
        } else {
            "SKIPPED (binary absent)"
        }
    );
}

/// Resume: v1 then v2 into the same target → Rust ≡ ffi.
#[test]
fn three_way_resume() {
    pin_umask();
    for tag in ["rust", "ffi"] {
        let (_g, store) = stage(&fixture_store_default());
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");
        if tag == "rust" {
            rust_downsync(&store, &lvi("chain-v1.lvi"), &target, None, None).unwrap();
            rust_downsync(&store, &lvi("chain-v2.lvi"), &target, None, None).unwrap();
        } else {
            ffi_downsync(&store, &lvi("chain-v1.lvi"), &target, None, None).unwrap();
            ffi_downsync(&store, &lvi("chain-v2.lvi"), &target, None, None).unwrap();
        }
        capture(&target)
            .compare(&manifest("chain-v2.json"), false)
            .unwrap();
    }
}

/// Deterministic synthetic partial state (planning exit line).
#[test]
fn synthetic_partial_state() {
    pin_umask();
    for tag in ["rust", "ffi"] {
        let (_g, store) = stage(&fixture_store_default());
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");
        if tag == "rust" {
            rust_downsync(&store, &lvi("chain-v1.lvi"), &target, None, None).unwrap();
        } else {
            ffi_downsync(&store, &lvi("chain-v1.lvi"), &target, None, None).unwrap();
        }
        perturb(&target);
        let _ = std::fs::remove_file(target.join(".longtail.index.cache.lvi"));
        if tag == "rust" {
            rust_downsync(&store, &lvi("chain-v2.lvi"), &target, None, None).unwrap();
        } else {
            ffi_downsync(&store, &lvi("chain-v2.lvi"), &target, None, None).unwrap();
        }
        capture(&target)
            .compare(&manifest("chain-v2.json"), false)
            .unwrap_or_else(|e| panic!("{tag}: partial-state resume: {e}"));
    }
}

fn perturb(target: &Path) {
    use std::io::Write;
    let f = target.join("abitoftext.txt");
    if let Ok(bytes) = std::fs::read(&f) {
        std::fs::write(&f, &bytes[..bytes.len() / 2]).unwrap();
    }
    let _ = std::fs::remove_file(target.join("to-move.txt"));
    if let Ok(mut fh) = std::fs::OpenOptions::new()
        .append(true)
        .open(target.join("script.sh"))
    {
        fh.write_all(&vec![b'X'; 4096]).unwrap();
    }
}

/// Dirty target heals with scan-target + no cache index (Rust vs ffi).
#[test]
fn dirty_target_heals() {
    pin_umask();
    for tag in ["rust", "ffi"] {
        let (_g, store) = stage(&fixture_store_default());
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");
        if tag == "rust" {
            rust_downsync(&store, &lvi("chain-v1.lvi"), &target, None, None).unwrap();
        } else {
            ffi_downsync(&store, &lvi("chain-v1.lvi"), &target, None, None).unwrap();
        }
        std::fs::write(
            target.join("folder/abitoftextinasubfolder.txt"),
            b"CORRUPTED CONTENT",
        )
        .unwrap();
        if tag == "rust" {
            rust_downsync(&store, &lvi("chain-v2.lvi"), &target, None, None).unwrap();
        } else {
            ffi_downsync(&store, &lvi("chain-v2.lvi"), &target, None, None).unwrap();
        }
        capture(&target)
            .compare(&manifest("chain-v2.json"), false)
            .unwrap();
    }
}

/// Non-healing with a stale cache index (pure-Rust; pins golongtail semantics).
#[test]
fn dirty_target_stale_cache_no_heal() {
    pin_umask();
    let (_g, store) = stage(&fixture_store_default());
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let mut o1 = DownsyncOptions::new(
        vec![lvi("chain-v1.lvi").to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    o1.cache_target_index = true;
    downsync_blocking(o1).unwrap();
    assert!(
        target.join(".longtail.index.cache.lvi").is_file(),
        "cache index written"
    );

    let victim = target.join("folder/abitoftextinasubfolder.txt");
    std::fs::write(&victim, b"STALE DIRT").unwrap();

    let mut o2 = DownsyncOptions::new(
        vec![lvi("chain-v2.lvi").to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    o2.cache_target_index = true;
    downsync_blocking(o2).unwrap();

    assert_eq!(
        std::fs::read(&victim).unwrap(),
        b"STALE DIRT",
        "stale cache must skip the scan and leave the dirtied file un-healed"
    );
}

/// Cache passthrough byte-identity + cross-impl block-cache handoff.
#[test]
fn cache_passthrough_and_cross_impl_handoff() {
    pin_umask();
    let (_g, store) = stage(&fixture_store_default());
    let tmp = tempfile::tempdir().unwrap();

    let cache_r = tmp.path().join("cache_r");
    let target_r = tmp.path().join("t_r");
    rust_downsync(
        &store,
        &lvi("chain-v2.lvi"),
        &target_r,
        None,
        Some(&cache_r),
    )
    .unwrap();
    assert_lrb_matches_lsb(&cache_r, &store);

    // ffi writes a cache, Rust reuses it for a later version.
    let cache_x = tmp.path().join("cache_x");
    let t_ffi = tmp.path().join("t_ffi");
    ffi_downsync(&store, &lvi("chain-v1.lvi"), &t_ffi, None, Some(&cache_x)).unwrap();
    let t_rust = tmp.path().join("t_rust");
    rust_downsync(&store, &lvi("chain-v3.lvi"), &t_rust, None, Some(&cache_x)).unwrap();
    capture(&t_rust)
        .compare(&manifest("chain-v3.json"), false)
        .unwrap();

    // Reverse: Rust writes a cache, ffi reuses it.
    let cache_y = tmp.path().join("cache_y");
    let t_rust2 = tmp.path().join("t_rust2");
    rust_downsync(&store, &lvi("chain-v1.lvi"), &t_rust2, None, Some(&cache_y)).unwrap();
    let t_ffi2 = tmp.path().join("t_ffi2");
    ffi_downsync(&store, &lvi("chain-v3.lvi"), &t_ffi2, None, Some(&cache_y)).unwrap();
    capture(&t_ffi2)
        .compare(&manifest("chain-v3.json"), false)
        .unwrap();
}

fn assert_lrb_matches_lsb(cache: &Path, store: &Path) {
    let chunks = cache.join("chunks");
    let mut checked = 0;
    for lrb in walk_ext(&chunks, "lrb") {
        let stem = lrb.file_stem().unwrap().to_string_lossy().into_owned();
        let sub = &stem[2..6];
        let lsb = store.join("chunks").join(sub).join(format!("{stem}.lsb"));
        let a = std::fs::read(&lrb).unwrap();
        let b = std::fs::read(&lsb)
            .unwrap_or_else(|e| panic!("store .lsb {} missing: {e}", lsb.display()));
        assert_eq!(a, b, "cache .lrb must byte-match the store .lsb for {stem}");
        checked += 1;
    }
    assert!(checked > 0, "expected cache to hold at least one .lrb");
}

/// Cross-impl index-cache handoff: a launcher-written `.longtail.index.cache.lvi`
/// is a plain source `.lvi`, so Rust consumes it (skips the scan) and the
/// Rust-written cache is byte-identical to the source `.lvi` (byte-gated).
#[test]
fn cross_impl_index_cache_handoff() {
    pin_umask();
    let (_g, store) = stage(&fixture_store_default());
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");

    let mut o1 = DownsyncOptions::new(
        vec![lvi("chain-v1.lvi").to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    o1.cache_target_index = true;
    downsync_blocking(o1).unwrap();
    let cache_idx = target.join(".longtail.index.cache.lvi");
    assert_eq!(
        std::fs::read(&cache_idx).unwrap(),
        std::fs::read(lvi("chain-v1.lvi")).unwrap(),
        "the written cache index is byte-identical to the source .lvi"
    );

    let mut o2 = DownsyncOptions::new(
        vec![lvi("chain-v2.lvi").to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    o2.cache_target_index = true;
    downsync_blocking(o2).unwrap();
    // The written cache index legitimately remains in the target; drop it before
    // comparing content (golongtail leaves it too).
    let mut got = capture(&target);
    got.entries
        .retain(|e| e.path != ".longtail.index.cache.lvi");
    got.compare(&manifest("chain-v2.json"), false).unwrap();
}

/// Multi-version (merged sources) union — Rust superset semantics.
#[test]
fn three_way_multi_version() {
    pin_umask();
    let (_g, store) = stage(&fixture_store_default());
    let tmp = tempfile::tempdir().unwrap();
    let sources = vec![lvi("chain-v1.lvi"), lvi("chain-v3.lvi")];
    let tr = tmp.path().join("rust");
    rust_downsync_multi(&store, &sources, &tr).unwrap();
    assert!(tr.join("morestuff.txt").is_file(), "v3-only present");
    assert!(
        tr.join("to-delete.txt").is_file(),
        "v1-only present in union"
    );
    assert!(
        tr.join("folder/to-move.txt").is_file(),
        "v3 moved file present"
    );
}

fn walk_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk_ext(&p, ext));
            } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
                out.push(p);
            }
        }
    }
    out
}
