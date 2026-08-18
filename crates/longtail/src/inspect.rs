//! Read-only inspection entry points used by `ls` / `print-version` /
//! `validate-version`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use longtail_core::{StoreIndex, VersionIndex, validate_store};
use longtail_store::AccessType;
use longtail_store::block_store::BlockStore;
use longtail_store::uri::{BlockStoreOpts, create_block_store_for_uri};

use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};

#[cfg(feature = "s3")]
fn default_s3() -> S3OptionsArg {
    longtail_store::S3Options::default()
}
#[cfg(not(feature = "s3"))]
#[allow(dead_code)]
fn default_s3() -> S3OptionsArg {}

/// Read + parse a version index from a URI (local path, `file://`, or `s3://`).
pub async fn read_version_index_from_uri(uri: &str) -> Result<VersionIndex, LongtailError> {
    let bytes = fs_util::read_from_uri(uri, &default_s3()).await?;
    Ok(VersionIndex::from_bytes(&bytes)?)
}

/// Options for [`validate_version`].
pub struct ValidateVersionOptions {
    pub storage_uri: String,
    pub version_index_path: String,
    pub remote_worker_count: usize,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl ValidateVersionOptions {
    pub fn new(storage_uri: impl Into<String>, version_index_path: impl Into<String>) -> Self {
        ValidateVersionOptions {
            storage_uri: storage_uri.into(),
            version_index_path: version_index_path.into(),
            remote_worker_count: 0,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// `validate-version`: confirm the store covers every chunk the version needs
/// (`GetExistingStoreIndex(all chunks, min-usage 0)` + `ValidateStore`,
/// cmd_validateversion.go:61-74).
pub async fn validate_version(opts: ValidateVersionOptions) -> Result<(), LongtailError> {
    let vi = read_version_index_from_uri(&opts.version_index_path).await?;
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .map_err(|e| LongtailError::InvalidArgument(format!("rayon pool: {e}")))?,
    );
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool,
        version_local_store_index: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options,
    };
    let store: Arc<dyn BlockStore> =
        create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let store_index = store.get_existing_content(&vi.chunk_hashes, 0).await?;
    store.close().await?;
    validate_store(&store_index, &vi).map_err(LongtailError::from)
}

fn single_thread_pool() -> Result<Arc<rayon::ThreadPool>, LongtailError> {
    Ok(Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .map_err(|e| LongtailError::InvalidArgument(format!("rayon pool: {e}")))?,
    ))
}

/// Read + parse a store index (`.lsi`) from a URI (local / `file://` / `s3://`).
pub async fn read_store_index_from_uri(uri: &str) -> Result<StoreIndex, LongtailError> {
    let bytes = fs_util::read_from_uri(uri, &default_s3()).await?;
    Ok(StoreIndex::from_bytes(&bytes)?)
}

/// Summary numbers for `print-store` (cmd_printstore.go).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreIndexStats {
    pub version: u32,
    pub hash_identifier: u32,
    pub block_count: u32,
    pub chunk_count: u32,
    /// Σ of every chunk size across all blocks (with duplicates).
    pub stored_chunks_size: u64,
    /// Σ of chunk sizes over the unique chunk-hash set.
    pub unique_stored_chunks_size: u64,
}

/// Compute `print-store`'s numbers (`--details` sizes are always computed here;
/// the CLI decides whether to show them).
pub fn store_index_stats(si: &StoreIndex) -> StoreIndexStats {
    let stored: u64 = si.chunk_sizes.iter().map(|&s| s as u64).sum();
    let mut seen: HashMap<u64, u32> = HashMap::with_capacity(si.chunk_hashes.len());
    for (i, &h) in si.chunk_hashes.iter().enumerate() {
        seen.entry(h).or_insert(si.chunk_sizes[i]);
    }
    let unique: u64 = seen.values().map(|&s| s as u64).sum();
    StoreIndexStats {
        version: longtail_core::STORE_INDEX_VERSION,
        hash_identifier: si.hash_identifier,
        block_count: si.block_count(),
        chunk_count: si.chunk_count(),
        stored_chunks_size: stored,
        unique_stored_chunks_size: unique,
    }
}

