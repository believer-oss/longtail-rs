//! golongtail CLI black-box tests ported from `commands/*_test.go`.
//!
//! Download-path commands (downsync/get/ls/validate-version/print-version) are
//! driven against the built `longtail` binary. The Go originals upsync first;
//! we satisfy them from committed fixtures instead (`fixtures/stores/default`
//! carries the v1/v2/v3 chain + zoo over one store; get-config JSONs are
//! synthesized at test time). The upload/maintenance path is exercised too;
//! only the `archive` feature (pack/unpack) remains `#[ignore]`d.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_longtail")
}

#[cfg(unix)]
fn pin_umask() {
    // SAFETY: single libc call; children inherit the umask so subprocess dir
    // creation matches the umask-022 fixture-generation environment.
    unsafe {
        libc::umask(0o022);
    }
}

/// No umask to pin: permissions are synthesized rather than read from the
/// filesystem (format-spec §7), so nothing here depends on the process umask.
#[cfg(not(unix))]
fn pin_umask() {}

fn run(args: &[&str], cwd: Option<&Path>) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.output().expect("spawn longtail binary")
}

fn run_ok(args: &[&str]) -> Output {
    let out = run(args, None);
    assert!(
        out.status.success(),
        "command {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn capture(dir: &Path) -> TreeManifest {
    TreeManifest::capture(dir).unwrap()
}

fn manifest(name: &str) -> TreeManifest {
    let p = fixtures_dir().join("manifests").join(name);
    TreeManifest::from_json(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

fn store() -> PathBuf {
    fixtures_dir().join("stores/default/store")
}

fn lvi(name: &str) -> PathBuf {
    fixtures_dir().join("stores/default").join(name)
}

/// Recursively copy a directory (fixtures → a mutable temp copy).
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

// =========================================================================
// Download path
// =========================================================================

/// cmd_downsync_test.go::TestDownsync — downsync a version into an empty target
/// reproduces the exact source tree.
#[test]
fn downsync() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("zoo.lvi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("zoo.json"), cfg!(windows))
        .expect("zoo tree matches manifest");
}

/// cmd_downsync_test.go::TestDownsyncNoTargetPath — the target folder is derived
/// from the source version name (basename before the first dot).
#[test]
fn downsync_no_target_path() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    // Copy chain-v1.lvi to <tmp>/game.v1.lvi → derived target "game".
    let src_lvi = tmp.path().join("game.v1.lvi");
    std::fs::copy(lvi("chain-v1.lvi"), &src_lvi).unwrap();
    let out = run(
        &[
            "downsync",
            "--storage-uri",
            store().to_str().unwrap(),
            "--source-path",
            "game.v1.lvi",
            "--no-cache-target-index",
        ],
        Some(tmp.path()),
    );
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let derived = tmp.path().join("game");
    assert!(derived.is_dir(), "derived target `game` should exist");
    assert!(derived.join("abitoftext.txt").is_file());
}

/// cmd_downsync_test.go::TestDownsyncWithVersionLSI — a version-local store index
/// produces the same tree as the master index.
#[test]
fn downsync_with_version_lsi() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
        "--version-local-store-index-path",
        lvi("chain-v1-store.lsi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v1.json"), cfg!(windows))
        .expect("v1 tree via version-local store index");
}

/// cmd_downsync_test.go::TestDownsyncWithCache — cache path populated; tree correct.
#[test]
fn downsync_with_cache() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache = tmp.path().join("cache");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("chain-v2.lvi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--cache-path",
        cache.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v2.json"), cfg!(windows))
        .expect("v2 tree with cache");
    // Cache populated with .lrb blocks.
    let lrb = count_ext(&cache, "lrb");
    assert!(lrb > 0, "cache should hold .lrb blocks");
}

/// cmd_downsync_test.go::TestDownsyncWithLSIAndCache.
#[test]
fn downsync_with_lsi_and_cache() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache = tmp.path().join("cache");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("chain-v3.lvi").to_str().unwrap(),
        "--version-local-store-index-path",
        lvi("chain-v3-store.lsi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--cache-path",
        cache.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v3.json"), cfg!(windows))
        .expect("v3 tree with lsi + cache");
    assert!(count_ext(&cache, "lrb") > 0);
}

/// cmd_downsync_test.go::TestDownsyncWithValidate.
#[test]
fn downsync_with_validate() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("zoo.lvi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--validate",
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("zoo.json"), cfg!(windows))
        .unwrap();
}

/// cmd_downsync_test.go::TestDownsyncWithVersionLSIWithValidate.
#[test]
fn downsync_with_version_lsi_with_validate() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("chain-v2.lvi").to_str().unwrap(),
        "--version-local-store-index-path",
        lvi("chain-v2-store.lsi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--validate",
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v2.json"), cfg!(windows))
        .unwrap();
}

/// cmd_downsync_test.go::TestDownsyncWithCacheWithValidate.
#[test]
fn downsync_with_cache_with_validate() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache = tmp.path().join("cache");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--cache-path",
        cache.to_str().unwrap(),
        "--validate",
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v1.json"), cfg!(windows))
        .unwrap();
}

/// cmd_downsync_test.go::TestDownsyncWithLSIAndCacheWithValidate.
#[test]
fn downsync_with_lsi_and_cache_with_validate() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache = tmp.path().join("cache");
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-path",
        lvi("chain-v3.lvi").to_str().unwrap(),
        "--version-local-store-index-path",
        lvi("chain-v3-store.lsi").to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--cache-path",
        cache.to_str().unwrap(),
        "--validate",
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v3.json"), cfg!(windows))
        .unwrap();
}

