//! StoreIndex (`.lsi`) codec + `MergeStoreIndex` (`docs/format-spec.md` §2).
//!
//! Verified against `Longtail_GetStoreIndexDataSize` (longtail.c:8913),
//! `InitStoreIndexFromData` (longtail.c:8979), and `Longtail_MergeStoreIndex`
//! (longtail.c:9151).

use std::collections::HashSet;

use crate::cursor::{Reader, Writer, checked_add, checked_mul};
use crate::error::FormatError;

/// The single supported on-disk version (`LONGTAIL_VERSION(1,0,0)`).
pub const VERSION: u32 = 0x0100_0000;

/// Fixed header: 4 × u32.
const HEADER_SIZE: usize = 4 * 4;

/// A parsed `.lsi` store index.
///
/// Blocks are described in some order (not canonical — golongtail emits them in
/// Go-map order, `rust-port-1-results.md` §4); [`StoreIndex::from_bytes`] /
/// [`StoreIndex::to_bytes`] preserve whatever order was parsed, so the
/// round-trip is byte-identical regardless. Each block `i`'s chunks occupy
/// `chunk_hashes[block_chunks_offsets[i] .. + block_chunk_counts[i]]` (and the
/// parallel `chunk_sizes`).
///
/// The `version` field is not stored (the reader rejects anything but
/// [`VERSION`]; the writer always emits it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreIndex {
    pub hash_identifier: u32,
    /// `m_BlockHashes` — length `B`.
    pub block_hashes: Vec<u64>,
    /// `m_ChunkHashes` — length `C`, grouped per block, contiguous.
    pub chunk_hashes: Vec<u64>,
    /// `m_BlockChunksOffsets` — length `B`; start offset into the chunk arrays.
    pub block_chunks_offsets: Vec<u32>,
    /// `m_BlockChunkCounts` — length `B`.
    pub block_chunk_counts: Vec<u32>,
    /// `m_BlockTags` — length `B`; per-block compression ID.
    pub block_tags: Vec<u32>,
    /// `m_ChunkSizes` — length `C`, parallel to `chunk_hashes`.
    pub chunk_sizes: Vec<u32>,
}

impl StoreIndex {
    /// An empty store index with the given hash identifier and no blocks.
    pub fn empty(hash_identifier: u32) -> StoreIndex {
        StoreIndex {
            hash_identifier,
            block_hashes: Vec::new(),
            chunk_hashes: Vec::new(),
            block_chunks_offsets: Vec::new(),
            block_chunk_counts: Vec::new(),
            block_tags: Vec::new(),
            chunk_sizes: Vec::new(),
        }
    }

    /// Number of blocks (`B`).
    pub fn block_count(&self) -> u32 {
        self.block_hashes.len() as u32
    }

    /// Total chunk-entry count across all blocks (`C`).
    pub fn chunk_count(&self) -> u32 {
        self.chunk_hashes.len() as u32
    }

    /// `Longtail_GetStoreIndexDataSize(B, C)` — the exact on-disk byte length.
    fn data_size(b: usize, c: usize) -> Result<usize, FormatError> {
        let mut total = HEADER_SIZE;
        total = checked_add(total, checked_mul(b, 8)?)?; // block_hashes
        total = checked_add(total, checked_mul(c, 8)?)?; // chunk_hashes
        total = checked_add(total, checked_mul(b, 4)?)?; // block_chunks_offsets
        total = checked_add(total, checked_mul(b, 4)?)?; // block_chunk_counts
        total = checked_add(total, checked_mul(b, 4)?)?; // block_tags
        total = checked_add(total, checked_mul(c, 4)?)?; // chunk_sizes
        Ok(total)
    }

    /// Parse a `.lsi` buffer. **Trailing bytes are rejected** (stricter than C,
    /// which accepts oversize buffers and silently drops the tail on rewrite) so
    /// the round-trip fixpoint is sound.
    pub fn from_bytes(data: &[u8]) -> Result<StoreIndex, FormatError> {
        if data.len() < HEADER_SIZE {
            return Err(FormatError::Truncated {
                expected: HEADER_SIZE,
                actual: data.len(),
            });
        }
        let mut r = Reader::new(data);
        let version = r.u32()?;
        if version != VERSION {
            return Err(FormatError::UnsupportedVersion {
                found: version,
                expected: VERSION,
            });
        }
        let hash_identifier = r.u32()?;
        let block_count = r.u32()? as usize;
        let chunk_count = r.u32()? as usize;

        let expected = Self::data_size(block_count, chunk_count)?;
        if data.len() < expected {
            return Err(FormatError::Truncated {
                expected,
                actual: data.len(),
            });
        }
        if data.len() > expected {
            return Err(FormatError::TrailingBytes {
                expected,
                actual: data.len(),
            });
        }

        let block_hashes = r.u64_vec(block_count)?;
        let chunk_hashes = r.u64_vec(chunk_count)?;
        let block_chunks_offsets = r.u32_vec(block_count)?;
        let block_chunk_counts = r.u32_vec(block_count)?;
        let block_tags = r.u32_vec(block_count)?;
        let chunk_sizes = r.u32_vec(chunk_count)?;

        Ok(StoreIndex {
            hash_identifier,
            block_hashes,
            chunk_hashes,
            block_chunks_offsets,
            block_chunk_counts,
            block_tags,
            chunk_sizes,
        })
    }

