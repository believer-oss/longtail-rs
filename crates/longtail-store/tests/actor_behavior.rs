//! RemoteBlockStore actor internals — the read retry ladder (under
//! tokio paused time), prefetch get-coalescing, read-only enforcement, and
//! stats counters.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use longtail_core::{BlockIndex, StoredBlock};
use longtail_store::blob::{BlobClient, BlobObject, BlobProperties, BlobStore, MemBlobStore};
use longtail_store::{AccessType, BlockStore, RemoteBlockStore, block_path};

fn make_block(seed: u8) -> StoredBlock {
    let s = seed as u64;
    let sizes = vec![10u32, 20, 30];
    let len: usize = sizes.iter().map(|&x| x as usize).sum();
    StoredBlock {
        block_index: BlockIndex {
            block_hash: s + 21412151,
            hash_identifier: 997,
            tag: 0,
            chunk_hashes: vec![s + 1, s + 2, s + 3],
            chunk_sizes: sizes,
        },
        payload: vec![seed; len],
    }
}

/// Write a block directly to a mem backend at its canonical `.lsb` path.
async fn seed_block(store: &MemBlobStore, block: &StoredBlock) {
    let client = store.new_client().await.unwrap();
    let key = block_path("chunks", block.block_index.block_hash);
    let mut obj = client.new_object(&key).await.unwrap();
    obj.write(block.to_bytes().into()).await.unwrap();
}

// --- a flaky blob store that fails the first N reads with a transient error ---

#[derive(Debug)]
struct FlakyStore {
    inner: MemBlobStore,
    fails_remaining: Arc<AtomicUsize>,
}

#[async_trait]
impl BlobStore for FlakyStore {
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, longtail_store::StoreError> {
        Ok(Box::new(FlakyClient {
            inner: self.inner.new_client().await?,
            fails_remaining: self.fails_remaining.clone(),
        }))
    }
    fn name(&self) -> String {
        "flaky".into()
    }
}

struct FlakyClient {
    inner: Box<dyn BlobClient>,
    fails_remaining: Arc<AtomicUsize>,
}

#[async_trait]
impl BlobClient for FlakyClient {
    async fn new_object(
        &self,
        path: &str,
    ) -> Result<Box<dyn BlobObject>, longtail_store::StoreError> {
        Ok(Box::new(FlakyObject {
            inner: self.inner.new_object(path).await?,
            fails_remaining: self.fails_remaining.clone(),
        }))
    }
    async fn get_objects(
        &self,
        prefix: &str,
    ) -> Result<Vec<BlobProperties>, longtail_store::StoreError> {
        self.inner.get_objects(prefix).await
    }
    fn supports_locking(&self) -> bool {
        self.inner.supports_locking()
    }
    fn name(&self) -> String {
        self.inner.name()
    }
}

struct FlakyObject {
    inner: Box<dyn BlobObject>,
    fails_remaining: Arc<AtomicUsize>,
}

#[async_trait]
impl BlobObject for FlakyObject {
    async fn exists(&self) -> Result<bool, longtail_store::StoreError> {
        self.inner.exists().await
    }
    async fn lock_write_version(&mut self) -> Result<bool, longtail_store::StoreError> {
        self.inner.lock_write_version().await
    }
    async fn read(&self) -> Result<Vec<u8>, longtail_store::StoreError> {
        // Fail with a transient (non-NotFound) error the first N times.
        loop {
            let n = self.fails_remaining.load(Ordering::SeqCst);
            if n == 0 {
                break;
            }
            if self
                .fails_remaining
                .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Err(longtail_store::StoreError::Backend("transient".into()));
            }
        }
        self.inner.read().await
    }
    async fn write(&mut self, data: bytes::Bytes) -> Result<bool, longtail_store::StoreError> {
        self.inner.write(data).await
    }
    async fn delete(&mut self) -> Result<(), longtail_store::StoreError> {
        self.inner.delete().await
    }
    fn name(&self) -> String {
        self.inner.name()
    }
}

/// The read retry ladder retries a transient failure and eventually succeeds;
/// under paused time the virtual sleeps resolve instantly.
#[tokio::test(start_paused = true)]
async fn retry_ladder_recovers_under_paused_time() {
    let mem = MemBlobStore::new("", true);
    let block = make_block(1);
    seed_block(&mem, &block).await;

    let flaky = Arc::new(FlakyStore {
        inner: mem,
        fails_remaining: Arc::new(AtomicUsize::new(3)),
    });
    let store = RemoteBlockStore::new(flaky, AccessType::ReadOnly, 2)
        .await
        .unwrap();

    let got = store
        .get_stored_block(block.block_index.block_hash)
        .await
        .expect("should recover after 3 transient failures");
    assert_eq!(got, block);

    let stats = store.stats();
    // ladder delays consumed: [0, 100ms, 250ms] → 3 retries.
    assert_eq!(stats.get_retry_count, 3, "expected exactly 3 retries");
    assert_eq!(stats.get_count, 1);
    store.close().await.unwrap();
}