/// cmd_downsync_test.go::TestDownsyncMissingChunks — clean error when the store
/// is missing chunks the version needs.
#[test]
fn downsync_missing_chunks() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let broken_store = tmp.path().join("store");
    copy_dir(&store(), &broken_store);
    // Delete every block → the store can no longer satisfy the version.
    delete_ext(&broken_store.join("chunks"), "lsb");
    let target = tmp.path().join("out");
    let out = run(
        &[
            "downsync",
            "--storage-uri",
            broken_store.to_str().unwrap(),
            "--source-path",
            lvi("zoo.lvi").to_str().unwrap(),
            "--target-path",
            target.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        None,
    );
    assert!(
        !out.status.success(),
        "downsync should fail when chunks are missing"
    );
}

/// cmd_downsync_test.go::TestDownsyncMissingIndex — clean error when the source
/// version index cannot be read.
#[test]
fn downsync_missing_index() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let out = run(
        &[
            "downsync",
            "--storage-uri",
            store().to_str().unwrap(),
            "--source-path",
            tmp.path().join("does-not-exist.lvi").to_str().unwrap(),
            "--target-path",
            target.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        None,
    );
    assert!(
        !out.status.success(),
        "downsync should fail on a missing source index"
    );
}

/// Regression: a corrupt target-index cache
/// (`.longtail.index.cache.lvi`) is a hard parse error, not silently ignored.
/// (When `--cache-target-index` is on — the default — the cache file is read as
/// the target index; a malformed one must fail cleanly.)
#[test]
fn downsync_corrupt_target_index_cache() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    std::fs::create_dir_all(&target).unwrap();
    // Plant a corrupt cache index (NOT a valid `.lvi`).
    std::fs::write(
        target.join(".longtail.index.cache.lvi"),
        b"this is not a valid version index",
    )
    .unwrap();
    // Default cache-target-index (no --no-cache-target-index) reads the cache.
    let out = run(
        &[
            "downsync",
            "--storage-uri",
            store().to_str().unwrap(),
            "--source-path",
            lvi("zoo.lvi").to_str().unwrap(),
            "--target-path",
            target.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        !out.status.success(),
        "downsync should fail hard on a corrupt target-index cache"
    );
}

