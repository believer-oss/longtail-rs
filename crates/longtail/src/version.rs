//! Folder scan → `CreateVersionIndex`, with parallel per-asset chunking on a
//! rayon pool. Sync (no tokio); this is the target-scan + byte-gate entry point.

use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use longtail_core::{FileInfos, Hash, HpcdcChunker, VersionIndex, assemble_version_index};
use rayon::prelude::*;
use tokio_util::sync::CancellationToken;

use crate::error::LongtailError;
use crate::fs_util;
use crate::path_filter::RegexPathFilter;
use crate::progress::{Progress, RateLimited};

/// Scan `root` (honoring `filter`) and build a [`VersionIndex`] with the C
/// range-split chunking, hashing with `hasher`. `compression_tag` is the uniform
/// per-asset compression id (`0` = no compression / target-scan / validate;
/// a real id for the byte-gate upsync-equivalent). Chunking runs in parallel on
/// `pool`; assembly is deterministic (FileInfos order).
#[allow(clippy::too_many_arguments)]
pub fn create_version_index_from_folder<H: Hash + Sync + ?Sized>(
    root: &Path,
    filter: &RegexPathFilter,
    hasher: &H,
    target_chunk_size: u32,
    compression_tag: u32,
    pool: &rayon::ThreadPool,
    cancel: &CancellationToken,
    on_scan: Option<&(dyn Fn(u64, u64, u64, u64) + Sync)>,
) -> Result<VersionIndex, LongtailError> {
    // Scan, then sort entries with FileInfos' exact byte-wise order so the
    // per-asset chunk lists align 1:1 with the assembled asset order.
    let mut entries = fs_util::scan_folder(root, filter)?;
    entries.sort_by(|a, b| a.relative_path.as_bytes().cmp(b.relative_path.as_bytes()));
    let file_infos = FileInfos::from_scanned_entries(entries.clone());

    let chunker = HpcdcChunker::from_target(target_chunk_size)?;
    let max_hash_size = (target_chunk_size as u64).saturating_mul(1024);

    // Progress denominators (files + bytes), known once the scan has stat'd
    // everything; the counters below are bumped per completed asset and forwarded
    // through `on_scan` (a throttled, thread-safe sink forwarder).
    let total_files = entries.iter().filter(|e| !e.is_dir).count() as u64;
    let total_bytes: u64 = entries.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
    let done_files = AtomicU64::new(0);
    let done_bytes = AtomicU64::new(0);

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
                let mut f = fs_util::open_asset(root, &entry.relative_path)?;
                let chunks =
                    chunk_asset_streaming(&mut f, entry.size, &chunker, max_hash_size, hasher)?;
                if let Some(cb) = on_scan {
                    let f = done_files.fetch_add(1, Ordering::Relaxed) + 1;
                    let b = done_bytes.fetch_add(entry.size, Ordering::Relaxed) + entry.size;
                    cb(f, total_files, b, total_bytes);
                }
                Ok(chunks)
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

/// Chunk `asset_size` bytes read **sequentially** from `reader` into
/// `(chunk_hash, size)` pairs, byte-identically to `longtail_core::chunk_asset`
/// over the whole buffer.
///
/// The C algorithm processes each asset in independent `max_hash_size` "parts"
/// (`asset_part_count = 1 + size / max_hash_size`, longtail.c:2402/2436) and
/// chunk boundaries never cross a part boundary. So we read one part at a time
/// and chunk it in isolation — boundary-identical to the whole-buffer path, but
/// peak memory is a single part (~`max_hash_size`), not the whole asset. This is
/// what keeps a multi-GB pak from residing in memory `N`-wide across the scan
/// pool. `chunk_with` + `hash` are the same pure core primitives `chunk_asset`
/// uses; only the byte source differs (a reader vs an in-memory slice).
fn chunk_asset_streaming<H: Hash + ?Sized>(
    reader: &mut impl Read,
    asset_size: u64,
    chunker: &HpcdcChunker,
    max_hash_size: u64,
    hasher: &H,
) -> Result<Vec<(u64, u32)>, LongtailError> {
    let max_hash_size = max_hash_size.max(1);
    let asset_part_count = 1 + asset_size / max_hash_size;
    let mut out = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut part = 0u64;
    while part < asset_part_count {
        let range_start = part * max_hash_size;
        let remaining = asset_size.saturating_sub(range_start);
        let job_size = remaining.min(max_hash_size) as usize;
        if job_size != 0 {
            buf.resize(job_size, 0);
            reader
                .read_exact(&mut buf)
                .map_err(|e| LongtailError::io("read asset part", e))?;
            chunker.chunk_with(&buf, |span| {
                let s = span.offset as usize;
                let e = s + span.size as usize;
                out.push((hasher.hash(&buf[s..e]), span.size));
            });
        }
        part += 1;
    }
    Ok(out)
}

/// Build a time-throttled `on_scan` forwarder for
/// [`create_version_index_from_folder`]: forwards a [`Progress`] sample to
/// `progress` at most once per ~100 ms — plus always the terminal
/// (`done_files == total_files`) sample, so the bar snaps to 100%. Uses
/// `try_lock` so a rayon worker never blocks on the throttle; a contended tick
/// is simply dropped. The returned closure is `Sync` (safe to call from the
/// scan's rayon workers).
pub(crate) fn scan_progress_forwarder(
    progress: Arc<RateLimited>,
) -> impl Fn(u64, u64, u64, u64) + Sync {
    // Start in the past so the first sample forwards immediately.
    let last = Mutex::new(Instant::now() - Duration::from_secs(1));
    move |done_files, total_files, done_bytes, total_bytes| {
        let terminal = total_files != 0 && done_files >= total_files;
        let due = match last.try_lock() {
            Ok(mut t) if t.elapsed() >= Duration::from_millis(100) => {
                *t = Instant::now();
                true
            }
            _ => false,
        };
        if terminal || due {
            progress.report(Progress {
                done_items: done_files,
                total_items: total_files,
                done_bytes,
                total_bytes,
            });
        }
    }
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
