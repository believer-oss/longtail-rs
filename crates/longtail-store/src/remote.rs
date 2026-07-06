//! The [`RemoteBlockStore`] actor — a tokio-native reshape of golongtail's
//! `remoteStore` (`remotestore.go` @49a20e1).
//!
//! Topology (the store's concurrency architecture):
//! - **One index-owner task** owns the in-memory [`StoreIndex`] and the
//!   accumulated block indexes, serialized behind an `mpsc` command channel — no
//!   shared-state lock on the index (Go's `contentIndexWorker` guarantee).
//! - **Block I/O** (`get`/`put`) runs directly on the calling task, sharing one
//!   cheaply-cloned [`BlobClient`], bounded by a [`Semaphore`] (worker-count
//!   equivalent — Go's `remoteWorker` pool).
//! - **Prefetch** is a `Mutex<PrefetchState>` (an in-flight
//!   `HashMap<u64, Shared<future>>` + an enqueued-but-undispatched `queued` set):
//!   get-coalescing falls out of `Shared` (structurally subsumes
//!   shareblockstore), plus a byte-denominated [`Semaphore`] for the 512 MiB
//!   prefetch budget.
//!
//!   **The budget gates BACKGROUND PREFETCH only — demand fetches always
//!   proceed**. This mirrors golongtail exactly:
//!   `PreflightGet` merely enqueues prefetch requests, touching no budget
//!   (remotestore.go:613-614/:1038-1041); background `remoteWorker`s pull a
//!   prefetch **only while** `prefetchMemory < maxPrefetchMemory` (:517/:535),
//!   while a demand `fetchBlock` has NO budget check and runs in every worker
//!   branch, including over budget (:504-506/:533-534/:560-561). Forward
//!   progress therefore never depends on budget availability — the budget bounds
//!   *memory held by not-yet-consumed prefetches*, never *progress* (the liveness
//!   invariant; any working set completes with any budget ≥ 1 permit).
//!
//!   Concretely: [`RemoteBlockStore::preflight_get`] records the wanted set into
//!   `queued` and spawns one background dispatch task per block **without**
//!   acquiring budget. Each task acquires its permit only when the prefetch is
//!   *dispatched* — and the in-flight map entry is created **only then**, after
//!   acquisition. A demand [`RemoteBlockStore::get_stored_block`] for a block
//!   whose prefetch is still parked on budget finds no map entry, so it fetches
//!   inline (bounded by the worker semaphore, exactly like Go) and **claims the
//!   hash** with a permit-less entry so a later-dispatched prefetch skips it
//!   (cf. Go's placeholder insert :283-284 / later-prefetch no-op :361-366).
//!   ⚠ Acquire-**at-dispatch** is a deliberate, stricter-than-Go divergence: Go
//!   debits the budget at fetch *completion* and only when no waiter claimed the
//!   block (remotestore.go:387-389; credit-back :270-271/:455-456; the pull-site
//!   check is a soft atomic read that can overshoot, :517). Dispatch-time
//!   acquisition is the same safer-accounting family as the
//!   estimate-not-actual-size divergence this module already documents (permit
//!   count = Σ chunk_sizes, an upper bound on the stored payload); oversize
//!   blocks clamp to the whole budget so a single block is always acquirable.
//! - **Flush** drains accumulated block indexes into the remote index via the
//!   sync module AND drains the prefetch backlog: the enqueued-but-undispatched
//!   `queued` set is cleared first, then unconsumed entries are dropped (Go's
//!   flushPrefetch drains the channel first, remotestore.go:433-440, then
//!   evicts, :442-462). Dropping the entries releases their budget permits,
//!   which wakes any budget-parked dispatch tasks; each finds its `queued`
//!   claim gone and abandons, releasing its permit. **close** flushes then
//!   stops the owner task.

