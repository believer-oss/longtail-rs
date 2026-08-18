//! The async, dyn-dispatchable [`BlockStore`] trait + atomic stats.
//!
//! Maps `Longtail_BlockStoreAPI` (the C completion-callback vtable) to plain
//! `async fn`s: `put_stored_block`/`get_stored_block`/
//! `preflight_get`/`get_existing_content`/`prune_blocks`/`flush`/`close`/`stats`.
//! Runtime composition (`Compress(Cache(Remote(…)))`) works because the trait is
//! object-safe (via `async_trait`).

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use longtail_core::{StoreIndex, StoredBlock};

use crate::error::StoreError;

/// The block store interface. All backends and decorators implement it.
#[async_trait]
pub trait BlockStore: Send + Sync {
    /// Store a block (skip-if-exists at the remote layer). Read-only stores
    /// return [`StoreError::AccessViolation`].
    async fn put_stored_block(&self, block: StoredBlock) -> Result<(), StoreError>;

    /// Fetch a block by hash. Missing → [`StoreError::NotFound`]; a block whose
    /// serialized hash disagrees with its path → [`StoreError::BadFormat`].
    async fn get_stored_block(&self, block_hash: u64) -> Result<StoredBlock, StoreError>;

    /// Hint that these blocks will be fetched soon (starts prefetching).
    async fn preflight_get(&self, block_hashes: &[u64]) -> Result<(), StoreError>;

    /// The subset of the store index covering `chunk_hashes`, honoring
    /// `min_block_usage_percent` (`GetExistingStoreIndex`).
    async fn get_existing_content(
        &self,
        chunk_hashes: &[u64],
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, StoreError>;

    /// Remove blocks not in `keep_block_hashes`. Stores that cannot prune
    /// return [`StoreError::NotSupported`].
    async fn prune_blocks(&self, keep_block_hashes: &[u64]) -> Result<u32, StoreError>;

    /// Drain in-flight writes and persist accumulated block indexes into the
    /// remote store index.
    async fn flush(&self) -> Result<(), StoreError>;

    /// Flush and stop the store's background tasks. Idempotent.
    async fn close(&self) -> Result<(), StoreError>;

    /// A snapshot of the running counters.
    fn stats(&self) -> StatsSnapshot;
}

/// Atomic per-op counters (`BlockStoreStats`-equivalent). A plain struct of
/// `AtomicU64` — no metrics framework.
#[derive(Debug, Default)]
pub struct BlockStoreStats {
    pub get_count: AtomicU64,
    pub get_byte_count: AtomicU64,
    pub get_chunk_count: AtomicU64,
    pub get_retry_count: AtomicU64,
    pub get_fail_count: AtomicU64,

    pub put_count: AtomicU64,
    pub put_byte_count: AtomicU64,
    pub put_chunk_count: AtomicU64,
    pub put_retry_count: AtomicU64,
    pub put_fail_count: AtomicU64,
}

impl BlockStoreStats {
    pub(crate) fn add(&self, field: &AtomicU64, n: u64) {
        field.fetch_add(n, Ordering::Relaxed);
    }

    /// A plain-value snapshot for callers (stats reporting).
    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            get_count: self.get_count.load(Ordering::Relaxed),
            get_byte_count: self.get_byte_count.load(Ordering::Relaxed),
            get_chunk_count: self.get_chunk_count.load(Ordering::Relaxed),
            get_retry_count: self.get_retry_count.load(Ordering::Relaxed),
            get_fail_count: self.get_fail_count.load(Ordering::Relaxed),
            put_count: self.put_count.load(Ordering::Relaxed),
            put_byte_count: self.put_byte_count.load(Ordering::Relaxed),
            put_chunk_count: self.put_chunk_count.load(Ordering::Relaxed),
            put_retry_count: self.put_retry_count.load(Ordering::Relaxed),
            put_fail_count: self.put_fail_count.load(Ordering::Relaxed),
        }
    }
}

/// A plain-value copy of [`BlockStoreStats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub get_count: u64,
    pub get_byte_count: u64,
    pub get_chunk_count: u64,
    pub get_retry_count: u64,
    pub get_fail_count: u64,
    pub put_count: u64,
    pub put_byte_count: u64,
    pub put_chunk_count: u64,
    pub put_retry_count: u64,
    pub put_fail_count: u64,
}
