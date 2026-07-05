//! Stage 5 core: `create_version_index` assembly + `CreateVersionDiff` +
//! `GetRequiredChunkHashes` + `MergeVersionIndex` + `ValidateStore` over
//! synthetic in-memory inputs. Miri-friendly (no fs, no fixtures, no native lib).

use longtail_core::VersionIndex;
use longtail_core::hash::Hash;
use longtail_core::{
    Blake3, FileEntry, FileInfos, Permissions, StoreIndex, ValidateError, create_version_diff,
    create_version_index, get_required_chunk_hashes, merge_version_index, validate_store,
};

/// Build a version index from `(path, bytes, is_dir, mode)` entries with a small
/// target chunk size so multi-chunk assets are exercised.
fn build(entries: &[(&str, &[u8], bool, u16)], target: u32) -> VersionIndex {
    let file_entries: Vec<FileEntry> = entries
        .iter()
        .map(|(p, b, is_dir, mode)| FileEntry {
            relative_path: (*p).to_string(),
            size: if *is_dir { 0 } else { b.len() as u64 },
            permissions: Permissions(*mode),
            is_dir: *is_dir,
        })
        .collect();
    let fi = FileInfos::from_scanned_entries(file_entries.clone());
    // Align contents with the sorted FileInfos order.
    let mut sorted = file_entries;
    sorted.sort_by(|a, b| a.relative_path.as_bytes().cmp(b.relative_path.as_bytes()));
    let contents: Vec<Vec<u8>> = sorted
        .iter()
        .map(|e| {
            if e.is_dir {
                Vec::new()
            } else {
                entries
                    .iter()
                    .find(|(p, ..)| *p == e.relative_path)
                    .map(|(_, b, ..)| b.to_vec())
                    .unwrap()
            }
        })
        .collect();
    create_version_index(&fi, &contents, &Blake3, target, None).unwrap()
}

#[test]
fn assembly_dedup_and_content_hash() {
    // Two files with identical content → chunk dedup (shared chunk), distinct
    // content hashes only if paths differ but content same → same content hash.
    let vi = build(
        &[
            ("a.bin", b"hello world hello world", false, 0o644),
            ("b.bin", b"hello world hello world", false, 0o644),
        ],
        1024,
    );
    assert_eq!(vi.asset_count(), 2);
    // Same content ⇒ identical content hashes.
    assert_eq!(vi.content_hashes[0], vi.content_hashes[1]);
    // Small files: one chunk each, deduped to a single unique chunk.
    assert_eq!(vi.chunk_count(), 1, "identical single chunk deduped");
    assert_eq!(vi.asset_chunk_index_count(), 2, "two asset->chunk slots");
    // Path hashes differ.
    assert_ne!(vi.path_hashes[0], vi.path_hashes[1]);
    // Content hash of a single-chunk asset = hash(chunk_hash_le_bytes).
    let ch = vi.chunk_hashes[0];
    assert_eq!(vi.content_hashes[0], Blake3.hash(&ch.to_le_bytes()));
}

#[test]
fn empty_file_and_dir_zero_chunks() {
    let vi = build(
        &[
            ("dir", b"", true, 0o755),
            ("empty", b"", false, 0o644),
            ("data.bin", b"some bytes here", false, 0o644),
        ],
        1024,
    );
    assert_eq!(vi.asset_count(), 3);
    // dir + empty file have zero chunks; content hash == hash of empty buffer.
    let empty_hash = Blake3.hash(&[]);
    // Directory "dir" sorts first, "data.bin" ... actually byte sort: "data.bin"
    // < "dir" < "empty". Find by path.
    for i in 0..3 {
        let p = vi.path(i).unwrap();
        if p == "dir/" || p == "empty" {
            assert_eq!(vi.asset_chunk_counts[i], 0);
            assert_eq!(vi.content_hashes[i], empty_hash, "zero-chunk content hash");
        }
    }
}

#[test]
fn diff_added_removed_modified_permissions() {
    // from (current) has a.txt, b.txt (0644), gone.txt.
    let from = build(
        &[
            ("a.txt", b"aaaa", false, 0o644),
            ("b.txt", b"bbbb", false, 0o644),
            ("gone.txt", b"x", false, 0o644),
        ],
        1024,
    );
    // to (desired): a.txt same, b.txt content changed + perm 0755, new.txt added.
    let to = build(
        &[
            ("a.txt", b"aaaa", false, 0o644),
            ("b.txt", b"BBBB-changed", false, 0o755),
            ("new.txt", b"new", false, 0o644),
        ],
        1024,
    );
    let diff = create_version_diff(&from, &to);
    // gone.txt removed.
    assert_eq!(diff.source_removed_asset_indexes.len(), 1);
    let rem_i = diff.source_removed_asset_indexes[0] as usize;
    assert_eq!(from.path(rem_i).unwrap(), "gone.txt");
    // new.txt added.
    assert_eq!(diff.target_added_asset_indexes.len(), 1);
    let add_i = diff.target_added_asset_indexes[0] as usize;
    assert_eq!(to.path(add_i).unwrap(), "new.txt");
    // b.txt content-modified AND permission-modified; a.txt neither.
    assert_eq!(diff.target_content_modified_asset_indexes.len(), 1);
    let cm_i = diff.target_content_modified_asset_indexes[0] as usize;
    assert_eq!(to.path(cm_i).unwrap(), "b.txt");
    assert_eq!(diff.target_permissions_modified_asset_indexes.len(), 1);
}