use std::collections::{HashMap, HashSet};
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
    /// The budget permit held by a *dispatched background prefetch*; `None` for
    /// a demand-claimed entry (demand fetches never touch the budget — Fix 1).
    /// Released when this entry is dropped (consumed by a get, or flushed).
    _permit: Option<OwnedSemaphorePermit>,
}

/// Prefetch bookkeeping. Invariants (all transitions happen
/// under the one mutex, and a hash is never in both sets at once):
///
/// - `entries[h]` exists ⇔ a fetch for `h` is actively running or completed
///   awaiting consumption. Entries are created ONLY (a) at background dispatch,
///   *after* the budget permit is acquired, or (b) by a demand get claiming the
///   hash (permit-less). A demand get may therefore always safely await an
///   entry's future — it can never be budget-parked.
/// - `queued` holds enqueued-but-undispatched background prefetches (budget not
///   yet acquired). A demand get must NOT wait on these: it removes the claim
///   and fetches inline; the parked dispatch task later finds its claim gone
///   and abandons. This pending-vs-dispatched split is what avoids the
///   whole-working-set budget deadlock.
#[derive(Default)]
struct PrefetchState {
    entries: HashMap<u64, PrefetchEntry>,
    queued: HashSet<u64>,
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
    prefetch: Arc<Mutex<PrefetchState>>,
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
            None,
        )
        .await
    }

    /// Construct with a pre-loaded, pre-merged **ReadOnly store-index override**
    /// (golongtail's `optionalStoreIndexPaths` merge, remotestore.go:1897). When
    /// `override_index` is `Some` and `access_type == ReadOnly`, the index-owner
    /// task seeds the store index from it and never scans the store's `.lsi`
    /// shards. Ignored for non-`ReadOnly` access types.
    pub async fn with_store_index_override(
        blob_store: Arc<dyn BlobStore>,
        access_type: AccessType,
        worker_count: usize,
        override_index: Option<StoreIndex>,
    ) -> Result<RemoteBlockStore, StoreError> {
        Self::with_prefetch_budget(
            blob_store,
            access_type,
            worker_count,
            DEFAULT_MAX_PREFETCH_BYTES,
            override_index,
        )
        .await
    }

    /// Construct with an explicit prefetch byte budget (for tests).
    pub async fn with_prefetch_budget(
        blob_store: Arc<dyn BlobStore>,
        access_type: AccessType,
        worker_count: usize,
        max_prefetch_bytes: usize,
        override_index: Option<StoreIndex>,
    ) -> Result<RemoteBlockStore, StoreError> {
        let worker_count = worker_count.max(1);
        let client: Arc<dyn crate::blob::BlobClient> = Arc::from(blob_store.new_client().await?);
        let stats = Arc::new(BlockStoreStats::default());
        let (index_tx, index_rx) = mpsc::channel::<IndexCommand>(64 + worker_count * 8);

        // Spawn the index-owner task with its own client. A ReadOnly override
        // pre-seeds the index (no shard scan).
        let owner_store = blob_store.clone();
        let seed = if access_type == AccessType::ReadOnly {
            override_index
        } else {
            None
        };
        tokio::spawn(async move {
            index_owner(owner_store, access_type, seed, index_rx).await;
        });

        // Per-acquire permit counts are u32-bounded; clamp the byte budget so a
        // whole-budget (oversize) clamp always fits one `acquire_many` call.
        let budget = max_prefetch_bytes
            .min(Semaphore::MAX_PERMITS)
            .min(u32::MAX as usize);

        Ok(RemoteBlockStore {
            access_type,
            client,
            worker_sem: Arc::new(Semaphore::new(worker_count)),
            prefetch: Arc::new(Mutex::new(PrefetchState::default())),
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

/// Dispatch one enqueued background prefetch. Acquires the
/// block's budget permits FIRST (this is the only place the prefetch budget is
/// awaited — the pull-site backpressure, Go's `remoteWorker` prefetch branch,
/// remotestore.go:516-517/:535-536), then — only if the `queued` claim is still
/// present — registers the in-flight entry and drives the fetch. Liveness never
/// depends on this task making progress: a demand get claims the hash out of
/// `queued` and fetches inline, after which this task wakes (when budget
/// frees), finds its claim gone, and abandons, releasing its permit.
#[allow(clippy::too_many_arguments)]
async fn dispatch_prefetch(
    prefetch: Arc<Mutex<PrefetchState>>,
    prefetch_sem: Arc<Semaphore>,
    client: Arc<dyn crate::blob::BlobClient>,
    worker_sem: Arc<Semaphore>,
    stats: Arc<BlockStoreStats>,
    hash: u64,
    permits: u32,
) {
    // Budget acquired at dispatch, not enqueue. Parks under budget pressure;
    // nothing awaits this task (map entries are created only below, after
    // acquisition), so parking here can never block a demand fetch.
    let permit = match prefetch_sem.acquire_many_owned(permits).await {
        Ok(p) => p,
        Err(_) => return, // semaphore closed — store torn down
    };
    let tx = {
        let mut st = prefetch.lock().await;
        // The claim may be gone: consumed by a demand get, or drained by
        // flush/close. Abandon; dropping `permit` releases the budget.
        if !st.queued.remove(&hash) {
            return;
        }
        // `queued` and `entries` are mutually exclusive (all transitions hold
        // the lock), so the claim's presence implies no entry exists.
        debug_assert!(!st.entries.contains_key(&hash));
        // The entry's Shared wraps a oneshot rather than the fetch itself so
        // this task drives the fetch WITHOUT holding a Shared handle: when the
        // (typical) single consumer awaits + removes the entry, it holds the
        // last reference and takes the block via `Arc::try_unwrap` copy-free.
        let (tx, rx) = oneshot::channel::<FetchResult>();
        let fut: SharedFetch = async move {
            match rx.await {
                Ok(r) => r,
                Err(_) => Err(Arc::new(StoreError::Backend(
                    "prefetch task dropped before completing".into(),
                ))),
            }
        }
        .boxed()
        .shared();
        st.entries.insert(
            hash,
            PrefetchEntry {
                fut,
                _permit: Some(permit),
            },
        );
        tx
    };
    // Drive the fetch to completion; the result stays in the entry — holding
    // the budget permit — until consumed or flushed. A failed send means the
    // entry was flushed away with no consumer waiting: drop the block.
    let res = fetch_stored_block(client, worker_sem, stats, hash).await;
    let _ = tx.send(res.map(Arc::new).map_err(Arc::new));
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
        // Coalesce with an in-flight (dispatched or demand-claimed) fetch if an
        // entry exists; otherwise claim the hash and fetch inline. A demand get
        // NEVER waits on a budget-parked (queued, undispatched) prefetch —
        // demand fetches always proceed, budget or not (Go's `fetchBlock` has
        // no budget check and runs in every worker branch including
        // over-budget, remotestore.go:504-506/:533-534/:560-561). Demand-fetch
        // memory is bounded by the worker semaphore, exactly like Go.
        let fut = {
            let mut st = self.prefetch.lock().await;
            if let Some(e) = st.entries.get(&block_hash) {
                e.fut.clone()
            } else {
                // Claim the hash (Go's placeholder insert, remotestore.go:
                // 283-284): remove any undispatched claim so its parked
                // dispatch task abandons, and register a permit-less entry so
                // a later-dispatched background prefetch skips it (Go's
                // later-prefetch no-op, :361-366). Concurrent demand gets for
                // the same block coalesce on this entry.
                st.queued.remove(&block_hash);
                let fut: SharedFetch = fetch_stored_block(
                    self.client.clone(),
                    self.worker_sem.clone(),
                    self.stats.clone(),
                    block_hash,
                )
                .map(|r| r.map(Arc::new).map_err(Arc::new))
                .boxed()
                .shared();
                st.entries.insert(
                    block_hash,
                    PrefetchEntry {
                        fut: fut.clone(),
                        _permit: None,
                    },
                );
                fut
            }
        };
        // The await consumes our Shared clone; removing the entry then drops
        // the map's clone (releasing a dispatched prefetch's budget permit).
        let result = fut.await;
        self.prefetch.lock().await.entries.remove(&block_hash);
        match result {
            // Sole holder (the common demand case) → move the block out copy-free.
            Ok(block) => Ok(Arc::try_unwrap(block).unwrap_or_else(|arc| (*arc).clone())),
            Err(e) => Err(clone_store_error(&e)),
        }
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

        // Enqueue WITHOUT acquiring budget (Go's PreflightGet/onPreflightMessage
        // only posts prefetch messages, remotestore.go:613-614/:1038-1041) —
        // acquiring the whole working set's budget here is exactly the
        // deadlock this split avoids. Budget is acquired per block at
        // background *dispatch*, in
        // the spawned task below.
        let mut st = self.prefetch.lock().await;
        for &hash in block_hashes {
            if st.entries.contains_key(&hash) || !st.queued.insert(hash) {
                continue; // already fetching, or already enqueued
            }
            // Estimate; unknown blocks get 1 permit; oversize clamps to the
            // whole budget so a single block is always
            // acquirable → any working set completes with any budget ≥ 1.
            let estimate = size_by_hash.get(&hash).copied().unwrap_or(1).max(1);
            let permits = estimate.min(self.max_prefetch_bytes).max(1) as u32;
            tokio::spawn(dispatch_prefetch(
                self.prefetch.clone(),
                self.prefetch_sem.clone(),
                self.client.clone(),
                self.worker_sem.clone(),
                self.stats.clone(),
                hash,
                permits,
            ));
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

    async fn prune_blocks(&self, keep_block_hashes: &[u64]) -> Result<u32, StoreError> {
        if self.access_type == AccessType::ReadOnly {
            return Err(StoreError::AccessViolation);
        }
        // Current store index (loaded via the owner; `added` is empty on the
        // prune path, so this is the authoritative on-store index).
        let source = self.get_index_snapshot().await?;
        let pruned = source.prune(keep_block_hashes);

        // Overwrite the store index FIRST, then delete orphan blocks
        // (remotestore.go:655-684 ordering — a crash leaves harmless orphans,
        // never dangling index entries).
        sync::overwrite_remote_store_index(&*self.client, &pruned).await?;

        let kept: std::collections::HashSet<u64> = pruned.block_hashes.iter().copied().collect();
        let mut pruned_count = 0u32;
        for &bh in &source.block_hashes {
            if kept.contains(&bh) {
                continue;
            }
            let key = sync::block_path("chunks", bh);
            if let Ok(mut obj) = self.client.new_object(&key).await
                && obj.delete().await.is_ok()
            {
                pruned_count += 1;
            }
        }
        Ok(pruned_count)
    }

    async fn flush(&self) -> Result<(), StoreError> {
        drain_prefetch(&self.prefetch).await;
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
        drain_prefetch(&self.prefetch).await;
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

/// Drain the prefetch machinery (Go's `flushPrefetch`, remotestore.go:433-462):
/// clear the enqueued-but-undispatched backlog FIRST (Go drains the channel
/// first, :433-440), then drop unconsumed entries (:442-462). Dropping the
/// entries releases their budget permits, which wakes budget-parked dispatch
/// tasks; each finds its `queued` claim gone and abandons.
async fn drain_prefetch(prefetch: &Mutex<PrefetchState>) {
    let mut st = prefetch.lock().await;
    st.queued.clear();
    st.entries.clear();
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
    seed_index: Option<StoreIndex>,
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
    // A ReadOnly override pre-seeds the index so `merged_index` never scans.
    let mut index: Option<StoreIndex> = seed_index;
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
/// (block-hash-sorted) order.
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
