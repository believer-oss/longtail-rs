//! `ChangeVersion2` apply flow, in C's order (`Longtail_ChangeVersion2`,
//! longtail.c:8720-8911): mkdir → preflight(all) → **deletes FIRST** (long-to-short,
//! 10-retry) → zero-size/dir + per-block positional writes (first touch truncates
//! to final size) → **permissions LAST** (only when `retain_permissions`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use longtail_core::{StoreIndex, VersionDiff, VersionIndex};
use longtail_store::block_store::BlockStore;
use tokio_util::sync::CancellationToken;

use crate::error::LongtailError;
use crate::fs_util;
use crate::progress::RateLimited;

/// Byte/asset counters produced by an apply.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ApplyStats {
    pub bytes_written: u64,
    pub assets_written: u32,
    pub assets_removed: u32,
}

/// One positional chunk write inside a block.
struct BlockWrite {
    rel: String,
    asset_offset: u64,
    chunk_hash: u64,
    chunk_size: u32,
}

/// A zero-chunk write asset (empty file or directory).
struct ZeroAsset {
    rel: String,
    is_dir: bool,
}

fn asset_path(vi: &VersionIndex, idx: u32) -> Result<String, LongtailError> {
    let p = vi.path(idx as usize)?;
    Ok(fs_util::strip_trailing_slash(p).to_string())
}

/// Apply `diff` (turning `current` into `desired`) to `target_root`, fetching
/// content from `store`, using the pre-retargetted `store_index`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn change_version2(
    store: &Arc<dyn BlockStore>,
    target_root: &Path,
    desired: &VersionIndex,
    current: &VersionIndex,
    diff: &VersionDiff,
    store_index: &StoreIndex,
    retain_permissions: bool,
    progress: &RateLimited,
    cancel: &CancellationToken,
) -> Result<ApplyStats, LongtailError> {
    let mut stats = ApplyStats::default();

    // 1. Ensure the target dir exists (longtail.c:8763).
    std::fs::create_dir_all(target_root)
        .map_err(|e| LongtailError::io(format!("mkdir {target_root:?}"), e))?;

    // 2. Preflight ALL retargetted store-index blocks (longtail.c:8780).
    store.preflight_get(&store_index.block_hashes).await?;

    // 3. Deletes FIRST (CleanUpRemoveAssets, longtail.c:8787 / :7758) — removed
    //    indexes are already sorted long-to-short; 10-retry loop lets a dir be
    //    removed after its children succeed.
    stats.assets_removed = delete_assets(target_root, current, diff, cancel)?;

    // 4. Build the write set = added + content-modified (longtail.c:8587).
    let chunk_to_block = build_chunk_block_map(store_index);
    let mut zero_assets: Vec<ZeroAsset> = Vec::new();
    let mut block_writes: HashMap<u64, Vec<BlockWrite>> = HashMap::new();
    let mut write_asset_indexes: Vec<u32> = diff.target_added_asset_indexes.clone();
    write_asset_indexes.extend_from_slice(&diff.target_content_modified_asset_indexes);

    for &idx in &write_asset_indexes {
        let ai = idx as usize;
        let is_dir = desired.is_dir(ai).unwrap_or(false);
        let rel = asset_path(desired, idx)?;
        let start = desired.asset_chunk_index_starts[ai] as usize;
        let count = desired.asset_chunk_counts[ai] as usize;
        if count == 0 {
            zero_assets.push(ZeroAsset { rel, is_dir });
            continue;
        }
        let mut asset_offset: u64 = 0;
        for k in 0..count {
            let cidx = desired.asset_chunk_indexes[start + k] as usize;
            let chunk_hash = desired.chunk_hashes[cidx];
            let chunk_size = desired.chunk_sizes[cidx];
            let block_hash = *chunk_to_block.get(&chunk_hash).ok_or_else(|| {
                LongtailError::Store(longtail_store::StoreError::NotFound(format!(
                    "chunk {chunk_hash:#018x} required by `{rel}` not in the store index"
                )))
            })?;
            block_writes
                .entry(block_hash)
                .or_default()
                .push(BlockWrite {
                    rel: rel.clone(),
                    asset_offset,
                    chunk_hash,
                    chunk_size,
                });
            asset_offset += chunk_size as u64;
        }
    }

    // 5a. Zero-size job: create dirs + empty files (longtail.c:8292).
    for z in &zero_assets {
        if cancel.is_cancelled() {
            return Err(LongtailError::Cancelled);
        }
        if z.is_dir {
            fs_util::create_dir(target_root, &z.rel)?;
        } else {
            let _ = fs_util::create_file_sized(target_root, &z.rel, 0)?;
            stats.assets_written += 1;
        }
    }

    // 5b. Pre-create + truncate all write files to their final size (first-touch
    //     semantic, concurrentchunkwrite.c:108), so positional writes fill them.
    let mut created: std::collections::HashSet<String> = std::collections::HashSet::new();
    for &idx in &write_asset_indexes {
        let ai = idx as usize;
        if desired.is_dir(ai).unwrap_or(false) || desired.asset_chunk_counts[ai] == 0 {
            continue;
        }
        let rel = asset_path(desired, idx)?;
        if created.insert(rel.clone()) {
            let size = desired.asset_sizes[ai];
            let _ = fs_util::create_file_sized(target_root, &rel, size)?;
            stats.assets_written += 1;
        }
    }

    // 6. Per-block positional writes (longtail.c:8347). Preflight has the fetches
    //    in flight; consuming them coalesces with the prefetch.
    let total_blocks = block_writes.len() as u32;
    let mut done_blocks = 0u32;
    progress.phase("Updating version");
    for (block_hash, writes) in &block_writes {
        if cancel.is_cancelled() {
            return Err(LongtailError::Cancelled);
        }
        let block = store.get_stored_block(*block_hash).await?;
        // chunk_hash -> (offset_in_block, size) over the decoded payload.
        let mut in_block: HashMap<u64, (usize, u32)> = HashMap::new();
        let mut off = 0usize;
        for (i, &sz) in block.block_index.chunk_sizes.iter().enumerate() {
            let ch = block.block_index.chunk_hashes[i];
            in_block.entry(ch).or_insert((off, sz));
            off += sz as usize;
        }
        for w in writes {
            let (bo, bsz) = in_block.get(&w.chunk_hash).copied().ok_or_else(|| {
                LongtailError::Store(longtail_store::StoreError::BadFormat(format!(
                    "block {block_hash:#018x} does not contain chunk {:#018x}",
                    w.chunk_hash
                )))
            })?;
            debug_assert_eq!(bsz, w.chunk_size);
            let end = bo + bsz as usize;
            let file = fs_util::open_for_write(target_root, &w.rel)?;
            fs_util::write_at(&file, w.asset_offset, &block.payload[bo..end])?;
            stats.bytes_written += bsz as u64;
        }
        done_blocks += 1;
        progress.report(done_blocks, total_blocks);
    }

    // 7. Permissions LAST (longtail.c:8900), only when retaining.
    if retain_permissions {
        for &idx in &diff.target_permissions_modified_asset_indexes {
            let rel = asset_path(desired, idx)?;
            fs_util::set_permissions(target_root, &rel, desired.permissions[idx as usize])?;
        }
        for &idx in &diff.target_added_asset_indexes {
            let ai = idx as usize;
            if desired.is_dir(ai).unwrap_or(false) {
                continue; // added dirs are not permission-set (longtail.c:7995)
            }
            let rel = asset_path(desired, idx)?;
            fs_util::set_permissions(target_root, &rel, desired.permissions[ai])?;
        }
    }

    Ok(stats)
}

