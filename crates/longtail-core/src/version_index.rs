//! VersionIndex (`.lvi`) codec (`docs/format-spec.md` §1).
//!
//! Verified against `Longtail_GetVersionIndexDataSize` (longtail.c:2552) and
//! `InitVersionIndexFromData` (longtail.c:2606).

use crate::cursor::{Reader, Writer, checked_add, checked_mul};
use crate::error::FormatError;
use crate::perms::Permissions;

/// The single supported on-disk version (`LONGTAIL_VERSION(0,0,2)`).
pub const VERSION: u32 = 0x0000_0002;

/// Fixed header: 6 × u32.
const HEADER_SIZE: usize = 6 * 4;

/// A parsed `.lvi` version index.
///
/// **Round-trip fidelity beats normalization.** The struct preserves exactly
/// what was parsed: the raw `name_data` blob and `name_offsets` are kept
/// verbatim, never re-derived. A wild file with non-canonical (but internally
/// consistent) offsets must still re-serialize byte-identically (compat gate ①).
/// Path *accessors* ([`VersionIndex::path`]) decode strings out of the blob on
/// demand; the canonical blob is only built by [`crate::FileInfos`] when
/// constructing a new index.
///
/// The `version` field is intentionally not stored: the reader rejects any value
/// other than [`VERSION`], so [`VersionIndex::to_bytes`] always writes that
/// constant.
///
/// All per-asset arrays have length `asset_count` (`A`), `asset_chunk_indexes`
/// has length `asset_chunk_index_count` (`ACI`), and all chunk arrays have
/// length `chunk_count` (`C`). [`VersionIndex::from_bytes`] guarantees these
/// invariants; hand-built structs must uphold them for [`VersionIndex::to_bytes`]
/// to produce a well-formed buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionIndex {
    pub hash_identifier: u32,
    pub target_chunk_size: u32,
    /// `m_PathHashes` — length `A`.
    pub path_hashes: Vec<u64>,
    /// `m_ContentHashes` — length `A`.
    pub content_hashes: Vec<u64>,
    /// `m_AssetSizes` — length `A` (`0` for directories).
    pub asset_sizes: Vec<u64>,
    /// `m_AssetChunkCounts` — length `A`.
    pub asset_chunk_counts: Vec<u32>,
    /// `m_AssetChunkIndexStarts` — length `A`.
    pub asset_chunk_index_starts: Vec<u32>,
    /// `m_AssetChunkIndexes` — length `ACI`.
    pub asset_chunk_indexes: Vec<u32>,
    /// `m_ChunkHashes` — length `C`.
    pub chunk_hashes: Vec<u64>,
    /// `m_ChunkSizes` — length `C`.
    pub chunk_sizes: Vec<u32>,
    /// `m_ChunkTags` — length `C`.
    pub chunk_tags: Vec<u32>,
    /// `m_NameOffsets` — length `A`; byte offsets into `name_data`.
    pub name_offsets: Vec<u32>,
    /// `m_Permissions` — length `A`; all 16 bits preserved.
    pub permissions: Vec<Permissions>,
    /// `m_NameData` — the raw concatenated NUL-terminated path blob (everything
    /// after the fixed arrays, to EOF). Kept verbatim.
    pub name_data: Vec<u8>,
}

impl VersionIndex {
    /// Number of assets (`A`).
    pub fn asset_count(&self) -> u32 {
        self.path_hashes.len() as u32
    }

    /// Number of unique chunks (`C`).
    pub fn chunk_count(&self) -> u32 {
        self.chunk_hashes.len() as u32
    }

    /// Length of the asset→chunk index map (`ACI`).
    pub fn asset_chunk_index_count(&self) -> u32 {
        self.asset_chunk_indexes.len() as u32
    }

