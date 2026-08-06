//! Malformed-input tests (pure lane): wrong version, truncation at every field
//! boundary, trailing bytes, and overflow-implying counts all return `Err` and
//! never panic. Plus a fuzz-ish proptest that mutates valid buffers and only
//! asserts the parser does not panic.

use longtail_core::{BlockIndex, FormatError, Permissions, StoreIndex, StoredBlock, VersionIndex};
use proptest::collection::vec;
use proptest::prelude::*;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: if cfg!(miri) { 8 } else { 256 },
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// A small but non-trivial valid VersionIndex (1 asset, 1 chunk, ACI 1).
fn sample_vi() -> VersionIndex {
    VersionIndex {
        hash_identifier: 0x626c_6b33,
        target_chunk_size: 32768,
        path_hashes: vec![0x1111_2222_3333_4444],
        content_hashes: vec![0x5555_6666_7777_8888],
        asset_sizes: vec![4],
        asset_chunk_counts: vec![1],
        asset_chunk_index_starts: vec![0],
        asset_chunk_indexes: vec![0],
        chunk_hashes: vec![0x9999_aaaa_bbbb_cccc],
        chunk_sizes: vec![4],
        chunk_tags: vec![0x7a74_6432],
        name_offsets: vec![0],
        permissions: vec![Permissions(0o644)],
        name_data: b"testfile\0".to_vec(),
    }
}

fn sample_si() -> StoreIndex {
    StoreIndex {
        hash_identifier: 0x626c_6b33,
        block_hashes: vec![0xaaaa_bbbb_cccc_dddd],
        chunk_hashes: vec![0x1111_2222_3333_4444, 0x5555_6666_7777_8888],
        block_chunks_offsets: vec![0],
        block_chunk_counts: vec![2],
        block_tags: vec![0],
        chunk_sizes: vec![10, 20],
    }
}

fn sample_bi() -> BlockIndex {
    BlockIndex {
        block_hash: 0xdead_beef_0000_1111,
        hash_identifier: 0x626c_6b33,
        tag: 0,
        chunk_hashes: vec![1, 2, 3],
        chunk_sizes: vec![4, 5, 6],
    }
}

// --- wrong version ---------------------------------------------------------

#[test]
fn version_index_wrong_version() {
    let mut bytes = sample_vi().to_bytes();
    bytes[0] = 0x03; // version 0x00000003
    assert_eq!(
        VersionIndex::from_bytes(&bytes),
        Err(FormatError::UnsupportedVersion {
            found: 0x0000_0003,
            expected: 0x0000_0002
        })
    );
}

#[test]
fn store_index_wrong_version() {
    let mut bytes = sample_si().to_bytes();
    bytes[3] = 0x02; // version high byte -> 0x02000000
    assert!(matches!(
        StoreIndex::from_bytes(&bytes),
        Err(FormatError::UnsupportedVersion { .. })
    ));
}

// --- truncation at every prefix length -------------------------------------

#[test]
fn version_index_truncation_never_panics() {
    let vi = sample_vi();
    let full = vi.to_bytes();
    // The fixed-array region ends where name_data begins; any prefix into it is
    // truncated, while a prefix at or past it is valid (the remainder is a
    // shorter name_data, which is legitimate — the tail IS name_data).
    let fixed = full.len() - vi.name_data.len();
    for len in 0..fixed {
        assert!(
            VersionIndex::from_bytes(&full[..len]).is_err(),
            "prefix len {len} (< fixed {fixed}) unexpectedly parsed"
        );
    }
    for len in fixed..=full.len() {
        assert!(
            VersionIndex::from_bytes(&full[..len]).is_ok(),
            "prefix len {len} (>= fixed {fixed}) unexpectedly rejected"
        );
    }
}

#[test]
fn store_index_truncation_never_panics() {
    let full = sample_si().to_bytes();
    for len in 0..full.len() {
        assert!(
            StoreIndex::from_bytes(&full[..len]).is_err(),
            "prefix len {len} unexpectedly parsed"
        );
    }
    assert!(StoreIndex::from_bytes(&full).is_ok());
}

