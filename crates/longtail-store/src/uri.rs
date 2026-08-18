//! Block-level URI dispatch — mirrors `remotestore.go:1949-2056`
//! (`CreateBlockStoreForURI`).
//!
//! Every scheme constructs a [`RemoteBlockStore`] over the corresponding blob
//! store — **including plain paths and `file://`** (Go routes those to C's
//! FSBlockStore; the Rust port deliberately does NOT port FSBlockStore as a
//! separate component — [`RemoteBlockStore`] over an [`FsBlobStore`] covers it,
//! a documented substitution).
//!
//! Composition is **compression-outermost** (`cmd_downsync.go:206-226`), so
//! cached blocks are stored compressed: `Compress(Cache(Remote))` with a cache
//! dir, `Compress(Remote)` without. (Go wraps ShareBlockStore outermost; that is
//! subsumed by the prefetch coalescing in [`RemoteBlockStore`].)
//!
//! Worker-count defaults (`CreateBlockStoreForURI` :1977-2032, documented at
//! commands/commands.go:12): fsblob → `NumCPU` (uncapped); networked (s3) →
//! `min(NumCPU, 8)`. A caller `worker_count` of `0` requests the default.

use std::path::PathBuf;
use std::sync::Arc;

use longtail_core::StoreIndex;

use crate::blob::{BlobStore, FsBlobStore};
use crate::block_store::BlockStore;
use crate::cache::CacheBlockStore;
use crate::compress::CompressBlockStore;
use crate::error::StoreError;
use crate::remote::RemoteBlockStore;
use crate::sync::AccessType;

#[cfg(feature = "s3")]
use crate::blob::{S3BlobStore, S3Options};

/// Options for [`create_block_store_for_uri`].
pub struct BlockStoreOpts {
    /// Access mode for the store index.
    pub access_type: AccessType,
    /// Concurrent block-I/O bound; `0` = resolve the scheme default.
    pub worker_count: usize,
    /// Optional local cache directory (`.lrb` blocks). `None` = no cache.
    pub cache_dir: Option<PathBuf>,
    /// The rayon pool the [`CompressBlockStore`] uses for codec work.
    pub pool: Arc<rayon::ThreadPool>,
    /// A pre-loaded, pre-merged **version-local store index override** for
    /// `ReadOnly` reads (golongtail's `optionalStoreIndexPaths`,
    /// remotestore.go:1897): when `Some` and `access_type == ReadOnly`, the
    /// remote store uses this index instead of scanning the store's `.lsi`
    /// shards. The facade reads the `.lvi`/`.lsi` URIs and merges them (falling
    /// back to `None` on any read/merge failure, matching Go's break-and-scan).
    pub version_local_store_index: Option<StoreIndex>,
    /// Ceiling on a single blob read; `None` uses
    /// [`crate::blob::DEFAULT_MAX_BLOB_BYTES`]. Raise it only for a store that
    /// genuinely writes blocks larger than the default — it exists so a store
    /// cannot choose this process's memory use.
    pub max_block_bytes: Option<u64>,
    /// S3 credential/endpoint options (feature `s3`).
    #[cfg(feature = "s3")]
    pub s3_options: S3Options,
}

impl BlockStoreOpts {
    /// Minimal options: an access type + codec pool; no cache, default workers.
    pub fn new(access_type: AccessType, pool: Arc<rayon::ThreadPool>) -> BlockStoreOpts {
        BlockStoreOpts {
            access_type,
            worker_count: 0,
            cache_dir: None,
            pool,
            version_local_store_index: None,
            max_block_bytes: None,
            #[cfg(feature = "s3")]
            s3_options: S3Options::default(),
        }
    }
}

fn networked_worker_count(requested: usize) -> usize {
    if requested != 0 {
        return requested;
    }
    num_cpus::get().clamp(1, 8)
}

fn local_worker_count(requested: usize) -> usize {
    if requested != 0 {
        return requested;
    }
    num_cpus::get().max(1)
}

/// The worker count [`create_block_store_for_uri`] resolves for `uri` when the
/// caller requests `requested` (`0` = the scheme default: fs → `NumCPU`
/// uncapped; networked (s3) → `min(NumCPU, 8)` — `CreateBlockStoreForURI`,
/// remotestore.go:1977-2032 @49a20e1). Exposed so callers can bound *their own*
/// concurrency (e.g. the facade's concurrent block apply) to
/// the same value without introducing a second knob.
pub fn resolved_worker_count(uri: &str, requested: usize) -> usize {
    // s3:// is the only networked scheme (gs/abfs are rejected by
    // `resolve_backend`); every other accepted form is a filesystem store.
    let is_networked = crate::blob::split_scheme(uri)
        .map(|(scheme, _)| scheme == "s3")
        .unwrap_or(false);
    if is_networked {
        networked_worker_count(requested)
    } else {
        local_worker_count(requested)
    }
}

/// Construct a composed block store for `uri`. Returns
/// `Compress(Cache(Remote))` or `Compress(Remote)`.
pub async fn create_block_store_for_uri(
    uri: &str,
    opts: BlockStoreOpts,
) -> Result<Arc<dyn BlockStore>, StoreError> {
    create_block_store_for_uri_with_budget(uri, opts, None, None).await
}

