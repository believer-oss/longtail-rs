//! The [`RemoteBlockStore`] actor — a tokio-native reshape of golongtail's
//! `remoteStore` (`remotestore.go`).
//!
//! Topology (plan §2, `rust-port-4.md` Task 4):
//! - **One index-owner task** owns the in-memory [`StoreIndex`] and the
//!   accumulated block indexes, serialized behind an `mpsc` command channel — no
//!   shared-state lock on the index (Go's `contentIndexWorker` guarantee).
//! - **Block I/O** (`get`/`put`) runs directly on the calling task, sharing one
//!   cheaply-cloned [`BlobClient`], bounded by a [`Semaphore`] (worker-count
//!   equivalent — Go's `remoteWorker` pool).
//! - **Prefetch** is a `Mutex<HashMap<u64, Shared<future>>>`: get-coalescing
//!   falls out of `Shared` (structurally subsumes shareblockstore), plus a
//!   byte-denominated [`Semaphore`] for the 512 MiB prefetch budget. The permit
//!   count for a block is Σ chunk_sizes from the store index (an upper bound on
//!   the stored payload); oversize blocks clamp to the whole budget rather than
//!   deadlock. Acquiring an *estimate up front* is a deliberate, safer-than-Go
//!   divergence (Go debits the actual size post-fetch, remotestore.go:517/:456).
//! - **Flush** drains accumulated block indexes into the remote index via the
//!   sync module; **close** flushes then stops the owner task.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use futures_util::FutureExt;
use futures_util::future::{BoxFuture, Shared};
use longtail_core::{BlockIndex, StoreIndex, StoredBlock};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

use crate::blob::BlobStore;
use crate::block_store::{BlockStore, BlockStoreStats, StatsSnapshot};
use crate::error::StoreError;
use crate::sync::{self, AccessType};

/// Default prefetch memory budget (`maxPrefetchMemory`, remotestore.go:992).
pub const DEFAULT_MAX_PREFETCH_BYTES: usize = 512 * 1024 * 1024;

/// The put conditional-conflict retry ladder (remotestore.go:152). **Only** used
/// when a conditional write loses its CAS (`ok == false`, no error); a hard
/// error fails immediately, and the write is skipped entirely if the block
/// already exists.
const PUT_RETRY_DELAYS: [std::time::Duration; 3] = [
    std::time::Duration::from_millis(100),
    std::time::Duration::from_millis(500),
    std::time::Duration::from_millis(2000),
];

type FetchResult = Result<Arc<StoredBlock>, Arc<StoreError>>;
type SharedFetch = Shared<BoxFuture<'static, FetchResult>>;

struct PrefetchEntry {
    fut: SharedFetch,
    /// Released when this entry is dropped (consumed by a get, or flushed).
    _permit: OwnedSemaphorePermit,
}

enum IndexCommand {
    AddBlock(BlockIndex),
    GetExistingContent {
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
        reply: oneshot::Sender<Result<StoreIndex, StoreError>>,
    },
    GetIndex {
        reply: oneshot::Sender<Result<StoreIndex, StoreError>>,
    },
    Flush {
        reply: oneshot::Sender<Result<(), StoreError>>,
    },
    Shutdown {
        reply: oneshot::Sender<Result<(), StoreError>>,
    },
}

/// A remote block store over any [`BlobStore`] backend.
pub struct RemoteBlockStore {
    access_type: AccessType,
    client: Arc<dyn crate::blob::BlobClient>,
    worker_sem: Arc<Semaphore>,
    prefetch: Arc<Mutex<HashMap<u64, PrefetchEntry>>>,
    prefetch_sem: Arc<Semaphore>,
    max_prefetch_bytes: usize,
    stats: Arc<BlockStoreStats>,
    index_tx: mpsc::Sender<IndexCommand>,
    closed: AtomicBool,
}

impl std::fmt::Debug for RemoteBlockStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteBlockStore")
            .field("access_type", &self.access_type)
            .field("store", &self.client.name())
            .finish()
    }
}

impl RemoteBlockStore {
    /// Construct a remote block store. `worker_count` bounds concurrent block
    /// I/O (Go's `numWorkerCount`).
    pub async fn new(
        blob_store: Arc<dyn BlobStore>,
        access_type: AccessType,
        worker_count: usize,
    ) -> Result<RemoteBlockStore, StoreError> {
        Self::with_prefetch_budget(
            blob_store,
            access_type,
            worker_count,
            DEFAULT_MAX_PREFETCH_BYTES,
        )
        .await
    }