#[test]
fn block_index_truncation_never_panics() {
    let full = sample_bi().to_bytes();
    for len in 0..full.len() {
        assert!(
            BlockIndex::from_bytes(&full[..len]).is_err(),
            "prefix len {len} unexpectedly parsed"
        );
    }
    assert!(BlockIndex::from_bytes(&full).is_ok());
}

// --- trailing bytes (StoreIndex / standalone BlockIndex are strict) ---------

#[test]
fn store_index_rejects_trailing_bytes() {
    let mut bytes = sample_si().to_bytes();
    let expected = bytes.len();
    bytes.push(0xff);
    assert_eq!(
        StoreIndex::from_bytes(&bytes),
        Err(FormatError::TrailingBytes {
            expected,
            actual: expected + 1
        })
    );
}

#[test]
fn block_index_rejects_trailing_bytes() {
    let mut bytes = sample_bi().to_bytes();
    let expected = bytes.len();
    bytes.push(0xff);
    assert_eq!(
        BlockIndex::from_bytes(&bytes),
        Err(FormatError::TrailingBytes {
            expected,
            actual: expected + 1
        })
    );
}

/// VersionIndex tolerates a tail (it IS name_data); StoredBlock tolerates a tail
/// (it IS the payload). Neither raises TrailingBytes.
#[test]
fn version_index_and_stored_block_absorb_tail() {
    // Empty-name_data VersionIndex (A=1) parses, tail becomes name_data.
    let mut vi = sample_vi();
    vi.name_data.clear();
    let bytes = vi.to_bytes();
    let parsed = VersionIndex::from_bytes(&bytes).unwrap();
    assert!(parsed.name_data.is_empty());

    // StoredBlock with a payload round-trips; the payload is the tail.
    // `sample_bi()` is tag 0 with chunk_sizes [4,5,6], so the payload must be at
    // least 15 bytes or `from_bytes` rejects it as truncated (see
    // `stored_block_rejects_payload_shorter_than_chunk_sizes`).
    let sb = StoredBlock {
        block_index: sample_bi(),
        payload: (0u8..15).collect(),
    };
    let bytes = sb.to_bytes();
    assert_eq!(StoredBlock::from_bytes(&bytes).unwrap(), sb);
}

// --- ACI < C strictness (deliberate, beyond C) -----------------------------

#[test]
fn version_index_rejects_aci_below_chunk_count() {
    // Hand-build a header with C=2, ACI=1 (invalid). Body bytes need not exist;
    // the check happens before array reads.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0000_0002u32.to_le_bytes()); // version
    bytes.extend_from_slice(&0u32.to_le_bytes()); // hash id
    bytes.extend_from_slice(&32768u32.to_le_bytes()); // target chunk size
    bytes.extend_from_slice(&0u32.to_le_bytes()); // asset count A=0
    bytes.extend_from_slice(&2u32.to_le_bytes()); // chunk count C=2
    bytes.extend_from_slice(&1u32.to_le_bytes()); // ACI=1 (< C)
    assert_eq!(
        VersionIndex::from_bytes(&bytes),
        Err(FormatError::InvalidAssetChunkIndexCount {
            asset_chunk_index_count: 1,
            chunk_count: 2
        })
    );
}

// --- overflow-implying counts ----------------------------------------------

#[test]
fn store_index_huge_counts_error_not_panic() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0100_0000u32.to_le_bytes()); // version
    bytes.extend_from_slice(&0u32.to_le_bytes()); // hash id
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // block count
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // chunk count
    // Way too small for the declared counts -> Err (Truncated on 64-bit, or
    // SizeOverflow on 32-bit usize). Must not panic.
    assert!(StoreIndex::from_bytes(&bytes).is_err());
}

#[test]
fn version_index_huge_counts_error_not_panic() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0000_0002u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&32768u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // A
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // C
    bytes.extend_from_slice(&u32::MAX.to_le_bytes()); // ACI (>= C, ok)
    assert!(VersionIndex::from_bytes(&bytes).is_err());
}

// --- accessor error paths on wild name blobs ------------------------------
// version_index.rs:88-114 is bounds-checked but the error paths were untested.
// A parsed-but-wild index (or a hand-built struct) can carry name offsets/blobs
// that trip each accessor error without panicking.

