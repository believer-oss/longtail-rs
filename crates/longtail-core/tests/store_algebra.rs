//! Pure-logic tests for the store-index algebra:
//! `StoreIndex::from_block_indexes` (CreateStoreIndexFromBlocks) and
//! `StoreIndex::get_existing_store_index` (GetExistingStoreIndex). The byte-level
//! agreement with C is proved by the testkit differential lane.

use longtail_core::{BlockIndex, StoreIndex};

fn block(hash: u64, id: u32, tag: u32, chunks: &[(u64, u32)]) -> BlockIndex {
    BlockIndex {
        block_hash: hash,
        hash_identifier: id,
        tag,
        chunk_hashes: chunks.iter().map(|&(h, _)| h).collect(),
        chunk_sizes: chunks.iter().map(|&(_, s)| s).collect(),
    }
}

#[test]
fn from_blocks_empty_is_empty_identifier_zero() {
    let si = StoreIndex::from_block_indexes(&[]).unwrap();
    assert_eq!(si.block_count(), 0);
    assert_eq!(si.chunk_count(), 0);
    assert_eq!(si.hash_identifier, 0);
    // Serializes to the bare 16-byte header.
    assert_eq!(si.to_bytes().len(), 16);
}

#[test]
fn from_blocks_concatenates_in_order_with_cumulative_offsets() {
    let a = block(100, 7, 1, &[(1, 10), (2, 20)]);
    let b = block(200, 7, 2, &[(3, 30)]);
    let si = StoreIndex::from_block_indexes(&[a, b]).unwrap();
    assert_eq!(si.block_hashes, vec![100, 200]);
    assert_eq!(si.block_tags, vec![1, 2]);
    assert_eq!(si.block_chunk_counts, vec![2, 1]);
    assert_eq!(si.block_chunks_offsets, vec![0, 2]);
    assert_eq!(si.chunk_hashes, vec![1, 2, 3]);
    assert_eq!(si.chunk_sizes, vec![10, 20, 30]);
    assert_eq!(si.hash_identifier, 7);
}

#[test]
fn from_blocks_hash_identifier_is_first_nonzero() {
    // C: hash_identifier = (hash_identifier == 0) ? block.id : hash_identifier.
    let si = StoreIndex::from_block_indexes(&[
        block(1, 0, 0, &[(1, 1)]),
        block(2, 5, 0, &[(2, 1)]),
        block(3, 7, 0, &[(3, 1)]),
    ])
    .unwrap();
    assert_eq!(si.hash_identifier, 5);
}

/// The `remotestore_test.go::TestGetExistingContent` scenario: 6 blocks, a query
/// touching every block → all 6 blocks / 18 chunks.
#[test]
fn get_existing_full_coverage() {
    let mut blocks = Vec::new();
    for seed in [0u64, 10, 20, 30, 40, 50] {
        blocks.push(block(
            seed + 21412151,
            997,
            2,
            &[(seed + 1, 10), (seed + 2, 20), (seed + 3, 30)],
        ));
    }
    let si = StoreIndex::from_block_indexes(&blocks).unwrap();
    let query = [1u64, 2, 11, 13, 21, 22, 32, 33, 41, 43, 51];
    let existing = si.get_existing_store_index(&query, 0);
    assert_eq!(existing.block_count(), 6);
    assert_eq!(existing.chunk_count(), 18);
}

#[test]
fn get_existing_empty_query_is_empty() {
    let si = StoreIndex::from_block_indexes(&[block(1, 7, 0, &[(1, 1)])]).unwrap();
    let existing = si.get_existing_store_index(&[], 0);
    assert_eq!(existing.block_count(), 0);
    assert_eq!(existing.hash_identifier, 0);
}

/// Greedy selection favours higher-usage blocks; a block that adds no new chunk
/// is dropped, and `min_block_usage_percent` filters low-usage blocks.
#[test]
fn get_existing_usage_percent_filtering() {
    // Block A: chunks {1:50, 2:50} → 100 total; Block B: {1:10, 3:90} → 100.
    let a = block(0xAA, 7, 0, &[(1, 50), (2, 50)]);
    let b = block(0xBB, 7, 0, &[(1, 10), (3, 90)]);
    let si = StoreIndex::from_block_indexes(&[a, b]).unwrap();

    // pct 0: A used 50%, B 10%; A claims chunk 1 first, B adds nothing → {A}.
    let e0 = si.get_existing_store_index(&[1], 0);
    assert_eq!(e0.block_hashes, vec![0xAA]);
    assert_eq!(e0.chunk_hashes, vec![1, 2]);

    // pct 40: A (50%) kept, B (10%) filtered → {A}.
    let e40 = si.get_existing_store_index(&[1], 40);
    assert_eq!(e40.block_hashes, vec![0xAA]);

    // pct 60: both below threshold → empty.
    let e60 = si.get_existing_store_index(&[1], 60);
    assert_eq!(e60.block_count(), 0);
}

/// A wild store index (offset/count off the arrays) never panics; the bad block
/// is skipped.
#[test]
fn get_existing_wild_index_no_panic() {
    let mut si = StoreIndex::empty(7);
    si.block_hashes.push(1);
    si.block_tags.push(0);
    si.block_chunk_counts.push(5); // claims 5 chunks…
    si.block_chunks_offsets.push(0);
    si.chunk_hashes.push(1); // …but only 1 present
    si.chunk_sizes.push(10);
    let existing = si.get_existing_store_index(&[1], 0);
    assert_eq!(existing.block_count(), 0); // skipped, no panic
}

// --- block_payload_sizes (download-prefetch permit sizing) ---

