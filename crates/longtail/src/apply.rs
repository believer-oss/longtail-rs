//! `ChangeVersion2` apply flow, in C's order (`Longtail_ChangeVersion2`,
//! longtail.c:8720-8911): mkdir → preflight(all) → **deletes FIRST** (long-to-short,
//! 10-retry) → zero-size/dir + per-block positional writes (first touch truncates
//! to final size) → **permissions LAST** (only when `retain_permissions`).
//!
//! The per-block writes run **concurrently**: N block tasks in
//! flight, bounded to the resolved remote worker count (one knob — the same
//! value that bounds the store's block I/O). Correctness rests on true range
//! disjointness: every `(asset, offset, len)` range has exactly ONE writer,
//! because per-asset chunk offsets strictly increase (step 4), the added /
//! content-modified asset sets are disjoint by construction of the diff, and
//! each chunk occurrence is assigned to exactly one block via the first-wins
//! `chunk_to_block` map. (The inverse is false — one chunk hash fans out to many
//! `(asset, offset)` targets — but all occurrences of a chunk land in the *same*
//! block's write list.) Step 5b pre-creates and truncates every write-plan file
//! to its final size strictly BEFORE the concurrent loop, so no task depends on
//! another for first-touch and any completion order yields byte-identical trees
//! (asserted by the permuted-completion-order test in this module).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use longtail_core::{StoreIndex, StoredBlock, VersionDiff, VersionIndex};
use longtail_store::block_store::BlockStore;
use tokio_util::sync::CancellationToken;