/// A not-found short-circuits the ladder with zero retries.
#[tokio::test(start_paused = true)]
async fn not_found_does_not_retry() {
    let mem = MemBlobStore::new("", true);
    let store = RemoteBlockStore::new(Arc::new(mem), AccessType::ReadOnly, 2)
        .await
        .unwrap();
    let err = store.get_stored_block(999).await.unwrap_err();
    assert!(err.is_not_found());
    assert_eq!(store.stats().get_retry_count, 0);
    assert_eq!(store.stats().get_fail_count, 1);
    store.close().await.unwrap();
}

/// Prefetch + get coalesce: after `preflight_get`, the following `get` is served
/// from the shared prefetch future — only ONE underlying fetch happens.
#[tokio::test]
async fn prefetch_coalesces_with_get() {
    let mem = MemBlobStore::new("", true);
    let blob_store: Arc<dyn BlobStore> = Arc::new(mem.clone());
    let store = RemoteBlockStore::new(blob_store, AccessType::ReadWrite, 4)
        .await
        .unwrap();

    // Put a block so it is discoverable via the (flushed) store index.
    let block = make_block(5);
    let hash = block.block_index.block_hash;
    store.put_stored_block(block.clone()).await.unwrap();
    store.flush().await.unwrap();

    store.preflight_get(&[hash]).await.unwrap();
    let got = store.get_stored_block(hash).await.unwrap();
    assert_eq!(got, block);

    // Exactly one fetch: the prefetch fetched, the get coalesced.
    assert_eq!(
        store.stats().get_count,
        1,
        "get should coalesce with prefetch"
    );
    store.close().await.unwrap();
}

/// A read-only store rejects puts with AccessViolation.
#[tokio::test]
async fn read_only_rejects_put() {
    let store = RemoteBlockStore::new(
        Arc::new(MemBlobStore::new("", true)),
        AccessType::ReadOnly,
        2,
    )
    .await
    .unwrap();
    let err = store.put_stored_block(make_block(3)).await.unwrap_err();
    assert!(matches!(err, longtail_store::StoreError::AccessViolation));
    store.close().await.unwrap();
}

/// `prune_blocks`: keeps the requested blocks, deletes the rest, and
/// rewrites the store index. A ReadOnly store rejects it with AccessViolation.
#[tokio::test]
async fn prune_blocks_keeps_and_deletes() {
    let backing = Arc::new(MemBlobStore::new("", true));
    let store = RemoteBlockStore::new(backing.clone(), AccessType::ReadWrite, 2)
        .await
        .unwrap();

    // Three blocks with disjoint chunk-hash ranges (seeds 1, 10, 20).
    let (b1, b10, b20) = (make_block(1), make_block(10), make_block(20));
    store.put_stored_block(b1.clone()).await.unwrap();
    store.put_stored_block(b10.clone()).await.unwrap();
    store.put_stored_block(b20.clone()).await.unwrap();
    store.flush().await.unwrap();

    // Keep b1 + b10; b20 must be deleted.
    let keep = vec![b1.block_index.block_hash, b10.block_index.block_hash];
    let pruned = store.prune_blocks(&keep).await.unwrap();
    assert_eq!(pruned, 1, "exactly the one unkept block is deleted");

    // b20's block file is gone.
    let key = block_path("chunks", b20.block_index.block_hash);
    let client = backing.new_client().await.unwrap();
    assert!(
        !client
            .new_object(&key)
            .await
            .unwrap()
            .exists()
            .await
            .unwrap(),
        "pruned block file removed"
    );

    // A fresh reader sees only the kept blocks in the store index.
    let reader = RemoteBlockStore::new(backing.clone(), AccessType::ReadOnly, 2)
        .await
        .unwrap();
    let kept = reader
        .get_existing_content(&b1.block_index.chunk_hashes, 0)
        .await
        .unwrap();
    assert_eq!(kept.block_count(), 1, "b1 still covered");
    let dropped = reader
        .get_existing_content(&b20.block_index.chunk_hashes, 0)
        .await
        .unwrap();
    assert_eq!(dropped.block_count(), 0, "b20 no longer covered");
    reader.close().await.unwrap();
    store.close().await.unwrap();
}

/// A read-only store rejects prune with AccessViolation.
#[tokio::test]
async fn prune_blocks_read_only_rejected() {
    let store = RemoteBlockStore::new(
        Arc::new(MemBlobStore::new("", true)),
        AccessType::ReadOnly,
        2,
    )
    .await
    .unwrap();
    let err = store.prune_blocks(&[]).await.unwrap_err();
    assert!(matches!(err, longtail_store::StoreError::AccessViolation));
    store.close().await.unwrap();
}
