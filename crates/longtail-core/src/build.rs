//! `CreateVersionIndex` pipeline + `MergeVersionIndex` (`docs/format-spec.md` §1;
//! `Longtail_CreateVersionIndex`/`Longtail_BuildVersionIndex`/`ChunkAssets`,
//! longtail.c:2343-3017; `Longtail_MergeVersionIndex`, longtail.c:3059-3330).
//!
//! Pure, I/O-free: the caller (the `longtail` facade's fs helpers) scans the
//! folder and reads each asset's bytes; this module chunks + hashes + assembles.
//! The chunk-job **range split** is byte-gate-critical and is reproduced exactly
//! from C (see [`chunk_asset`]).

use std::collections::HashMap;

use crate::chunker::{ChunkerError, HpcdcChunker};
use crate::file_infos::FileInfos;
use crate::hash::Hash;
use crate::perms::Permissions;
use crate::version_index::VersionIndex;

/// Chunk one asset's bytes exactly as C's `ChunkAssets`/`DynamicChunking`
/// (longtail.c:2343-2311): split the asset into **independent ranges** of
/// `max_hash_size = target_chunk_size * 1024` bytes, chunk each range with a
/// **fresh chunker session** (boundaries reset at range boundaries), and hash
/// each resulting chunk's bytes. A trailing zero-size range yields zero chunks.
///
/// `chunker` must be `HpcdcChunker::from_target(target_chunk_size)`; `max_hash_size`
/// must be `(target_chunk_size as u64) * 1024` (passed in so the caller builds
/// both once and chunks assets in parallel). Returns `(chunk_hash, chunk_size)`
/// per chunk in asset order (range-major, then chunk-order within a range).
///
/// The per-range ≤ min-chunk-size single-chunk shortcut (C: `hash_size <=
/// chunker_min_size`, longtail.c:2051) is subsumed by the streaming chunker's own
/// `left <= min` tail rule: feeding a range slice shorter than `min` (>= 48)
/// emits exactly one chunk covering it, identical to C's single-read path.
pub fn chunk_asset<H: Hash + ?Sized>(
    bytes: &[u8],
    chunker: &HpcdcChunker,
    max_hash_size: u64,
    hasher: &H,
) -> Vec<(u64, u32)> {
    let max_hash_size = max_hash_size.max(1);
    let asset_size = bytes.len() as u64;
    // asset_part_count = 1 + size / max_hash_size (longtail.c:2402/:2432).
    let asset_part_count = 1 + asset_size / max_hash_size;
    let mut out = Vec::new();
    let mut part: u64 = 0;
    while part < asset_part_count {
        let range_start = part * max_hash_size; // :2436
        let remaining = asset_size.saturating_sub(range_start);
        let job_size = remaining.min(max_hash_size); // :2437
        if job_size != 0 {
            let start = range_start as usize;
            let end = start + job_size as usize;
            chunker.chunk_with(&bytes[start..end], |span| {
                let s = span.offset as usize;
                let e = s + span.size as usize;
                out.push((hasher.hash(&bytes[start + s..start + e]), span.size));
            });
        }
        part += 1;
    }
    out
}