use crate::error::LongtailError;
use crate::fs_util;
use crate::progress::{Progress, RateLimited};

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
/// `apply_concurrency` bounds the in-flight block tasks (the caller passes the
/// resolved remote worker count — `longtail_store::resolved_worker_count`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn change_version2(
    store: &Arc<dyn BlockStore>,
    target_root: &Path,
    desired: &VersionIndex,
    current: &VersionIndex,
    diff: &VersionDiff,
    store_index: &StoreIndex,
    retain_permissions: bool,
    delete_removed: bool,
    verify: Option<Arc<dyn longtail_core::Hash + Send + Sync>>,
    apply_concurrency: usize,
    progress: &Arc<RateLimited>,
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
    //
    //    Skipped entirely under `delete_removed == false` (the repair shape): the
    //    write set below is unaffected, so every asset the version names is still
    //    checked and rewritten, and everything else is left where it is. Safe to
    //    skip because the removed set is disjoint from the write set by
    //    construction — a path present in both versions is content- or
    //    permissions-modified, never "removed" (diff.rs:72-91).
    stats.assets_removed = if delete_removed {
        delete_assets(target_root, current, diff, cancel)?
    } else {
        0
    };

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

    // Modes relaxed below so a write could land, to be put back before step 7.
    // An asset whose content changed but whose permissions did not is in
    // `target_content_modified_asset_indexes` and in neither list step 7 walks
    // (diff.rs:75-82 tests the two independently), so step 7 will not restore it
    // for us — and under `retain_permissions == false` step 7 does not run at all.
    let mut relaxed = RelaxedModes::new(target_root);

    // 5a. Zero-size job: create dirs + empty files (longtail.c:8292).
    for z in &zero_assets {
        if cancel.is_cancelled() {
            return Err(LongtailError::Cancelled);
        }
        if z.is_dir {
            relaxed.unlock_parents(&z.rel)?;
            fs_util::create_dir(target_root, &z.rel)?;
        } else {
            relaxed.unlock_parents(&z.rel)?;
            relaxed.unlock(&z.rel)?;
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
            // A read-only asset already on disk (a previous run chmod'd it to its
            // recorded mode) cannot be truncate-opened: the open below checks the
            // owner write bit and returns EACCES. C's legacy write path relaxes it
            // the same way (longtail.c:5315-5345, restored at :5675); the
            // ChangeVersion2 path it replaced never did, which is why a second
            // downsync of a modified `0444` asset failed.
            relaxed.unlock_parents(&rel)?;
            relaxed.unlock(&rel)?;
            let _ = fs_util::create_file_sized(target_root, &rel, size)?;
            stats.assets_written += 1;
        }
    }

    // 6. Per-block positional writes (longtail.c:8347), N block tasks in flight
    //    (Fix 2). Preflight enqueued the background fetches; each task's demand
    //    get coalesces with (or claims ahead of) its prefetch. First-error-wins:
    //    the first failure stops launching, in-flight tasks are drained (never
    //    detached mid-write), and the first error is propagated. Progress is a
    //    count of COMPLETED blocks (not launch position), reported under a lock
    //    so emissions stay monotone.
    let total_blocks = block_writes.len() as u32;
    // Total decompressed bytes to fetch = Σ full-block payload sizes over the
    // blocks the write plan needs (each fetched block's payload == Σ its chunk
    // sizes in the store index). This matches the per-block `payload.len()`
    // summed into `done_bytes` below, so the byte progress never overshoots.
    let block_bytes = block_decompressed_sizes(store_index);
    let total_bytes: u64 = block_writes
        .keys()
        .map(|bh| block_bytes.get(bh).copied().unwrap_or(0))
        .sum();
    progress.phase("Updating version");
    let done_blocks = Arc::new(AtomicU32::new(0));
    let done_bytes = Arc::new(AtomicU64::new(0));
    let bytes_written = Arc::new(AtomicU64::new(0));
    let report_lock = Arc::new(std::sync::Mutex::new(()));
    let sem = Arc::new(tokio::sync::Semaphore::new(apply_concurrency.max(1)));
    let mut tasks: tokio::task::JoinSet<Result<(), LongtailError>> = tokio::task::JoinSet::new();
    let mut first_err: Option<LongtailError> = None;

    for (block_hash, writes) in block_writes {
        // Cancellation honored between blocks (pre-Fix-2 granularity): stop
        // launching; in-flight blocks complete whole (resumable target).
        if cancel.is_cancelled() {
            first_err.get_or_insert(LongtailError::Cancelled);
        }
        // Reap finished tasks so a failure stops the launch loop promptly.
        while let Some(res) = tasks.try_join_next() {
            if let Err(e) = flatten_apply_task(res) {
                first_err.get_or_insert(e);
            }
        }
        if first_err.is_some() {
            break;
        }
        let permit = sem
            .clone()
            .acquire_owned()
            .await
            .expect("apply semaphore never closes");
        let store = store.clone();
        let target_root = target_root.to_path_buf();
        let progress = progress.clone();
        let done_blocks = done_blocks.clone();
        let done_bytes = done_bytes.clone();
        let bytes_written = bytes_written.clone();
        let report_lock = report_lock.clone();
        let verify = verify.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let block = store.get_stored_block(block_hash).await?;
            // Full decompressed block payload we just fetched — the download
            // byte dimension (captured before `block` moves into the writer).
            let payload_len = block.payload.len() as u64;
            // The sync positional writes run on the blocking pool — N tasks
            // doing sync file I/O on the tokio workers is a known footgun.
            let n = tokio::task::spawn_blocking(move || {
                write_block_chunks(&target_root, block_hash, &block, &writes, verify.as_deref())
            })
            .await
            .map_err(|e| {
                LongtailError::io(
                    "apply block-write task",
                    std::io::Error::other(format!("join error: {e}")),
                )
            })??;
            bytes_written.fetch_add(n, Ordering::Relaxed);
            done_bytes.fetch_add(payload_len, Ordering::Relaxed);
            done_blocks.fetch_add(1, Ordering::Relaxed);
            {
                // Fresh load under the lock → reported values never decrease.
                let _g = report_lock.lock().unwrap();
                progress.report(Progress {
                    done_items: done_blocks.load(Ordering::Relaxed) as u64,
                    total_items: total_blocks as u64,
                    done_bytes: done_bytes.load(Ordering::Relaxed),
                    total_bytes,
                });
            }
            Ok(())
        });
    }
    // Drain all in-flight tasks (bounded, terminates promptly). First error wins.
    while let Some(res) = tasks.join_next().await {
        if let Err(e) = flatten_apply_task(res) {
            first_err.get_or_insert(e);
        }
    }
    // A cancellation that fired while the final blocks were in flight (e.g.
    // from a progress callback) is still honored: pre-Fix-2 the serial loop
    // re-checked the token before every remaining block; with N in flight
    // there may be no "next block", so re-check after the drain. In-flight
    // blocks completed whole, so the target stays resumable.
    if first_err.is_none() && cancel.is_cancelled() {
        first_err = Some(LongtailError::Cancelled);
    }
    // Before step 7, and before the error check, so a failed or cancelled run
    // does not leave an asset more permissive than it found it — the target
    // survives both, and the next run resumes over it.
    relaxed.restore();

    if let Some(e) = first_err {
        return Err(e);
    }
    stats.bytes_written = bytes_written.load(Ordering::Relaxed);

    // 7. Permissions LAST (longtail.c:8900), only when retaining. Runs after the
    // restore above, so an asset whose recorded mode did change still ends at the
    // new one rather than the mode it happened to have on disk.
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