/// cmd_downsync_test.go::TestMultiVersionDownsync — downsync of multiple merged
/// source versions produces the union tree. (Full three-way tree agreement is in
/// the testkit differential lane; here we assert the union superset semantics.)
#[test]
fn multi_version_downsync() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let sources = format!(
        "{}|{}",
        lvi("chain-v1.lvi").to_str().unwrap(),
        lvi("chain-v3.lvi").to_str().unwrap()
    );
    run_ok(&[
        "downsync",
        "--storage-uri",
        store().to_str().unwrap(),
        "--source-paths",
        &sources,
        "--target-path",
        target.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    // v3-only file present (overlay wins) …
    assert!(
        target.join("morestuff.txt").is_file(),
        "v3-only file present"
    );
    assert!(
        target.join("folder/to-move.txt").is_file(),
        "v3 moved file present"
    );
    // … and a v1-only file also present (union).
    assert!(
        target.join("to-delete.txt").is_file(),
        "v1-only file present in union"
    );
}

/// cmd_get_test.go::TestGet — `get` reads a get-config JSON and downsyncs it.
#[test]
fn get() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let gc = tmp.path().join("gc");
    copy_dir(&fixtures_dir().join("get-configs"), &gc);
    // Reference: direct downsync of the same version.
    let reference = tmp.path().join("ref");
    run_ok(&[
        "downsync",
        "--storage-uri",
        gc.join("store").to_str().unwrap(),
        "--source-path",
        gc.join("version.lvi").to_str().unwrap(),
        "--target-path",
        reference.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    // get via config (relative URIs → run with cwd = gc dir).
    let target = tmp.path().join("out");
    let out = run(
        &[
            "get",
            "--source-path",
            "get-config.json",
            "--target-path",
            target.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        Some(&gc),
    );
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    capture(&target)
        .compare(&capture(&reference), cfg!(windows))
        .expect("get tree == direct downsync tree");
}

/// cmd_get_test.go::TestGetWithVersionLSI — get-config carrying a lsi path.
#[test]
fn get_with_version_lsi() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let config = synth_config(
        tmp.path(),
        "cfg.json",
        &[(
            lvi("chain-v1.lvi"),
            store(),
            Some(lvi("chain-v1-store.lsi")),
        )],
    );
    run_ok(&[
        "get",
        "--source-path",
        config.to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v1.json"), cfg!(windows))
        .unwrap();
}

/// cmd_get_test.go::TestGetWithCache.
#[test]
fn get_with_cache() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache = tmp.path().join("cache");
    let config = synth_config(
        tmp.path(),
        "cfg.json",
        &[(lvi("chain-v2.lvi"), store(), None)],
    );
    run_ok(&[
        "get",
        "--source-path",
        config.to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--cache-path",
        cache.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v2.json"), cfg!(windows))
        .unwrap();
    assert!(count_ext(&cache, "lrb") > 0);
}

/// cmd_get_test.go::TestGetWithLSIAndCache.
#[test]
fn get_with_lsi_and_cache() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let cache = tmp.path().join("cache");
    let config = synth_config(
        tmp.path(),
        "cfg.json",
        &[(
            lvi("chain-v3.lvi"),
            store(),
            Some(lvi("chain-v3-store.lsi")),
        )],
    );
    run_ok(&[
        "get",
        "--source-path",
        config.to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--cache-path",
        cache.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&target)
        .compare(&manifest("chain-v3.json"), cfg!(windows))
        .unwrap();
    assert!(count_ext(&cache, "lrb") > 0);
}

/// cmd_get_test.go::TestMultiVersionGet — multiple configs, one store, union tree.
#[test]
fn multi_version_get() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let c1 = synth_config(
        tmp.path(),
        "c1.json",
        &[(lvi("chain-v1.lvi"), store(), None)],
    );
    let c3 = synth_config(
        tmp.path(),
        "c3.json",
        &[(lvi("chain-v3.lvi"), store(), None)],
    );
    let sources = format!("{}|{}", c1.to_str().unwrap(), c3.to_str().unwrap());
    run_ok(&[
        "get",
        "--source-paths",
        &sources,
        "--target-path",
        target.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    assert!(target.join("morestuff.txt").is_file());
    assert!(
        target.join("to-delete.txt").is_file(),
        "v1-only file present in union"
    );
}

/// cmd_get_test.go::TestMultiVersionGetMismatchStoreURI — mismatched storage
/// URIs across configs error clearly.
#[test]
fn multi_version_get_mismatch_store_uri() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    let c1 = synth_config(
        tmp.path(),
        "c1.json",
        &[(lvi("chain-v1.lvi"), store(), None)],
    );
    // Second config points at a DIFFERENT storage-uri.
    let other_store = fixtures_dir().join("stores/comp-lz4/store");
    let c2 = synth_config(
        tmp.path(),
        "c2.json",
        &[(lvi("chain-v2.lvi"), other_store, None)],
    );
    let sources = format!("{}|{}", c1.to_str().unwrap(), c2.to_str().unwrap());
    let out = run(
        &[
            "get",
            "--source-paths",
            &sources,
            "--target-path",
            target.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        None,
    );
    assert!(!out.status.success(), "mismatched storage-uri should error");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("storage-uri") || stderr.contains("get-config"),
        "error should mention storage-uri mismatch: {stderr}"
    );
}

/// cmd_ls_test.go::TestLs — `ls` lists a path inside a version index.
#[test]
fn ls() {
    let out = run_ok(&[
        "ls",
        "--version-index-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
        ".",
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("abitoftext.txt"), "ls root: {stdout}");
    assert!(stdout.contains("folder"), "ls should list the folder dir");
    // ls a subdirectory.
    let out2 = run_ok(&[
        "ls",
        "--version-index-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
        "folder",
    ]);
    let s2 = String::from_utf8_lossy(&out2.stdout);
    assert!(s2.contains("abitoftextinasubfolder.txt"), "ls folder: {s2}");
    assert!(
        !s2.contains("empty-file"),
        "ls folder must not list root entries"
    );
}

/// cmd_validateversion_test.go::TestValidateVersion — valid store passes; a store
/// with the blocks removed fails.
#[test]
fn validate_version() {
    // Valid.
    run_ok(&[
        "validate-version",
        "--storage-uri",
        store().to_str().unwrap(),
        "--version-index-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
    ]);
    // Broken store (blocks removed) → failure.
    let tmp = tempfile::tempdir().unwrap();
    let broken = tmp.path().join("store");
    copy_dir(&store(), &broken);
    delete_ext(&broken.join("chunks"), "lsb");
    // Also drop the store index so no chunks are reported present.
    let _ = std::fs::remove_file(broken.join("store.lsi"));
    let out = run(
        &[
            "validate-version",
            "--storage-uri",
            broken.to_str().unwrap(),
            "--version-index-path",
            lvi("chain-v1.lvi").to_str().unwrap(),
        ],
        None,
    );
    assert!(
        !out.status.success(),
        "validate-version on an empty store should fail"
    );
}

/// cmd_printversion_test.go::TestPrintVersionIndex — prints the summary (+ compact).
#[test]
fn print_version_index() {
    let out = run_ok(&[
        "print-version",
        "--version-index-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
    ]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("Hash Identifier:     blake3"),
        "print-version: {s}"
    );
    assert!(s.contains("Asset Count:"), "print-version: {s}");
    let out2 = run_ok(&[
        "print-version",
        "--version-index-path",
        lvi("chain-v1.lvi").to_str().unwrap(),
        "--compact",
    ]);
    let s2 = String::from_utf8_lossy(&out2.stdout);
    assert!(s2.contains('\t'), "compact output is tab-separated: {s2}");
    assert!(s2.contains("blake3"));
}

// ---- helpers ----

fn count_ext(dir: &Path, ext: &str) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                n += count_ext(&p, ext);
            } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
                n += 1;
            }
        }
    }
    n
}

fn delete_ext(dir: &Path, ext: &str) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                delete_ext(&p, ext);
            } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
                std::fs::remove_file(&p).unwrap();
            }
        }
    }
}

/// Write a get-config JSON referencing one or more `(source_lvi, storage, lsi?)`
/// with absolute URIs (cwd-independent). Multiple entries write multiple configs?
/// No — a get-config is one version; multi-config get uses multiple files.
fn synth_config(
    dir: &Path,
    name: &str,
    entries: &[(PathBuf, PathBuf, Option<PathBuf>)],
) -> PathBuf {
    assert_eq!(entries.len(), 1, "a get-config JSON holds one version");
    let (src, storage, lsi) = &entries[0];
    let mut json = serde_json::json!({
        "source-path": src.to_str().unwrap(),
        "storage-uri": storage.to_str().unwrap(),
    });
    if let Some(lsi) = lsi {
        json["version-local-store-index-path"] = serde_json::json!(lsi.to_str().unwrap());
    }
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    path
}

// =========================================================================
// Upload / maintenance path
// =========================================================================
//
// These are round-trip black-box tests over ad-hoc source trees (the seeded
// corpus is not committed as folders): upsync → downsync/get → tree compare,
// mirroring golongtail's `upsync … ; downsync … ; validateContent` pattern.
// All stores/targets are temp dirs (fixtures stay read-only).

