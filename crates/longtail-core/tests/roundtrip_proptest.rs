//! Proptest round-trip fixpoint (pure lane): for every format, arbitrary valid
//! structs satisfy `from_bytes(to_bytes(x)) == x` and `to_bytes(from_bytes(b))
//! == b`. Strategies deliberately include empty (all counts 0), odd counts
//! (which force `u64` arrays onto 4-byte boundaries), mixed dirs/files, and
//! degenerate names.
//!
//! Case count is capped under miri (see `config`) so
//! `cargo +nightly miri test -p longtail-core` stays tractable.

use longtail_core::{BlockIndex, Permissions, StoreIndex, StoredBlock, VersionIndex};
use proptest::collection::vec;
use proptest::prelude::*;

fn config() -> ProptestConfig {
    ProptestConfig {
        // Keep miri runs short; a healthy sweep otherwise.
        cases: if cfg!(miri) { 8 } else { 256 },
        // No `.proptest-regressions` files: keep the tests hermetic (required
        // under miri's filesystem isolation, tidy everywhere else).
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

fn vi_strategy() -> impl Strategy<Value = VersionIndex> {
    // a = asset count, c = chunk count, aci = c + extra (so ACI >= C always).
    (0usize..5, 0usize..5, 0usize..4).prop_flat_map(|(a, c, extra)| {
        let aci = c + extra;
        (
            (any::<u32>(), any::<u32>()),
            (
                vec(any::<u64>(), a),
                vec(any::<u64>(), a),
                vec(any::<u64>(), a),
                vec(any::<u32>(), a),
                vec(any::<u32>(), a),
            ),
            (
                vec(any::<u32>(), aci),
                vec(any::<u64>(), c),
                vec(any::<u32>(), c),
                vec(any::<u32>(), c),
            ),
            (
                vec(any::<u32>(), a),
                vec(any::<u16>(), a),
                vec(any::<u8>(), 0usize..20),
            ),
        )
            .prop_map(
                |(
                    (hash_identifier, target_chunk_size),
                    (
                        path_hashes,
                        content_hashes,
                        asset_sizes,
                        asset_chunk_counts,
                        asset_chunk_index_starts,
                    ),
                    (asset_chunk_indexes, chunk_hashes, chunk_sizes, chunk_tags),
                    (name_offsets, permissions_raw, name_data),
                )| {
                    VersionIndex {
                        hash_identifier,
                        target_chunk_size,
                        path_hashes,
                        content_hashes,
                        asset_sizes,
                        asset_chunk_counts,
                        asset_chunk_index_starts,
                        asset_chunk_indexes,
                        chunk_hashes,
                        chunk_sizes,
                        chunk_tags,
                        name_offsets,
                        permissions: permissions_raw.into_iter().map(Permissions).collect(),
                        name_data,
                    }
                },
            )
    })
}

fn si_strategy() -> impl Strategy<Value = StoreIndex> {
    // Offsets/counts are arbitrary here — round-trip preserves them verbatim
    // regardless of internal consistency (merge, which needs consistency, is
    // exercised separately).
    (0usize..6, 0usize..8).prop_flat_map(|(b, c)| {
        (
            any::<u32>(),
            vec(any::<u64>(), b),
            vec(any::<u64>(), c),
            vec(any::<u32>(), b),
            vec(any::<u32>(), b),
            vec(any::<u32>(), b),
            vec(any::<u32>(), c),
        )
            .prop_map(
                |(
                    hash_identifier,
                    block_hashes,
                    chunk_hashes,
                    block_chunks_offsets,
                    block_chunk_counts,
                    block_tags,
                    chunk_sizes,
                )| {
                    StoreIndex {
                        hash_identifier,
                        block_hashes,
                        chunk_hashes,
                        block_chunks_offsets,
                        block_chunk_counts,
                        block_tags,
                        chunk_sizes,
                    }
                },
            )
    })
}

fn bi_strategy() -> impl Strategy<Value = BlockIndex> {
    (0usize..8).prop_flat_map(|n| {
        (
            any::<u64>(),
            any::<u32>(),
            any::<u32>(),
            vec(any::<u64>(), n),
            vec(any::<u32>(), n),
        )
            .prop_map(
                |(block_hash, hash_identifier, tag, chunk_hashes, chunk_sizes)| BlockIndex {
                    block_hash,
                    hash_identifier,
                    tag,
                    chunk_hashes,
                    chunk_sizes,
                },
            )
    })
}

fn sb_strategy() -> impl Strategy<Value = StoredBlock> {
    (bi_strategy(), vec(any::<u8>(), 0usize..40)).prop_map(|(block_index, payload)| StoredBlock {
        block_index,
        payload,
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn version_index_fixpoint(x in vi_strategy()) {
        let bytes = x.to_bytes();
        let parsed = VersionIndex::from_bytes(&bytes).expect("valid vi parses");
        prop_assert_eq!(&parsed, &x);
        prop_assert_eq!(parsed.to_bytes(), bytes);
    }

    #[test]
    fn store_index_fixpoint(x in si_strategy()) {
        let bytes = x.to_bytes();
        let parsed = StoreIndex::from_bytes(&bytes).expect("valid si parses");
        prop_assert_eq!(&parsed, &x);
        prop_assert_eq!(parsed.to_bytes(), bytes);
    }

    #[test]
    fn block_index_fixpoint(x in bi_strategy()) {
        let bytes = x.to_bytes();
        let parsed = BlockIndex::from_bytes(&bytes).expect("valid bi parses");
        prop_assert_eq!(&parsed, &x);
        prop_assert_eq!(parsed.to_bytes(), bytes);
    }

    #[test]
    fn stored_block_fixpoint(x in sb_strategy()) {
        let bytes = x.to_bytes();
        let parsed = StoredBlock::from_bytes(&bytes).expect("valid sb parses");
        prop_assert_eq!(&parsed, &x);
        prop_assert_eq!(parsed.to_bytes(), bytes);
    }
}

/// Explicit non-canonical name-offsets buffer (work-order requirement): a
/// VersionIndex whose `name_offsets` are internally consistent but NOT the
/// cumulative-ordered offsets a fresh build would produce must still round-trip
/// byte-identically — proof that we preserve offsets/blob verbatim rather than
/// re-deriving them.
#[test]
fn non_canonical_name_offsets_roundtrip() {
    // Two assets. name_data holds "second\0first\0"; asset 0 points at "first"
    // (offset 7), asset 1 points at "second" (offset 0). Not cumulative.
    let name_data = b"second\0first\0".to_vec();
    let vi = VersionIndex {
        hash_identifier: 0x626c_6b33, // "blk3"
        target_chunk_size: 32768,
        path_hashes: vec![1, 2],
        content_hashes: vec![3, 4],
        asset_sizes: vec![5, 6],
        asset_chunk_counts: vec![0, 0],
        asset_chunk_index_starts: vec![0, 0],
        asset_chunk_indexes: vec![],
        chunk_hashes: vec![],
        chunk_sizes: vec![],
        chunk_tags: vec![],
        name_offsets: vec![7, 0], // asset 0 -> "first", asset 1 -> "second"
        permissions: vec![Permissions(0o644), Permissions(0o755)],
        name_data,
    };
    let bytes = vi.to_bytes();
    let parsed = VersionIndex::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, vi);
    assert_eq!(parsed.to_bytes(), bytes);
    // Accessors decode against the non-canonical offsets correctly.
    assert_eq!(parsed.path(0).unwrap(), "first");
    assert_eq!(parsed.path(1).unwrap(), "second");
}
