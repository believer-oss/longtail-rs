//! The async upload-path orchestration (`cmd_upsync.go`): scan+chunk the source
//! (or read a pre-built `.lvi`), compute the missing content, pack + write the
//! blocks, then write the target `.lvi` and (optionally) the version-local
//! `.lsi = MergeStoreIndex(existing, missing)`.
//!
//! [`write_content`] (the `Longtail_WriteContent` port) is shared with
//! clone-store.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use longtail_core::{StoreIndex, StoredBlock, VersionIndex, create_missing_content};
use longtail_store::AccessType;
use longtail_store::block_store::BlockStore;
use longtail_store::uri::{BlockStoreOpts, create_block_store_for_uri};
use tokio_util::sync::CancellationToken;

use crate::compression::compression_type_for_name;
use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};
use crate::hash_util::make_hasher;
use crate::options::{UpsyncOptions, UpsyncReport};
use crate::path_filter::{RegexPathFilter, TARGET_INDEX_CACHE_NAME, relative_within};
use crate::progress::{NullProgress, Progress, ProgressSink, RateLimited};
use crate::version::create_version_index_from_folder;

/// The default upsync block-packing parameters (golongtail `options.go`).
pub const DEFAULT_TARGET_BLOCK_SIZE: u32 = 8 * 1024 * 1024; // 8 MiB
pub const DEFAULT_MAX_CHUNKS_PER_BLOCK: u32 = 1024;
pub const DEFAULT_TARGET_CHUNK_SIZE: u32 = 32768;
/// Upsync-side minimum block usage percent (`commands/options.go:93`). This is
/// where the 80 belongs — downsync passes 0.
pub const DEFAULT_MIN_BLOCK_USAGE_PERCENT: u32 = 80;