fn write_file(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, content).unwrap();
}

/// v1: two files. v2: + `c.txt`. v3: + unique `d.txt` (its block is prune-bait).
fn make_v1(dir: &Path) {
    write_file(dir, "a.txt", &"content-a-".repeat(30));
    write_file(dir, "folder/b.txt", &"content-b-".repeat(30));
}
fn make_v2(dir: &Path) {
    make_v1(dir);
    write_file(dir, "c.txt", &"content-c-".repeat(30));
}
fn make_v3(dir: &Path) {
    make_v2(dir);
    write_file(dir, "d.txt", &"unique-d-content-".repeat(40));
}

fn run_upsync(store: &Path, src: &Path, lvi: &Path, extra: &[&str]) {
    let mut args = vec![
        "upsync",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-path",
        src.to_str().unwrap(),
        "--target-path",
        lvi.to_str().unwrap(),
    ];
    args.extend_from_slice(extra);
    run_ok(&args);
}

fn run_downsync_ok(store: &Path, lvi: &Path, target: &Path, extra: &[&str]) {
    let mut args = vec![
        "downsync",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-path",
        lvi.to_str().unwrap(),
        "--target-path",
        target.to_str().unwrap(),
        "--no-cache-target-index",
    ];
    args.extend_from_slice(extra);
    run_ok(&args);
}

/// Build a three-version store; returns `(store, v1.lvi, v2.lvi, v3.lvi, v1dir)`.
fn three_version_store(tmp: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let store = tmp.join("store");
    let (s1, s2, s3) = (tmp.join("s1"), tmp.join("s2"), tmp.join("s3"));
    make_v1(&s1);
    make_v2(&s2);
    make_v3(&s3);
    let (l1, l2, l3) = (tmp.join("v1.lvi"), tmp.join("v2.lvi"), tmp.join("v3.lvi"));
    run_upsync(&store, &s1, &l1, &[]);
    run_upsync(&store, &s2, &l2, &[]);
    run_upsync(&store, &s3, &l3, &[]);
    (store, l1, l2, l3, s1)
}

/// cmd_upsync_test.go::TestUpsync — upsync then downsync reproduces the tree.
#[test]
fn upsync() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let out = tmp.path().join("out");
    run_downsync_ok(&store, &lvi, &out, &[]);
    capture(&out)
        .compare(&capture(&src), cfg!(windows))
        .expect("upsync→downsync tree");
}

/// cmd_upsync_test.go::TestUpsyncWithLSI — the version-local .lsi downsyncs the tree.
#[test]
fn upsync_with_lsi() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    let lsi = tmp.path().join("v.lsi");
    run_upsync(
        &store,
        &src,
        &lvi,
        &["--version-local-store-index-path", lsi.to_str().unwrap()],
    );
    assert!(lsi.is_file(), "upsync should write the version-local .lsi");
    let out = tmp.path().join("out");
    run_downsync_ok(
        &store,
        &lvi,
        &out,
        &["--version-local-store-index-path", lsi.to_str().unwrap()],
    );
    capture(&out)
        .compare(&capture(&src), cfg!(windows))
        .unwrap();
}

/// cmd_upsync_test.go::TestUpsyncWithBrokenLSI — a corrupt version-local .lsi is
/// tolerated (downsync falls back to scanning the store index).
#[test]
fn upsync_with_broken_lsi() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    let lsi = tmp.path().join("v.lsi");
    run_upsync(
        &store,
        &src,
        &lvi,
        &["--version-local-store-index-path", lsi.to_str().unwrap()],
    );
    // Corrupt the lsi → downsync must fall back to the store index and succeed.
    std::fs::write(&lsi, b"not a valid store index").unwrap();
    let out = tmp.path().join("out");
    run_downsync_ok(
        &store,
        &lvi,
        &out,
        &["--version-local-store-index-path", lsi.to_str().unwrap()],
    );
    capture(&out)
        .compare(&capture(&src), cfg!(windows))
        .unwrap();
}

/// cmd_put — `put` derives storage/.lvi/.lsi from the get-config path, upsyncs,
/// writes the get-config; `get` then reproduces the tree.
#[test]
fn put() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let config = work.join("game.json");
    run_ok(&[
        "put",
        "--target-path",
        config.to_str().unwrap(),
        "--source-path",
        src.to_str().unwrap(),
    ]);
    assert!(config.is_file(), "put should write the get-config");
    // Derived layout: <work>/store, <work>/version-data/version-index/game.lvi.
    assert!(work.join("store").is_dir(), "derived store dir");
    let out = tmp.path().join("out");
    run_ok(&[
        "get",
        "--source-path",
        config.to_str().unwrap(),
        "--target-path",
        out.to_str().unwrap(),
        "--no-cache-target-index",
    ]);
    capture(&out)
        .compare(&capture(&src), cfg!(windows))
        .unwrap();
}

/// cmd_initremotestore_test.go::TestInitRemoteStore — deleting store.lsi then
/// init-remote-store rebuilds a usable store index.
#[test]
fn init_remote_store() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, l1, _l2, _l3, s1) = three_version_store(tmp.path());
    // Remove the store index; the store is now index-less.
    std::fs::remove_file(store.join("store.lsi")).ok();
    delete_ext(&store, "lsi");
    run_ok(&[
        "init-remote-store",
        "--storage-uri",
        store.to_str().unwrap(),
    ]);
    // After rebuild, v1 downsyncs correctly again.
    let out = tmp.path().join("out");
    run_downsync_ok(&store, &l1, &out, &[]);
    capture(&out).compare(&capture(&s1), cfg!(windows)).unwrap();
}

