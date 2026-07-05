//! Stage 5 / Stage 7 checklist: golongtail CLI black-box tests ported from
//! `commands/*_test.go`.
//!
//! Download-path commands (downsync/get/ls/validate-version/print-version) are
//! implemented here (Stage 5), driving the built `longtail` binary. The Go
//! originals upsync first; we satisfy them from committed fixtures instead
//! (`fixtures/stores/default` carries the v1/v2/v3 chain + zoo over one store;
//! get-config JSONs are synthesized at test time). Everything on the
//! upload/maintenance path stays `#[ignore]`d for Stage 7.

#![cfg(unix)]
#![cfg_attr(miri, ignore)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_longtail")
}

fn pin_umask() {
    // SAFETY: single libc call; children inherit the umask so subprocess dir
    // creation matches the umask-022 fixture-generation environment.
    unsafe {
        libc::umask(0o022);
    }
}

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
// Download path — Stage 5
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
        .compare(&manifest("zoo.json"), false)
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
        .compare(&manifest("chain-v1.json"), false)
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
        .compare(&manifest("chain-v2.json"), false)
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
        .compare(&manifest("chain-v3.json"), false)
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
        .compare(&manifest("zoo.json"), false)
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
        .compare(&manifest("chain-v2.json"), false)
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
        .compare(&manifest("chain-v1.json"), false)
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
        .compare(&manifest("chain-v3.json"), false)
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
        .compare(&capture(&reference), false)
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
        .compare(&manifest("chain-v1.json"), false)
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
        .compare(&manifest("chain-v2.json"), false)
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
        .compare(&manifest("chain-v3.json"), false)
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
// Upload / maintenance path — Stage 7 (unchanged)
// =========================================================================

/// Source: cmd_upsync_test.go::TestUpsync.
#[test]
#[ignore = "Stage 7"]
fn upsync() {
    todo!()
}

/// Source: cmd_upsync_test.go::TestUpsyncWithLSI.
#[test]
#[ignore = "Stage 7"]
fn upsync_with_lsi() {
    todo!()
}

/// Source: cmd_upsync_test.go::TestUpsyncWithBrokenLSI.
#[test]
#[ignore = "Stage 7"]
fn upsync_with_broken_lsi() {
    todo!()
}

/// Source: cmd_get_test.go / cmd_put — `put`.
#[test]
#[ignore = "Stage 7"]
fn put() {
    todo!()
}

/// Source: cmd_initremotestore_test.go::TestInitRemoteStore.
#[test]
#[ignore = "Stage 7"]
fn init_remote_store() {
    todo!()
}

/// Source: cmd_createversionstoreindex_test.go::TestCreateVersionStoreIndex.
#[test]
#[ignore = "Stage 7"]
fn create_version_store_index() {
    todo!()
}

/// Source: cmd_prunestore_test.go::TestPrune.
#[test]
#[ignore = "Stage 7"]
fn prune_store() {
    todo!()
}

/// Source: cmd_prunestore_index_test.go::TestPruneIndex.
#[test]
#[ignore = "Stage 7"]
fn prune_store_index() {
    todo!()
}

/// Source: cmd_prunestore_block_test.go::TestPruneStoreBlocks.
#[test]
#[ignore = "Stage 7"]
fn prune_store_blocks() {
    todo!()
}

/// Source: cmd_clonestore_test.go::TestCloneStore.
#[test]
#[ignore = "Stage 7"]
fn clone_store() {
    todo!()
}

/// Source: cmd_printstore_test.go::TestPrintStoreIndex.
#[test]
#[ignore = "Stage 7"]
fn print_store_index() {
    todo!()
}

/// Source: cmd_printversionusage_test.go::TestPrintVersionUsage.
#[test]
#[ignore = "Stage 7"]
fn print_version_usage() {
    todo!()
}

/// Source: cmd_dumpversionassets_test.go::TestDumpVersionAssets.
#[test]
#[ignore = "Stage 7"]
fn dump_version_assets() {
    todo!()
}

/// Source: cmd_cp_test.go::TestCp.
#[test]
#[ignore = "Stage 7"]
fn cp() {
    todo!()
}

/// Source: cmd_pack_test.go::TestPack.
#[test]
#[ignore = "Stage 7"]
fn pack() {
    todo!()
}

/// Source: cmd_unpack_test.go::TestUnpack.
#[test]
#[ignore = "Stage 7"]
fn unpack() {
    todo!()
}
