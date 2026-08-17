//! Proptest round-trip fixpoint, no C library needed: for every format, arbitrary valid
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
                    // Constrain the asset→chunk map to be structurally valid:
                    // `from_bytes` rejects an out-of-range map, so the fixpoint
                    // is stated over indexes a real writer could emit.
                    // Everything else stays arbitrary — the codec is verbatim
                    // about values it does not constrain.
                    let c = chunk_hashes.len();
                    let asset_chunk_indexes: Vec<u32> = if c == 0 {
                        // With no chunks the only valid map is the empty one.
                        Vec::new()
                    } else {
                        asset_chunk_indexes
                            .into_iter()
                            .map(|i| i % c as u32)
                            .collect()
                    };
                    let aci = asset_chunk_indexes.len();
                    let (starts, counts): (Vec<u32>, Vec<u32>) = asset_chunk_index_starts
                        .iter()
                        .zip(asset_chunk_counts.iter())
                        .map(|(&s, &n)| {
                            let start = if aci == 0 { 0 } else { s as usize % (aci + 1) };
                            let headroom = aci - start;
                            let count = if headroom == 0 {
                                0
                            } else {
                                n as usize % (headroom + 1)
                            };
                            (start as u32, count as u32)
                        })
                        .unzip();
                    VersionIndex {
                        hash_identifier,
                        target_chunk_size,
                        path_hashes,
                        content_hashes,
                        asset_sizes,
                        asset_chunk_counts: counts,
                        asset_chunk_index_starts: starts,
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

/// A **canonical** store index (offsets cumulative, arrays consistent), built
/// the way every real writer builds one — from a block list. Random block
/// hashes may collide (giving internal duplicates) and the per-block identifiers
/// may differ, so this also reaches `merge_consuming`'s fallback and
/// conflict-error paths, not just the fast path.
fn canonical_si_strategy() -> impl Strategy<Value = StoreIndex> {
    vec(bi_strategy(), 0usize..6)
        .prop_map(|blocks| StoreIndex::from_block_indexes(&blocks).expect("small block set"))
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
    (bi_strategy(), vec(any::<u8>(), 0usize..40)).prop_map(|(mut block_index, payload)| {
        // An uncompressed block's payload *is* its chunks, and `from_bytes`
        // rejects one too short to cover them, so distribute the payload across
        // the chunk sizes rather than leaving them arbitrary (arbitrary `u32`s
        // would claim gigabytes for a 40-byte payload). Compressed blocks keep
        // arbitrary sizes: there the payload is an opaque frame.
        if block_index.tag == 0 {
            let n = block_index.chunk_sizes.len();
            if let (Some(each), Some(remainder)) =
                (payload.len().checked_div(n), payload.len().checked_rem(n))
            {
                for (i, size) in block_index.chunk_sizes.iter_mut().enumerate() {
                    *size = (each + usize::from(i == n - 1) * remainder) as u32;
                }
            }
        }
        StoredBlock {
            block_index,
            payload,
        }
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

    /// `merge_consuming` is byte-for-byte equal to `merge` on canonical inputs
    /// (the fast path — plus the fallback whenever random hashes collide or the
    /// per-block identifiers conflict). Load-bearing: the S3 shard name is the
    /// sha256 of these bytes.
    #[test]
    fn merge_consuming_matches_merge_canonical(a in canonical_si_strategy(), b in canonical_si_strategy()) {
        match (a.merge(&b), a.clone().merge_consuming(&b)) {
            (Ok(m), Ok(c)) => prop_assert_eq!(m.to_bytes(), c.to_bytes()),
            (Err(_), Err(_)) => {}
            (m, c) => prop_assert!(false, "Ok/Err mismatch: merge.is_ok()={} consuming.is_ok()={}", m.is_ok(), c.is_ok()),
        }
    }

    /// Same equivalence over *arbitrary* (often non-canonical / inconsistent)
    /// indexes — this stresses the fallback to the allocating `merge`, including
    /// the error paths, which must match exactly.
    #[test]
    fn merge_consuming_matches_merge_arbitrary(a in si_strategy(), b in si_strategy()) {
        match (a.merge(&b), a.clone().merge_consuming(&b)) {
            (Ok(m), Ok(c)) => prop_assert_eq!(m.to_bytes(), c.to_bytes()),
            (Err(_), Err(_)) => {}
            (m, c) => prop_assert!(false, "Ok/Err mismatch: merge.is_ok()={} consuming.is_ok()={}", m.is_ok(), c.is_ok()),
        }
    }
}

/// Explicit non-canonical name-offsets buffer: a
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