/// Assemble a [`VersionIndex`] from already-chunked assets, exactly as
/// `Longtail_BuildVersionIndex` (longtail.c:2709-3017): first-occurrence chunk
/// dedup keeping the first occurrence's size+tag, per-asset content hash over the
/// concatenated chunk-hash `u64`s, path hash over the path bytes (incl. a
/// directory's trailing `/`), and the FileInfos-order asset layout.
///
/// `per_asset_chunks[a]` is asset `a`'s `(chunk_hash, chunk_size)` list in order
/// (must align with `file_infos` asset order). `asset_tags` is the per-asset
/// compression tag (golongtail feeds a **uniform** array — every asset the single
/// chosen compression ID); `None` zero-fills `m_ChunkTags` (validate rescan).
pub fn assemble_version_index<H: Hash + ?Sized>(
    file_infos: &FileInfos,
    per_asset_chunks: &[Vec<(u64, u32)>],
    hasher: &H,
    target_chunk_size: u32,
    asset_tags: Option<&[u32]>,
) -> VersionIndex {
    let asset_count = file_infos.count() as usize;

    let mut path_hashes = Vec::with_capacity(asset_count);
    let mut content_hashes = Vec::with_capacity(asset_count);
    let mut asset_chunk_counts = Vec::with_capacity(asset_count);
    let mut asset_chunk_index_starts = Vec::with_capacity(asset_count);
    let mut asset_chunk_indexes: Vec<u32> = Vec::new();

    // Compact (deduped) chunk arrays, first-occurrence order.
    let mut chunk_hashes: Vec<u64> = Vec::new();
    let mut chunk_sizes: Vec<u32> = Vec::new();
    let mut chunk_tags: Vec<u32> = Vec::new();
    let mut lut: HashMap<u64, u32> = HashMap::new();

    let mut flat_offset: u32 = 0;
    for a in 0..asset_count {
        // Path hash over exactly strlen(path) bytes (dir paths include trailing
        // `/`; no NUL) — longtail.c:1272/:1296.
        let path_bytes = file_infos.path_bytes(a).unwrap_or(&[]);
        path_hashes.push(hasher.hash(path_bytes));

        let chunks = per_asset_chunks.get(a).map(|v| v.as_slice()).unwrap_or(&[]);

        // Content hash over the asset's concatenated chunk-hash u64s (LE bytes,
        // matching x86 native memory order); empty buffer for zero-chunk assets
        // (longtail.c:2521-2522).
        let mut content_buf = Vec::with_capacity(chunks.len() * 8);
        for &(h, _) in chunks {
            content_buf.extend_from_slice(&h.to_le_bytes());
        }
        content_hashes.push(hasher.hash(&content_buf));

        asset_chunk_index_starts.push(flat_offset);
        asset_chunk_counts.push(chunks.len() as u32);

        let tag = asset_tags.and_then(|t| t.get(a).copied()).unwrap_or(0);
        for &(h, sz) in chunks {
            let compact = *lut.entry(h).or_insert_with(|| {
                let idx = chunk_hashes.len() as u32;
                chunk_hashes.push(h);
                chunk_sizes.push(sz);
                chunk_tags.push(tag);
                idx
            });
            asset_chunk_indexes.push(compact);
            flat_offset += 1;
        }
    }

    VersionIndex {
        hash_identifier: hasher.id(),
        target_chunk_size,
        path_hashes,
        content_hashes,
        asset_sizes: file_infos.sizes.clone(),
        asset_chunk_counts,
        asset_chunk_index_starts,
        asset_chunk_indexes,
        chunk_hashes,
        chunk_sizes,
        chunk_tags,
        name_offsets: file_infos.path_start_offsets.clone(),
        permissions: file_infos.permissions.clone(),
        name_data: file_infos.path_data.clone(),
    }
}

/// Convenience: chunk every asset then assemble. `asset_contents[a]` is asset
/// `a`'s raw bytes (empty for directories/empty files), aligned with
/// `file_infos`. The facade parallelizes the [`chunk_asset`] step on its rayon
/// pool and calls [`assemble_version_index`] directly; this convenience is for
/// tests and small inputs.
pub fn create_version_index<H: Hash + ?Sized>(
    file_infos: &FileInfos,
    asset_contents: &[Vec<u8>],
    hasher: &H,
    target_chunk_size: u32,
    asset_tags: Option<&[u32]>,
) -> Result<VersionIndex, ChunkerError> {
    let chunker = HpcdcChunker::from_target(target_chunk_size)?;
    let max_hash_size = (target_chunk_size as u64).saturating_mul(1024);
    let per_asset: Vec<Vec<(u64, u32)>> = asset_contents
        .iter()
        .map(|bytes| chunk_asset(bytes, &chunker, max_hash_size, hasher))
        .collect();
    Ok(assemble_version_index(
        file_infos,
        &per_asset,
        hasher,
        target_chunk_size,
        asset_tags,
    ))
}

/// Errors from [`merge_version_index`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MergeVersionError {
    /// The two indexes disagree on `hash_identifier` or `target_chunk_size`
    /// (longtail.c:3072-3073 precondition).
    #[error("cannot merge version indexes: {field} mismatch ({base} vs {overlay})")]
    Mismatch {
        field: &'static str,
        base: u32,
        overlay: u32,
    },
}

/// One asset's full record, extracted from a version index for merging.
struct AssetRecord {
    path: Vec<u8>,
    content_hash: u64,
    size: u64,
    permissions: Permissions,
    // (chunk_hash, chunk_size, chunk_tag)
    chunks: Vec<(u64, u32, u32)>,
}

fn extract_record(vi: &VersionIndex, ai: usize) -> AssetRecord {
    let start = vi.asset_chunk_index_starts[ai] as usize;
    let count = vi.asset_chunk_counts[ai] as usize;
    let mut chunks = Vec::with_capacity(count);
    for k in 0..count {
        let cidx = vi.asset_chunk_indexes[start + k] as usize;
        chunks.push((
            vi.chunk_hashes[cidx],
            vi.chunk_sizes[cidx],
            vi.chunk_tags[cidx],
        ));
    }
    AssetRecord {
        path: vi.path_bytes(ai).unwrap_or(&[]).to_vec(),
        content_hash: vi.content_hashes[ai],
        size: vi.asset_sizes[ai],
        permissions: vi.permissions[ai],
        chunks,
    }
}