    /// The raw path bytes of asset `i` (up to, not including, the NUL). For a
    /// directory this ends with `/`, matching `Longtail_VersionIndex`'s
    /// `m_NameData` and the C `GetPath` accessor.
    ///
    /// Fallible by design: nothing in the format guarantees an in-range offset
    /// or a NUL terminator inside a wild `name_data` blob.
    pub fn path_bytes(&self, i: usize) -> Result<&[u8], FormatError> {
        let count = self.name_offsets.len();
        let offset = *self
            .name_offsets
            .get(i)
            .ok_or(FormatError::IndexOutOfBounds { index: i, count })?
            as usize;
        if offset > self.name_data.len() {
            return Err(FormatError::NameOffsetOutOfBounds {
                offset,
                len: self.name_data.len(),
            });
        }
        let tail = &self.name_data[offset..];
        match tail.iter().position(|&b| b == 0) {
            Some(nul) => Ok(&tail[..nul]),
            None => Err(FormatError::UnterminatedName { offset }),
        }
    }

    /// The path of asset `i` decoded as UTF-8 (directories end with `/`).
    /// Fallible: a wild blob need not be valid UTF-8.
    pub fn path(&self, i: usize) -> Result<&str, FormatError> {
        let bytes = self.path_bytes(i)?;
        let offset = self.name_offsets[i] as usize;
        std::str::from_utf8(bytes).map_err(|_| FormatError::InvalidUtf8 { offset })
    }

    /// Whether asset `i` is a directory (its name ends with `/`).
    pub fn is_dir(&self, i: usize) -> Result<bool, FormatError> {
        Ok(self.path_bytes(i)?.last() == Some(&b'/'))
    }

    /// The byte size of everything up to and including the fixed arrays (i.e.
    /// the offset at which `name_data` starts). Checked against attacker
    /// counts.
    fn fixed_size(a: usize, c: usize, aci: usize) -> Result<usize, FormatError> {
        // 3 × u64[A] + name_offsets u32[A] + asset_chunk_counts u32[A]
        //   + asset_chunk_index_starts u32[A] + permissions u16[A]
        // + asset_chunk_indexes u32[ACI]
        // + chunk_hashes u64[C] + chunk_sizes u32[C] + chunk_tags u32[C]
        let mut total = HEADER_SIZE;
        total = checked_add(total, checked_mul(a, 8)?)?; // path_hashes
        total = checked_add(total, checked_mul(a, 8)?)?; // content_hashes
        total = checked_add(total, checked_mul(a, 8)?)?; // asset_sizes
        total = checked_add(total, checked_mul(a, 4)?)?; // asset_chunk_counts
        total = checked_add(total, checked_mul(a, 4)?)?; // asset_chunk_index_starts
        total = checked_add(total, checked_mul(aci, 4)?)?; // asset_chunk_indexes
        total = checked_add(total, checked_mul(c, 8)?)?; // chunk_hashes
        total = checked_add(total, checked_mul(c, 4)?)?; // chunk_sizes
        total = checked_add(total, checked_mul(c, 4)?)?; // chunk_tags
        total = checked_add(total, checked_mul(a, 4)?)?; // name_offsets
        total = checked_add(total, checked_mul(a, 2)?)?; // permissions
        Ok(total)
    }