/// cmd_createversionstoreindex_test.go::TestCreateVersionStoreIndex.
#[test]
fn create_version_store_index() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let lsi = tmp.path().join("v.lsi");
    run_ok(&[
        "create-version-store-index",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-path",
        lvi.to_str().unwrap(),
        "--version-local-store-index-path",
        lsi.to_str().unwrap(),
    ]);
    assert!(lsi.is_file());
    let out = tmp.path().join("out");
    run_downsync_ok(
        &store,
        &lvi,
        &out,
        &["--version-local-store-index-path", lsi.to_str().unwrap()],
    );
    capture(&out)
        .compare(&capture(&src), cfg!(windows))
        .unwrap();
}

/// cmd_prunestore_test.go::TestPrune — keep v1+v2; v3's unique block is deleted,
/// so v1/v2 still downsync but v3 fails. Plus a dry-run that deletes nothing.
#[test]
fn prune_store() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, l1, l2, l3, s1) = three_version_store(tmp.path());
    let files = tmp.path().join("files.txt");
    std::fs::write(
        &files,
        format!("{}\n{}\n", l1.to_str().unwrap(), l2.to_str().unwrap()),
    )
    .unwrap();

    // Dry-run first: nothing deleted, v3 still downsyncs.
    run_ok(&[
        "prune-store",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-paths",
        files.to_str().unwrap(),
        "--dry-run",
    ]);
    let dry = tmp.path().join("dry");
    run_downsync_ok(&store, &l3, &dry, &[]);

    // Real prune.
    run_ok(&[
        "prune-store",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-paths",
        files.to_str().unwrap(),
    ]);
    // v1 still good.
    let out1 = tmp.path().join("out1");
    run_downsync_ok(&store, &l1, &out1, &[]);
    capture(&out1)
        .compare(&capture(&s1), cfg!(windows))
        .unwrap();
    // v3 now fails (its block was pruned).
    let out3 = tmp.path().join("out3");
    let bad = run(
        &[
            "downsync",
            "--storage-uri",
            store.to_str().unwrap(),
            "--source-path",
            l3.to_str().unwrap(),
            "--target-path",
            out3.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        None,
    );
    assert!(!bad.status.success(), "v3 downsync should fail after prune");
}

/// A `--source-paths` file that resolves to nothing must not be read as "keep
/// nothing, delete everything".
///
/// The realistic way to produce one is a listing command that fails or matches
/// nothing and is redirected to a file, which yields an empty or all-blank file
/// rather than an error. Deleting every block on that input is silent, total and
/// irreversible, so the command refuses unless asked explicitly.
#[test]
fn prune_store_refuses_an_empty_keep_set() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, l1, _l2, _l3, _s1) = three_version_store(tmp.path());
    let before = count_ext(&store, "lsb");
    assert!(before > 0, "fixture store should contain blocks");

    for (name, contents) in [("empty.txt", ""), ("blank.txt", "\n   \n\t\n")] {
        let list = tmp.path().join(name);
        std::fs::write(&list, contents).unwrap();

        // Refused in dry-run too: a dry run is the safety surface, so it is the
        // place the mistake should surface rather than the one place it passes.
        for extra in [&["--dry-run"][..], &[][..]] {
            let mut argv = vec![
                "prune-store",
                "--storage-uri",
                store.to_str().unwrap(),
                "--source-paths",
                list.to_str().unwrap(),
            ];
            argv.extend_from_slice(extra);
            let out = run(&argv, None);
            assert!(
                !out.status.success(),
                "{name} {extra:?}: prune-store must refuse an empty keep-set"
            );
        }
        assert_eq!(
            count_ext(&store, "lsb"),
            before,
            "{name}: no block may be deleted by a refused prune"
        );
    }

    // The escape hatch still works, and still prunes everything — which is why
    // it has to be asked for.
    let list = tmp.path().join("empty.txt");
    run_ok(&[
        "prune-store",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-paths",
        list.to_str().unwrap(),
        "--allow-empty-keep-set",
    ]);
    assert_eq!(
        count_ext(&store, "lsb"),
        0,
        "--allow-empty-keep-set should delete every block"
    );

    // And the store really is gone: a version that downsynced before now fails.
    let out = tmp.path().join("after");
    let bad = run(
        &[
            "downsync",
            "--storage-uri",
            store.to_str().unwrap(),
            "--source-path",
            l1.to_str().unwrap(),
            "--target-path",
            out.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        None,
    );
    assert!(!bad.status.success(), "everything was pruned; v1 must fail");
}

/// cmd_prunestore_index_test.go::TestPruneIndex — only the index is rewritten;
/// v3 downsync still fails (blocks remain but are unreachable via the index).
#[test]
fn prune_store_index() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, l1, l2, l3, _s1) = three_version_store(tmp.path());
    let index = store.join("store.lsi");
    let files = tmp.path().join("files.txt");
    std::fs::write(
        &files,
        format!("{}\n{}\n", l1.to_str().unwrap(), l2.to_str().unwrap()),
    )
    .unwrap();
    run_ok(&[
        "prune-store-index",
        "--store-index-path",
        index.to_str().unwrap(),
        "--source-paths",
        files.to_str().unwrap(),
    ]);
    // v1 still good; v3 unreachable.
    let out1 = tmp.path().join("out1");
    run_downsync_ok(&store, &l1, &out1, &[]);
    let out3 = tmp.path().join("out3");
    let bad = run(
        &[
            "downsync",
            "--storage-uri",
            store.to_str().unwrap(),
            "--source-path",
            l3.to_str().unwrap(),
            "--target-path",
            out3.to_str().unwrap(),
            "--no-cache-target-index",
        ],
        None,
    );
    assert!(
        !bad.status.success(),
        "v3 downsync should fail after index prune"
    );
}

