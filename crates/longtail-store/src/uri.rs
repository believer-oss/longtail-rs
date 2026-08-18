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
//! subsumed by the prefetch coalescing in [`RemoteBlockStore`], per plan §6.)
//!
//! Worker-count defaults (`CreateBlockStoreForURI` :1977-2032, documented at
//! commands/commands.go:12): fsblob → `NumCPU` (uncapped); networked (s3) →
//! `min(NumCPU, 8)`. A caller `worker_count` of `0` requests the default.

use std::path::PathBuf;
use std::sync::Arc;

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

/// Construct a composed block store for `uri`. Returns
/// `Compress(Cache(Remote))` or `Compress(Remote)`.
pub async fn create_block_store_for_uri(
    uri: &str,
    opts: BlockStoreOpts,
) -> Result<Arc<dyn BlockStore>, StoreError> {
    let (blob_store, worker_count): (Arc<dyn BlobStore>, usize) = resolve_backend(uri, &opts)?;

    let remote: Arc<dyn BlockStore> =
        Arc::new(RemoteBlockStore::new(blob_store, opts.access_type, worker_count).await?);

    let base: Arc<dyn BlockStore> = match &opts.cache_dir {
        Some(dir) => Arc::new(CacheBlockStore::new(dir, remote).await?),
        None => remote,
    };

    Ok(Arc::new(CompressBlockStore::new(base, opts.pool.clone())))
}

fn resolve_backend(
    uri: &str,
    opts: &BlockStoreOpts,
) -> Result<(Arc<dyn BlobStore>, usize), StoreError> {
    // fsblob:// and UNC/network paths → fs blob store (local worker count).
    if let Some(rest) = uri.strip_prefix("fsblob://") {
        return Ok((
            Arc::new(FsBlobStore::new(rest, true)),
            local_worker_count(opts.worker_count),
        ));
    }
    if uri.starts_with("\\\\?\\") || uri.starts_with('\\') {
        return Ok((
            Arc::new(FsBlobStore::new(uri, true)),
            local_worker_count(opts.worker_count),
        ));
    }

    if let Some((scheme, rest)) = crate::blob::split_scheme(uri) {
        match scheme {
            "gs" => {
                return Err(StoreError::NotSupported(format!(
                    "gs:// (GCS) block stores are not supported (planning §6); uri `{uri}`"
                )));
            }
            "abfs" | "abfss" => {
                return Err(StoreError::NotSupported(format!(
                    "azure storage not implemented; uri `{uri}`"
                )));
            }
            "file" => {
                return Ok((
                    Arc::new(FsBlobStore::new(rest, true)),
                    local_worker_count(opts.worker_count),
                ));
            }
            "s3" => {
                #[cfg(feature = "s3")]
                {
                    let store = S3BlobStore::from_uri_with_options(uri, opts.s3_options.clone())?;
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
                        Arc::new(FsBlobStore::new(uri, true)),
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
        Arc::new(FsBlobStore::new(uri, true)),
        local_worker_count(opts.worker_count),
    ))
}