#[test]
fn diff_sort_orders() {
    // Removed should be long-to-short; added short-to-long.
    let from = build(
        &[
            ("d", b"", true, 0o755),
            ("d/deep", b"", true, 0o755),
            ("d/deep/f.txt", b"x", false, 0o644),
        ],
        1024,
    );
    let to = build(&[("z.txt", b"z", false, 0o644)], 1024);
    let diff = create_version_diff(&from, &to);
    // All of `from` removed; deepest path first.
    let removed_paths: Vec<String> = diff
        .source_removed_asset_indexes
        .iter()
        .map(|&i| from.path(i as usize).unwrap().to_string())
        .collect();
    assert_eq!(removed_paths[0], "d/deep/f.txt", "deepest removed first");
    assert!(
        removed_paths[0].len() >= removed_paths[removed_paths.len() - 1].len(),
        "removed sorted long-to-short"
    );
}

#[test]
fn required_chunks_added_then_modified() {
    let from = build(&[("keep.txt", b"keep", false, 0o644)], 1024);
    let to = build(
        &[
            ("keep.txt", b"keep", false, 0o644),
            ("added.txt", b"added content", false, 0o644),
        ],
        1024,
    );
    let diff = create_version_diff(&from, &to);
    let required = get_required_chunk_hashes(&to, &diff);
    // Only the added asset's chunk(s) are required (keep.txt unchanged).
    let add_i = diff.target_added_asset_indexes[0] as usize;
    let start = to.asset_chunk_index_starts[add_i] as usize;
    let cidx = to.asset_chunk_indexes[start] as usize;
    assert!(required.contains(&to.chunk_hashes[cidx]));
    assert_eq!(required.len(), to.asset_chunk_counts[add_i] as usize);
}

#[test]
fn merge_overlay_wins() {
    let base = build(
        &[
            ("shared.txt", b"base-content", false, 0o644),
            ("base-only.txt", b"only in base", false, 0o644),
        ],
        1024,
    );
    let overlay = build(
        &[
            ("shared.txt", b"OVERLAY-content", false, 0o755),
            ("overlay-only.txt", b"only in overlay", false, 0o644),
        ],
        1024,
    );
    let merged = merge_version_index(&base, &overlay).unwrap();
    assert_eq!(merged.asset_count(), 3, "union of paths");
    // shared.txt takes the overlay's content hash + permissions.
    for i in 0..merged.asset_count() as usize {
        if merged.path(i).unwrap() == "shared.txt" {
            let oi = (0..overlay.asset_count() as usize)
                .find(|&j| overlay.path(j).unwrap() == "shared.txt")
                .unwrap();
            assert_eq!(merged.content_hashes[i], overlay.content_hashes[oi]);
            assert_eq!(merged.permissions[i], overlay.permissions[oi]);
        }
    }
}

#[test]
fn merge_mismatched_target_chunk_size_errors() {
    let a = build(&[("x", b"x", false, 0o644)], 1024);
    let b = build(&[("y", b"y", false, 0o644)], 2048);
    assert!(merge_version_index(&a, &b).is_err());
}

#[test]
fn validate_store_ok_missing_and_size_precedence() {
    let vi = build(&[("f.bin", b"content bytes for f", false, 0o644)], 1024);
    // A store index covering exactly the version's chunks with correct sizes.
    let mut block_hashes = Vec::new();
    let mut chunk_hashes = Vec::new();
    let mut chunk_sizes = Vec::new();
    let mut offsets = Vec::new();
    let mut counts = Vec::new();
    let mut tags = Vec::new();
    offsets.push(0u32);
    counts.push(vi.chunk_count());
    tags.push(0u32);
    block_hashes.push(0x1234u64);
    for i in 0..vi.chunk_count() as usize {
        chunk_hashes.push(vi.chunk_hashes[i]);
        chunk_sizes.push(vi.chunk_sizes[i]);
    }
    let good = StoreIndex {
        hash_identifier: vi.hash_identifier,
        block_hashes: block_hashes.clone(),
        chunk_hashes: chunk_hashes.clone(),
        block_chunks_offsets: offsets.clone(),
        block_chunk_counts: counts.clone(),
        block_tags: tags.clone(),
        chunk_sizes: chunk_sizes.clone(),
    };
    assert!(validate_store(&good, &vi).is_ok());

    // Missing chunk → ENOENT-equivalent.
    let mut missing = good.clone();
    missing.chunk_hashes[0] = missing.chunk_hashes[0].wrapping_add(1);
    assert!(matches!(
        validate_store(&missing, &vi),
        Err(ValidateError::MissingChunks { .. })
    ));

    // Size mismatch is a property of the version index (Σ chunk sizes ≠ asset
    // size). Corrupt the asset size AND make a chunk missing → EINVAL wins.
    let mut bad_vi = vi.clone();
    bad_vi.asset_sizes[0] = bad_vi.asset_sizes[0].wrapping_add(999);
    match validate_store(&missing, &bad_vi) {
        Err(ValidateError::SizeMismatch { .. }) => {}
        other => panic!("expected size mismatch (EINVAL precedence), got {other:?}"),
    }
}
