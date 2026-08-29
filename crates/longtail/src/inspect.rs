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
///
/// Takes the S3 options rather than defaulting them: a URI is not enough to
/// reach a store behind a custom endpoint, and defaulting here silently ignored
/// the caller's endpoint while the same command honoured it for block I/O.
pub async fn read_version_index_from_uri(
    uri: &str,
    s3_options: &S3OptionsArg,
) -> Result<VersionIndex, LongtailError> {
    let bytes = fs_util::read_from_uri(uri, s3_options).await?;
    Ok(VersionIndex::from_bytes(&bytes)?)
}

/// Options for [`validate_version`].
#[non_exhaustive]
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
    let vi = read_version_index_from_uri(&opts.version_index_path, &crate::s3_arg!(opts)).await?;
    let pool = Arc::new(crate::version::build_pool(1)?);
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool,
        version_local_store_index: None,
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options,
    };
    let store: Arc<dyn BlockStore> =
        create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let fetched = store
        .get_existing_content(&vi.chunk_hashes, 0)
        .await
        .map_err(LongtailError::from);
    let store_index = crate::store_lifecycle::finish_store(&store, fetched).await?;
    validate_store(&store_index, &vi).map_err(LongtailError::from)
}

fn single_thread_pool() -> Result<Arc<rayon::ThreadPool>, LongtailError> {
    // Via `build_pool` so this pool gets the same panic handler as every other:
    // a panicking codec job must not abort the process.
    Ok(Arc::new(crate::version::build_pool(1)?))
}

/// Read + parse a store index (`.lsi`) from a URI (local / `file://` / `s3://`).
///
/// Takes the S3 options for the same reason as [`read_version_index_from_uri`].
pub async fn read_store_index_from_uri(
    uri: &str,
    s3_options: &S3OptionsArg,
) -> Result<StoreIndex, LongtailError> {
    let bytes = fs_util::read_from_uri(uri, s3_options).await?;
    Ok(StoreIndex::from_bytes(&bytes)?)
}

/// Summary numbers for `print-store` (cmd_printstore.go).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoreIndexStats {
    pub version: u32,
    pub hash_identifier: u32,
    pub block_count: u32,
    pub chunk_count: u32,
    /// Σ of every chunk size across all blocks (with duplicates).
    pub stored_chunks_size: u64,
}

/// Compute `print-store`'s cheap numbers: counts and the total stored bytes,
/// all a single pass or less.
///
/// The deduplicated total is [`unique_stored_chunks_size`], kept separate
/// because it is the expensive one and only `--details` shows it.
pub fn store_index_stats(si: &StoreIndex) -> StoreIndexStats {
    StoreIndexStats {
        version: longtail_core::STORE_INDEX_VERSION,
        hash_identifier: si.hash_identifier,
        block_count: si.block_count(),
        chunk_count: si.chunk_count(),
        stored_chunks_size: si.chunk_sizes.iter().map(|&s| s as u64).sum(),
    }
}

/// Σ of chunk sizes counted once per distinct chunk hash.
///
/// Separate from [`store_index_stats`] because deduplicating is the expensive
/// part and only `--details` prints the result — a `print-store` without it was
/// paying for a number nobody saw.
///
/// A `HashMap<u64, u32>` reserved to the chunk count cost more than the index it
/// summarised: at ~143M chunks it took ~2.7 GB before a single entry went in.
/// Sorting an index permutation is bounded at four bytes per chunk whatever the
/// store's deduplication ratio, and since a chunk hash identifies its content,
/// equal hashes carry equal sizes — so summing the first of each run is the same
/// answer.
pub fn unique_stored_chunks_size(si: &StoreIndex) -> u64 {
    let mut order: Vec<u32> = (0..si.chunk_hashes.len() as u32).collect();
    order.sort_unstable_by_key(|&i| si.chunk_hashes[i as usize]);
    let mut unique: u64 = 0;
    let mut prev: Option<u64> = None;
    for &i in &order {
        let h = si.chunk_hashes[i as usize];
        if prev != Some(h) {
            unique += si.chunk_sizes[i as usize] as u64;
            prev = Some(h);
        }
    }
    unique
}

