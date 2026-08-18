//! Error type for the on-disk format layer.
//!
//! Every fallible parse/merge/accessor returns [`FormatError`]. Malformed input
//! must always surface as an `Err` — never a panic and never a silent wrap.

use thiserror::Error;

/// Errors produced while parsing, serializing, merging, or accessing the
/// on-disk formats.
///
/// The variants map onto the source-verified C reader checks (see
/// `docs/format-spec.md`), plus the deliberate Rust-side strictness this port
/// adds (rejecting `ACI < C` and trailing bytes, and returning
/// `Err` where C would silently wrap 32-bit arithmetic).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormatError {
    /// The format's version field did not match the single supported constant.
    /// Readers reject any other value (longtail.c:2633 / :9004).
    #[error("unsupported format version: found {found:#010x}, expected {expected:#010x}")]
    UnsupportedVersion { found: u32, expected: u32 },

    /// The buffer is smaller than the header counts imply. `expected` is the
    /// minimum byte length required, `actual` is the buffer length.
    #[error("buffer truncated: need at least {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },

    /// The buffer is larger than the format occupies. Rejected for StoreIndex
    /// and standalone BlockIndex (stricter than C, which silently drops the
    /// tail) so the round-trip fixpoint is sound. Never raised for VersionIndex
    /// (its tail is `name_data`) or StoredBlock (its tail is the payload).
    #[error("trailing bytes: format occupies {expected} bytes but buffer is {actual}")]
    TrailingBytes { expected: usize, actual: usize },

    /// A VersionIndex declared `asset_chunk_index_count < chunk_count`.
    /// Deliberate Rust-side strictness: C's equivalent check is defanged
    /// (`Longtail_GetVersionIndexDataSize` returns `EINVAL` as a `size_t` that
    /// then trivially passes the caller's truncation compare, longtail.c:2564).
    #[error("asset_chunk_index_count ({asset_chunk_index_count}) < chunk_count ({chunk_count})")]
    InvalidAssetChunkIndexCount {
        asset_chunk_index_count: u32,
        chunk_count: u32,
    },

    /// A size computation over attacker-controlled header counts overflowed
    /// `usize` (or a merge chunk-offset accumulation overflowed `u32` — the
    /// adversarial case where C's 32-bit arithmetic silently wraps).
    #[error("size computation overflowed")]
    SizeOverflow,

    /// `merge` was called with two non-empty StoreIndexes that carry different
    /// hash identifiers (`Longtail_MergeStoreIndex` returns `EINVAL`,
    /// longtail.c:9184).
    #[error("conflicting hash identifiers in merge: local {local:#010x} != remote {remote:#010x}")]
    ConflictingHashIdentifier { local: u32, remote: u32 },

    /// An accessor was given an index past the number of entries.
    #[error("index {index} out of bounds (count {count})")]
    IndexOutOfBounds { index: usize, count: usize },

    /// A name offset points at or past the end of the name-data blob.
    #[error("name offset {offset} out of bounds (name_data len {len})")]
    NameOffsetOutOfBounds { offset: usize, len: usize },

    /// A VersionIndex asset's chunk range `[start, start+count)` falls outside
    /// `m_AssetChunkIndexes`. Checked at parse time because six consumers index
    /// that map with plain `[]`; C reads out of bounds here instead.
    #[error(
        "asset {asset} chunk range [{start}, {start}+{count}) exceeds \
         asset_chunk_index_count {len}"
    )]
    AssetChunkRangeOutOfBounds {
        asset: usize,
        start: u32,
        count: u32,
        len: usize,
    },

    /// An entry of `m_AssetChunkIndexes` points past the chunk arrays. Same
    /// rationale as [`FormatError::AssetChunkRangeOutOfBounds`].
    #[error("asset_chunk_indexes[{position}] = {index} exceeds chunk_count {chunk_count}")]
    AssetChunkIndexOutOfBounds {
        position: usize,
        index: u32,
        chunk_count: usize,
    },

    /// A name string starting at `offset` has no NUL terminator before the end
    /// of the name-data blob.
    #[error("name at offset {offset} is not NUL-terminated")]
    UnterminatedName { offset: usize },

    /// A name string starting at `offset` is not valid UTF-8.
    #[error("name at offset {offset} is not valid UTF-8")]
    InvalidUtf8 { offset: usize },

    /// A StoreIndex block's chunk group `[offset, offset+count)` falls outside
    /// the chunk arrays. Only reachable from a hand-built / wild index (a parse
    /// validates total size but not per-block offset consistency); C would read
    /// out of bounds here, Rust returns this error instead.
    #[error(
        "block {block} chunk range [{offset}, {offset}+{count}) exceeds chunk array length {len}"
    )]
    ChunkRangeOutOfBounds {
        block: usize,
        offset: usize,
        count: usize,
        len: usize,
    },
}