/// Modes temporarily relaxed so an existing read-only asset could be rewritten,
/// and the obligation to put them back.
///
/// [`restore`](Self::restore) is called explicitly before step 7 rather than left
/// to the drop, because step 7 assigns the *recorded* mode and must be the last
/// word; a drop running after it would undo that. The [`Drop`] impl only covers
/// the early returns between step 5 and there, where nothing else would.
/// Restoring is best-effort by nature — it is cleanup, and reporting a chmod
/// failure in place of the error that caused the unwind would bury the cause.
struct RelaxedModes<'a> {
    root: &'a Path,
    entries: Vec<(String, fs_util::PriorMode)>,
    /// Directories already walked, so the per-file sweep in step 5b does not
    /// re-stat the same ancestors once per asset.
    walked: std::collections::HashSet<String>,
}

impl<'a> RelaxedModes<'a> {
    fn new(root: &'a Path) -> Self {
        RelaxedModes {
            root,
            entries: Vec::new(),
            walked: std::collections::HashSet::new(),
        }
    }

    /// Make `rel` writable if it exists and is not, recording what to put back.
    fn unlock(&mut self, rel: &str) -> Result<(), LongtailError> {
        if let Some(prior) = fs_util::unlock_for_rewrite(self.root, rel)? {
            self.entries.push((rel.to_string(), prior));
        }
        Ok(())
    }

    /// Relax the directories leading to `rel`, so an asset can be created inside
    /// them. Creating a file needs write on the *parent*, so a directory left at
    /// a mode without the owner write bit blocks a new asset beneath it just as a
    /// read-only file blocks its own rewrite. Step 7 does chmod directories (its
    /// first loop takes permissions-modified assets without a dir skip), so a
    /// version can leave the target in exactly that state for the next one.
    ///
    /// Walks top-down, so an ancestor is relaxed before anything tries to create
    /// the directory beneath it. `root` itself is the operator's and is left
    /// alone. Missing directories yield nothing to record — `unlock` reports only
    /// what it actually changed.
    fn unlock_parents(&mut self, rel: &str) -> Result<(), LongtailError> {
        let Some((dirs, _leaf)) = rel.rsplit_once('/') else {
            return Ok(()); // directly under the root
        };
        let mut prefix = String::new();
        for part in dirs.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if self.walked.insert(prefix.clone()) {
                self.unlock(&prefix)?;
            }
        }
        Ok(())
    }

    /// Put every recorded mode back. Draining leaves the drop with nothing to do,
    /// so calling this and then unwinding cannot restore twice.
    fn restore(&mut self) {
        for (rel, prior) in self.entries.drain(..) {
            if let Err(e) = fs_util::restore_mode(self.root, &rel, prior) {
                tracing::warn!(
                    asset = %rel,
                    error = %e,
                    "could not restore an asset's permissions; it is left writable"
                );
            }
        }
    }
}

