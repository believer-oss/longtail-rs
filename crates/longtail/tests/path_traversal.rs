//! Regression gate for the `fs_util::safe_join` containment guard (review
//! findings SEC-01 / SEC-02, `docs/review/06-security.md`).
//!
//! A `.lvi` names its assets, and the launcher downloads indexes it did not
//! write. `Path::join` *discards* the root when handed an absolute path, so
//! before the guard a version index could make `downsync` create, truncate,
//! write, chmod, and — via the deletes-first phase — unlink files anywhere the
//! process could reach.
//!
//! These tests drive the public `downsync` entry point rather than `safe_join`
//! directly (that has unit coverage in `fs_util`), because the property under
//! test is that the guard is *on the path* and cannot be bypassed. The unit
//! tests prove the predicate; these prove the plumbing.
//!
//! Every asset here is zero-length, which keeps the test independent of any
//! store contents: apply materialises zero-size assets without fetching a
//! block, so the write attempt happens before any store I/O.

#![cfg(unix)]

use std::path::Path;

use longtail::{DownsyncOptions, LongtailError, downsync_blocking};
use longtail_core::{Permissions, VersionIndex};

fn pin_umask() {
    unsafe {
        libc::umask(0o022);
    }
}

/// FNV-1a over the name. The diff keys assets by *path hash*, so two indexes
/// naming different files must hash differently or `diff_and_retarget` treats
/// them as the same asset and produces neither an add nor a delete. (The first
/// draft of this test used a constant here and passed vacuously: nothing was
/// deleted, so the victim survived for the wrong reason.)
fn path_hash(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A single-asset, zero-length version index whose one asset is `name`.
///
/// Hand-built rather than produced by `create_version_index`, because the whole
/// point is a name that a legitimate folder scan can never generate.
fn one_asset_index(name: &str) -> VersionIndex {
    let mut name_data = name.as_bytes().to_vec();
    name_data.push(0);
    VersionIndex {
        // Not re-exported at the crate root, unlike the `Blake3` type itself
        // (a small instance of review finding API-01's surface inconsistency).
        hash_identifier: longtail_core::hash::BLAKE3_ID,
        target_chunk_size: 32768,
        path_hashes: vec![path_hash(name)],
        content_hashes: vec![path_hash(name)],
        asset_sizes: vec![0],
        asset_chunk_counts: vec![0],
        asset_chunk_index_starts: vec![0],
        asset_chunk_indexes: vec![],
        chunk_hashes: vec![],
        chunk_sizes: vec![],
        chunk_tags: vec![],
        name_offsets: vec![0],
        permissions: vec![Permissions(0o644)],
        name_data,
    }
}

fn write_lvi(dir: &Path, file: &str, name: &str) -> String {
    let p = dir.join(file);
    std::fs::write(&p, one_asset_index(name).to_bytes()).unwrap();
    p.to_string_lossy().into_owned()
}

/// `storage_uri` for an empty fs store: no `store.lsi`, so an empty index.
fn empty_store(dir: &Path) -> String {
    let s = dir.join("store");
    std::fs::create_dir_all(&s).unwrap();
    s.to_string_lossy().into_owned()
}

fn assert_unsafe_path(err: LongtailError, ctx: &str) {
    assert!(
        matches!(err, LongtailError::UnsafeAssetPath { .. }),
        "{ctx}: expected UnsafeAssetPath, got {err:?} ({})",
        err.full_chain()
    );
}

/// SEC-01, write arm: a `..` asset name must not escape `--target-path`.
#[test]
fn relative_escape_is_refused_and_writes_nothing_outside() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let inner = tmp.path().join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let escaped = tmp.path().join("escaped.txt");

    let src = write_lvi(tmp.path(), "v.lvi", "../escaped.txt");
    let mut opts =
        DownsyncOptions::new(vec![src], empty_store(tmp.path()), inner.to_string_lossy());
    opts.cache_target_index = false;

    let err = downsync_blocking(opts).expect_err("a `..` asset name must be refused");
    assert_unsafe_path(err, "relative escape");
    assert!(
        !escaped.exists(),
        "the guard failed: {escaped:?} was created outside the target root"
    );
}

