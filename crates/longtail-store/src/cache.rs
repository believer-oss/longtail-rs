//! [`CacheBlockStore`] — a two-tier local/remote block cache (port of
//! `cacheblockstore`, port-map §3).
//!
//! **Warm-cache compat** (`rust-port-4.md`): existing launcher caches were
//! written by C's FSBlockStore as `chunks/<first-4-hex>/0x<hash16>.lrb`
//! (golongtail passes an empty extension → C's default `.lrb`). This store uses
//! the same block-path scheme with the `.lrb` extension and probes block files
//! directly. The stored bytes are byte-identical to the `.lsb` stored-block
//! output — only the extension differs (the Stage 1 passthrough property).
//!
//! The cache-dir `store.lsi` C maintains is treated as **advisory**: this store
//! never reads or trusts it (content queries forward to the remote), so a stale
//! cache index cannot cause a wrong answer. Deliberate compat choice, cheap to
//! change since caches are disposable.
//!
//! Composition (`rust-port-4.md`): compression is outermost, so a
//! `CacheBlockStore` stores whatever bytes the remote returns — **compressed**
//! blocks. It never (de)compresses; that is [`crate::compress::CompressBlockStore`]'s
//! job one layer up.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use longtail_core::{StoreIndex, StoredBlock};

use crate::blob::{BlobClient, BlobStore, FsBlobStore};
use crate::block_store::{BlockStore, StatsSnapshot};
use crate::error::StoreError;

/// A local filesystem cache in front of a remote [`BlockStore`].
pub struct CacheBlockStore {
    cache_client: Arc<dyn BlobClient>,
    remote: Arc<dyn BlockStore>,
}

impl CacheBlockStore {
    /// Open a cache rooted at `cache_dir` (blocks stored as `.lrb`) in front of
    /// `remote`.
    pub async fn new(
        cache_dir: impl AsRef<Path>,
        remote: Arc<dyn BlockStore>,
    ) -> Result<CacheBlockStore, StoreError> {
        let store = FsBlobStore::new(cache_dir, false);
        let cache_client: Arc<dyn BlobClient> = Arc::from(store.new_client().await?);
        Ok(CacheBlockStore {
            cache_client,
            remote,
        })
    }
}

/// Cache block path: `chunks/<first-4-hex>/0x<16-hex>.lrb` (C's FSBlockStore
/// default extension).
fn cache_block_path(block_hash: u64) -> String {
    let file_name = format!("0x{block_hash:016x}.lrb");
    let sub = &file_name[2..6];
    format!("chunks/{sub}/{file_name}")
}

#[async_trait]
impl BlockStore for CacheBlockStore {
    async fn put_stored_block(&self, block: StoredBlock) -> Result<(), StoreError> {
        // Write-through to the cache (skip-if-exists), then the remote.
        let key = cache_block_path(block.block_index.block_hash);
        let mut obj = self.cache_client.new_object(&key).await?;
        if !obj.exists().await? {
            let _ = obj.write(&block.to_bytes()).await; // best-effort cache fill
        }
        self.remote.put_stored_block(block).await
    }

    async fn get_stored_block(&self, block_hash: u64) -> Result<StoredBlock, StoreError> {
        let key = cache_block_path(block_hash);
        // Probe the cache file directly.
        // Cache hit only when the file exists, parses, and matches the hash;
        // otherwise (corrupt/mismatched) fall through to the remote.
        let obj = self.cache_client.new_object(&key).await?;
        if obj.exists().await.unwrap_or(false)
            && let Ok(data) = obj.read().await
            && let Ok(block) = StoredBlock::from_bytes(&data)
            && block.block_index.block_hash == block_hash
        {
            return Ok(block);
        }
        // Miss → fetch from remote and write back to the cache.
        let block = self.remote.get_stored_block(block_hash).await?;
        let mut wb = self.cache_client.new_object(&key).await?;
        if !wb.exists().await.unwrap_or(false) {
            let _ = wb.write(&block.to_bytes()).await; // best-effort write-back
        }
        Ok(block)
    }

    async fn preflight_get(&self, block_hashes: &[u64]) -> Result<(), StoreError> {
        // Only prefetch blocks not already cached (best-effort filter).
        let mut missing = Vec::new();
        for &h in block_hashes {
            let key = cache_block_path(h);
            let cached = match self.cache_client.new_object(&key).await {
                Ok(obj) => obj.exists().await.unwrap_or(false),
                Err(_) => false,
            };
            if !cached {
                missing.push(h);
            }
        }
        self.remote.preflight_get(&missing).await
    }

    async fn get_existing_content(
        &self,
        chunk_hashes: &[u64],
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, StoreError> {
        self.remote
            .get_existing_content(chunk_hashes, min_block_usage_percent)
            .await
    }

    async fn prune_blocks(&self, keep_block_hashes: &[u64]) -> Result<u32, StoreError> {
        self.remote.prune_blocks(keep_block_hashes).await
    }

    async fn flush(&self) -> Result<(), StoreError> {
        self.remote.flush().await
    }

    async fn close(&self) -> Result<(), StoreError> {
        self.remote.close().await
    }

    fn stats(&self) -> StatsSnapshot {
        self.remote.stats()
    }
}