/// [`create_block_store_for_uri`] with two side-channel knobs the plain
/// constructor defaults:
///
/// - `max_prefetch_bytes` — the underlying [`RemoteBlockStore`]'s prefetch byte
///   budget (`None` → [`crate::remote::DEFAULT_MAX_PREFETCH_BYTES`]).
///   Test-oriented (the deadlock regression test); correctness must never depend
///   on it — it bounds memory held by unconsumed prefetches, never progress.
/// - `cache_size_limit` — the local block cache's byte budget (`None` →
///   unbounded). When set, the [`CacheBlockStore`] tracks per-block access time
///   and LRU-evicts to the budget on close. This is a real production knob
///   (`downsync`/`get`); it lives here rather than on [`BlockStoreOpts`] only to
///   avoid touching every literal construction of that struct.
pub async fn create_block_store_for_uri_with_budget(
    uri: &str,
    opts: BlockStoreOpts,
    max_prefetch_bytes: Option<usize>,
    cache_size_limit: Option<u64>,
) -> Result<Arc<dyn BlockStore>, StoreError> {
    let (blob_store, worker_count): (Arc<dyn BlobStore>, usize) = resolve_backend(uri, &opts)?;

    // The version-local store index override only applies to ReadOnly reads
    // (remotestore.go:1897); for other access types it is ignored.
    let override_index = if opts.access_type == AccessType::ReadOnly {
        opts.version_local_store_index.clone()
    } else {
        None
    };

    let remote: Arc<dyn BlockStore> = Arc::new(
        RemoteBlockStore::with_prefetch_budget(
            blob_store,
            opts.access_type,
            worker_count,
            max_prefetch_bytes.unwrap_or(crate::remote::DEFAULT_MAX_PREFETCH_BYTES),
            override_index,
        )
        .await?,
    );

    let base: Arc<dyn BlockStore> = match &opts.cache_dir {
        Some(dir) => Arc::new(CacheBlockStore::new(dir, remote, cache_size_limit).await?),
        None => remote,
    };

    Ok(Arc::new(CompressBlockStore::new(base, opts.pool.clone())))
}

fn resolve_backend(
    uri: &str,
    opts: &BlockStoreOpts,
) -> Result<(Arc<dyn BlobStore>, usize), StoreError> {
    // Set fs `enable_locking` by access type: a read-only downsync needs no write
    // CAS, and an enabled read-lock scatters never-unlinked `._lck` files into
    // customer stores. Only writing access types lock.
    let fs_locking = opts.access_type != AccessType::ReadOnly;
    let max_blob_bytes = opts
        .max_block_bytes
        .unwrap_or(crate::blob::DEFAULT_MAX_BLOB_BYTES);

    // fsblob:// and UNC/network paths → fs blob store (local worker count).
    if let Some(rest) = uri.strip_prefix("fsblob://") {
        return Ok((
            Arc::new(FsBlobStore::new(rest, fs_locking).with_max_read_bytes(max_blob_bytes)),
            local_worker_count(opts.worker_count),
        ));
    }
    if uri.starts_with("\\\\?\\") || uri.starts_with('\\') {
        return Ok((
            Arc::new(FsBlobStore::new(uri, fs_locking).with_max_read_bytes(max_blob_bytes)),
            local_worker_count(opts.worker_count),
        ));
    }

    if let Some((scheme, rest)) = crate::blob::split_scheme(uri) {
        match scheme {
            "gs" => {
                return Err(StoreError::NotSupported(format!(
                    "gs:// (GCS) block stores are not supported; uri `{uri}`"
                )));
            }
            "abfs" | "abfss" => {
                return Err(StoreError::NotSupported(format!(
                    "azure storage not implemented; uri `{uri}`"
                )));
            }
            "file" => {
                return Ok((
                    Arc::new(
                        FsBlobStore::new(rest, fs_locking).with_max_read_bytes(max_blob_bytes),
                    ),
                    local_worker_count(opts.worker_count),
                ));
            }
            "s3" => {
                #[cfg(feature = "s3")]
                {
                    // `BlockStoreOpts::max_block_bytes` wins when set, so one
                    // knob governs both backends; otherwise the S3 options keep
                    // their own default.
                    let mut s3_options = opts.s3_options.clone();
                    if let Some(limit) = opts.max_block_bytes {
                        s3_options.max_read_bytes = limit;
                    }
                    let store = S3BlobStore::from_uri_with_options(uri, s3_options)?;
                    return Ok((Arc::new(store), networked_worker_count(opts.worker_count)));
                }
                #[cfg(not(feature = "s3"))]
                {
                    return Err(StoreError::NotSupported(
                        "s3:// support was compiled out (feature `s3`)".into(),
                    ));
                }
            }
            _ => {
                if scheme.len() == 1 {
                    // Windows drive letter `c:\...` — a path.
                    return Ok((
                        Arc::new(
                            FsBlobStore::new(uri, fs_locking).with_max_read_bytes(max_blob_bytes),
                        ),
                        local_worker_count(opts.worker_count),
                    ));
                }
                return Err(StoreError::InvalidUri {
                    uri: uri.to_string(),
                    reason: format!("unknown scheme `{scheme}`"),
                });
            }
        }
    }

    // No scheme → filesystem path (fs blob store, local worker count).
    Ok((
        Arc::new(FsBlobStore::new(uri, fs_locking).with_max_read_bytes(max_blob_bytes)),
        local_worker_count(opts.worker_count),
    ))
}