#[test]
fn block_payload_sizes_sums_only_requested_blocks() {
    let a = block(100, 7, 1, &[(1, 10), (2, 20)]); // Σ = 30
    let b = block(200, 7, 2, &[(3, 30)]); //           Σ = 30
    let c = block(300, 7, 3, &[(4, 5), (5, 7), (6, 9)]); // Σ = 21
    let si = StoreIndex::from_block_indexes(&[a, b, c]).unwrap();

    // Only the requested hashes are returned; sizes are the per-block Σ.
    let sizes = si.block_payload_sizes(&[100, 300]);
    assert_eq!(sizes.len(), 2);
    assert_eq!(sizes.get(&100), Some(&30));
    assert_eq!(sizes.get(&300), Some(&21));
    assert_eq!(sizes.get(&200), None); // not requested → absent

    // A hash not in the store is simply absent (no entry, no panic).
    let sizes = si.block_payload_sizes(&[200, 999]);
    assert_eq!(sizes.get(&200), Some(&30));
    assert_eq!(sizes.get(&999), None);

    // Equivalent to the old block_index_at-based sum, for every block.
    for b in 0..si.block_count() as usize {
        let bi = si.block_index_at(b).unwrap();
        let want: u64 = bi.chunk_sizes.iter().map(|&s| s as u64).sum();
        assert_eq!(
            si.block_payload_sizes(&[bi.block_hash]).get(&bi.block_hash),
            Some(&want)
        );
    }
}

#[test]
fn block_payload_sizes_skips_malformed_block() {
    // A block whose chunk range runs off the arrays contributes no entry
    // (mirrors block_index_at's bounds check) rather than panicking.
    let mut si = StoreIndex::from_block_indexes(&[block(100, 7, 1, &[(1, 10)])]).unwrap();
    si.block_chunk_counts[0] = 5; // claim 5 chunks; only 1 present
    let sizes = si.block_payload_sizes(&[100]);
    assert_eq!(sizes.get(&100), None);
}

// --- merge_consuming (in-place union; byte-identical to merge) ---

// Assert both APIs agree byte-for-byte on the Ok path.
fn assert_merge_eq(a: &StoreIndex, b: &StoreIndex) {
    let via_merge = a.merge(b).expect("merge ok");
    let via_consuming = a.clone().merge_consuming(b).expect("merge_consuming ok");
    assert_eq!(
        via_merge.to_bytes(),
        via_consuming.to_bytes(),
        "merge_consuming bytes must equal merge bytes"
    );
}

#[test]
fn merge_consuming_empty_sides() {
    let empty = StoreIndex::from_block_indexes(&[]).unwrap();
    let x = StoreIndex::from_block_indexes(&[block(100, 7, 1, &[(1, 10), (2, 20)])]).unwrap();
    assert_merge_eq(&empty, &empty); // both empty
    assert_merge_eq(&x, &empty); // remote empty (canonicalizes local)
    assert_merge_eq(&empty, &x); // local empty (builds remote into empty self)
}

#[test]
fn merge_consuming_disjoint_and_overlap() {
    let a = StoreIndex::from_block_indexes(&[
        block(100, 7, 1, &[(1, 10)]),
        block(200, 7, 2, &[(2, 20), (3, 30)]),
    ])
    .unwrap();
    // Disjoint remote → both blocks appended after local's.
    let b = StoreIndex::from_block_indexes(&[block(300, 7, 3, &[(4, 40)])]).unwrap();
    assert_merge_eq(&a, &b);
    // Overlapping block hash (200) → local wins the tie, remote's 200 skipped.
    let c = StoreIndex::from_block_indexes(&[
        block(200, 7, 9, &[(9, 99)]), // same hash as a's block 200, different content
        block(400, 7, 4, &[(5, 50)]),
    ])
    .unwrap();
    assert_merge_eq(&a, &c);
}

#[test]
fn merge_consuming_fallback_internal_dup_local() {
    // `local` with a duplicate block hash is NOT canonical-Pass-1-reproducible
    // (Pass 1 dedups it), so merge_consuming must fall back — and still match.
    let a = StoreIndex::from_block_indexes(&[
        block(100, 7, 1, &[(1, 10)]),
        block(100, 7, 2, &[(2, 20)]), // duplicate block hash
    ])
    .unwrap();
    let b = StoreIndex::from_block_indexes(&[block(300, 7, 3, &[(3, 30)])]).unwrap();
    assert_merge_eq(&a, &b);
}

#[test]
fn merge_consuming_fallback_non_canonical_local() {
    // Hand-built local with a non-cumulative offset: merge canonicalizes it,
    // so reusing self verbatim would diverge — merge_consuming must fall back.
    let mut a = StoreIndex::from_block_indexes(&[
        block(100, 7, 1, &[(1, 10)]),
        block(200, 7, 2, &[(2, 20)]),
    ])
    .unwrap();
    a.block_chunks_offsets[1] = 0; // wrong offset (should be 1) → non-canonical
    let b = StoreIndex::from_block_indexes(&[block(300, 7, 3, &[(3, 30)])]).unwrap();
    assert_merge_eq(&a, &b);
}

#[test]
fn merge_consuming_conflicting_identifier_errors_like_merge() {
    let a = StoreIndex::from_block_indexes(&[block(100, 7, 1, &[(1, 10)])]).unwrap();
    let b = StoreIndex::from_block_indexes(&[block(200, 9, 2, &[(2, 20)])]).unwrap();
    // Both non-empty with differing hash identifiers → both must Err.
    assert!(a.merge(&b).is_err());
    assert!(a.clone().merge_consuming(&b).is_err());
}
