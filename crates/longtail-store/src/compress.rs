//! [`CompressBlockStore`] — compress on put / decompress on get, via the core
//! payload-framing codec, CPU-bridged to a caller-supplied rayon pool.
//!
//! Ports `compressblockstore`. The block index's `tag` selects the
//! codec; `tag == 0` is passthrough (the core's `encode_block_payload` /
//! `decode_block_payload` handle tag 0 as identity). Compression is the
//! *outermost* decorator: cached blocks are stored
//! compressed, so this layer sits above [`crate::cache::CacheBlockStore`].
//!
//! CPU work never uses `spawn_blocking`: `pool.spawn` + a
//! `tokio::sync::oneshot` bridges the rayon pool to the async caller.

use std::sync::Arc;

use async_trait::async_trait;
use longtail_core::compress::{decode_block_payload, encode_block_payload};
use longtail_core::{StoreIndex, StoredBlock};
use tokio::sync::oneshot;

use crate::block_store::{BlockStore, StatsSnapshot};
use crate::error::StoreError;

/// Wraps a backing [`BlockStore`], (de)compressing block payloads.
pub struct CompressBlockStore {
    inner: Arc<dyn BlockStore>,
    pool: Arc<rayon::ThreadPool>,
}

impl CompressBlockStore {
    /// `inner` is the backing store (typically a [`crate::cache::CacheBlockStore`]
    /// or [`crate::remote::RemoteBlockStore`]); `pool` runs the codec work.
    pub fn new(inner: Arc<dyn BlockStore>, pool: Arc<rayon::ThreadPool>) -> CompressBlockStore {
        CompressBlockStore { inner, pool }
    }
}

/// Run `f` on the rayon pool, awaiting its result on the tokio runtime.
///
/// A panic inside `f` unwinds the rayon worker and drops the sender without
/// sending, so the receive fails. That is reported as an error rather than
/// re-panicked here: the input driving this work comes from the store, and a
/// malformed block should reach the caller as a failed download on every path,
/// not as a dead process on one of them. The pool's own panic handler (set in
/// the facade's `build_pool`) is what keeps rayon from aborting first.
async fn on_pool<F, T>(pool: &rayon::ThreadPool, f: F) -> Result<T, StoreError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    pool.spawn(move || {
        let _ = tx.send(f());
    });
    rx.await
        .map_err(|_| StoreError::Backend("codec worker panicked before producing a result".into()))
}

#[async_trait]
impl BlockStore for CompressBlockStore {
    async fn put_stored_block(&self, block: StoredBlock) -> Result<(), StoreError> {
        let StoredBlock {
            block_index,
            payload,
        } = block;
        let tag = block_index.tag;
        let framed = on_pool(&self.pool, move || encode_block_payload(tag, &payload)).await??;
        self.inner
            .put_stored_block(StoredBlock {
                block_index,
                payload: framed,
            })
            .await
    }

    async fn get_stored_block(&self, block_hash: u64) -> Result<StoredBlock, StoreError> {
        let block = self.inner.get_stored_block(block_hash).await?;
        let StoredBlock {
            block_index,
            payload,
        } = block;
        let tag = block_index.tag;
        let max_uncompressed = block_index
            .uncompressed_len()
            .try_into()
            .unwrap_or(usize::MAX);
        let raw = on_pool(&self.pool, move || {
            decode_block_payload(tag, &payload, max_uncompressed)
        })
        .await??;
        // `decode_block_payload` checks the decoded length against the frame's
        // own declared `uncompressed_size` — both numbers come from the same
        // untrusted bytes, so they agree with each other and say nothing about
        // the block index. Cross-check against the chunks the block claims,
        // because the apply path slices this buffer using `chunk_sizes` and
        // panics on a short one. `StoredBlock::from_bytes` covers the same
        // invariant for `tag == 0`, where the payload is not decoded at all.
        let required = block_index.uncompressed_len();
        if (raw.len() as u64) < required {
            return Err(StoreError::BadFormat(format!(
                "block {:#018x} decoded to {} bytes but its index claims {required}",
                block_index.block_hash,
                raw.len(),
            )));
        }
        Ok(StoredBlock {
            block_index,
            payload: raw,
        })
    }

    async fn preflight_get(&self, block_hashes: &[u64]) -> Result<(), StoreError> {
        self.inner.preflight_get(block_hashes).await
    }

    async fn get_existing_content(
        &self,
        chunk_hashes: &[u64],
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, StoreError> {
        self.inner
            .get_existing_content(chunk_hashes, min_block_usage_percent)
            .await
    }

    async fn prune_blocks(&self, keep_block_hashes: &[u64]) -> Result<u32, StoreError> {
        self.inner.prune_blocks(keep_block_hashes).await
    }

    async fn flush(&self) -> Result<(), StoreError> {
        self.inner.flush().await
    }

    async fn close(&self) -> Result<(), StoreError> {
        self.inner.close().await
    }

    fn stats(&self) -> StatsSnapshot {
        self.inner.stats()
    }
}
