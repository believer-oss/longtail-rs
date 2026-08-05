//! BlockIndex + StoredBlock (`.lsb`) codecs (`docs/format-spec.md` §3).
//!
//! Verified against `Longtail_GetBlockIndexDataSize` (longtail.c:3585),
//! `Longtail_InitBlockIndexFromData` (longtail.c:3654), and
//! `Longtail_InitStoredBlockFromData` (longtail.c:4005). **No version field** —
//! parse validation is the truncation check only.

use crate::cursor::{Reader, Writer, checked_add, checked_mul};
use crate::error::FormatError;

/// Fixed BlockIndex header: `u64` block hash + 3 × `u32`. Note the header leads
/// with a `u64`, so it is NOT all-`u32` (spec cross-cutting rules).
const HEADER_SIZE: usize = 8 + 4 + 4 + 4;

/// A parsed block index (the fixed-size front matter of a `.lsb`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockIndex {
    pub block_hash: u64,
    pub hash_identifier: u32,
    pub tag: u32,
    /// `m_ChunkHashes` — length `n` (the chunk count).
    pub chunk_hashes: Vec<u64>,
    /// `m_ChunkSizes` — length `n`; **uncompressed** chunk byte sizes.
    pub chunk_sizes: Vec<u32>,
}

impl BlockIndex {
    /// Number of chunks packed in this block (`n`).
    pub fn chunk_count(&self) -> u32 {
        self.chunk_hashes.len() as u32
    }

    /// `Longtail_GetBlockIndexDataSize(n)` — bytes up to and including the chunk
    /// arrays (the payload, if any, follows immediately).
    fn data_size(n: usize) -> Result<usize, FormatError> {
        let mut total = HEADER_SIZE;
        total = checked_add(total, checked_mul(n, 8)?)?; // chunk_hashes
        total = checked_add(total, checked_mul(n, 4)?)?; // chunk_sizes
        Ok(total)
    }

    /// Read the block-index region from the front of `data`, returning the
    /// parsed index and the number of bytes it consumed. Shared by
    /// [`BlockIndex::from_bytes`] (rejects any tail) and
    /// [`StoredBlock::from_bytes`] (the tail is the payload).
    fn read_prefix(data: &[u8]) -> Result<(BlockIndex, usize), FormatError> {
        if data.len() < HEADER_SIZE {
            return Err(FormatError::Truncated {
                expected: HEADER_SIZE,
                actual: data.len(),
            });
        }
        let mut r = Reader::new(data);
        let block_hash = r.u64()?;
        let hash_identifier = r.u32()?;
        let chunk_count = r.u32()? as usize;
        let tag = r.u32()?;

        let expected = Self::data_size(chunk_count)?;
        if data.len() < expected {
            return Err(FormatError::Truncated {
                expected,
                actual: data.len(),
            });
        }

        let chunk_hashes = r.u64_vec(chunk_count)?;
        let chunk_sizes = r.u32_vec(chunk_count)?;
        Ok((
            BlockIndex {
                block_hash,
                hash_identifier,
                tag,
                chunk_hashes,
                chunk_sizes,
            },
            r.pos(),
        ))
    }

    /// Parse a standalone block-index buffer. **Trailing bytes are rejected**
    /// (stricter than C) so the round-trip fixpoint is sound.
    pub fn from_bytes(data: &[u8]) -> Result<BlockIndex, FormatError> {
        let (bi, consumed) = Self::read_prefix(data)?;
        if consumed != data.len() {
            return Err(FormatError::TrailingBytes {
                expected: consumed,
                actual: data.len(),
            });
        }
        Ok(bi)
    }

    /// Serialize the block-index region.
    pub fn to_bytes(&self) -> Vec<u8> {
        let n = self.chunk_hashes.len();
        let cap = Self::data_size(n).unwrap_or(HEADER_SIZE);
        let mut w = Writer::with_capacity(cap);
        self.write_to(&mut w);
        w.into_vec()
    }

    fn write_to(&self, w: &mut Writer) {
        w.u64(self.block_hash);
        w.u32(self.hash_identifier);
        w.u32(self.chunk_hashes.len() as u32);
        w.u32(self.tag);
        w.u64_slice(&self.chunk_hashes);
        w.u32_slice(&self.chunk_sizes);
    }
}

/// A parsed stored block (`.lsb`): a block index immediately followed by the
/// opaque payload.
///
/// The payload's `[uncompressed u32][compressed u32][compressed bytes]` framing
/// for `tag != 0` belongs to the compression layer; at this layer the
/// payload is kept as opaque bytes = everything after the block-index region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlock {
    pub block_index: BlockIndex,
    /// Everything after the block-index region. For `tag == 0` this is the raw
    /// concatenated chunk bytes (`Σ chunk_sizes`); for `tag != 0` it is the
    /// compressed frame.
    pub payload: Vec<u8>,
}

impl StoredBlock {
    /// Parse a `.lsb` buffer: the block index followed by the payload (the tail
    /// is the payload, so oversize buffers are not rejected — the payload can be
    /// any length, including 0).
    pub fn from_bytes(data: &[u8]) -> Result<StoredBlock, FormatError> {
        let (block_index, consumed) = BlockIndex::read_prefix(data)?;
        let payload = data[consumed..].to_vec();
        // For an uncompressed block the payload *is* the concatenated chunks, so
        // it must be long enough to cover them: the apply path builds
        // `(offset, size)` pairs straight from `chunk_sizes` and slices the
        // payload with them, which panics on a short block (format-spec §3,
        // "m_Tag == 0 → payload length is Σ m_ChunkSizes").
        //
        // Deliberately `<`, not `!=`: C derives the payload size from the file
        // length and ignores a longer tail, so rejecting an oversize payload
        // would refuse blocks a real store may contain.
        if block_index.tag == 0 {
            let required: u64 = block_index.chunk_sizes.iter().map(|&s| u64::from(s)).sum();
            if (payload.len() as u64) < required {
                return Err(FormatError::Truncated {
                    expected: consumed.saturating_add(required as usize),
                    actual: data.len(),
                });
            }
        }
        Ok(StoredBlock {
            block_index,
            payload,
        })
    }

    /// Serialize to a `.lsb` byte buffer: block-index region then payload.
    pub fn to_bytes(&self) -> Vec<u8> {
        let n = self.block_index.chunk_hashes.len();
        let cap = BlockIndex::data_size(n)
            .map(|d| d.saturating_add(self.payload.len()))
            .unwrap_or(HEADER_SIZE);
        let mut w = Writer::with_capacity(cap);
        self.block_index.write_to(&mut w);
        w.bytes(&self.payload);
        w.into_vec()
    }
}
