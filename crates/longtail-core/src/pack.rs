//! Block packing — `CreateStoreIndex` + `CreateMissingContent` (the upsync
//! compat center). Pure, I/O-free: given a version index's chunk hashes/sizes/tags
//! it produces the [`StoreIndex`] describing the blocks to write.
//!
//! Verified against `Longtail_CreateMissingContent` (longtail.c:6882),
//! `Longtail_CreateStoreIndex` (longtail.c:6745, greedy fill loop :6801-6860),
//! `DiffHashes` (longtail.c:6620, reorder-to-version-order :6716-6741),
//! `GetUniqueHashes` (longtail.c:4307), and `Longtail_CreateBlockIndex`
//! (longtail.c:3712 — block hash = `HashBuffer` over the in-block chunk-hash
//! array **in packing order**).

use std::collections::HashSet;

use crate::block::BlockIndex;
use crate::error::FormatError;
use crate::hash::Hash;
use crate::store_index::StoreIndex;
use crate::version_index::VersionIndex;

/// The longtail block hash: `HashBuffer` over the block's chunk-hash array
/// serialized as little-endian `u64`s **in packing order** (`m_ChunkHashes`
/// bytes, longtail.c:3757). Identical byte layout to a version index's
/// per-asset content hash (`build::assemble_version_index`).
pub fn block_hash<H: Hash + ?Sized>(hasher: &H, chunk_hashes: &[u64]) -> u64 {
    let mut buf = Vec::with_capacity(chunk_hashes.len() * 8);
    for &h in chunk_hashes {
        buf.extend_from_slice(&h.to_le_bytes());
    }
    hasher.hash(&buf)
}

/// `GetUniqueHashes` (longtail.c:4307): return the indexes of the unique hashes
/// in **first-occurrence order**. For a duplicate hash C overwrites the stored
/// index with the *last* occurrence ("Take the last chunk hash we find",
/// longtail.c:4335); since a longtail chunk hash uniquely determines its content
/// (and thus its size), the first/last choice does not affect the packed output,
/// but the exact behavior is reproduced for fidelity.
fn unique_hash_indexes(hashes: &[u64]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut pos: std::collections::HashMap<u64, usize> =
        std::collections::HashMap::with_capacity(hashes.len());
    for (i, &h) in hashes.iter().enumerate() {
        match pos.get(&h) {
            Some(&p) => out[p] = i as u32, // duplicate: keep last-occurrence index
            None => {
                pos.insert(h, out.len());
                out.push(i as u32);
            }
        }
    }
    out
}

/// `Longtail_CreateStoreIndex` (longtail.c:6745) — the greedy block-fill loop
/// (:6801-6860). Groups the (already order-significant) chunk list into blocks:
/// a block closes when the next chunk's `tag` differs, when the block reaches
/// `max_chunks_per_block` chunks, or when adding the next chunk would exceed
/// `max_block_size + max_block_size/10` (10% overshoot). The first chunk of a
/// block is always taken regardless of size. Each block's hash is
/// [`block_hash`] over its chunk hashes in packing order.
///
/// `chunk_sizes` and `chunk_tags` are parallel to `chunk_hashes`; an empty
/// `chunk_tags` means tag `0` for every chunk (`optional_chunk_tags == NULL`).
/// `chunk_count == 0` yields an empty store index (identifier `0`, longtail.c:6774).
pub fn create_store_index<H: Hash + ?Sized>(
    chunk_hashes: &[u64],
    chunk_sizes: &[u32],
    chunk_tags: &[u32],
    max_block_size: u32,
    max_chunks_per_block: u32,
    hasher: &H,
) -> Result<StoreIndex, FormatError> {
    if chunk_hashes.is_empty() {
        return Ok(StoreIndex::empty(0));
    }
    let tag_of = |chunk_index: usize| -> u32 { chunk_tags.get(chunk_index).copied().unwrap_or(0) };
    let size_of = |chunk_index: usize| -> u32 { chunk_sizes[chunk_index] };
    let max_chunks_per_block = max_chunks_per_block.max(1);
    // Overshoot allowance: max_block_size + max_block_size/10 (longtail.c:6829).
    let size_limit = (max_block_size as u64) + (max_block_size as u64) / 10;

    let unique = unique_hash_indexes(chunk_hashes);
    let unique_count = unique.len();

    let mut blocks: Vec<BlockIndex> = Vec::new();
    let mut i = 0usize;
    while i < unique_count {
        let first = unique[i] as usize;
        let current_tag = tag_of(first);
        let mut current_size = size_of(first) as u64;
        let mut stored: Vec<usize> = vec![first];

        while i + 1 < unique_count {
            let next = unique[i + 1] as usize;
            let next_size = size_of(next);
            let next_tag = tag_of(next);
            if next_tag != current_tag {
                break;
            }
            if stored.len() as u32 == max_chunks_per_block {
                break;
            }
            if current_size + next_size as u64 > size_limit {
                break;
            }
            current_size += next_size as u64;
            stored.push(next);
            i += 1;
        }

        let bh: Vec<u64> = stored.iter().map(|&c| chunk_hashes[c]).collect();
        let bs: Vec<u32> = stored.iter().map(|&c| chunk_sizes[c]).collect();
        blocks.push(BlockIndex {
            block_hash: block_hash(hasher, &bh),
            hash_identifier: hasher.id(),
            tag: current_tag,
            chunk_hashes: bh,
            chunk_sizes: bs,
        });
        i += 1;
    }

    StoreIndex::from_block_indexes(&blocks)
}