/// cmd_prunestore_block_test.go::TestPruneStoreBlocks — prune the index, then
/// delete the now-orphan block file; the on-disk .lsb count drops.
#[test]
fn prune_store_blocks() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (store, l1, l2, _l3, _s1) = three_version_store(tmp.path());
    let index = store.join("store.lsi");
    let chunks = store.join("chunks");
    let files = tmp.path().join("files.txt");
    std::fs::write(
        &files,
        format!("{}\n{}\n", l1.to_str().unwrap(), l2.to_str().unwrap()),
    )
    .unwrap();
    let before = count_ext(&chunks, "lsb");
    // Shrink the index to v1+v2 blocks.
    run_ok(&[
        "prune-store-index",
        "--store-index-path",
        index.to_str().unwrap(),
        "--source-paths",
        files.to_str().unwrap(),
    ]);
    // Dry-run: nothing deleted.
    run_ok(&[
        "prune-store-blocks",
        "--store-index-path",
        index.to_str().unwrap(),
        "--blocks-root-path",
        chunks.to_str().unwrap(),
        "--dry-run",
    ]);
    assert_eq!(count_ext(&chunks, "lsb"), before, "dry-run deletes nothing");
    // Real: the orphan block is deleted.
    run_ok(&[
        "prune-store-blocks",
        "--store-index-path",
        index.to_str().unwrap(),
        "--blocks-root-path",
        chunks.to_str().unwrap(),
    ]);
    assert!(count_ext(&chunks, "lsb") < before, "orphan block deleted");
}

/// cmd_clonestore_test.go::TestCloneStore — clone v1/v2/v3 into a target store,
/// then downsync each from the target.
#[test]
fn clone_store() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (src_store, l1, l2, l3, s1) = three_version_store(tmp.path());
    let tgt_store = tmp.path().join("tgt-store");
    let materialize = tmp.path().join("materialize");
    let (t1, t2, t3) = (
        tmp.path().join("t1.lvi"),
        tmp.path().join("t2.lvi"),
        tmp.path().join("t3.lvi"),
    );
    let sources = tmp.path().join("sources.txt");
    let targets = tmp.path().join("targets.txt");
    std::fs::write(
        &sources,
        format!(
            "{}\n{}\n{}\n",
            l1.to_str().unwrap(),
            l2.to_str().unwrap(),
            l3.to_str().unwrap()
        ),
    )
    .unwrap();
    std::fs::write(
        &targets,
        format!(
            "{}\n{}\n{}\n",
            t1.to_str().unwrap(),
            t2.to_str().unwrap(),
            t3.to_str().unwrap()
        ),
    )
    .unwrap();
    run_ok(&[
        "clone-store",
        "--source-storage-uri",
        src_store.to_str().unwrap(),
        "--target-storage-uri",
        tgt_store.to_str().unwrap(),
        "--target-path",
        materialize.to_str().unwrap(),
        "--source-paths",
        sources.to_str().unwrap(),
        "--target-paths",
        targets.to_str().unwrap(),
    ]);
    // v1 downsyncs from the TARGET store.
    let out = tmp.path().join("out");
    run_downsync_ok(&tgt_store, &t1, &out, &[]);
    capture(&out).compare(&capture(&s1), cfg!(windows)).unwrap();
}

/// cmd_printstore_test.go::TestPrintStoreIndex.
#[test]
fn print_store_index() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let out = run_ok(&[
        "print-store",
        "--store-index-path",
        store.join("store.lsi").to_str().unwrap(),
    ]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("Hash Identifier:     blake3"),
        "print-store: {s}"
    );
    assert!(s.contains("Block Count:"), "print-store: {s}");
    // Compact + details.
    let out2 = run_ok(&[
        "print-store",
        "--store-index-path",
        store.join("store.lsi").to_str().unwrap(),
        "--compact",
        "--details",
    ]);
    let s2 = String::from_utf8_lossy(&out2.stdout);
    assert!(s2.contains('\t') && s2.contains("blake3"), "compact: {s2}");
}

/// cmd_printversionusage_test.go::TestPrintVersionUsage.
#[test]
fn print_version_usage() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let out = run_ok(&[
        "print-version-usage",
        "--storage-uri",
        store.to_str().unwrap(),
        "--version-index-path",
        lvi.to_str().unwrap(),
    ]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("Block Usage:"), "print-version-usage: {s}");
    assert!(
        s.contains("Asset Fragmentation:"),
        "print-version-usage: {s}"
    );
}

/// cmd_dumpversionassets_test.go::TestDumpVersionAssets.
#[test]
fn dump_version_assets() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let out = run_ok(&[
        "dump-version-assets",
        "--version-index-path",
        lvi.to_str().unwrap(),
    ]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("a.txt"), "dump: {s}");
    assert!(
        s.contains("folder/b.txt") || s.contains("folder"),
        "dump: {s}"
    );
    // With details: rwx bits present.
    let out2 = run_ok(&[
        "dump-version-assets",
        "--version-index-path",
        lvi.to_str().unwrap(),
        "--details",
    ]);
    let s2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        s2.contains("-rw-") || s2.contains("drwx"),
        "dump details: {s2}"
    );
}

/// cmd_cp_test.go::TestCp — copy one asset out of a version.
#[test]
fn cp() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let dst = tmp.path().join("copied-b.txt");
    run_ok(&[
        "cp",
        "--storage-uri",
        store.to_str().unwrap(),
        "--version-index-path",
        lvi.to_str().unwrap(),
        "folder/b.txt",
        dst.to_str().unwrap(),
    ]);
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        std::fs::read(src.join("folder/b.txt")).unwrap(),
        "cp'd content matches the source asset"
    );
}