#[test]
fn path_accessor_name_offset_out_of_bounds() {
    let mut vi = sample_vi();
    // Offset past the end of the name blob.
    vi.name_offsets = vec![vi.name_data.len() as u32 + 1];
    assert_eq!(
        vi.path_bytes(0),
        Err(FormatError::NameOffsetOutOfBounds {
            offset: vi.name_data.len() + 1,
            len: vi.name_data.len(),
        })
    );
}

#[test]
fn path_accessor_unterminated_name() {
    let mut vi = sample_vi();
    // A blob with no NUL terminator from the offset onward.
    vi.name_data = b"no-terminator-here".to_vec();
    vi.name_offsets = vec![0];
    assert_eq!(
        vi.path_bytes(0),
        Err(FormatError::UnterminatedName { offset: 0 })
    );
}

#[test]
fn path_accessor_invalid_utf8() {
    let mut vi = sample_vi();
    // Valid framing (NUL-terminated) but invalid UTF-8 bytes.
    vi.name_data = vec![0xff, 0xfe, 0x00];
    vi.name_offsets = vec![0];
    // path_bytes succeeds (raw bytes); path() (UTF-8 decode) fails.
    assert_eq!(vi.path_bytes(0).unwrap(), &[0xff, 0xfe]);
    assert_eq!(vi.path(0), Err(FormatError::InvalidUtf8 { offset: 0 }));
}

#[test]
fn path_accessor_index_out_of_bounds() {
    let vi = sample_vi(); // A = 1
    assert_eq!(
        vi.path_bytes(5),
        Err(FormatError::IndexOutOfBounds { index: 5, count: 1 })
    );
}

// --- fuzz-ish: mutating valid buffers never panics -------------------------

proptest! {
    #![proptest_config(config())]

    #[test]
    fn mutating_version_index_never_panics(
        seed in vec(any::<u8>(), 0usize..64),
        cut in 0usize..80,
        idx in any::<prop::sample::Index>(),
        byte in any::<u8>(),
    ) {
        // Start from a real valid buffer, then corrupt it.
        let mut bytes = sample_vi().to_bytes();
        bytes.extend_from_slice(&seed); // extend into name_data territory
        if !bytes.is_empty() {
            let i = idx.index(bytes.len());
            bytes[i] = byte;
        }
        let cut = cut.min(bytes.len());
        // Neither the truncated nor the byte-flipped buffer may panic.
        let _ = VersionIndex::from_bytes(&bytes[..cut]);
        let _ = VersionIndex::from_bytes(&bytes);
    }

    #[test]
    fn mutating_store_index_never_panics(cut in 0usize..64, idx in any::<prop::sample::Index>(), byte in any::<u8>()) {
        let mut bytes = sample_si().to_bytes();
        let i = idx.index(bytes.len());
        bytes[i] = byte;
        let cut = cut.min(bytes.len());
        let _ = StoreIndex::from_bytes(&bytes[..cut]);
        let _ = StoreIndex::from_bytes(&bytes);
    }

    #[test]
    fn mutating_stored_block_never_panics(cut in 0usize..64, idx in any::<prop::sample::Index>(), byte in any::<u8>()) {
        // 15 bytes = Σ sample_bi().chunk_sizes, so the unmutated block is valid
        // and every rejection below comes from the mutation, not the fixture.
        let sb = StoredBlock { block_index: sample_bi(), payload: vec![9; 15] };
        let mut bytes = sb.to_bytes();
        let i = idx.index(bytes.len());
        bytes[i] = byte;
        let cut = cut.min(bytes.len());
        let _ = StoredBlock::from_bytes(&bytes[..cut]);
        let _ = StoredBlock::from_bytes(&bytes);
    }
}

// --- asset→chunk map validation -------------------------------------------
//
// Consumers index this map directly, so a wild map is a panic on the download
// path; `from_bytes` rejects it once for all of them. On a 32-bit target an
// unchecked `start + count` would wrap into a small in-bounds value instead of
// panicking — a silently wrong answer rather than a loud failure.

