//! A version index cannot write, or delete, outside `--target-path`.
//!
//! A `.lvi` names its assets and is read from a store this process did not
//! write, so its names are untrusted. `Path::join` *discards* the root when
//! handed an absolute path, which makes an unguarded join a write primitive —
//! and, because apply deletes before it writes, a delete primitive too.
//!
//! These drive the public `downsync` entry point rather than `fs_util::safe_join`
//! directly (which has its own unit tests): the property under test is that the
//! guard is reached on every path into the filesystem, not that the predicate is
//! correct.
//!
//! Every asset here is zero-length, which keeps these independent of any store
//! contents — apply materialises zero-size assets without fetching a block, so
//! the write attempt happens before any store I/O.

use std::path::Path;

use longtail::{DownsyncOptions, LongtailError, downsync_blocking};
use longtail_core::{Permissions, VersionIndex};

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

/// FNV-1a over the name. The diff keys assets by *path hash*, so two indexes
/// naming different files must hash differently — give them the same hash and
/// `diff_and_retarget` treats them as one asset, producing neither an add nor a
/// delete, and a test asserting "the victim survived" passes without the
/// operation ever being attempted.
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

/// A `..` asset name must not escape `--target-path`.
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

/// An absolute asset name is the form `Path::join` silently honours by
/// discarding the root, so it escapes without any `..` at all.
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

/// The delete arm. `--target-index-path` supplies the *current* index directly,
/// so a hostile entry reaches the deletes-first phase without a prior run having
/// written anything.
///
/// An unlink outside the target root is the worst outcome in this class: it
/// destroys data the tool never owned and cannot undo.
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
    // Required, not optional: if this succeeds, the delete phase skipped the
    // hostile entry and the survival assertion above proves nothing.
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
/// target tree is followed even when it points outside. That is intended.
///
/// longtail never creates symlinks — `scan_folder` skips non-file/non-dir
/// entries and the format has no symlink asset type — so any symlink in the
/// target was placed by whoever controls the target directory, the same operator
/// who supplied `--target-path`. Following it is what they asked for: an install
/// whose asset directory is symlinked to another drive is a legitimate setup,
/// and canonicalise-and-contain would refuse it. An attacker able to plant a
/// symlink there already has write access and does not need longtail to escape.
#[cfg(unix)]
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

// --- allocation sized from data, not from a header field ---------------------

/// A version index claiming an absurd chunk count must not be able to choose an
/// allocation size.
///
/// `from_bytes` validates the asset→chunk map, so this cannot arrive by parsing
/// — but `VersionIndex`'s fields are `pub`, and this is exactly what a
/// hand-built one looks like. Sizing a `Vec` from the header count would request
/// tens of gigabytes; an allocation failure in Rust runs the alloc error handler
/// and aborts, so there would be no error to observe and no test to write.
/// Reaching the assertion at all is part of the result.
#[test]
fn an_absurd_chunk_count_is_refused_rather_than_allocated() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();

    // One asset claiming 0xFFFF_FFFF chunks while the map holds one entry.
    let mut vi = one_asset_index("asset.bin");
    vi.asset_sizes = vec![4];
    vi.asset_chunk_counts = vec![u32::MAX];
    vi.asset_chunk_index_starts = vec![0];
    vi.asset_chunk_indexes = vec![0];
    vi.chunk_hashes = vec![7];
    vi.chunk_sizes = vec![4];
    vi.chunk_tags = vec![0];

    let lvi = tmp.path().join("hostile.lvi");
    std::fs::write(&lvi, vi.to_bytes()).unwrap();

    let mut opts = longtail::CpOptions::new(
        empty_store(tmp.path()),
        lvi.to_string_lossy(),
        "asset.bin",
        tmp.path().join("out.bin").to_string_lossy(),
    );
    opts.remote_worker_count = 1;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let err = rt
        .block_on(longtail::cp(opts))
        .expect_err("an out-of-range chunk count must be refused");
    // Any typed error is acceptable; the point is that one exists to inspect.
    let msg = err.full_chain();
    assert!(!msg.is_empty(), "expected a reportable error, got: {err:?}");
}