/// Options for [`init_remote_store`].
#[non_exhaustive]
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
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options,
    };
    let store = create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    // Forcing the index load triggers the Init rebuild + write-back; the empty
    // chunk request returns an empty retargetted index (block count via a
    // full-store retarget below would double-scan, so we report from the flush).
    let loaded = store
        .get_existing_content(&[], 0)
        .await
        .map(|_| ())
        .map_err(LongtailError::from);
    let stats = store.stats();
    // `Init` rebuilds and persists the index on close, so a failed load must
    // still reach it rather than abandoning a half-initialised store.
    crate::store_lifecycle::finish_store(&store, loaded).await?;
    // The rebuilt store index is now persisted; report gets as a rough progress
    // signal (Go logs the block count from the rebuild — not surfaced here).
    Ok(stats.get_count as u32)
}

/// Options for [`create_version_store_index`].
#[non_exhaustive]
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
    let vi = read_version_index_from_uri(&opts.source_path, &crate::s3_arg!(opts)).await?;
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool: single_thread_pool()?,
        version_local_store_index: None,
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store = create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let fetched = store
        .get_existing_content(&vi.chunk_hashes, 0)
        .await
        .map_err(LongtailError::from);
    let retargetted = crate::store_lifecycle::finish_store(&store, fetched).await?;
    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options;
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();
    fs_util::write_to_uri(
        &opts.version_local_store_index_path,
        retargetted.to_bytes().into(),
        &s3,
    )
    .await
}

/// Options for [`print_version_usage_stats`].
#[non_exhaustive]
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
#[non_exhaustive]
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
    let vi = read_version_index_from_uri(&opts.version_index_path, &crate::s3_arg!(opts)).await?;
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: opts.cache_path.clone(),
        pool: single_thread_pool()?,
        version_local_store_index: None,
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store = create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let fetched = store
        .get_existing_content(&vi.chunk_hashes, 0)
        .await
        .map_err(LongtailError::from);
    let existing = crate::store_lifecycle::finish_store(&store, fetched).await?;

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

#[cfg(test)]
mod tests {
    use longtail_core::StoreIndex;

    use super::{store_index_stats, unique_stored_chunks_size};

    /// A chunk stored in more than one block counts once. The permutation sort
    /// replaced a hash map here, so the dedup itself needs holding to the answer
    /// rather than to the implementation.
    #[test]
    fn unique_size_counts_a_repeated_chunk_once() {
        let si = StoreIndex {
            hash_identifier: 1,
            block_hashes: vec![10, 11],
            // Chunk `7` appears in both blocks; `8` and `9` once each.
            chunk_hashes: vec![7, 8, 7, 9],
            block_chunks_offsets: vec![0, 2],
            block_chunk_counts: vec![2, 2],
            block_tags: vec![0, 0],
            chunk_sizes: vec![100, 200, 100, 300],
        };
        assert_eq!(store_index_stats(&si).stored_chunks_size, 700);
        assert_eq!(unique_stored_chunks_size(&si), 600);
    }

    /// Every chunk distinct — unique equals stored.
    #[test]
    fn unique_size_equals_stored_when_nothing_repeats() {
        let si = StoreIndex {
            hash_identifier: 1,
            block_hashes: vec![10],
            chunk_hashes: vec![1, 2, 3],
            block_chunks_offsets: vec![0],
            block_chunk_counts: vec![3],
            block_tags: vec![0],
            chunk_sizes: vec![5, 6, 7],
        };
        assert_eq!(store_index_stats(&si).stored_chunks_size, 18);
        assert_eq!(unique_stored_chunks_size(&si), 18);
    }

    /// An empty index must not panic or report anything.
    #[test]
    fn unique_size_of_an_empty_index_is_zero() {
        let si = StoreIndex::empty(1);
        assert_eq!(store_index_stats(&si).stored_chunks_size, 0);
        assert_eq!(unique_stored_chunks_size(&si), 0);
    }
}