// ---- pack/unpack + ArchiveIndex (archive feature — not yet implemented) ----

/// Source: cmd_pack_test.go::TestPack.
#[test]
#[ignore = "requires the unimplemented archive feature (pack/unpack)"]
fn pack() {
    todo!()
}

/// Source: cmd_unpack_test.go::TestUnpack.
#[test]
#[ignore = "requires the unimplemented archive feature (pack/unpack)"]
fn unpack() {
    todo!()
}

/// `--s3-endpoint-resolver-uri` must reach the read-only inspection commands.
///
/// These open no block store, so they build their S3 options from the flag
/// themselves; miss that and the flag parses, is accepted, and silently sends
/// the request to AWS instead of the configured endpoint — which surfaces as a
/// credentials or not-found error against a store the operator can demonstrably
/// reach, and reads as "the store is broken".
///
/// Hermetic: port 1 on loopback refuses immediately, and the assertion is that
/// the connection was attempted *there*. A run that ignored the flag would reach
/// the network instead, so this must never be relaxed into "any error".
#[cfg(feature = "s3")]
#[test]
fn s3_endpoint_flag_reaches_the_inspection_commands() {
    // Both option-less readers: the version-index one and the store-index one.
    let cases = [
        ("print-version", "--version-index-path", "s3://bucket/v.lvi"),
        (
            "dump-version-assets",
            "--version-index-path",
            "s3://bucket/v.lvi",
        ),
        ("print-store", "--store-index-path", "s3://bucket/store.lsi"),
    ];
    for (cmd, flag, uri) in cases {
        let out = Command::new(bin())
            .args([
                cmd,
                flag,
                uri,
                "--s3-endpoint-resolver-uri",
                "http://127.0.0.1:1",
            ])
            // Credentials must resolve for the request to be attempted at all;
            // these are never sent anywhere but loopback.
            .env("AWS_ACCESS_KEY_ID", "test")
            .env("AWS_SECRET_ACCESS_KEY", "test")
            .env("AWS_REGION", "us-east-1")
            .output()
            .expect("spawn longtail binary");

        assert!(
            !out.status.success(),
            "{cmd} should fail: nothing is listening"
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("127.0.0.1:1"),
            "{cmd} did not use the configured endpoint; stderr: {err}"
        );
    }
}

/// `clone-store --create-version-local-store-index` derives the `.lsi` path from
/// the target `.lvi` path by replacing every `.lvi` — which golongtail also does
/// (`strings.Replace(targetFilePath, ".lvi", ".lsi", -1)`). A target carrying no
/// `.lvi` therefore derives to *itself*, and the store index would be written over
/// the version index written moments earlier through the same truncating write.
///
/// Refusing is a deliberate divergence: it is the one input where upstream's
/// derivation destroys the artefact it just produced. The version index must
/// survive, which is what makes the failure recoverable by re-running without the
/// flag.
#[test]
fn clone_store_refuses_a_target_that_would_overwrite_its_version_index() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (src_store, l1, _l2, _l3, _s1) = three_version_store(tmp.path());
    let tgt_store = tmp.path().join("tgt-store");
    let materialize = tmp.path().join("materialize");
    // No `.lvi` anywhere in the name — a plausible convention, not a hostile input.
    let target = tmp.path().join("game-v7.index");
    let sources = tmp.path().join("sources.txt");
    let targets = tmp.path().join("targets.txt");
    std::fs::write(&sources, format!("{}\n", l1.to_str().unwrap())).unwrap();
    std::fs::write(&targets, format!("{}\n", target.to_str().unwrap())).unwrap();

    let out = run(
        &[
            "clone-store",
            "--source-storage-uri",
            src_store.to_str().unwrap(),
            "--target-storage-uri",
            tgt_store.to_str().unwrap(),
            "--target-path",
            materialize.to_str().unwrap(),
            "--source-paths",
            sources.to_str().unwrap(),
            "--target-paths",
            targets.to_str().unwrap(),
            "--create-version-local-store-index",
        ],
        None,
    );
    assert!(
        !out.status.success(),
        "a target that derives onto itself must fail, not report success"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("overwrite") || err.contains("written over"),
        "the error must say what it refused to do: {err}"
    );

    // The version index it already wrote must still be a version index.
    let printed = run_ok(&[
        "print-version",
        "--version-index-path",
        target.to_str().unwrap(),
    ]);
    let s = String::from_utf8_lossy(&printed.stdout);
    assert!(
        s.contains("Asset Count:") || s.contains("Hash Identifier:"),
        "the version index must survive the refusal: {s}"
    );
}

/// The same flag on a well-formed `.lvi` target writes both artefacts, to
/// different paths. Guards the refusal above against being over-broad.
#[test]
fn clone_store_writes_a_version_local_store_index_beside_the_version_index() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let (src_store, l1, _l2, _l3, _s1) = three_version_store(tmp.path());
    let tgt_store = tmp.path().join("tgt-store");
    let materialize = tmp.path().join("materialize");
    let target = tmp.path().join("t1.lvi");
    let sources = tmp.path().join("sources.txt");
    let targets = tmp.path().join("targets.txt");
    std::fs::write(&sources, format!("{}\n", l1.to_str().unwrap())).unwrap();
    std::fs::write(&targets, format!("{}\n", target.to_str().unwrap())).unwrap();

    run_ok(&[
        "clone-store",
        "--source-storage-uri",
        src_store.to_str().unwrap(),
        "--target-storage-uri",
        tgt_store.to_str().unwrap(),
        "--target-path",
        materialize.to_str().unwrap(),
        "--source-paths",
        sources.to_str().unwrap(),
        "--target-paths",
        targets.to_str().unwrap(),
        "--create-version-local-store-index",
    ]);

    let lsi = tmp.path().join("t1.lsi");
    assert!(
        lsi.is_file(),
        "the version-local store index must be written"
    );
    assert!(target.is_file(), "the version index must still be there");
    run_ok(&["print-store", "--store-index-path", lsi.to_str().unwrap()]);
    run_ok(&[
        "print-version",
        "--version-index-path",
        target.to_str().unwrap(),
    ]);
}

