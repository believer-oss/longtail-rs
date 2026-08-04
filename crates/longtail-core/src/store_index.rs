//! StoreIndex (`.lsi`) codec + `MergeStoreIndex` (`docs/format-spec.md` §2).
//!
//! Verified against `Longtail_GetStoreIndexDataSize` (longtail.c:8913),
//! `InitStoreIndexFromData` (longtail.c:8979), and `Longtail_MergeStoreIndex`
//! (longtail.c:9151).

use std::collections::{HashMap, HashSet};

use crate::block::BlockIndex;
use crate::cursor::{Reader, Writer, checked_add, checked_mul};
use crate::error::FormatError;

/// The single supported on-disk version (`LONGTAIL_VERSION(1,0,0)`).
pub const VERSION: u32 = 0x0100_0000;

/// Fixed header: 4 × u32.
const HEADER_SIZE: usize = 4 * 4;

/// A parsed `.lsi` store index.
///
/// Blocks are described in some order (not canonical — golongtail emits them in
/// Go-map order); [`StoreIndex::from_bytes`] /
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
    /// (longtail.c:9151 @96241fe) **byte-for-byte on the success path** (the S3
    /// store-index shard name is the sha256 of these bytes, so byte-identity is
    /// load-bearing).
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
    /// Reserve capacity in all six parallel arrays ahead of a bulk build
    /// (`blocks` block entries, `chunks` chunk entries) so they don't
    /// grow-by-double. Purely an allocation hint — never changes contents.
    fn reserve_capacity(&mut self, blocks: usize, chunks: usize) {
        self.block_hashes.reserve(blocks);
        self.block_chunks_offsets.reserve(blocks);
        self.block_chunk_counts.reserve(blocks);
        self.block_tags.reserve(blocks);
        self.chunk_hashes.reserve(chunks);
        self.chunk_sizes.reserve(chunks);
    }

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
        // Pre-size the output arrays to the union upper bound (dedup only ever
        // removes), killing the doubling-realloc overshoot — at GB scale the
        // grow-by-doubling transiently allocated up to ~2× the final buffers.
        out.reserve_capacity(
            local_block_count + remote_block_count,
            local.chunk_hashes.len() + remote.chunk_hashes.len(),
        );

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

    /// Whether the block/chunk arrays are in **canonical layout**: the six
    /// parallel arrays have consistent lengths and `block_chunks_offsets` is
    /// exactly cumulative from `0` (block `i` starts at `Σ counts[0..i]`), with
    /// the final block's range ending precisely at `chunk_hashes.len()`. Any
    /// store index parsed from a valid `.lsi` or produced by [`Self::merge`] /
    /// [`Self::from_block_indexes`] is canonical — the writers always emit this
    /// form. This is the precondition under which [`Self::merge_consuming`] may
    /// reuse `self` verbatim as merge Pass 1's output (see there).
    fn is_canonical(&self) -> bool {
        let b = self.block_hashes.len();
        if self.block_chunk_counts.len() != b
            || self.block_chunks_offsets.len() != b
            || self.block_tags.len() != b
            || self.chunk_hashes.len() != self.chunk_sizes.len()
        {
            return false;
        }
        let mut expected: u64 = 0;
        for i in 0..b {
            if u64::from(self.block_chunks_offsets[i]) != expected {
                return false;
            }
            expected += u64::from(self.block_chunk_counts[i]);
        }
        expected == self.chunk_hashes.len() as u64
    }

    /// [`Self::merge`] that **consumes `self`** and, in the common case, reuses
    /// its allocations as the union instead of building a fresh output alongside
    /// both inputs. **Byte-identical to `self.merge(other)` in every case** (the
    /// S3 shard name is the sha256 of these bytes, so this is load-bearing).
    ///
    /// Why `self` can be reused: merge Pass 1 copies `local`'s unique blocks, in
    /// order, re-deriving cumulative chunk offsets. When `local` is already
    /// [canonical](Self::is_canonical) and has no internal duplicate block
    /// hashes, that Pass-1 output is **bit-identical to `local` itself** — so we
    /// keep `self` as-is and only append Pass 2 (the remote-only blocks). That
    /// drops the merge high-water mark from `local + remote + output` (~3 shards
    /// at the two-file steady state) to `output + remote` (~2 shards): roughly
    /// one whole `store_*.lsi` off the read/union peak that dominates a big
    /// `validate-version` / `downsync` / `upsync` against a month-end store.
    ///
    /// When `local` is non-canonical or carries internal duplicate block hashes
    /// (rare — no writer here produces either), it falls back to the allocating
    /// [`Self::merge`] so the result is still exact.
    pub fn merge_consuming(mut self, other: &StoreIndex) -> Result<StoreIndex, FormatError> {
        let local_block_count = self.block_hashes.len();
        let remote_block_count = other.block_hashes.len();

        // Hash-identifier + conflict rules — identical to `merge`.
        let hash_identifier = if local_block_count == 0 {
            if remote_block_count == 0 {
                return Ok(StoreIndex::empty(0));
            }
            other.hash_identifier
        } else {
            let id = self.hash_identifier;
            if remote_block_count != 0 && id != other.hash_identifier {
                return Err(FormatError::ConflictingHashIdentifier {
                    local: id,
                    remote: other.hash_identifier,
                });
            }
            id
        };

        // Pass 2 needs the local block-hash set regardless. If it comes up short,
        // `local` has internal duplicate block hashes that Pass 1 would dedup
        // away — so `self` would NOT equal Pass 1's output; likewise if `local`
        // is not canonical. Either way, fall back to the allocating merge for an
        // exact result.
        let mut local_seen: HashSet<u64> = HashSet::with_capacity(local_block_count);
        for i in 0..local_block_count {
            local_seen.insert(self.block_hashes[i]);
        }
        if local_seen.len() != local_block_count || !self.is_canonical() {
            return self.merge(other);
        }

        // Fast path: `self` already equals Pass 1's output. Set the identifier
        // (a no-op when `local` is non-empty), reserve for the appended tail, and
        // append Pass 2 in remote order — exactly `merge`'s Pass 2.
        self.hash_identifier = hash_identifier;
        self.reserve_capacity(remote_block_count, other.chunk_hashes.len());
        let mut remote_seen: HashSet<u64> = HashSet::with_capacity(remote_block_count);
        for i in 0..remote_block_count {
            let bh = other.block_hashes[i];
            if local_seen.contains(&bh) {
                continue; // present in local — local wins the tie
            }
            if !remote_seen.insert(bh) {
                continue; // internal duplicate — skip
            }
            Self::push_block(&mut self, other, i)?;
        }
        Ok(self)
    }

    /// Concatenate a set of [`BlockIndex`] into a store index, matching
    /// `Longtail_CreateStoreIndexFromBlocks` (longtail.c:9064) **byte-for-byte**.
    ///
    /// Semantics (source-cited):
    /// - **Hash identifier** (longtail.c:9078-9086): the first block whose
    ///   identifier is non-zero wins; `hash_identifier` starts at `0` and is only
    ///   overwritten while still `0`, so an all-zero-id (or empty) input yields
    ///   `0`. (Exactly `hash_identifier = (hash_identifier == 0) ? block.id : hash_identifier`.)
    /// - **No dedup, no sort** (longtail.c:9110-9121): blocks are copied in the
    ///   *given order*; each block's chunk hashes/sizes are appended contiguously
    ///   and `block_chunks_offsets` is a running cumulative counter.
    /// - **Version** forced to [`VERSION`] (longtail.c:9104).
    ///
    /// The caller owns the ordering: golongtail's `contentIndexWorker` feeds
    /// blocks in Go-map (nondeterministic) order, so there is no Go order to
    /// match; where the Rust store layer assembles the list itself it must first
    /// sort by block hash for determinism,
    /// then round-trip / merge byte-identity carries the rest.
    ///
    /// (Accepted adversarial divergence: the cumulative chunk offset is a
    /// `u32::try_from` — a pathological set summing beyond `u32::MAX` chunks
    /// returns [`FormatError::SizeOverflow`] where C's `uint32_t` counter wraps.)
    pub fn from_block_indexes(blocks: &[BlockIndex]) -> Result<StoreIndex, FormatError> {
        let mut hash_identifier = 0u32;
        for block in blocks {
            if hash_identifier == 0 {
                hash_identifier = block.hash_identifier;
            }
        }
        let mut out = StoreIndex::empty(hash_identifier);
        // Exact pre-size: one block each, Σ chunk counts total (no dedup here).
        let total_chunks: usize = blocks.iter().map(|b| b.chunk_hashes.len()).sum();
        out.reserve_capacity(blocks.len(), total_chunks);
        for block in blocks {
            let n = block.chunk_hashes.len();
            // C `memcpy`s `block_chunk_count` entries from each array; the two
            // parallel arrays are validated to agree here (a malformed BlockIndex
            // is impossible to construct from `from_bytes`, but callers may build
            // one directly).
            if block.chunk_sizes.len() != n {
                return Err(FormatError::ChunkRangeOutOfBounds {
                    block: out.block_hashes.len(),
                    offset: 0,
                    count: n,
                    len: block.chunk_sizes.len(),
                });
            }
            let out_offset =
                u32::try_from(out.chunk_hashes.len()).map_err(|_| FormatError::SizeOverflow)?;
            out.block_hashes.push(block.block_hash);
            out.block_tags.push(block.tag);
            out.block_chunk_counts.push(n as u32);
            out.block_chunks_offsets.push(out_offset);
            out.chunk_hashes.extend_from_slice(&block.chunk_hashes);
            out.chunk_sizes.extend_from_slice(&block.chunk_sizes);
        }
        Ok(out)
    }

    /// Reconstruct the [`BlockIndex`] for store block `b` (the inverse of
    /// [`StoreIndex::from_block_indexes`]; = `Longtail_MakeBlockIndex`,
    /// longtail.c:9127). Returns `None` if `b` is out of range or the block's
    /// chunk range runs off the arrays.
    pub fn block_index_at(&self, b: usize) -> Option<BlockIndex> {
        if b >= self.block_hashes.len() {
            return None;
        }
        let count = *self.block_chunk_counts.get(b)? as usize;
        let offset = *self.block_chunks_offsets.get(b)? as usize;
        let end = offset.checked_add(count)?;
        if end > self.chunk_hashes.len() || end > self.chunk_sizes.len() {
            return None;
        }
        Some(BlockIndex {
            block_hash: self.block_hashes[b],
            hash_identifier: self.hash_identifier,
            tag: self.block_tags[b],
            chunk_hashes: self.chunk_hashes[offset..end].to_vec(),
            chunk_sizes: self.chunk_sizes[offset..end].to_vec(),
        })
    }

    /// Total decompressed payload size (Σ chunk sizes) for each block whose hash
    /// is in `wanted`, computed straight from the packed arrays without
    /// materializing a [`BlockIndex`] per block. Used by the download prefetch to
    /// size permits for just the requested working set — the alternative
    /// (cloning the whole union index and mapping every block) allocates a full
    /// copy of a multi-GB store index per `preflight_get`. First occurrence of a
    /// block hash wins; blocks whose chunk range runs off the arrays are skipped
    /// (they contribute no size, mirroring [`Self::block_index_at`]'s bounds
    /// check).
    pub fn block_payload_sizes(&self, wanted: &[u64]) -> HashMap<u64, u64> {
        let want: HashSet<u64> = wanted.iter().copied().collect();
        let mut out: HashMap<u64, u64> = HashMap::with_capacity(want.len());
        for b in 0..self.block_hashes.len() {
            let block_hash = self.block_hashes[b];
            if !want.contains(&block_hash) || out.contains_key(&block_hash) {
                continue;
            }
            let (Some(&count), Some(&offset)) = (
                self.block_chunk_counts.get(b),
                self.block_chunks_offsets.get(b),
            ) else {
                continue;
            };
            let (count, offset) = (count as usize, offset as usize);
            let Some(end) = offset.checked_add(count) else {
                continue;
            };
            if end > self.chunk_sizes.len() {
                continue;
            }
            let size: u64 = self.chunk_sizes[offset..end]
                .iter()
                .map(|&s| s as u64)
                .sum();
            out.insert(block_hash, size);
        }
        out
    }

    /// Keep only the blocks whose hash is in `keep_block_hashes`, matching
    /// `Longtail_PruneStoreIndex` (longtail.c:9287). Source block **order is
    /// preserved** and chunk offsets are rebuilt cumulatively; the
    /// `hash_identifier` is carried over from the source **even when the result
    /// is empty** (longtail.c:9406 — a divergence from
    /// [`StoreIndex::from_block_indexes`], which would yield `0`). Keep-hashes
    /// not present in the source are ignored; duplicate keep-hashes are deduped.
    pub fn prune(&self, keep_block_hashes: &[u64]) -> StoreIndex {
        let keep: HashSet<u64> = keep_block_hashes.iter().copied().collect();
        let mut out = StoreIndex::empty(self.hash_identifier);
        for b in 0..self.block_hashes.len() {
            if !keep.contains(&self.block_hashes[b]) {
                continue;
            }
            // A wild source block (offset/count off the arrays) is skipped
            // rather than panicking (C would read OOB).
            if Self::push_block(&mut out, self, b).is_err() {
                continue;
            }
        }
        out.hash_identifier = self.hash_identifier;
        out
    }

    /// Select the subset of blocks that cover the requested `chunk_hashes`,
    /// matching `Longtail_GetExistingStoreIndex` (longtail.c:7087).
    ///
    /// Greedy usage-percent selection (source-cited):
    /// - Build the unique requested-chunk set (longtail.c:7155-7165).
    /// - When `min_block_usage_percent <= 100` (longtail.c:7169): for every store
    ///   block compute `block_use` (Σ sizes of its chunks that are requested) and
    ///   `block_size` (Σ all its chunk sizes); a block is a *potential* if
    ///   `block_use > 0` and (when `min_block_usage_percent > 0`) its usage
    ///   percent `block_use*100/block_size` is `>= min_block_usage_percent`
    ///   (longtail.c:7190-7203).
    /// - Sort potentials usage-high-to-low, tie-broken by ascending potential
    ///   index — a total order, so the result is deterministic
    ///   (`SortBlockUsageHighToLow`, longtail.c:7059-7085 / QSORT :7220).
    /// - Greedily walk potentials; a block is kept only if it claims at least one
    ///   not-yet-claimed requested chunk (longtail.c:7222-7257). Output block
    ///   order = the order blocks first claim a chunk.
    /// - Empty result (no potentials / no found blocks) → an empty store index
    ///   with identifier `0` (`CreateStoreIndexFromBlocks(0,0)`,
    ///   longtail.c:7209/:7263).
    ///
    /// **Deliberate divergence from C:** the kept block's `tag` is taken from
    /// the block's own `block_tags[b]` (the correct value, matching
    /// `Longtail_MakeBlockIndex`, longtail.c:9145 @96241fe).
    /// `Longtail_GetExistingStoreIndex` instead indexes `m_BlockTags` with the
    /// *chunk* offset (longtail.c:7307) — a latent C bug that reads the wrong
    /// slot (or out of bounds) whenever a kept block's chunk offset differs from
    /// its block index. We reproduce every other output byte exactly and mirror
    /// the (correct) `MakeBlockIndex` tag.
    ///
    /// Never panics on a wild store index (bounds-checked via
    /// [`StoreIndex::block_index_at`]); such blocks are simply skipped.
    pub fn get_existing_store_index(
        &self,
        chunk_hashes: &[u64],
        min_block_usage_percent: u32,
    ) -> StoreIndex {
        let requested: HashSet<u64> = chunk_hashes.iter().copied().collect();
        let unique_chunk_count = requested.len();

        let block_count = self.block_hashes.len();
        let mut found_block_hashes: Vec<u64> = Vec::new();

        if min_block_usage_percent <= 100 {
            // (block_index, usage_percent) potentials, built in ascending block
            // order (so the vector index doubles as C's tie-break key).
            let mut potentials: Vec<(usize, u32)> = Vec::new();
            for b in 0..block_count {
                let count = self.block_chunk_counts[b] as usize;
                let offset = self.block_chunks_offsets[b] as usize;
                let end = match offset.checked_add(count) {
                    Some(e) if e <= self.chunk_hashes.len() && e <= self.chunk_sizes.len() => e,
                    _ => continue, // wild block — skip (C would read OOB)
                };
                let mut block_use: u32 = 0;
                let mut block_size: u32 = 0;
                for idx in offset..end {
                    let cs = self.chunk_sizes[idx];
                    block_size = block_size.wrapping_add(cs);
                    if requested.contains(&self.chunk_hashes[idx]) {
                        block_use = block_use.wrapping_add(cs);
                    }
                }
                if block_use > 0 {
                    let pct = if block_size == 0 {
                        0
                    } else {
                        ((block_use as u64 * 100) / block_size as u64) as u32
                    };
                    if min_block_usage_percent > 0 && pct < min_block_usage_percent {
                        continue;
                    }
                    potentials.push((b, pct));
                }
            }

            if !potentials.is_empty() {
                // usage high-to-low, tie-break ascending potential index.
                let mut order: Vec<usize> = (0..potentials.len()).collect();
                order.sort_by(|&a, &b| potentials[b].1.cmp(&potentials[a].1).then(a.cmp(&b)));

                let mut claimed: HashSet<u64> = HashSet::new();
                let mut block_added: HashSet<u64> = HashSet::new();
                let mut found_chunk_count = 0usize;
                for &po in &order {
                    if found_chunk_count >= unique_chunk_count {
                        break;
                    }
                    let b = potentials[po].0;
                    let count = self.block_chunk_counts[b] as usize;
                    let offset = self.block_chunks_offsets[b] as usize;
                    for idx in offset..(offset + count) {
                        let ch = self.chunk_hashes[idx];
                        if !requested.contains(&ch) {
                            continue;
                        }
                        if !claimed.insert(ch) {
                            continue; // already claimed by a higher-usage block
                        }
                        found_chunk_count += 1;
                        let bh = self.block_hashes[b];
                        if block_added.insert(bh) {
                            found_block_hashes.push(bh);
                        }
                    }
                }
            }
        }

        if found_block_hashes.is_empty() {
            return StoreIndex::empty(0);
        }

        // Map each found block hash to its (first) store block index and rebuild.
        let mut by_hash: std::collections::HashMap<u64, usize> =
            std::collections::HashMap::with_capacity(block_count);
        for b in 0..block_count {
            by_hash.entry(self.block_hashes[b]).or_insert(b);
        }
        let mut blocks: Vec<BlockIndex> = Vec::with_capacity(found_block_hashes.len());
        for bh in &found_block_hashes {
            if let Some(&b) = by_hash.get(bh)
                && let Some(bi) = self.block_index_at(b)
            {
                blocks.push(bi);
            }
        }
        // `from_block_indexes` on validated slices cannot fail here.
        StoreIndex::from_block_indexes(&blocks).unwrap_or_else(|_| StoreIndex::empty(0))
    }
}
