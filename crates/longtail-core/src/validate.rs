//! `ValidateStore` — does a store index cover a version index?
//! (`Longtail_ValidateStore`, longtail.c:9423-9505).

use std::collections::HashSet;

use crate::store_index::StoreIndex;
use crate::version_index::VersionIndex;

/// Why a store index fails to satisfy a version index. `EINVAL` (size mismatch)
/// takes precedence over `ENOENT` (missing chunk) — longtail.c:9487-9500.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValidateError {
    /// One or more non-directory assets have Σ(chunk sizes) ≠ asset size
    /// (`EINVAL`-equivalent). Wins over [`ValidateError::MissingChunks`].
    #[error("store does not match version: {count} asset(s) with size mismatch")]
    SizeMismatch { count: u32 },
    /// One or more of the version's chunk hashes are absent from the store index
    /// (`ENOENT`-equivalent).
    #[error("store is missing content: {count} chunk(s) not in the store index")]
    MissingChunks { count: u32 },
}

/// Validate that `store_index` can materialize `version_index`
/// (`Longtail_ValidateStore`). Checks, in C's precedence:
/// 1. every `version_index` chunk hash is present in `store_index`
///    (`ENOENT`); and
/// 2. every non-dir asset's Σ(chunk sizes) equals its asset size (`EINVAL`).
///
/// Returns `SizeMismatch` if any size check fails (even alongside missing
/// chunks); else `MissingChunks` if any chunk is absent; else `Ok(())`.
pub fn validate_store(
    store_index: &StoreIndex,
    version_index: &VersionIndex,
) -> Result<(), ValidateError> {
    let present: HashSet<u64> = store_index.chunk_hashes.iter().copied().collect();

    // Missing-chunk check over the version index's own chunk-hash list.
    let mut missing: u32 = 0;
    for &h in &version_index.chunk_hashes {
        if !present.contains(&h) {
            missing += 1;
        }
    }

    // Per-asset size check (dirs exempt).
    let mut size_mismatch: u32 = 0;
    for a in 0..version_index.asset_count() as usize {
        let is_dir = version_index.is_dir(a).unwrap_or(false);
        if is_dir {
            continue;
        }
        let start = version_index.asset_chunk_index_starts[a] as usize;
        let count = version_index.asset_chunk_counts[a] as usize;
        let mut summed: u64 = 0;
        for k in 0..count {
            let cidx = version_index.asset_chunk_indexes[start + k] as usize;
            summed = summed.wrapping_add(version_index.chunk_sizes[cidx] as u64);
        }
        if summed != version_index.asset_sizes[a] {
            size_mismatch += 1;
        }
    }

    if size_mismatch > 0 {
        return Err(ValidateError::SizeMismatch {
            count: size_mismatch,
        });
    }
    if missing > 0 {
        return Err(ValidateError::MissingChunks { count: missing });
    }
    Ok(())
}