impl Drop for RelaxedModes<'_> {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Write one fetched block's chunk occurrences into their pre-created target
/// files (positional writes, longtail.c:8347). Sync — runs under
/// `spawn_blocking`. Returns the bytes written. Safe to run concurrently with
/// other blocks' writers: the write ranges are disjoint (module docs).
///
/// Each distinct target file is opened once per block (not once per chunk),
/// and runs that are contiguous in BOTH the asset and the block payload merge
/// into a single `pwrite` — chunks usually sit consecutively in both, so this
/// collapses tens of thousands of open/pwrite/close syscalls into a handful.
fn write_block_chunks(
    target_root: &Path,
    block_hash: u64,
    block: &StoredBlock,
    writes: &[BlockWrite],
    verify: Option<&(dyn longtail_core::Hash + Send + Sync)>,
) -> Result<u64, LongtailError> {
    let mut verified: std::collections::HashSet<u64> = std::collections::HashSet::new();
    // chunk_hash -> (offset_in_block, size) over the decoded payload.
    let mut in_block: HashMap<u64, (usize, u32)> = HashMap::new();
    let mut off = 0usize;
    for (i, &sz) in block.block_index.chunk_sizes.iter().enumerate() {
        let ch = block.block_index.chunk_hashes[i];
        in_block.entry(ch).or_insert((off, sz));
        off += sz as usize;
    }
    // (asset_offset, block_offset, len) per target file.
    let mut by_file: HashMap<&str, Vec<(u64, usize, usize)>> = HashMap::new();
    for w in writes {
        let (bo, bsz) = in_block.get(&w.chunk_hash).copied().ok_or_else(|| {
            LongtailError::Store(longtail_store::StoreError::BadFormat(format!(
                "block {block_hash:#018x} does not contain chunk {:#018x}",
                w.chunk_hash
            )))
        })?;
        // Opt-in content authentication. The block hash covers only the block's
        // chunk-hash array (pack.rs), and nothing else on the read path re-hashes a
        // payload, so a block carrying substituted bytes under intact chunk hashes
        // is otherwise accepted. Hashing here, before the write loop below, means a
        // block that fails leaves the target untouched rather than half-rewritten.
        // Each distinct chunk is hashed once even when it fans out to many assets.
        if let Some(hasher) = verify
            && verified.insert(w.chunk_hash)
        {
            let end = bo
                .checked_add(bsz as usize)
                .filter(|e| *e <= block.payload.len())
                .ok_or_else(|| {
                    LongtailError::Store(longtail_store::StoreError::BadFormat(format!(
                        "block {block_hash:#018x} is shorter than the range it gives chunk {:#018x}",
                        w.chunk_hash
                    )))
                })?;
            let actual = hasher.hash(&block.payload[bo..end]);
            if actual != w.chunk_hash {
                return Err(LongtailError::ValidationMismatch(format!(
                    "block {block_hash:#018x} carries chunk {:#018x} whose bytes hash to \
                     {actual:#018x}; the store's content does not match its index",
                    w.chunk_hash
                )));
            }
        }
        // The block's own index and the version index disagreeing about a chunk's
        // size is a corrupt or hostile store, not a bug in this process, so it has
        // to be checked in the profile that ships. As a `debug_assert` it was
        // compiled out of release, where an oversized `bsz` writes past the chunk's
        // range into the neighbouring one — silent corruption of an asset that
        // every later verification step then agrees on.
        if bsz != w.chunk_size {
            return Err(LongtailError::Store(longtail_store::StoreError::BadFormat(
                format!(
                    "block {block_hash:#018x} sizes chunk {:#018x} at {bsz} bytes, the version \
                     index at {}",
                    w.chunk_hash, w.chunk_size
                ),
            )));
        }
        by_file
            .entry(w.rel.as_str())
            .or_default()
            .push((w.asset_offset, bo, bsz as usize));
    }
    let mut written = 0u64;
    for (rel, mut runs) in by_file {
        // Disjoint single-writer ranges (module docs) ⇒ per-file offsets are
        // unique; sort then merge doubly-contiguous neighbors.
        runs.sort_unstable_by_key(|&(asset_off, _, _)| asset_off);
        let file = fs_util::open_for_write(target_root, rel)?;
        let mut i = 0usize;
        while i < runs.len() {
            let (asset_off, block_off, mut len) = runs[i];
            let mut j = i + 1;
            while j < runs.len() {
                let (next_ao, next_bo, next_len) = runs[j];
                if next_ao == asset_off + len as u64 && next_bo == block_off + len {
                    len += next_len;
                    j += 1;
                } else {
                    break;
                }
            }
            fs_util::write_at(&file, asset_off, &block.payload[block_off..block_off + len])?;
            written += len as u64;
            i = j;
        }
    }
    Ok(written)
}

/// Flatten a joined apply-task result. Tasks are never aborted (in-flight
/// blocks always complete whole), so a `JoinError` here is a panic — surfaced
/// as an error, not propagated as a panic.
fn flatten_apply_task(
    res: Result<Result<(), LongtailError>, tokio::task::JoinError>,
) -> Result<(), LongtailError> {
    match res {
        Ok(r) => r,
        Err(e) => Err(LongtailError::Internal(format!(
            "apply block task panicked: {e}"
        ))),
    }
}