    /// Construct with an explicit prefetch byte budget (for tests).
    pub async fn with_prefetch_budget(
        blob_store: Arc<dyn BlobStore>,
        access_type: AccessType,
        worker_count: usize,
        max_prefetch_bytes: usize,
    ) -> Result<RemoteBlockStore, StoreError> {
        let worker_count = worker_count.max(1);
        let client: Arc<dyn crate::blob::BlobClient> = Arc::from(blob_store.new_client().await?);
        let stats = Arc::new(BlockStoreStats::default());
        let (index_tx, index_rx) = mpsc::channel::<IndexCommand>(64 + worker_count * 8);

        // Spawn the index-owner task with its own client.
        let owner_store = blob_store.clone();
        tokio::spawn(async move {
            index_owner(owner_store, access_type, index_rx).await;
        });

        // Semaphore permits are u32-bounded; clamp the byte budget.
        let budget = max_prefetch_bytes.min(Semaphore::MAX_PERMITS);

        Ok(RemoteBlockStore {
            access_type,
            client,
            worker_sem: Arc::new(Semaphore::new(worker_count)),
            prefetch: Arc::new(Mutex::new(HashMap::new())),
            prefetch_sem: Arc::new(Semaphore::new(budget)),
            max_prefetch_bytes: budget,
            stats,
            index_tx,
            closed: AtomicBool::new(false),
        })
    }

    async fn get_index_snapshot(&self) -> Result<StoreIndex, StoreError> {
        let (reply, rx) = oneshot::channel();
        self.index_tx
            .send(IndexCommand::GetIndex { reply })
            .await
            .map_err(|_| StoreError::WorkerGone)?;
        rx.await.map_err(|_| StoreError::WorkerGone)?
    }
}

/// Fetch + parse + validate a stored block by hash, bounded by `worker_sem`.
/// Shared by direct gets and prefetch.
async fn fetch_stored_block(
    client: Arc<dyn crate::blob::BlobClient>,
    worker_sem: Arc<Semaphore>,
    stats: Arc<BlockStoreStats>,
    block_hash: u64,
) -> Result<StoredBlock, StoreError> {
    let _permit = worker_sem
        .acquire()
        .await
        .map_err(|_| StoreError::WorkerGone)?;
    stats.add(&stats.get_count, 1);
    let key = sync::block_path("chunks", block_hash);
    let (data, retries) = match sync::read_blob_with_retry(&*client, &key).await {
        Ok(v) => v,
        Err(e) => {
            stats.add(&stats.get_fail_count, 1);
            return Err(e);
        }
    };
    stats.add(&stats.get_retry_count, retries as u64);
    let block = match StoredBlock::from_bytes(&data) {
        Ok(b) => b,
        Err(_) => {
            stats.add(&stats.get_fail_count, 1);
            return Err(StoreError::BadFormat(format!(
                "failed to parse stored block `{key}`"
            )));
        }
    };
    if block.block_index.block_hash != block_hash {
        stats.add(&stats.get_fail_count, 1);
        return Err(StoreError::BadFormat(format!(
            "block hash does not match path `{key}`"
        )));
    }
    stats.add(&stats.get_byte_count, data.len() as u64);
    stats.add(
        &stats.get_chunk_count,
        block.block_index.chunk_count() as u64,
    );
    Ok(block)
}

#[async_trait]
impl BlockStore for RemoteBlockStore {
    async fn put_stored_block(&self, block: StoredBlock) -> Result<(), StoreError> {
        if self.access_type == AccessType::ReadOnly {
            return Err(StoreError::AccessViolation);
        }
        let block_hash = block.block_index.block_hash;
        let chunk_count = block.block_index.chunk_count();
        let key = sync::block_path("chunks", block_hash);

        let _permit = self
            .worker_sem
            .acquire()
            .await
            .map_err(|_| StoreError::WorkerGone)?;
        self.stats.add(&self.stats.put_count, 1);

        let mut obj = self.client.new_object(&key).await?;
        // Skip-if-exists (remotestore.go:145).
        if !obj.exists().await? {
            let bytes = block.to_bytes();
            // Unconditional write; the {100ms,500ms,2s} ladder only triggers on a
            // conditional-write conflict (ok == false, no error).
            let mut ok = match obj.write(&bytes).await {
                Ok(ok) => ok,
                Err(e) => {
                    self.stats.add(&self.stats.put_fail_count, 1);
                    return Err(e);
                }
            };
            if !ok {
                for delay in PUT_RETRY_DELAYS {
                    self.stats.add(&self.stats.put_retry_count, 1);
                    tokio::time::sleep(delay).await;
                    ok = match obj.write(&bytes).await {
                        Ok(ok) => ok,
                        Err(e) => {
                            self.stats.add(&self.stats.put_fail_count, 1);
                            return Err(e);
                        }
                    };
                    if ok {
                        break;
                    }
                }
                if !ok {
                    self.stats.add(&self.stats.put_fail_count, 1);
                    return Err(StoreError::Backend(format!(
                        "failed to put stored block `{key}` even after retries"
                    )));
                }
            }
            self.stats
                .add(&self.stats.put_byte_count, bytes.len() as u64);
            self.stats
                .add(&self.stats.put_chunk_count, chunk_count as u64);
        }

        // Always accumulate the block index (whether written or skipped).
        self.index_tx
            .send(IndexCommand::AddBlock(block.block_index))
            .await
            .map_err(|_| StoreError::WorkerGone)?;
        Ok(())
    }