/// Build a `.lvi` byte buffer with a hand-chosen asset→chunk map. `A = 1`,
/// `C = 1`, `ACI = 1`, one chunk of size 4.
fn vi_bytes_with_map(start: u32, count: u32, chunk_index: u32) -> Vec<u8> {
    let vi = VersionIndex {
        hash_identifier: 0x626c_6b33,
        target_chunk_size: 32768,
        path_hashes: vec![1],
        content_hashes: vec![2],
        asset_sizes: vec![4],
        asset_chunk_counts: vec![count],
        asset_chunk_index_starts: vec![start],
        asset_chunk_indexes: vec![chunk_index],
        chunk_hashes: vec![7],
        chunk_sizes: vec![4],
        chunk_tags: vec![0],
        name_offsets: vec![0],
        permissions: vec![Permissions(0o644)],
        name_data: b"a\0".to_vec(),
    };
    vi.to_bytes()
}

#[test]
fn version_index_accepts_a_consistent_map() {
    // The control: without this, a check that rejected everything would look
    // like a pass in the two tests below.
    let vi = VersionIndex::from_bytes(&vi_bytes_with_map(0, 1, 0)).expect("consistent map");
    assert_eq!(vi.asset_chunk_indexes, vec![0]);
}

#[test]
fn version_index_rejects_asset_chunk_range_past_the_map() {
    // start = u32::MAX: `validate.rs:56` would index asset_chunk_indexes[MAX].
    let err = VersionIndex::from_bytes(&vi_bytes_with_map(u32::MAX, 1, 0))
        .expect_err("a start past ACI must be rejected");
    assert!(
        matches!(err, FormatError::AssetChunkRangeOutOfBounds { .. }),
        "wrong error: {err:?}"
    );
    // count overhanging the end is the same defect reached the other way.
    let err = VersionIndex::from_bytes(&vi_bytes_with_map(0, 2, 0))
        .expect_err("a count past ACI must be rejected");
    assert!(
        matches!(err, FormatError::AssetChunkRangeOutOfBounds { .. }),
        "wrong error: {err:?}"
    );
}

#[test]
fn version_index_rejects_chunk_index_past_the_chunk_arrays() {
    // `validate.rs:57` would index chunk_sizes[7] with C = 1.
    let err = VersionIndex::from_bytes(&vi_bytes_with_map(0, 1, 7))
        .expect_err("a chunk index past C must be rejected");
    assert!(
        matches!(err, FormatError::AssetChunkIndexOutOfBounds { .. }),
        "wrong error: {err:?}"
    );
}

// --- stored-block payload length -------------------------------------------

#[test]
fn stored_block_rejects_payload_shorter_than_chunk_sizes() {
    // sample_bi(): tag 0, chunk_sizes [4,5,6] → Σ = 15.
    let short = StoredBlock {
        block_index: sample_bi(),
        payload: vec![0; 14],
    };
    let err = StoredBlock::from_bytes(&short.to_bytes())
        .expect_err("a payload one byte short of Σ chunk_sizes must be rejected");
    assert!(
        matches!(err, FormatError::Truncated { .. }),
        "wrong error: {err:?}"
    );
}

#[test]
fn stored_block_accepts_a_longer_payload_deliberately() {
    // Asymmetric on purpose: C derives the payload size from the file length and
    // ignores a longer tail, so an `==` check could refuse a block a real store
    // contains. Only *short* is an error.
    let long = StoredBlock {
        block_index: sample_bi(),
        payload: vec![0; 16],
    };
    let parsed = StoredBlock::from_bytes(&long.to_bytes()).expect("a longer tail is accepted");
    assert_eq!(parsed.payload.len(), 16);
}

#[test]
fn stored_block_payload_rule_applies_only_to_uncompressed_blocks() {
    // tag != 0 → the payload is an opaque compressed frame, and its length has
    // no relationship to Σ chunk_sizes (which are *uncompressed* sizes).
    let mut bi = sample_bi();
    bi.tag = 0xdead_beef;
    let compressed = StoredBlock {
        block_index: bi,
        payload: vec![0; 2],
    };
    StoredBlock::from_bytes(&compressed.to_bytes())
        .expect("a compressed block is not bound by Σ chunk_sizes");
}