/// Options for [`init_remote_store`].
pub struct InitRemoteStoreOptions {
    pub storage_uri: String,
    pub remote_worker_count: usize,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl InitRemoteStoreOptions {
    pub fn new(storage_uri: impl Into<String>) -> Self {
        InitRemoteStoreOptions {
            storage_uri: storage_uri.into(),
            remote_worker_count: 0,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// `init-remote-store` (cmd_initremotestore.go): open the store with the `Init`
/// access type (rebuild the store index from a block scan) and persist it.
/// Returns the rebuilt block count.
pub async fn init_remote_store(opts: InitRemoteStoreOptions) -> Result<u32, LongtailError> {
    let store_opts = BlockStoreOpts {
        access_type: AccessType::Init,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool: single_thread_pool()?,
        version_local_store_index: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options,
    };
    let store = create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    // Forcing the index load triggers the Init rebuild + write-back; the empty
    // chunk request returns an empty retargetted index (block count via a
    // full-store retarget below would double-scan, so we report from the flush).
    let _ = store.get_existing_content(&[], 0).await?;
    store.flush().await?;
    let stats = store.stats();
    store.close().await?;
    // The rebuilt store index is now persisted; report gets as a rough progress
    // signal (Go logs the block count from the rebuild — not surfaced here).
    Ok(stats.get_count as u32)
}

/// Options for [`create_version_store_index`].
pub struct CreateVersionStoreIndexOptions {
    /// A **version-index** URI (`--source-path`) — NOT a source folder.
    pub source_path: String,
    /// Output `.lsi` URI (`--version-local-store-index-path`).
    pub version_local_store_index_path: String,
    pub storage_uri: String,
    pub remote_worker_count: usize,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl CreateVersionStoreIndexOptions {
    pub fn new(
        source_path: impl Into<String>,
        version_local_store_index_path: impl Into<String>,
        storage_uri: impl Into<String>,
    ) -> Self {
        CreateVersionStoreIndexOptions {
            source_path: source_path.into(),
            version_local_store_index_path: version_local_store_index_path.into(),
            storage_uri: storage_uri.into(),
            remote_worker_count: 0,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// `create-version-store-index` (cmd_createversionstoreindex.go): read a version
/// index, retarget the store to just its chunks (`GetExistingStoreIndex`, usage
/// percent hardcoded to **0**), and write the resulting `.lsi`.
pub async fn create_version_store_index(
    opts: CreateVersionStoreIndexOptions,
) -> Result<(), LongtailError> {
    let vi = read_version_index_from_uri(&opts.source_path).await?;
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool: single_thread_pool()?,
        version_local_store_index: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store = create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let retargetted = store.get_existing_content(&vi.chunk_hashes, 0).await?;
    store.close().await?;
    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options;
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();
    fs_util::write_to_uri(
        &opts.version_local_store_index_path,
        &retargetted.to_bytes(),
        &s3,
    )
    .await
}

/// Options for [`print_version_usage_stats`].
pub struct PrintVersionUsageOptions {
    pub storage_uri: String,
    pub version_index_path: String,
    pub cache_path: Option<PathBuf>,
    pub remote_worker_count: usize,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl PrintVersionUsageOptions {
    pub fn new(storage_uri: impl Into<String>, version_index_path: impl Into<String>) -> Self {
        PrintVersionUsageOptions {
            storage_uri: storage_uri.into(),
            version_index_path: version_index_path.into(),
            cache_path: None,
            remote_worker_count: 0,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// `print-version-usage` numbers (cmd_printVersionUsage.go:145-181).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionUsageStats {
    pub block_usage_percent: u32,
    pub asset_fragmentation_percent: u32,
}

/// Compute block-usage + asset-fragmentation for a version against a store.
///
/// Divergence from golongtail (documented): golongtail fetches every covering
/// block to build the chunk→block map; the store index returned by
/// `get_existing_content` already carries that map, so we derive it there (no
/// block downloads). The arithmetic mirrors cmd_printVersionUsage.go:145-178.
pub async fn print_version_usage_stats(
    opts: PrintVersionUsageOptions,
) -> Result<VersionUsageStats, LongtailError> {
    let vi = read_version_index_from_uri(&opts.version_index_path).await?;
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: opts.cache_path.clone(),
        pool: single_thread_pool()?,
        version_local_store_index: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store = create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let existing = store.get_existing_content(&vi.chunk_hashes, 0).await?;
    store.close().await?;

    // chunk_hash → block_hash from the retargetted store index.
    let mut block_lookup: HashMap<u64, u64> = HashMap::new();
    let mut block_chunk_count: u64 = 0;
    for b in 0..existing.block_count() as usize {
        let count = existing.block_chunk_counts[b] as usize;
        let off = existing.block_chunks_offsets[b] as usize;
        block_chunk_count += count as u64;
        for k in 0..count {
            block_lookup.insert(existing.chunk_hashes[off + k], existing.block_hashes[b]);
        }
    }
    let block_usage = (100 * existing.chunk_count() as u64)
        .checked_div(block_chunk_count)
        .map_or(100, |v| v as u32);

    // Asset fragmentation (cmd_printVersionUsage.go:150-178).
    let mut asset_count: u64 = 0;
    let mut asset_fragment_count: u64 = 0;
    for a in 0..vi.asset_count() as usize {
        let count = vi.asset_chunk_counts[a] as usize;
        if count == 0 {
            continue;
        }
        asset_count += 1;
        let start = vi.asset_chunk_index_starts[a] as usize;
        let mut last_block: Option<u64> = None;
        for k in 0..count {
            let ci = vi.asset_chunk_indexes[start + k] as usize;
            let ch = vi.chunk_hashes[ci];
            let blk = block_lookup.get(&ch).copied();
            if blk != last_block {
                asset_fragment_count += 1;
                last_block = blk;
            }
        }
    }
    // Mirrors Go's `(100*frag)/count - 100` with u32 wrapping semantics; 0 when
    // there are no assets with chunks.
    let fragmentation = (100 * asset_fragment_count)
        .checked_div(asset_count)
        .map_or(0, |v| (v as u32).wrapping_sub(100));

    Ok(VersionUsageStats {
        block_usage_percent: block_usage,
        asset_fragmentation_percent: fragmentation,
    })
}