    async fn get_stored_block(&self, block_hash: u64) -> Result<StoredBlock, StoreError> {
        // Coalesce with an in-flight prefetch if present (Shared future).
        let prefetched = {
            let map = self.prefetch.lock().await;
            map.get(&block_hash).map(|e| e.fut.clone())
        };
        if let Some(fut) = prefetched {
            let result = fut.await;
            // Consume the entry (releases its budget permit).
            self.prefetch.lock().await.remove(&block_hash);
            return match result {
                Ok(block) => Ok((*block).clone()),
                Err(e) => Err(clone_store_error(&e)),
            };
        }
        fetch_stored_block(
            self.client.clone(),
            self.worker_sem.clone(),
            self.stats.clone(),
            block_hash,
        )
        .await
    }

    async fn preflight_get(&self, block_hashes: &[u64]) -> Result<(), StoreError> {
        if block_hashes.is_empty() {
            return Ok(());
        }
        // Size prefetch permits from the store index (Σ chunk_sizes per block).
        let index = self.get_index_snapshot().await?;
        let mut size_by_hash: HashMap<u64, usize> = HashMap::new();
        for b in 0..index.block_count() as usize {
            if let Some(bi) = index.block_index_at(b) {
                let sz: usize = bi.chunk_sizes.iter().map(|&s| s as usize).sum();
                size_by_hash.insert(bi.block_hash, sz);
            }
        }

        for &hash in block_hashes {
            {
                let map = self.prefetch.lock().await;
                if map.contains_key(&hash) {
                    continue; // already prefetching
                }
            }
            // Estimate; unknown blocks get 1 permit; oversize clamps to budget.
            let estimate = size_by_hash.get(&hash).copied().unwrap_or(1).max(1);
            let permits = estimate.min(self.max_prefetch_bytes).max(1) as u32;
            let permit = self
                .prefetch_sem
                .clone()
                .acquire_many_owned(permits)
                .await
                .map_err(|_| StoreError::WorkerGone)?;

            let fut: SharedFetch = fetch_stored_block(
                self.client.clone(),
                self.worker_sem.clone(),
                self.stats.clone(),
                hash,
            )
            .map(|r| r.map(Arc::new).map_err(Arc::new))
            .boxed()
            .shared();

            // Drive the fetch eagerly (Shared futures are lazy).
            let driver = fut.clone();
            tokio::spawn(async move {
                let _ = driver.await;
            });

            let mut map = self.prefetch.lock().await;
            map.entry(hash).or_insert(PrefetchEntry {
                fut,
                _permit: permit,
            });
        }
        Ok(())
    }