/// golongtail publishes a second spelling for nine subcommands and a `version`
/// subcommand (`--help`, v0.4.5). This CLI is described as a drop-in
/// replacement, so a pipeline step spelled `longtail stats …` or
/// `longtail validate …` has to run rather than die at clap parse with exit 2 —
/// at the *first* invocation after a switchover, not gradually.
#[test]
fn golongtail_subcommand_aliases_are_accepted() {
    for alias in [
        "validate",
        "printVersionIndex",
        "printStoreIndex",
        "stats",
        "dump",
        "init",
        "createVersionStoreIndex",
        "cloneStore",
        "pruneStore",
    ] {
        let out = run(&[alias, "--help"], None);
        assert!(
            out.status.success(),
            "alias `{alias}` must resolve: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = run_ok(&["version"]);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.trim().starts_with(char::is_numeric),
        "`version` must print a version number, got {s:?}"
    );
}

/// The eight globals golongtail defines that this CLI did not. Each has to parse;
/// what it then does varies by flag and is asserted separately below.
#[test]
fn golongtail_global_flags_are_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);

    for flag in [
        "--show-store-stats",
        "--mem-trace",
        "--mem-trace-detailed",
        "--log-coloring",
        "--log-console-timestamp",
        "--log-to-console",
        "--no-log-to-console",
    ] {
        let out = run(
            &[
                flag,
                "print-version",
                "--version-index-path",
                lvi.to_str().unwrap(),
            ],
            None,
        );
        assert!(
            out.status.success(),
            "global `{flag}` must parse: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let csv = tmp.path().join("trace.csv");
    let out = run(
        &[
            "--mem-trace-csv",
            csv.to_str().unwrap(),
            "print-version",
            "--version-index-path",
            lvi.to_str().unwrap(),
        ],
        None,
    );
    assert!(out.status.success(), "--mem-trace-csv must parse");
}

/// The accepted-and-ignored flags say so. Silently dropping them would leave a
/// pipeline asking for memory traces that never arrive.
#[test]
fn ignored_compatibility_flags_warn() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);

    let out = run_ok(&[
        "--mem-trace",
        "print-version",
        "--version-index-path",
        lvi.to_str().unwrap(),
    ]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("mem-trace") && err.contains("do nothing"),
        "--mem-trace must say it does nothing: {err}"
    );
}

/// `--log-file-path` is golongtail's json log sink, and `--no-log-to-console`
/// leaves it as the only one. A path that cannot be opened fails the run rather
/// than proceeding without the record the operator asked for.
#[test]
fn log_file_path_writes_json_and_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let log = tmp.path().join("log.json");

    // `--mem-trace` is a guaranteed event on an otherwise quiet command.
    let out = run_ok(&[
        "--mem-trace",
        "--no-log-to-console",
        "--log-file-path",
        log.to_str().unwrap(),
        "print-version",
        "--version-index-path",
        lvi.to_str().unwrap(),
    ]);
    assert!(
        out.stderr.is_empty(),
        "--no-log-to-console must silence stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let body = std::fs::read_to_string(&log).expect("log file");
    assert!(
        body.contains("\"level\":\"WARN\"") && body.contains("mem-trace"),
        "the log file must hold json records: {body}"
    );

    let bad = run(
        &[
            "--log-file-path",
            tmp.path()
                .join("no-such-dir")
                .join("x.json")
                .to_str()
                .unwrap(),
            "print-version",
            "--version-index-path",
            lvi.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        !bad.status.success(),
        "an unopenable log file must fail the run"
    );
}

/// `--show-store-stats` is golongtail's spelling of `--show-stats`; both must
/// produce the stats report rather than one of them being silently inert.
#[test]
fn show_store_stats_is_an_alias_for_show_stats() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_v2(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");
    run_upsync(&store, &src, &lvi, &[]);
    let out_a = tmp.path().join("a");
    let out_b = tmp.path().join("b");

    let a = run_ok(&[
        "--show-stats",
        "downsync",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-path",
        lvi.to_str().unwrap(),
        "--target-path",
        out_a.to_str().unwrap(),
    ]);
    let b = run_ok(&[
        "--show-store-stats",
        "downsync",
        "--storage-uri",
        store.to_str().unwrap(),
        "--source-path",
        lvi.to_str().unwrap(),
        "--target-path",
        out_b.to_str().unwrap(),
    ]);
    // The report goes to stderr, alongside the progress bar it follows.
    let (sa, sb) = (
        String::from_utf8_lossy(&a.stderr),
        String::from_utf8_lossy(&b.stderr),
    );
    assert!(
        sa.contains("downsync complete:"),
        "--show-stats printed no report: {sa}"
    );
    assert!(
        sb.contains("downsync complete:"),
        "--show-store-stats printed no report: {sb}"
    );
    assert_eq!(
        sa.lines().count(),
        sb.lines().count(),
        "the two spellings must produce the same report shape:\n{sa}\n---\n{sb}"
    );
}