/// Map each block hash to its decompressed payload size (Σ of its chunk sizes),
/// for the download byte total. Mirrors the block→chunk-range walk in
/// [`build_chunk_block_map`].
fn block_decompressed_sizes(store_index: &StoreIndex) -> HashMap<u64, u64> {
    let mut map = HashMap::new();
    for b in 0..store_index.block_count() as usize {
        let count = store_index.block_chunk_counts[b] as usize;
        let offset = store_index.block_chunks_offsets[b] as usize;
        let end = match offset.checked_add(count) {
            Some(e) if e <= store_index.chunk_sizes.len() => e,
            _ => continue,
        };
        let size: u64 = store_index.chunk_sizes[offset..end]
            .iter()
            .map(|&s| s as u64)
            .sum();
        // First block wins, matching `build_chunk_block_map`'s dedup so the set
        // of summed blocks lines up with the blocks actually fetched.
        map.entry(store_index.block_hashes[b]).or_insert(size);
    }
    map
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

#[cfg(test)]
mod tests {
    //! Concurrent-apply unit tests: write-order independence (a permuted block
    //! completion order produces byte-identical trees), the step-5b first-touch
    //! ordering (every write-plan file exists at final size before the FIRST
    //! block fetch), and first-error-wins termination.

    use std::collections::{HashMap, HashSet};
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;

    use longtail_core::{
        Blake3, BlockIndex, FileEntry, FileInfos, Permissions, StoreIndex, StoredBlock,
        VersionIndex, assemble_version_index, create_version_diff,
    };
    use longtail_store::StoreError;
    use longtail_store::block_store::{BlockStore, StatsSnapshot};
    use tokio_util::sync::CancellationToken;

    use super::change_version2;
    use crate::progress::{NullProgress, RateLimited};

    type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    /// A scripted block store: serves blocks from a map, optionally holding
    /// each `get` behind a per-hash gate so the test controls the exact
    /// completion order; records arrival/completion order and (for the
    /// first-touch assertion) verifies the expected files exist at final size
    /// on every fetch.
    struct MockStore {
        blocks: HashMap<u64, StoredBlock>,
        gates: HashMap<u64, tokio::sync::watch::Receiver<bool>>,
        arrived: StdMutex<Vec<u64>>,
        completed: StdMutex<Vec<u64>>,
        /// `(absolute_path, final_size)` expected to exist during every fetch.
        expect_files: Vec<(PathBuf, u64)>,
        violations: StdMutex<Vec<String>>,
        missing: HashSet<u64>,
    }

    impl MockStore {
        fn new(blocks: HashMap<u64, StoredBlock>) -> MockStore {
            MockStore {
                blocks,
                gates: HashMap::new(),
                arrived: StdMutex::new(Vec::new()),
                completed: StdMutex::new(Vec::new()),
                expect_files: Vec::new(),
                violations: StdMutex::new(Vec::new()),
                missing: HashSet::new(),
            }
        }
    }

    // `BlockStore` is an `#[async_trait]` trait; the crate has no async-trait
    // dependency, so implement the desugared form directly.
    impl BlockStore for MockStore {
        fn put_stored_block<'l, 'a>(&'l self, _b: StoredBlock) -> BoxFut<'a, Result<(), StoreError>>
        where
            'l: 'a,
            Self: 'a,
        {
            Box::pin(async { Err(StoreError::NotSupported("mock".into())) })
        }

        fn get_stored_block<'l, 'a>(
            &'l self,
            block_hash: u64,
        ) -> BoxFut<'a, Result<StoredBlock, StoreError>>
        where
            'l: 'a,
            Self: 'a,
        {
            Box::pin(async move {
                self.arrived.lock().unwrap().push(block_hash);
                // First-touch invariant: every write-plan file must already
                // exist, truncated to its final size, BEFORE any block fetch
                // (step 5b runs strictly before the concurrent loop).
                for (path, size) in &self.expect_files {
                    match std::fs::metadata(path) {
                        Ok(m) if m.len() == *size => {}
                        Ok(m) => self.violations.lock().unwrap().push(format!(
                            "{path:?}: size {} != expected {size} during fetch {block_hash:#x}",
                            m.len()
                        )),
                        Err(e) => self
                            .violations
                            .lock()
                            .unwrap()
                            .push(format!("{path:?}: missing during fetch ({e})")),
                    }
                }
                if self.missing.contains(&block_hash) {
                    return Err(StoreError::NotFound(format!("block {block_hash:#x}")));
                }
                if let Some(gate) = self.gates.get(&block_hash) {
                    let mut gate = gate.clone();
                    gate.wait_for(|open| *open)
                        .await
                        .map_err(|_| StoreError::Backend("gate dropped".into()))?;
                }
                let block = self
                    .blocks
                    .get(&block_hash)
                    .cloned()
                    .ok_or_else(|| StoreError::NotFound(format!("block {block_hash:#x}")))?;
                self.completed.lock().unwrap().push(block_hash);
                Ok(block)
            })
        }

        fn preflight_get<'l0, 'l1, 'a>(
            &'l0 self,
            _hashes: &'l1 [u64],
        ) -> BoxFut<'a, Result<(), StoreError>>
        where
            'l0: 'a,
            'l1: 'a,
            Self: 'a,
        {
            Box::pin(async { Ok(()) })
        }

        fn get_existing_content<'l0, 'l1, 'a>(
            &'l0 self,
            _chunk_hashes: &'l1 [u64],
            _min_block_usage_percent: u32,
        ) -> BoxFut<'a, Result<StoreIndex, StoreError>>
        where
            'l0: 'a,
            'l1: 'a,
            Self: 'a,
        {
            Box::pin(async { Err(StoreError::NotSupported("mock".into())) })
        }

        fn prune_blocks<'l0, 'l1, 'a>(
            &'l0 self,
            _keep: &'l1 [u64],
        ) -> BoxFut<'a, Result<u32, StoreError>>
        where
            'l0: 'a,
            'l1: 'a,
            Self: 'a,
        {
            Box::pin(async { Err(StoreError::NotSupported("mock".into())) })
        }

        fn flush<'l, 'a>(&'l self) -> BoxFut<'a, Result<(), StoreError>>
        where
            'l: 'a,
            Self: 'a,
        {
            Box::pin(async { Ok(()) })
        }

        fn close<'l, 'a>(&'l self) -> BoxFut<'a, Result<(), StoreError>>
        where
            'l: 'a,
            Self: 'a,
        {
            Box::pin(async { Ok(()) })
        }

        fn stats(&self) -> StatsSnapshot {
            StatsSnapshot::default()
        }
    }

    // --- synthetic version/store fixture ---

    const B1: u64 = 0xB10C_0001;
    const B2: u64 = 0xB10C_0002;
    const B3: u64 = 0xB10C_0003;

    struct Scenario {
        desired: VersionIndex,
        current: VersionIndex,
        store_index: StoreIndex,
        blocks: HashMap<u64, StoredBlock>,
        /// `(rel_path, bytes)`, sorted by path.
        expected: Vec<(String, Vec<u8>)>,
    }

    /// Four files over five chunks in three blocks, exercising multi-chunk
    /// assets, dedup fan-out (chunk `c2` appears in two assets), a multi-asset
    /// block, and a zero-chunk (empty) file:
    ///   a.bin     = c1 ‖ c2 ‖ c3   (c1,c3 ∈ B1; c2 ∈ B2)
    ///   c.bin     = c5             (c5 ∈ B3)
    ///   empty.txt = ∅
    ///   sub/b.bin = c2 ‖ c4        (c2 ∈ B2; c4 ∈ B3)
    fn scenario() -> Scenario {
        let chunks: [(u64, u8, usize); 5] = [
            (0x1111, 0xA1, 100),  // c1
            (0x2222, 0xB2, 200),  // c2
            (0x3333, 0xC3, 50),   // c3
            (0x4444, 0xD4, 300),  // c4
            (0x5555, 0xE5, 1000), // c5
        ];
        let bytes_of = |i: usize| vec![chunks[i].1; chunks[i].2];
        let c = |i: usize| (chunks[i].0, chunks[i].2 as u32);

        // FileInfos sorts by path bytes; keep this list pre-sorted so the
        // per-asset chunk lists align: a.bin < c.bin < empty.txt < sub/b.bin.
        let entries = vec![
            FileEntry {
                relative_path: "a.bin".into(),
                size: 350,
                permissions: Permissions(0o644),
                is_dir: false,
            },
            FileEntry {
                relative_path: "c.bin".into(),
                size: 1000,
                permissions: Permissions(0o644),
                is_dir: false,
            },
            FileEntry {
                relative_path: "empty.txt".into(),
                size: 0,
                permissions: Permissions(0o644),
                is_dir: false,
            },
            FileEntry {
                relative_path: "sub/b.bin".into(),
                size: 500,
                permissions: Permissions(0o644),
                is_dir: false,
            },
        ];
        let per_asset: Vec<Vec<(u64, u32)>> = vec![
            vec![c(0), c(1), c(2)], // a.bin
            vec![c(4)],             // c.bin
            vec![],                 // empty.txt
            vec![c(1), c(3)],       // sub/b.bin (c2 dedup fan-out)
        ];
        let fi = FileInfos::from_scanned_entries(entries);
        let desired = assemble_version_index(&fi, &per_asset, &Blake3, 32768, None);
        let current = assemble_version_index(
            &FileInfos::from_scanned_entries(Vec::new()),
            &[],
            &Blake3,
            32768,
            None,
        );

        let block_defs: [(u64, Vec<usize>); 3] =
            [(B1, vec![0, 2]), (B2, vec![1]), (B3, vec![3, 4])];
        let mut blocks = HashMap::new();
        let mut block_indexes = Vec::new();
        for (hash, chunk_ids) in &block_defs {
            let mut payload = Vec::new();
            let mut chunk_hashes = Vec::new();
            let mut chunk_sizes = Vec::new();
            for &i in chunk_ids {
                payload.extend_from_slice(&bytes_of(i));
                chunk_hashes.push(chunks[i].0);
                chunk_sizes.push(chunks[i].2 as u32);
            }
            let bi = BlockIndex {
                block_hash: *hash,
                hash_identifier: longtail_core::Hash::id(&Blake3),
                tag: 0,
                chunk_hashes,
                chunk_sizes,
            };
            block_indexes.push(bi.clone());
            blocks.insert(
                *hash,
                StoredBlock {
                    block_index: bi,
                    payload,
                },
            );
        }
        let store_index = StoreIndex::from_block_indexes(&block_indexes).unwrap();

        let expected = vec![
            (
                "a.bin".to_string(),
                [bytes_of(0), bytes_of(1), bytes_of(2)].concat(),
            ),
            ("c.bin".to_string(), bytes_of(4)),
            ("empty.txt".to_string(), Vec::new()),
            ("sub/b.bin".to_string(), [bytes_of(1), bytes_of(3)].concat()),
        ];
        Scenario {
            desired,
            current,
            store_index,
            blocks,
            expected,
        }
    }

    fn capture_tree(root: &Path, expected: &[(String, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
        expected
            .iter()
            .map(|(rel, _)| {
                (
                    rel.clone(),
                    std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}")),
                )
            })
            .collect()
    }

    async fn run_apply(
        mock: Arc<MockStore>,
        sc: &Scenario,
        target: &Path,
        concurrency: usize,
    ) -> Result<super::ApplyStats, crate::LongtailError> {
        let store: Arc<dyn BlockStore> = mock;
        let diff = create_version_diff(&sc.current, &sc.desired);
        let progress = Arc::new(RateLimited::new(Arc::new(NullProgress)));
        let cancel = CancellationToken::new();
        change_version2(
            &store,
            target,
            &sc.desired,
            &sc.current,
            &diff,
            &sc.store_index,
            false, // retain_permissions
            true,  // delete_removed
            None,  // verify
            concurrency,
            &progress,
            &cancel,
        )
        .await
    }

    /// **Write-order independence**: gate every block fetch, release the gates
    /// in three different permutations (forcing three different completion
    /// orders), and assert the resulting trees are byte-identical to each
    /// other and to the expected content.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn permuted_completion_order_yields_byte_identical_trees() {
        let sc = scenario();
        let permutations: [[u64; 3]; 3] = [[B1, B2, B3], [B3, B1, B2], [B2, B3, B1]];
        let mut trees: Vec<Vec<(String, Vec<u8>)>> = Vec::new();

        for perm in &permutations {
            let tmp = tempfile::tempdir().unwrap();
            let target = tmp.path().join("out");

            let mut mock = MockStore::new(sc.blocks.clone());
            let mut gate_txs: HashMap<u64, tokio::sync::watch::Sender<bool>> = HashMap::new();
            for &h in &[B1, B2, B3] {
                let (tx, rx) = tokio::sync::watch::channel(false);
                mock.gates.insert(h, rx);
                gate_txs.insert(h, tx);
            }
            let mock = Arc::new(mock);

            // Concurrency ≥ block count so all three fetches are in flight
            // before any completes; the releaser then dictates the exact
            // completion order.
            let releaser = {
                let mock = mock.clone();
                let perm = *perm;
                async move {
                    while mock.arrived.lock().unwrap().len() < 3 {
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    for h in perm {
                        gate_txs[&h].send(true).unwrap();
                        while !mock.completed.lock().unwrap().contains(&h) {
                            tokio::time::sleep(Duration::from_millis(2)).await;
                        }
                    }
                }
            };
            let apply = run_apply(mock.clone(), &sc, &target, 8);
            let (stats, ()) =
                tokio::time::timeout(Duration::from_secs(60), futures_join(apply, releaser))
                    .await
                    .expect("permuted apply must not hang");
            let stats = stats.expect("apply");

            assert_eq!(
                mock.completed.lock().unwrap().as_slice(),
                perm,
                "the gates dictate the completion order"
            );
            assert_eq!(stats.bytes_written, 350 + 1000 + 500);
            assert_eq!(stats.assets_written, 4, "3 content files + 1 empty");
            trees.push(capture_tree(&target, &sc.expected));
        }

        for tree in &trees {
            assert_eq!(
                tree, &sc.expected,
                "tree content matches the chunk-assembled expectation"
            );
        }
        assert_eq!(trees[0], trees[1], "permutation order must not matter");
        assert_eq!(trees[1], trees[2], "permutation order must not matter");
    }

    /// A tiny two-future join to avoid pulling futures-util into the crate.
    async fn futures_join<A, B>(a: A, b: B) -> (A::Output, B::Output)
    where
        A: Future,
        B: Future,
    {
        tokio::join!(a, b)
    }

    /// **First-touch ordering**: step 5b pre-creates + truncates every
    /// write-plan file to its final size strictly BEFORE the concurrent block
    /// loop — asserted inside every mock fetch.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn write_plan_files_precreated_before_first_fetch() {
        let sc = scenario();
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");

        let mut mock = MockStore::new(sc.blocks.clone());
        mock.expect_files = vec![
            (target.join("a.bin"), 350),
            (target.join("c.bin"), 1000),
            (target.join("empty.txt"), 0),
            (target.join("sub/b.bin"), 500),
        ];
        let mock = Arc::new(mock);

        let stats = tokio::time::timeout(
            Duration::from_secs(60),
            run_apply(mock.clone(), &sc, &target, 4),
        )
        .await
        .expect("apply must not hang")
        .expect("apply");

        let violations = mock.violations.lock().unwrap();
        assert!(
            violations.is_empty(),
            "files must be pre-created at final size before any fetch: {violations:?}"
        );
        assert_eq!(stats.assets_written, 4);
        assert_eq!(capture_tree(&target, &sc.expected), sc.expected);
    }

    /// **First-error-wins**: a failing block fetch terminates the apply with
    /// that error (no hang, no panic), while in-flight siblings drain.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn first_error_wins_and_apply_terminates() {
        let sc = scenario();
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("out");

        let mut mock = MockStore::new(sc.blocks.clone());
        mock.missing.insert(B2);
        let mock = Arc::new(mock);

        let err = tokio::time::timeout(Duration::from_secs(60), run_apply(mock, &sc, &target, 4))
            .await
            .expect("failing apply must not hang")
            .expect_err("apply must fail when a block is missing");
        assert!(
            matches!(&err, crate::LongtailError::Store(StoreError::NotFound(_))),
            "expected the missing block's NotFound, got {err:?}"
        );
    }

    /// A block whose own index sizes a chunk differently from the version index
    /// must be refused, in every profile.
    ///
    /// This was a `debug_assert_eq!`, so release — the profile that ships — used
    /// the block's size unchecked. An oversized value writes past the chunk's
    /// range into the next chunk's bytes of the same asset, which no later step
    /// detects: the asset ends up the length the index expects, and every hash the
    /// download verifies is the store's own.
    #[test]
    fn a_block_disagreeing_about_a_chunk_size_is_refused() {
        use longtail_core::{BlockIndex, StoredBlock};

        use super::{BlockWrite, write_block_chunks};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("asset.bin"), vec![0u8; 64]).unwrap();

        // The block sizes the chunk at 32 bytes...
        let block = StoredBlock {
            block_index: BlockIndex {
                block_hash: 0xABCD,
                hash_identifier: 0,
                tag: 0,
                chunk_hashes: vec![0x1111],
                chunk_sizes: vec![32],
            },
            payload: vec![7u8; 32],
        };
        // ...the write plan, taken from the version index, at 64.
        let writes = vec![BlockWrite {
            rel: "asset.bin".to_string(),
            asset_offset: 0,
            chunk_hash: 0x1111,
            chunk_size: 64,
        }];

        let err = write_block_chunks(root, 0xABCD, &block, &writes, None)
            .expect_err("a size disagreement must not be written through");
        assert!(
            format!("{err:?}").contains("sizes chunk"),
            "the error should name the disagreement: {err:?}"
        );
        assert_eq!(
            std::fs::read(root.join("asset.bin")).unwrap(),
            vec![0u8; 64],
            "the asset must be left untouched"
        );
    }
}