/// Merge two version indexes (`Longtail_MergeVersionIndex`, longtail.c:3059).
/// **Overlay wins**: for any path present in both, the overlay's content, size,
/// permissions, and chunks completely replace the base's (longtail.c:3211); the
/// base contributes only paths the overlay lacks. Assets are re-sorted
/// short-to-long by path byte length (longtail.c:3258, parents before children).
///
/// The output is a well-formed version index; ordering is deterministic but the
/// chunk-array layout is not held byte-identical to C (the download path consumes
/// it structurally — the multi-source scenarios gate on the resulting tree, not
/// the merged bytes).
pub fn merge_version_index(
    base: &VersionIndex,
    overlay: &VersionIndex,
) -> Result<VersionIndex, MergeVersionError> {
    if base.hash_identifier != overlay.hash_identifier {
        return Err(MergeVersionError::Mismatch {
            field: "hash_identifier",
            base: base.hash_identifier,
            overlay: overlay.hash_identifier,
        });
    }
    if base.target_chunk_size != overlay.target_chunk_size {
        return Err(MergeVersionError::Mismatch {
            field: "target_chunk_size",
            base: base.target_chunk_size,
            overlay: overlay.target_chunk_size,
        });
    }

    // Build the union of assets by path hash — base first, then overlay-only
    // (overlay wins for shared paths). `records` holds (path_hash, record).
    let mut order: Vec<u64> = Vec::new(); // path hashes in union order
    let mut records: HashMap<u64, AssetRecord> = HashMap::new();

    for ai in 0..base.asset_count() as usize {
        let ph = base.path_hashes[ai];
        if records.insert(ph, extract_record(base, ai)).is_none() {
            order.push(ph);
        }
    }
    for ai in 0..overlay.asset_count() as usize {
        let ph = overlay.path_hashes[ai];
        let existed = records.insert(ph, extract_record(overlay, ai)).is_some();
        if !existed {
            order.push(ph);
        }
    }

    // Sort short-to-long by path byte length, tie-broken by union position.
    let mut pos: HashMap<u64, usize> = HashMap::new();
    for (i, ph) in order.iter().enumerate() {
        pos.insert(*ph, i);
    }
    order.sort_by(|a, b| {
        let la = records[a].path.len();
        let lb = records[b].path.len();
        la.cmp(&lb).then(pos[a].cmp(&pos[b]))
    });

    // Assemble: rebuild the name blob + dedup chunks in asset order.
    let mut path_hashes = Vec::with_capacity(order.len());
    let mut content_hashes = Vec::with_capacity(order.len());
    let mut asset_sizes = Vec::with_capacity(order.len());
    let mut permissions = Vec::with_capacity(order.len());
    let mut name_offsets = Vec::with_capacity(order.len());
    let mut name_data: Vec<u8> = Vec::new();
    let mut asset_chunk_counts = Vec::with_capacity(order.len());
    let mut asset_chunk_index_starts = Vec::with_capacity(order.len());
    let mut asset_chunk_indexes: Vec<u32> = Vec::new();
    let mut chunk_hashes: Vec<u64> = Vec::new();
    let mut chunk_sizes: Vec<u32> = Vec::new();
    let mut chunk_tags: Vec<u32> = Vec::new();
    let mut lut: HashMap<u64, u32> = HashMap::new();

    let mut flat_offset: u32 = 0;
    for ph in &order {
        let rec = &records[ph];
        path_hashes.push(*ph);
        content_hashes.push(rec.content_hash);
        asset_sizes.push(rec.size);
        permissions.push(rec.permissions);
        name_offsets.push(name_data.len() as u32);
        name_data.extend_from_slice(&rec.path);
        name_data.push(0);
        asset_chunk_index_starts.push(flat_offset);
        asset_chunk_counts.push(rec.chunks.len() as u32);
        for &(h, sz, tag) in &rec.chunks {
            let compact = *lut.entry(h).or_insert_with(|| {
                let idx = chunk_hashes.len() as u32;
                chunk_hashes.push(h);
                chunk_sizes.push(sz);
                chunk_tags.push(tag);
                idx
            });
            asset_chunk_indexes.push(compact);
            flat_offset += 1;
        }
    }

    Ok(VersionIndex {
        hash_identifier: base.hash_identifier,
        target_chunk_size: base.target_chunk_size,
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