    /// Parse a `.lvi` buffer. The trailing bytes after the fixed arrays are the
    /// `name_data` blob (possibly empty), so — unlike StoreIndex/BlockIndex —
    /// oversize buffers are not rejected; the remainder is `name_data`.
    pub fn from_bytes(data: &[u8]) -> Result<VersionIndex, FormatError> {
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
        let target_chunk_size = r.u32()?;
        let asset_count = r.u32()?;
        let chunk_count = r.u32()?;
        let asset_chunk_index_count = r.u32()?;

        // Deliberate Rust-side strictness beyond C (see FormatError doc).
        if asset_chunk_index_count < chunk_count {
            return Err(FormatError::InvalidAssetChunkIndexCount {
                asset_chunk_index_count,
                chunk_count,
            });
        }

        let a = asset_count as usize;
        let c = chunk_count as usize;
        let aci = asset_chunk_index_count as usize;

        // Truncation check BEFORE reading arrays (matches C's up-front compare).
        let fixed = Self::fixed_size(a, c, aci)?;
        if data.len() < fixed {
            return Err(FormatError::Truncated {
                expected: fixed,
                actual: data.len(),
            });
        }

        let path_hashes = r.u64_vec(a)?;
        let content_hashes = r.u64_vec(a)?;
        let asset_sizes = r.u64_vec(a)?;
        let asset_chunk_counts = r.u32_vec(a)?;
        let asset_chunk_index_starts = r.u32_vec(a)?;
        let asset_chunk_indexes = r.u32_vec(aci)?;
        let chunk_hashes = r.u64_vec(c)?;
        let chunk_sizes = r.u32_vec(c)?;
        let chunk_tags = r.u32_vec(c)?;
        let name_offsets = r.u32_vec(a)?;
        let permissions = r.u16_vec(a)?.into_iter().map(Permissions).collect();
        // name_data = everything remaining (by definition; may be empty).
        let name_data = r.remaining().to_vec();

        // Validate the asset→chunk map here, once, rather than in every consumer
        // that walks it — they index it directly, and an out-of-range entry has
        // no handler anywhere, only a panic. Both indices are u32s straight from
        // the file, and on a 32-bit target `start + count` would *wrap* into a
        // small in-bounds value rather than panic, which is worse: a silently
        // wrong answer. C reads out of bounds here, so there is no defined
        // behaviour to preserve. O(A + ACI) integer compares over a buffer that
        // is already resident.
        for (asset, (&start, &count)) in asset_chunk_index_starts
            .iter()
            .zip(asset_chunk_counts.iter())
            .enumerate()
        {
            let end = (start as usize)
                .checked_add(count as usize)
                .ok_or(FormatError::SizeOverflow)?;
            if end > aci {
                return Err(FormatError::AssetChunkRangeOutOfBounds {
                    asset,
                    start,
                    count,
                    len: aci,
                });
            }
        }
        if let Some((position, &index)) = asset_chunk_indexes
            .iter()
            .enumerate()
            .find(|(_, i)| **i as usize >= c)
        {
            return Err(FormatError::AssetChunkIndexOutOfBounds {
                position,
                index,
                chunk_count: c,
            });
        }

        Ok(VersionIndex {
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
            permissions,
            name_data,
        })
    }

    /// Serialize to a `.lvi` byte buffer. Pure function of the parsed content;
    /// writes the header (with counts derived from the array lengths), the fixed
    /// arrays in order, then the raw `name_data` blob.
    pub fn to_bytes(&self) -> Vec<u8> {
        let a = self.path_hashes.len();
        let c = self.chunk_hashes.len();
        let aci = self.asset_chunk_indexes.len();
        let cap = Self::fixed_size(a, c, aci)
            .map(|f| f.saturating_add(self.name_data.len()))
            .unwrap_or(HEADER_SIZE);
        let mut w = Writer::with_capacity(cap);
        w.u32(VERSION);
        w.u32(self.hash_identifier);
        w.u32(self.target_chunk_size);
        w.u32(a as u32);
        w.u32(c as u32);
        w.u32(aci as u32);
        w.u64_slice(&self.path_hashes);
        w.u64_slice(&self.content_hashes);
        w.u64_slice(&self.asset_sizes);
        w.u32_slice(&self.asset_chunk_counts);
        w.u32_slice(&self.asset_chunk_index_starts);
        w.u32_slice(&self.asset_chunk_indexes);
        w.u64_slice(&self.chunk_hashes);
        w.u32_slice(&self.chunk_sizes);
        w.u32_slice(&self.chunk_tags);
        w.u32_slice(&self.name_offsets);
        for p in &self.permissions {
            w.u16(p.bits());
        }
        w.bytes(&self.name_data);
        w.into_vec()
    }
}
