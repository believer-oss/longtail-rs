//! Folder scan → `CreateVersionIndex`, with parallel per-asset chunking on a
//! rayon pool. Sync (no tokio); this is the target-scan + byte-gate entry point.

use std::path::Path;

use longtail_core::{
    FileInfos, Hash, HpcdcChunker, VersionIndex, assemble_version_index, chunk_asset,
};
use rayon::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::error::LongtailError;
use crate::fs_util;
use crate::path_filter::RegexPathFilter;

/// Scan `root` (honoring `filter`) and build a [`VersionIndex`] with the C
/// range-split chunking, hashing with `hasher`. `compression_tag` is the uniform
/// per-asset compression id (`0` = no compression / target-scan / validate;
/// a real id for the byte-gate upsync-equivalent). Chunking runs in parallel on
/// `pool`; assembly is deterministic (FileInfos order).
pub fn create_version_index_from_folder<H: Hash + Sync + ?Sized>(
    root: &Path,
    filter: &RegexPathFilter,
    hasher: &H,
    target_chunk_size: u32,
    compression_tag: u32,
    pool: &rayon::ThreadPool,
    cancel: &CancellationToken,
) -> Result<VersionIndex, LongtailError> {
    // Scan, then sort entries with FileInfos' exact byte-wise order so the
    // per-asset chunk lists align 1:1 with the assembled asset order.
    let mut entries = fs_util::scan_folder(root, filter)?;
    entries.sort_by(|a, b| a.relative_path.as_bytes().cmp(b.relative_path.as_bytes()));
    let file_infos = FileInfos::from_scanned_entries(entries.clone());

    let chunker = HpcdcChunker::from_target(target_chunk_size)?;
    let max_hash_size = (target_chunk_size as u64).saturating_mul(1024);

    // Parallel read+chunk per asset, preserving order (collect into a Vec).
    let per_asset: Vec<Vec<(u64, u32)>> = pool.install(|| {
        entries
            .par_iter()
            .map(|entry| -> Result<Vec<(u64, u32)>, LongtailError> {
                if cancel.is_cancelled() {
                    return Err(LongtailError::Cancelled);
                }
                if entry.is_dir {
                    return Ok(Vec::new());
                }
                let bytes = fs_util::read_asset(root, &entry.relative_path, false)?;
                Ok(chunk_asset(&bytes, &chunker, max_hash_size, hasher))
            })
            .collect::<Result<Vec<_>, _>>()
    })?;

    let tags = vec![compression_tag; per_asset.len()];
    Ok(assemble_version_index(
        &file_infos,
        &per_asset,
        hasher,
        target_chunk_size,
        Some(&tags),
    ))
}

/// Build a rayon pool sized by `worker_count` (`0` = logical CPUs). Shared by
/// the download and upload orchestration.
pub(crate) fn build_pool(worker_count: usize) -> Result<rayon::ThreadPool, LongtailError> {
    let n = if worker_count == 0 {
        num_cpus::get().max(1)
    } else {
        worker_count
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .build()
        .map_err(|e| LongtailError::InvalidArgument(format!("failed to build rayon pool: {e}")))
}