    async fn get_existing_content(
        &self,
        chunk_hashes: &[u64],
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, StoreError> {
        let (reply, rx) = oneshot::channel();
        self.index_tx
            .send(IndexCommand::GetExistingContent {
                chunk_hashes: chunk_hashes.to_vec(),
                min_block_usage_percent,
                reply,
            })
            .await
            .map_err(|_| StoreError::WorkerGone)?;
        rx.await.map_err(|_| StoreError::WorkerGone)?
    }

    async fn prune_blocks(&self, _keep_block_hashes: &[u64]) -> Result<u32, StoreError> {
        Err(StoreError::NotSupported(
            "prune_blocks is deferred to Stage 7".into(),
        ))
    }

    async fn flush(&self) -> Result<(), StoreError> {
        // Drop all prefetched-but-unconsumed blocks (releases their budget).
        self.prefetch.lock().await.clear();
        let (reply, rx) = oneshot::channel();
        self.index_tx
            .send(IndexCommand::Flush { reply })
            .await
            .map_err(|_| StoreError::WorkerGone)?;
        rx.await.map_err(|_| StoreError::WorkerGone)?
    }

    async fn close(&self) -> Result<(), StoreError> {
        if self.closed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.prefetch.lock().await.clear();
        let (reply, rx) = oneshot::channel();
        if self
            .index_tx
            .send(IndexCommand::Shutdown { reply })
            .await
            .is_err()
        {
            return Ok(()); // task already gone
        }
        rx.await.map_err(|_| StoreError::WorkerGone)?
    }

    fn stats(&self) -> StatsSnapshot {
        self.stats.snapshot()
    }
}

/// A best-effort clone of a `StoreError` behind an `Arc` (for the coalesced
/// prefetch path, where the error is shared).
fn clone_store_error(e: &StoreError) -> StoreError {
    match e {
        StoreError::NotFound(s) => StoreError::NotFound(s.clone()),
        StoreError::BadFormat(s) => StoreError::BadFormat(s.clone()),
        other => StoreError::Backend(format!("{other}")),
    }
}

/// The index-owner task loop.
async fn index_owner(
    blob_store: Arc<dyn BlobStore>,
    access_type: AccessType,
    mut rx: mpsc::Receiver<IndexCommand>,
) {
    let client = match blob_store.new_client().await {
        Ok(c) => c,
        Err(e) => {
            // Drain commands, failing index-needing ones (Go's error path).
            while let Some(cmd) = rx.recv().await {
                fail_command(cmd, || clone_store_error(&e));
            }
            return;
        }
    };
    let mut index: Option<StoreIndex> = None;
    let mut added: Vec<BlockIndex> = Vec::new();

    loop {
        let cmd = match rx.recv().await {
            Some(c) => c,
            None => {
                // Dropped without close(): do the final persist as a fallback.
                let _ = persist(&mut index, &mut added, &*client, access_type, true).await;
                break;
            }
        };
        match cmd {
            IndexCommand::AddBlock(bi) => added.push(bi),
            IndexCommand::GetIndex { reply } => {
                let r = merged_index(&mut index, &added, &blob_store, &*client, access_type).await;
                let _ = reply.send(r);
            }
            IndexCommand::GetExistingContent {
                chunk_hashes,
                min_block_usage_percent,
                reply,
            } => {
                let r = merged_index(&mut index, &added, &blob_store, &*client, access_type)
                    .await
                    .map(|idx| {
                        idx.get_existing_store_index(&chunk_hashes, min_block_usage_percent)
                    });
                let _ = reply.send(r);
            }
            IndexCommand::Flush { reply } => {
                let r = persist(&mut index, &mut added, &*client, access_type, false).await;
                let _ = reply.send(r);
            }
            IndexCommand::Shutdown { reply } => {
                let r = persist(&mut index, &mut added, &*client, access_type, true).await;
                let _ = reply.send(r);
                break;
            }
        }
    }
}

fn fail_command(cmd: IndexCommand, err: impl Fn() -> StoreError) {
    match cmd {
        IndexCommand::AddBlock(_) => {}
        IndexCommand::GetIndex { reply } => {
            let _ = reply.send(Err(err()));
        }
        IndexCommand::GetExistingContent { reply, .. } => {
            let _ = reply.send(Err(err()));
        }
        IndexCommand::Flush { reply } | IndexCommand::Shutdown { reply } => {
            let _ = reply.send(Err(err()));
        }
    }
}

/// Ensure the store index is loaded, then return it merged with any accumulated
/// block indexes (`getCurrentStoreIndex` + `addBlocksToStoreIndex`).
async fn merged_index(
    index: &mut Option<StoreIndex>,
    added: &[BlockIndex],
    blob_store: &Arc<dyn BlobStore>,
    client: &dyn crate::blob::BlobClient,
    access_type: AccessType,
) -> Result<StoreIndex, StoreError> {
    if index.is_none() {
        let loaded = sync::read_remote_store_index(&**blob_store, client, access_type).await?;
        *index = Some(loaded);
    }
    let base = index.as_ref().unwrap();
    if added.is_empty() {
        return Ok(base.clone());
    }
    let added_idx = store_index_from_added(added)?;
    base.merge(&added_idx).map_err(StoreError::from)
}

/// Build a store index from accumulated block indexes, in a deterministic
/// (block-hash-sorted) order (`rust-port-4.md`).
fn store_index_from_added(added: &[BlockIndex]) -> Result<StoreIndex, StoreError> {
    let mut sorted: Vec<BlockIndex> = added.to_vec();
    sorted.sort_by_key(|b| b.block_hash);
    StoreIndex::from_block_indexes(&sorted).map_err(StoreError::from)
}

/// Persist accumulated block indexes into the remote store index via the sync
/// module. `final_persist` also persists an empty index for `Init` (matching
/// Go's close: `accessType == Init || len(added) > 0`).
async fn persist(
    index: &mut Option<StoreIndex>,
    added: &mut Vec<BlockIndex>,
    client: &dyn crate::blob::BlobClient,
    access_type: AccessType,
    final_persist: bool,
) -> Result<(), StoreError> {
    if access_type == AccessType::ReadOnly {
        added.clear();
        return Ok(());
    }
    let should = if final_persist {
        access_type == AccessType::Init || !added.is_empty()
    } else {
        !added.is_empty()
    };
    if !should {
        return Ok(());
    }
    let added_idx = store_index_from_added(added)?;
    match sync::add_to_remote_store_index(client, &added_idx).await {
        Ok(Some(new_index)) => *index = Some(new_index),
        Ok(None) => {}
        Err(e) => return Err(e),
    }
    added.clear();
    Ok(())
}