/// Upsync a source folder into a store, writing the target `.lvi` and (if a
/// path is set) the version-local `.lsi`.
#[tracing::instrument(
    name = "upsync",
    skip_all,
    fields(
        storage_uri = %opts.storage_uri,
        source_path = %opts.source_path,
        worker_count = opts.worker_count,
        remote_worker_count = opts.remote_worker_count,
    )
)]
pub async fn upsync(opts: UpsyncOptions) -> Result<UpsyncReport, LongtailError> {
    if opts.use_legacy_write {
        return Err(LongtailError::LegacyWriteUnsupported);
    }
    let source_folder = PathBuf::from(&opts.source_path);

    // A version index is a description of a folder, so a folder never carries one
    // as content. The target-index cache is the case that matters: it lives inside
    // a downsynced folder, so upsyncing that folder used to publish one machine's
    // cache to every consumer of the version. A supplied `source_index_path` gets
    // the same treatment when it sits inside the folder it describes.
    let mut never_content = vec![TARGET_INDEX_CACHE_NAME.to_string()];
    if let Some(src_index) = opts.source_index_path.as_deref().filter(|s| !s.is_empty())
        && let Some(rel) = relative_within(&source_folder, src_index)
    {
        never_content.push(rel);
    }
    let filter = RegexPathFilter::new(
        opts.include_filter_regex.as_deref(),
        opts.exclude_filter_regex.as_deref(),
    )?
    .never_paths(never_content);

    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let pool = match &opts.pool {
        Some(p) => p.clone(),
        None => Arc::new(crate::version::build_pool(opts.worker_count)?),
    };
    let cancel = opts.cancel.clone().unwrap_or_default();

    // Same rate-limited-sink pattern as downsync (downsync.rs): wrap the caller's
    // sink (or a no-op) once; `phase`/`report` coalesce emissions.
    let progress: Arc<dyn ProgressSink> = opts
        .progress
        .clone()
        .unwrap_or_else(|| Arc::new(NullProgress));
    let progress = Arc::new(RateLimited::new(progress));

    let mut phases = Vec::new();
    let mut timer = Instant::now();
    let mut lap = |name: &str, t: &mut Instant| {
        let now = Instant::now();
        let ms = now.duration_since(*t).as_millis() as u64;
        *t = now;
        phases.push(crate::options::PhaseTiming {
            phase: name.to_string(),
            millis: ms,
        });
    };

    // 1. Build the version index (scan+chunk, or read a pre-built `.lvi`).
    let compression_tag =
        compression_type_for_name(&opts.compression_algorithm).ok_or_else(|| {
            LongtailError::InvalidArgument(format!(
                "unknown compression algorithm `{}`",
                opts.compression_algorithm
            ))
        })?;

    progress.phase("Indexing version");
    let version_index =
        if let Some(src_index) = opts.source_index_path.as_deref().filter(|s| !s.is_empty()) {
            // Pre-built index: scanning is skipped; hasher comes from the index.
            let bytes = fs_util::read_from_uri(src_index, &s3).await?;
            VersionIndex::from_bytes(&bytes)?
        } else {
            let hash_id = crate::hash_util::hash_identifier_for_name(&opts.hash_algorithm)?;
            let hasher = make_hasher(hash_id)?;
            let on_scan = crate::version::scan_progress_forwarder(progress.clone());
            create_version_index_from_folder(
                &source_folder,
                &filter,
                hasher.as_ref(),
                opts.target_chunk_size,
                compression_tag,
                &pool,
                &cancel,
                Some(&on_scan),
            )?
        };
    lap("index_version", &mut timer);

    // The hasher matching the version index's hash identifier (block hashes must
    // be computed with the same algorithm the chunk hashes were).
    let hasher = make_hasher(version_index.hash_identifier)?;

    // 2. Open the store ReadWrite (Compress(Remote), no cache).
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadWrite,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool: pool.clone(),
        version_local_store_index: None,
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store: Arc<dyn BlockStore> =
        create_block_store_for_uri(&opts.storage_uri, store_opts).await?;

    // Fallible work runs inside this block so the flush + close below happen on
    // a cancel or a failure too: an interrupted upload has still written blocks
    // that the store owes a write-back and an index persist.
    let written = async {
        // 3. Existing content covering this version's chunks (min usage 80).
        let existing = store
            .get_existing_content(&version_index.chunk_hashes, opts.min_block_usage_percent)
            .await?;

        // 4. Missing content = version chunks not already covered, packed into
        // blocks.
        let missing = create_missing_content(
            hasher.as_ref(),
            &existing,
            &version_index,
            opts.target_block_size,
            opts.max_chunks_per_block,
        )?;
        lap("compute_missing", &mut timer);

        // 5. Write content ONLY when there are missing blocks (cmd_upsync.go:145).
        let mut wc = WriteContentStats::default();
        if missing.block_count() > 0 {
            progress.phase("Writing content");
            wc = write_content(
                &store,
                &source_folder,
                &version_index,
                &missing,
                &progress,
                &cancel,
            )
            .await?;
        }
        Ok::<_, LongtailError>((existing, missing, wc))
    }
    .await;

    let (existing, missing, wc) = crate::store_lifecycle::finish_store(&store, written).await?;
    let store_stats = store.stats();
    lap("write_content", &mut timer);

    // 6. Write the target `.lvi` (always, regardless of missing count).
    fs_util::write_to_uri(&opts.target_path, version_index.to_bytes().into(), &s3).await?;

    // 7. Write the version-local `.lsi = merge(existing, missing)` if requested.
    if let Some(lsi_path) = opts
        .version_local_store_index_path
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        let version_local = existing.merge(&missing)?;
        fs_util::write_to_uri(lsi_path, version_local.to_bytes().into(), &s3).await?;
    }
    lap("write_indexes", &mut timer);

    Ok(UpsyncReport {
        target_path: opts.target_path.clone(),
        phases,
        blocks_written: missing.block_count(),
        blocks_missing: missing.block_count(),
        bytes_written: wc.raw_bytes,
        chunks_written: missing.chunk_count(),
        store_stats: store_stats.into(),
    })
}

/// Running totals from [`write_content`].
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct WriteContentStats {
    /// Uncompressed bytes assembled into block payloads.
    pub raw_bytes: u64,
}