    /// Serialize to a `.lsi` byte buffer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let b = self.block_hashes.len();
        let c = self.chunk_hashes.len();
        let cap = Self::data_size(b, c).unwrap_or(HEADER_SIZE);
        let mut w = Writer::with_capacity(cap);
        w.u32(VERSION);
        w.u32(self.hash_identifier);
        w.u32(b as u32);
        w.u32(c as u32);
        w.u64_slice(&self.block_hashes);
        w.u64_slice(&self.chunk_hashes);
        w.u32_slice(&self.block_chunks_offsets);
        w.u32_slice(&self.block_chunk_counts);
        w.u32_slice(&self.block_tags);
        w.u32_slice(&self.chunk_sizes);
        w.into_vec()
    }

    /// Append `source` block `src_block`'s chunk group to `out`, rebuilding the
    /// output offset cumulatively. Bounds-checked (a wild index may carry an
    /// offset/count that runs off the chunk arrays; C would read OOB, we `Err`).
    fn push_block(
        out: &mut StoreIndex,
        source: &StoreIndex,
        src_block: usize,
    ) -> Result<(), FormatError> {
        let count = source.block_chunk_counts[src_block] as usize;
        let offset = source.block_chunks_offsets[src_block] as usize;
        let end = checked_add(offset, count)?;
        if end > source.chunk_hashes.len() || end > source.chunk_sizes.len() {
            return Err(FormatError::ChunkRangeOutOfBounds {
                block: src_block,
                offset,
                count,
                len: source.chunk_hashes.len().min(source.chunk_sizes.len()),
            });
        }
        let out_offset =
            u32::try_from(out.chunk_hashes.len()).map_err(|_| FormatError::SizeOverflow)?;
        out.block_hashes.push(source.block_hashes[src_block]);
        out.block_chunk_counts
            .push(source.block_chunk_counts[src_block]);
        out.block_chunks_offsets.push(out_offset);
        out.block_tags.push(source.block_tags[src_block]);
        out.chunk_hashes
            .extend_from_slice(&source.chunk_hashes[offset..end]);
        out.chunk_sizes
            .extend_from_slice(&source.chunk_sizes[offset..end]);
        Ok(())
    }

    /// Merge two store indexes, matching `Longtail_MergeStoreIndex`
    /// (longtail.c:9151) **byte-for-byte on the success path** (Stage 4 shard
    /// naming hashes these bytes).
    ///
    /// Semantics (derived from source, cited by line):
    /// - **Hash identifier** (longtail.c:9166-9188): local's if local is
    ///   non-empty; else remote's; else — both empty — `0` (the both-empty case
    ///   routes through `Longtail_CreateStoreIndexFromBlocks(0, 0, …)`
    ///   longtail.c:9173, which leaves the identifier `0`, longtail.c:9079).
    /// - **Error** (longtail.c:9182-9186): `EINVAL` when *both* inputs are
    ///   non-empty and their hash identifiers differ. When one side is empty its
    ///   identifier is never compared.
    /// - **Block order / dedup** (longtail.c:9210-9235): local's unique blocks
    ///   first, in local order (internal duplicate block-hashes deduped, first
    ///   occurrence kept via `PutUnique`'s no-overwrite semantics), then
    ///   remote-only blocks in remote order (also internally deduped). On a
    ///   block-hash tie across the two inputs, the local block wins entirely —
    ///   its tag, chunk hashes, and chunk sizes are copied (remote is skipped,
    ///   longtail.c:9224).
    /// - **Offsets rebuilt cumulatively** (longtail.c:9250-9280): the merge
    ///   *canonicalizes* the chunk layout, even for `merge(x, empty)`.
    /// - **Version** forced to [`VERSION`] (longtail.c:9246).
    ///
    /// (Consequence of checked arithmetic: an adversarial pair whose merged
    /// chunk offset would exceed `u32` returns [`FormatError::SizeOverflow`]
    /// where C's `uint32_t` accumulation silently wraps — accepted, adversarial
    /// only.)
    pub fn merge(&self, other: &StoreIndex) -> Result<StoreIndex, FormatError> {
        let local = self;
        let remote = other;
        let local_block_count = local.block_hashes.len();
        let remote_block_count = remote.block_hashes.len();

        let hash_identifier = if local_block_count == 0 {
            if remote_block_count == 0 {
                // Both empty → CreateStoreIndexFromBlocks(0, 0) → identifier 0.
                return Ok(StoreIndex::empty(0));
            }
            remote.hash_identifier
        } else {
            let id = local.hash_identifier;
            if remote_block_count != 0 && id != remote.hash_identifier {
                return Err(FormatError::ConflictingHashIdentifier {
                    local: id,
                    remote: remote.hash_identifier,
                });
            }
            id
        };

        let mut out = StoreIndex::empty(hash_identifier);

        // Pass 1: local's unique blocks, in local order (first occurrence wins).
        let mut local_seen: HashSet<u64> = HashSet::with_capacity(local_block_count);
        for i in 0..local_block_count {
            let bh = local.block_hashes[i];
            if !local_seen.insert(bh) {
                continue; // internal duplicate — skip (PutUnique no-overwrite)
            }
            Self::push_block(&mut out, local, i)?;
        }

        // Pass 2: remote-only blocks, in remote order (also internally deduped).
        let mut remote_seen: HashSet<u64> = HashSet::with_capacity(remote_block_count);
        for i in 0..remote_block_count {
            let bh = remote.block_hashes[i];
            if local_seen.contains(&bh) {
                continue; // present in local — local wins the tie
            }
            if !remote_seen.insert(bh) {
                continue; // internal duplicate — skip
            }
            Self::push_block(&mut out, remote, i)?;
        }

        Ok(out)
    }
}
