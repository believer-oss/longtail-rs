//! Pure `StoreIndex::merge` semantics (no ffi). Byte-identity vs C is proven in
//! the testkit differential lane; these lock down the logic itself.

use longtail_core::{FormatError, StoreIndex};

/// `(chunk_hash, chunk_size)`.
type Chunk = (u64, u32);
/// `(block_hash, tag, chunks)`.
type Block<'a> = (u64, u32, &'a [Chunk]);

/// Build a consistent StoreIndex from block specs, with cumulative offsets.
fn si(hash_id: u32, blocks: &[Block]) -> StoreIndex {
    let mut s = StoreIndex::empty(hash_id);
    for &(bh, tag, chunks) in blocks {
        s.block_hashes.push(bh);
        s.block_tags.push(tag);
        s.block_chunks_offsets.push(s.chunk_hashes.len() as u32);
        s.block_chunk_counts.push(chunks.len() as u32);
        for &(ch, cs) in chunks {
            s.chunk_hashes.push(ch);
            s.chunk_sizes.push(cs);
        }
    }
    s
}

#[test]
fn empty_times_empty_gives_identifier_zero() {
    // The case a Rust implementer naturally gets wrong: both empty → id 0,
    // NOT either input's identifier (C routes through CreateStoreIndexFromBlocks
    // which leaves it 0).
    let a = StoreIndex::empty(0x1111_1111);
    let b = StoreIndex::empty(0x2222_2222);
    let m = a.merge(&b).unwrap();
    assert_eq!(m.hash_identifier, 0);
    assert_eq!(m.block_count(), 0);
    assert_eq!(m.chunk_count(), 0);
    assert_eq!(m.to_bytes(), StoreIndex::empty(0).to_bytes());
    // 16-byte header only: [version][0][0][0].
    assert_eq!(m.to_bytes().len(), 16);
}

#[test]
fn local_nonempty_remote_empty_uses_local_id() {
    let a = si(0xABCD, &[(1, 7, &[(10, 100)])]);
    let b = StoreIndex::empty(0x9999); // empty, id ignored
    let m = a.merge(&b).unwrap();
    assert_eq!(m.hash_identifier, 0xABCD);
    assert_eq!(m.block_hashes, vec![1]);
    assert_eq!(m.chunk_hashes, vec![10]);
}

#[test]
fn local_empty_remote_nonempty_uses_remote_id() {
    let a = StoreIndex::empty(0x9999); // empty, id ignored
    let b = si(0xBEEF, &[(2, 3, &[(20, 200)])]);
    let m = a.merge(&b).unwrap();
    assert_eq!(m.hash_identifier, 0xBEEF);
    assert_eq!(m.block_hashes, vec![2]);
    assert_eq!(m.chunk_hashes, vec![20]);
}

#[test]
fn conflicting_identifiers_when_both_nonempty_is_einval() {
    let a = si(1, &[(1, 0, &[(10, 100)])]);
    let b = si(2, &[(2, 0, &[(20, 200)])]);
    assert_eq!(
        a.merge(&b),
        Err(FormatError::ConflictingHashIdentifier {
            local: 1,
            remote: 2
        })
    );
}

#[test]
fn local_wins_on_block_hash_tie() {
    // Same block hash H in both, different tag/sizes. Local must win entirely.
    const H: u64 = 0xF00D_F00D_F00D_F00D;
    let local = si(5, &[(H, 10, &[(100, 1)])]);
    let remote = si(5, &[(H, 20, &[(999, 9)])]);
    let m = local.merge(&remote).unwrap();
    assert_eq!(m.block_hashes, vec![H]);
    assert_eq!(m.block_tags, vec![10]); // local tag
    assert_eq!(m.chunk_hashes, vec![100]); // local chunk
    assert_eq!(m.chunk_sizes, vec![1]);
    assert_eq!(m.block_count(), 1);
}

#[test]
fn internal_duplicate_in_local_is_deduped_first_wins() {
    // Local lists the same block hash twice with different chunk groups; the
    // first occurrence is kept.
    let local = si(5, &[(7, 1, &[(70, 7)]), (7, 2, &[(71, 8)])]);
    let m = local.merge(&StoreIndex::empty(5)).unwrap();
    assert_eq!(m.block_hashes, vec![7]);
    assert_eq!(m.block_tags, vec![1]);
    assert_eq!(m.chunk_hashes, vec![70]);
    assert_eq!(m.chunk_sizes, vec![7]);
}

#[test]
fn remote_only_blocks_appended_after_local_in_order() {
    let local = si(5, &[(1, 0, &[(10, 1)])]);
    let remote = si(
        5,
        &[(1, 0, &[(10, 1)]), (2, 0, &[(20, 2)]), (3, 0, &[(30, 3)])],
    );
    let m = local.merge(&remote).unwrap();
    // Local's block 1 first, then remote-only 2, 3.
    assert_eq!(m.block_hashes, vec![1, 2, 3]);
    assert_eq!(m.chunk_hashes, vec![10, 20, 30]);
    assert_eq!(m.block_chunks_offsets, vec![0, 1, 2]);
    assert_eq!(m.block_chunk_counts, vec![1, 1, 1]);
}

#[test]
fn merge_canonicalizes_non_cumulative_offsets() {
    // Hand-build a store index whose chunk groups are stored out of block order:
    // block A (1 chunk) at offset 2, block B (2 chunks) at offset 0.
    let src = StoreIndex {
        hash_identifier: 5,
        block_hashes: vec![0xA, 0xB],
        chunk_hashes: vec![0xB0, 0xB1, 0xA0], // B's chunks, then A's
        block_chunks_offsets: vec![2, 0],
        block_chunk_counts: vec![1, 2],
        block_tags: vec![100, 200],
        chunk_sizes: vec![10, 20, 30],
    };
    let m = src.merge(&StoreIndex::empty(5)).unwrap();
    // Output canonicalized: block order A, B; chunks contiguous in block order.
    assert_eq!(m.block_hashes, vec![0xA, 0xB]);
    assert_eq!(m.block_chunks_offsets, vec![0, 1]);
    assert_eq!(m.block_chunk_counts, vec![1, 2]);
    assert_eq!(m.chunk_hashes, vec![0xA0, 0xB0, 0xB1]);
    assert_eq!(m.chunk_sizes, vec![30, 10, 20]);
    assert_eq!(m.block_tags, vec![100, 200]);
}

#[test]
fn merge_rejects_corrupt_chunk_range() {
    // Offset/count run off the end of the chunk arrays; C would read OOB, we err.
    let bad = StoreIndex {
        hash_identifier: 5,
        block_hashes: vec![1],
        chunk_hashes: vec![10],
        block_chunks_offsets: vec![0],
        block_chunk_counts: vec![5], // claims 5 chunks, only 1 present
        block_tags: vec![0],
        chunk_sizes: vec![100],
    };
    assert!(matches!(
        bad.merge(&StoreIndex::empty(5)),
        Err(FormatError::ChunkRangeOutOfBounds { .. })
    ));
}