/// `Longtail_WriteContent` (longtail.c:4760 + `WriteContentBlockJob` :4559):
/// for each block in `missing` (in block order), assemble its payload by reading
/// each chunk's raw bytes from the source folder (via an asset-part lookup), then
/// `put_stored_block` through the composed (compressing) store.
///
/// The block index (hash/tag/chunk arrays) comes straight from `missing`
/// (`StoreIndex::block_index_at`); the compression happens in the store's
/// `CompressBlockStore` on put.
///
/// `progress` reports completed-block count against `missing.block_count()`; the
/// loop is serial (one sequential `put_stored_block` per block), so a plain
/// counter suffices — no lock, unlike apply.rs's concurrent path. The caller
/// sets the phase label before calling.
pub(crate) async fn write_content(
    store: &Arc<dyn BlockStore>,
    source_folder: &Path,
    version_index: &VersionIndex,
    missing: &StoreIndex,
    progress: &RateLimited,
    cancel: &CancellationToken,
) -> Result<WriteContentStats, LongtailError> {
    // Asset-part lookup: chunk_hash → (asset_index, byte offset within asset),
    // first occurrence in asset order (CreateAssetPartLookup, longtail.c:4429).
    let mut lookup: HashMap<u64, (usize, u64)> = HashMap::new();
    for a in 0..version_index.asset_count() as usize {
        let start = version_index.asset_chunk_index_starts[a] as usize;
        let count = version_index.asset_chunk_counts[a] as usize;
        let mut offset = 0u64;
        for k in 0..count {
            let ci = version_index.asset_chunk_indexes[start + k] as usize;
            let ch = version_index.chunk_hashes[ci];
            let cs = version_index.chunk_sizes[ci];
            lookup.entry(ch).or_insert((a, offset));
            offset += cs as u64;
        }
    }

    // Byte dimension: total decompressed payload = Σ all missing chunk sizes
    // (every block is written); `done_bytes` accrues each block's payload length.
    let total_bytes: u64 = missing.chunk_sizes.iter().map(|&s| s as u64).sum();
    let mut done_bytes = 0u64;

    let mut stats = WriteContentStats::default();
    // One open file handle per source asset (chunks within a block are
    // asset-order-grouped, so the asset changes monotonically and reads stay
    // sequential). We read each chunk's byte range positionally into the block
    // payload rather than slurping the whole asset — so a multi-GB pak never
    // resides in memory (peak here is one block's payload, ~target_block_size).
    let mut cached: Option<(usize, std::fs::File)> = None;

    let total_blocks = missing.block_count();
    for b in 0..missing.block_count() as usize {
        if cancel.is_cancelled() {
            return Err(LongtailError::Cancelled);
        }
        let count = missing.block_chunk_counts[b] as usize;
        let off = missing.block_chunks_offsets[b] as usize;
        let mut payload: Vec<u8> = Vec::new();
        for k in 0..count {
            let ch = missing.chunk_hashes[off + k];
            let cs = missing.chunk_sizes[off + k] as usize;
            let (asset_index, asset_offset) = *lookup.get(&ch).ok_or_else(|| {
                LongtailError::InvalidArgument(format!(
                    "missing chunk {ch:#018x} not found in any source asset"
                ))
            })?;
            let file = match &mut cached {
                Some((idx, f)) if *idx == asset_index => f,
                _ => {
                    let path = version_index.path(asset_index)?;
                    let path = fs_util::strip_trailing_slash(path).to_string();
                    let f = fs_util::open_asset(source_folder, &path)?;
                    cached = Some((asset_index, f));
                    &mut cached.as_mut().unwrap().1
                }
            };
            // Read exactly this chunk's bytes at its offset, straight into the
            // block payload. A short read means the source asset is shorter than
            // the index claims (the old whole-file path returned InvalidArgument).
            file.seek(SeekFrom::Start(asset_offset))
                .map_err(|e| LongtailError::io(format!("seek asset {asset_index}"), e))?;
            let start = payload.len();
            payload.resize(start + cs, 0);
            file.read_exact(&mut payload[start..]).map_err(|e| {
                LongtailError::InvalidArgument(format!(
                    "source asset for chunk {ch:#018x} is shorter than indexed \
                     (read {cs} at offset {asset_offset} failed): {e}"
                ))
            })?;
        }
        stats.raw_bytes += payload.len() as u64;
        let block_index = missing.block_index_at(b).ok_or_else(|| {
            LongtailError::InvalidArgument(format!("missing store index block {b} is malformed"))
        })?;
        let payload_len = payload.len() as u64;
        store
            .put_stored_block(StoredBlock {
                block_index,
                payload,
            })
            .await?;
        done_bytes += payload_len;
        progress.report(Progress {
            done_items: b as u64 + 1,
            total_items: total_blocks as u64,
            done_bytes,
            total_bytes,
        });
    }
    Ok(stats)
}