/// SEC-01, absolute arm: this is the form `Path::join` silently honours by
/// discarding the root entirely, so it escapes without any `..` at all.
#[test]
fn absolute_asset_path_is_refused() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let escaped = tmp.path().join("abs-escaped.txt");

    let src = write_lvi(tmp.path(), "abs.lvi", escaped.to_str().unwrap());
    let mut opts =
        DownsyncOptions::new(vec![src], empty_store(tmp.path()), target.to_string_lossy());
    opts.cache_target_index = false;

    let err = downsync_blocking(opts).expect_err("an absolute asset name must be refused");
    assert_unsafe_path(err, "absolute escape");
    assert!(
        !escaped.exists(),
        "the guard failed: {escaped:?} was created"
    );
}

/// SEC-02, delete arm. `--target-index-path` supplies the *current* index
/// directly, so a hostile entry there reaches the deletes-first phase without
/// needing a prior run to have written anything — the precondition-free variant
/// of the cached-index chain.
///
/// The victim file must survive: an unlink outside the target root is the worst
/// outcome in this class, because it destroys data the tool never owned.
#[test]
fn hostile_current_index_cannot_delete_outside_the_target() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();

    let victim = tmp.path().join("victim.txt");
    std::fs::write(&victim, b"must survive").unwrap();

    // Current index claims `../victim.txt` exists; the source index does not
    // list it, so apply's deletes-first phase wants it gone.
    let current = write_lvi(tmp.path(), "current.lvi", "../victim.txt");
    let source = write_lvi(tmp.path(), "source.lvi", "kept.txt");

    let mut opts = DownsyncOptions::new(
        vec![source],
        empty_store(tmp.path()),
        target.to_string_lossy(),
    );
    opts.cache_target_index = false;
    opts.target_index_path = Some(current);
    opts.scan_target = false;

    let result = downsync_blocking(opts);
    assert!(
        victim.exists(),
        "the guard failed: {victim:?} was deleted from outside the target root"
    );
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        b"must survive",
        "victim file was modified"
    );
    // Required, not optional: if this ever succeeds, the delete phase silently
    // skipped the hostile entry and the test proves nothing about the guard.
    let err = result.expect_err("a hostile current index must be refused, not ignored");
    assert_unsafe_path(err, "hostile current index");
}

/// The guard must not be over-broad: an ordinary nested asset still downloads.
/// Without this, a regression that rejects everything would look like a pass.
#[test]
fn ordinary_nested_asset_still_materialises() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();

    let src = write_lvi(tmp.path(), "ok.lvi", "sub/dir/file.txt");
    let mut opts =
        DownsyncOptions::new(vec![src], empty_store(tmp.path()), target.to_string_lossy());
    opts.cache_target_index = false;

    downsync_blocking(opts).expect("a legitimate nested asset must still be written");
    assert!(
        target.join("sub/dir/file.txt").exists(),
        "legitimate asset was not created — the guard is over-broad"
    );
}

/// `safe_join` is deliberately **lexical**, so a pre-existing symlink inside the
/// target tree is followed even when it points outside. This test pins that as
/// intended behaviour, not an oversight.
///
/// The reasoning, which belongs with the trust boundary in `docs/rust-port.md`:
/// longtail never creates symlinks (`scan_folder` skips non-file/non-dir
/// entries and the format has no symlink asset type), so any symlink in the
/// target tree was placed there by whoever controls the target directory — the
/// same operator who supplied `--target-path`. Following it is what they asked
/// for; a game install whose asset directory is symlinked to another drive is a
/// legitimate and common setup. An attacker who can plant a symlink there
/// already has write access to the target and does not need longtail to escape.
///
/// The cost of the alternative is real: canonicalise-and-contain would refuse
/// that drive-spanning install. If this is ever revisited, note the delete arm
/// is the sharper edge (unlinking through a symlink into a user's other drive),
/// and that a canonicalising check needs `dunce` on Windows, where
/// `fs::canonicalize` returns a `\\?\` verbatim path that will not compare
/// equal to a normal root.
#[test]
fn symlink_inside_target_is_followed_by_design() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();

    // A pre-existing symlink inside the target tree pointing out of it.
    std::os::unix::fs::symlink(&outside, target.join("link")).unwrap();

    let src = write_lvi(tmp.path(), "sym.lvi", "link/through-link.txt");
    let mut opts =
        DownsyncOptions::new(vec![src], empty_store(tmp.path()), target.to_string_lossy());
    opts.cache_target_index = false;

    downsync_blocking(opts).expect("an operator-placed symlink must still be usable");
    assert!(
        outside.join("through-link.txt").exists(),
        "a symlink inside the target stopped being followed — if that was deliberate, \
         update this test and the trust boundary in docs/rust-port.md together"
    );
}