/// `Longtail_CreateMissingContent` (longtail.c:6882): diff the version's chunk
/// hashes against those already covered by `store_index`, then pack the missing
/// chunks (in **version-index chunk order**, matching `DiffHashes`'
/// reorder-to-version-order step, longtail.c:6716-6741) into blocks via
/// [`create_store_index`].
///
/// A version index's chunk arrays are already deduplicated, so the diff reduces
/// to "version chunks not present in the store"; the sorted-set machinery of C's
/// `DiffHashes` produces the same *set*, and its reorder step restores exactly
/// this version-index order.
pub fn create_missing_content<H: Hash + ?Sized>(
    hasher: &H,
    store_index: &StoreIndex,
    version_index: &VersionIndex,
    max_block_size: u32,
    max_chunks_per_block: u32,
) -> Result<StoreIndex, FormatError> {
    let present: HashSet<u64> = store_index.chunk_hashes.iter().copied().collect();

    let mut added_hashes: Vec<u64> = Vec::new();
    let mut added_sizes: Vec<u32> = Vec::new();
    let mut added_tags: Vec<u32> = Vec::new();
    for (i, &h) in version_index.chunk_hashes.iter().enumerate() {
        if present.contains(&h) {
            continue;
        }
        added_hashes.push(h);
        added_sizes.push(version_index.chunk_sizes[i]);
        added_tags.push(version_index.chunk_tags.get(i).copied().unwrap_or(0));
    }

    if added_hashes.is_empty() {
        return Ok(StoreIndex::empty(0));
    }

    create_store_index(
        &added_hashes,
        &added_sizes,
        &added_tags,
        max_block_size,
        max_chunks_per_block,
        hasher,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Blake3;

    fn si_from(chunks: &[(u64, u32, u32)], max_bs: u32, max_cpb: u32) -> StoreIndex {
        let h: Vec<u64> = chunks.iter().map(|c| c.0).collect();
        let s: Vec<u32> = chunks.iter().map(|c| c.1).collect();
        let t: Vec<u32> = chunks.iter().map(|c| c.2).collect();
        create_store_index(&h, &s, &t, max_bs, max_cpb, &Blake3).unwrap()
    }

    #[test]
    fn empty_input_is_empty_index() {
        let si = create_store_index(&[], &[], &[], 1024, 8, &Blake3).unwrap();
        assert_eq!(si.block_count(), 0);
        assert_eq!(si.hash_identifier, 0);
    }

    #[test]
    fn single_block_groups_same_tag() {
        // Three small chunks, same tag, well under the block size → one block.
        let si = si_from(&[(1, 10, 5), (2, 10, 5), (3, 10, 5)], 8 * 1024 * 1024, 1024);
        assert_eq!(si.block_count(), 1);
        assert_eq!(si.chunk_count(), 3);
        assert_eq!(si.block_chunk_counts[0], 3);
        assert_eq!(si.chunk_hashes, vec![1, 2, 3]);
    }

    #[test]
    fn tag_change_closes_block() {
        let si = si_from(&[(1, 10, 5), (2, 10, 7), (3, 10, 7)], 8 * 1024 * 1024, 1024);
        assert_eq!(si.block_count(), 2);
        assert_eq!(si.block_chunk_counts, vec![1, 2]);
        assert_eq!(si.block_tags, vec![5, 7]);
    }

    #[test]
    fn max_chunks_per_block_closes_block() {
        let si = si_from(
            &[(1, 1, 0), (2, 1, 0), (3, 1, 0), (4, 1, 0)],
            8 * 1024 * 1024,
            2,
        );
        assert_eq!(si.block_count(), 2);
        assert_eq!(si.block_chunk_counts, vec![2, 2]);
    }

    #[test]
    fn size_overshoot_closes_block() {
        // max_block_size = 100 → limit = 110. First chunk 100, next 20 → 120 > 110
        // → close after the first (first chunk always taken).
        let si = si_from(&[(1, 100, 0), (2, 20, 0), (3, 20, 0)], 100, 1024);
        assert_eq!(si.block_count(), 2);
        assert_eq!(si.block_chunk_counts, vec![1, 2]); // 100 | 20+20
    }

    #[test]
    fn first_chunk_always_taken_even_if_oversize() {
        // A single chunk larger than the block size still forms one block.
        let si = si_from(&[(1, 1000, 0)], 100, 1024);
        assert_eq!(si.block_count(), 1);
        assert_eq!(si.block_chunk_counts, vec![1]);
    }

    #[test]
    fn block_hash_is_over_chunk_hashes_in_order() {
        let si = si_from(&[(0x11, 4, 0), (0x22, 4, 0)], 8 * 1024 * 1024, 1024);
        let expected = block_hash(&Blake3, &[0x11, 0x22]);
        assert_eq!(si.block_hashes[0], expected);
    }
}
