//! The async upload-path orchestration (`cmd_upsync.go`): scan+chunk the source
//! (or read a pre-built `.lvi`), compute the missing content, pack + write the
//! blocks, then write the target `.lvi` and (optionally) the version-local
//! `.lsi = MergeStoreIndex(existing, missing)`.
//!
//! [`write_content`] (the `Longtail_WriteContent` port) is shared with
//! clone-store.

use std::collections::HashMap;
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
use crate::path_filter::RegexPathFilter;
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
pub async fn upsync(opts: UpsyncOptions) -> Result<UpsyncReport, LongtailError> {
    if opts.use_legacy_write {
        return Err(LongtailError::LegacyWriteUnsupported);
    }
    let source_folder = PathBuf::from(&opts.source_path);

    let filter = RegexPathFilter::new(
        opts.include_filter_regex.as_deref(),
        opts.exclude_filter_regex.as_deref(),
    )?;

    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let pool = match &opts.pool {
        Some(p) => p.clone(),
        None => Arc::new(crate::version::build_pool(opts.worker_count)?),
    };
    let cancel = opts.cancel.clone().unwrap_or_default();

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

    let version_index =
        if let Some(src_index) = opts.source_index_path.as_deref().filter(|s| !s.is_empty()) {
            // Pre-built index: scanning is skipped; hasher comes from the index.
            let bytes = fs_util::read_from_uri(src_index, &s3).await?;
            VersionIndex::from_bytes(&bytes)?
        } else {
            let hash_id = crate::hash_util::hash_identifier_for_name(&opts.hash_algorithm)?;
            let hasher = make_hasher(hash_id)?;
            create_version_index_from_folder(
                &source_folder,
                &filter,
                hasher.as_ref(),
                opts.target_chunk_size,
                compression_tag,
                &pool,
                &cancel,
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
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store: Arc<dyn BlockStore> =
        create_block_store_for_uri(&opts.storage_uri, store_opts).await?;

    // 3. Existing content covering this version's chunks (min usage 80).
    let existing = store
        .get_existing_content(&version_index.chunk_hashes, opts.min_block_usage_percent)
        .await?;

    // 4. Missing content = version chunks not already covered, packed into blocks.
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
        wc = write_content(&store, &source_folder, &version_index, &missing, &cancel).await?;
    }
    store.flush().await?;
    store.close().await?;
    let store_stats = store.stats();
    lap("write_content", &mut timer);

    // 6. Write the target `.lvi` (always, regardless of missing count).
    fs_util::write_to_uri(&opts.target_path, &version_index.to_bytes(), &s3).await?;

    // 7. Write the version-local `.lsi = merge(existing, missing)` if requested.
    if let Some(lsi_path) = opts
        .version_local_store_index_path
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        let version_local = existing.merge(&missing)?;
        fs_util::write_to_uri(lsi_path, &version_local.to_bytes(), &s3).await?;
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
pub(crate) async fn write_content(
    store: &Arc<dyn BlockStore>,
    source_folder: &Path,
    version_index: &VersionIndex,
    missing: &StoreIndex,
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

    let mut stats = WriteContentStats::default();
    // Single-asset read cache (chunks within a block are asset-order-grouped, so
    // the asset changes monotonically — one open per asset per block, as in C).
    let mut cached: Option<(usize, Vec<u8>)> = None;

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
            let bytes = match &cached {
                Some((idx, data)) if *idx == asset_index => data,
                _ => {
                    let path = version_index.path(asset_index)?;
                    let path = fs_util::strip_trailing_slash(path).to_string();
                    let data = fs_util::read_asset(source_folder, &path, false)?;
                    cached = Some((asset_index, data));
                    &cached.as_ref().unwrap().1
                }
            };
            let start = asset_offset as usize;
            let end = start + cs;
            if end > bytes.len() {
                return Err(LongtailError::InvalidArgument(format!(
                    "source asset for chunk {ch:#018x} is shorter than indexed ({} < {end})",
                    bytes.len()
                )));
            }
            payload.extend_from_slice(&bytes[start..end]);
        }
        stats.raw_bytes += payload.len() as u64;
        let block_index = missing.block_index_at(b).ok_or_else(|| {
            LongtailError::InvalidArgument(format!("missing store index block {b} is malformed"))
        })?;
        store
            .put_stored_block(StoredBlock {
                block_index,
                payload,
            })
            .await?;
    }
    Ok(stats)
}