/// Build a `chunk_hash -> block_hash` map (first block containing a chunk wins,
/// matching C's `PutUnique`, longtail.c:8640).
fn build_chunk_block_map(store_index: &StoreIndex) -> HashMap<u64, u64> {
    let mut map = HashMap::new();
    for b in 0..store_index.block_count() as usize {
        let count = store_index.block_chunk_counts[b] as usize;
        let offset = store_index.block_chunks_offsets[b] as usize;
        let end = match offset.checked_add(count) {
            Some(e) if e <= store_index.chunk_hashes.len() => e,
            _ => continue,
        };
        let block_hash = store_index.block_hashes[b];
        for &ch in &store_index.chunk_hashes[offset..end] {
            map.entry(ch).or_insert(block_hash);
        }
    }
    map
}

/// `CleanUpRemoveAssets` (longtail.c:7758): remove `diff.source_removed`
/// (indexes into `current`, already sorted long-to-short) with a 10-pass retry
/// loop so a dir is removed after its children succeed. Files that persist after
/// the last pass are a hard error; dirs that persist are left (Go tolerates).
fn delete_assets(
    target_root: &Path,
    current: &VersionIndex,
    diff: &VersionDiff,
    cancel: &CancellationToken,
) -> Result<u32, LongtailError> {
    let mut remove: Vec<Option<u32>> = diff
        .source_removed_asset_indexes
        .iter()
        .map(|&i| Some(i))
        .collect();
    if remove.is_empty() {
        return Ok(0);
    }
    let mut removed_count = 0u32;
    let mut retry = 10;
    while retry > 0 && (removed_count as usize) < remove.len() {
        retry -= 1;
        for slot in remove.iter_mut() {
            if cancel.is_cancelled() {
                return Err(LongtailError::Cancelled);
            }
            let idx = match *slot {
                Some(i) => i,
                None => continue,
            };
            let ai = idx as usize;
            let is_dir = current.is_dir(ai).unwrap_or(false);
            let rel = fs_util::strip_trailing_slash(current.path(ai)?).to_string();
            let last_pass = retry == 0;
            match fs_util::remove_asset(target_root, &rel, is_dir) {
                Ok(true) => {
                    *slot = None;
                    removed_count += 1;
                }
                Ok(false) => {
                    // Still present (likely a non-empty dir); leave for a retry.
                    if last_pass && !is_dir {
                        return Err(LongtailError::io(
                            format!("failed to remove `{rel}`"),
                            std::io::Error::other("still present after retries"),
                        ));
                    }
                }
                Err(e) => {
                    if last_pass {
                        return Err(e);
                    }
                }
            }
        }
    }
    Ok(removed_count)
}
