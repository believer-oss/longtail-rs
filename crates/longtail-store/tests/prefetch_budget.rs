//! Stage 7a Fix 1: the prefetch budget gates BACKGROUND PREFETCH only — demand
//! fetches always proceed (golongtail's model: enqueue touches no budget,
//! remotestore.go:613-614/:1038-1041; workers pull prefetches only while under
//! budget, :516-517/:535-536; demand `fetchBlock` has no budget check,
//! :504-506/:533-534/:560-561).
//!
//! Liveness invariant under test: **forward progress never depends on budget
//! availability** — the budget bounds memory held by not-yet-consumed
//! prefetches, never progress. Any working set completes with ANY budget ≥ 1
//! permit (and demand-only progress holds even at budget 0).
//!
//! Most tests run under `start_paused` virtual time, so a reintroduced
//! budget-park on the demand path deterministically trips the timeout instead
//! of hanging CI.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use longtail_core::{BlockIndex, StoredBlock};
use longtail_store::blob::{BlobClient, BlobObject, BlobProperties, BlobStore, MemBlobStore};
use longtail_store::{AccessType, BlockStore, RemoteBlockStore, StoreError, block_path};

/// Same shape as the Stage 4 actor tests: three chunks of 10+20+30 = 60 bytes.
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
    obj.write(&block.to_bytes()).await.unwrap();
}

/// A ReadWrite store over `mem` with an explicit prefetch budget.
async fn store_with_budget(mem: &MemBlobStore, budget: usize) -> RemoteBlockStore {
    RemoteBlockStore::with_prefetch_budget(
        Arc::new(mem.clone()),
        AccessType::ReadWrite,
        4,
        budget,
        None,
    )
    .await
    .unwrap()
}

/// Poll until `cond` holds (virtual time makes this instant-or-panic).
async fn wait_until(mut cond: impl FnMut() -> bool, what: &str) {
    for _ in 0..2000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not reached: {what}");
}

const GUARD: Duration = Duration::from_secs(30);

/// **The required Fix 1 regression**: a demand get for a block whose prefetch
/// is enqueued but NOT yet dispatched completes with ZERO budget available.
/// Budget 0 means no background prefetch can ever dispatch — pre-fix, the
/// preflight itself would park forever; post-fix, enqueue never touches the
/// budget and the demand get never awaits a budget-parked prefetch.
#[tokio::test(start_paused = true)]
async fn demand_get_completes_with_zero_budget() {
    let mem = MemBlobStore::new("", true);
    let block = make_block(1);
    seed_block(&mem, &block).await;

    let store = store_with_budget(&mem, 0).await;

    // Enqueue a background prefetch that can never dispatch (0 permits).
    tokio::time::timeout(GUARD, store.preflight_get(&[block.block_index.block_hash]))
        .await
        .expect("preflight must not block on budget")
        .unwrap();

    // The demand get must not await the parked prefetch: no map entry exists
    // for it (entries are created only at dispatch, after budget acquisition),
    // so the get claims the hash and fetches inline.
    let got = tokio::time::timeout(GUARD, store.get_stored_block(block.block_index.block_hash))
        .await
        .expect("demand get must complete with zero budget")
        .unwrap();
    assert_eq!(got, block);
    assert_eq!(store.stats().get_count, 1, "exactly one inline fetch");

    // The claim removed the queued marker, so the (never-dispatchable) backlog
    // holds nothing that could fetch this block twice.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(store.stats().get_count, 1, "no duplicate background fetch");
    store.close().await.unwrap();
}

/// Liveness invariant: any working set completes with ANY budget ≥ 1 permit.
/// Budget = 1 byte, working set = 8 blocks × 60 bytes: every block's permit
/// request clamps to the whole budget, so at most one background prefetch is
/// dispatched at a time and the rest park — demand gets bypass regardless.
#[tokio::test(start_paused = true)]
async fn any_working_set_completes_with_one_permit_budget() {
    let mem = MemBlobStore::new("", true);
    let store = store_with_budget(&mem, 1).await;
    let blocks: Vec<StoredBlock> = (0..8u8).map(|s| make_block(s * 10)).collect();
    for b in &blocks {
        store.put_stored_block(b.clone()).await.unwrap();
    }
    store.flush().await.unwrap();

    let hashes: Vec<u64> = blocks.iter().map(|b| b.block_index.block_hash).collect();
    tokio::time::timeout(GUARD, store.preflight_get(&hashes))
        .await
        .expect("preflight must not block on budget")
        .unwrap();

    for b in &blocks {
        let got = tokio::time::timeout(GUARD, store.get_stored_block(b.block_index.block_hash))
            .await
            .expect("get must complete regardless of budget pressure")
            .unwrap();
        assert_eq!(&got, b);
    }
    // Every block fetched exactly once: dispatched prefetches coalesce with
    // their gets, parked ones are claimed by the demand gets — never both.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        store.stats().get_count,
        8,
        "each block fetched exactly once"
    );
    store.close().await.unwrap();
}

/// An oversize block (estimate > whole budget) clamps to the budget and still
/// dispatches — the Stage 4 clamp rule that keeps single blocks acquirable.
#[tokio::test(start_paused = true)]
async fn oversize_block_clamps_to_budget_and_dispatches() {
    let mem = MemBlobStore::new("", true);
    let store = store_with_budget(&mem, 8).await; // budget < the 60-byte block
    let block = make_block(3);
    let hash = block.block_index.block_hash;
    store.put_stored_block(block.clone()).await.unwrap();
    store.flush().await.unwrap();

    store.preflight_get(&[hash]).await.unwrap();
    // The background prefetch dispatches (its 60-byte estimate clamped to the
    // 8-permit budget) — get_count ticks at fetch start.
    wait_until(|| store.stats().get_count >= 1, "prefetch dispatched").await;

    let got = tokio::time::timeout(GUARD, store.get_stored_block(hash))
        .await
        .expect("get must coalesce with the dispatched prefetch")
        .unwrap();
    assert_eq!(got, block);
    assert_eq!(store.stats().get_count, 1, "coalesced, single fetch");
    store.close().await.unwrap();
}

/// Permit accounting on the error path: a failed prefetch's permit is released
/// when the error is consumed, so subsequent prefetches can dispatch.
#[tokio::test(start_paused = true)]
async fn error_path_releases_budget_permit() {
    let mem = MemBlobStore::new("", true);
    let real = make_block(7);
    seed_block(&mem, &real).await;

    // Budget = 1 permit total; the missing block's unknown size estimates to 1.
    let store = store_with_budget(&mem, 1).await;
    let missing_hash = 0xdead_beefu64;

    store.preflight_get(&[missing_hash]).await.unwrap();
    wait_until(
        || store.stats().get_fail_count >= 1,
        "missing-block prefetch dispatched and failed",
    )
    .await;

    // Consume the error → entry dropped → permit released.
    let err = tokio::time::timeout(GUARD, store.get_stored_block(missing_hash))
        .await
        .expect("error consumption must not hang")
        .unwrap_err();
    assert!(err.is_not_found(), "expected NotFound, got {err:?}");

    // The released permit lets the next background prefetch dispatch (the
    // real block's size is unknown to the empty index → 1 permit).
    let real_hash = real.block_index.block_hash;
    store.preflight_get(&[real_hash]).await.unwrap();
    wait_until(
        || store.stats().get_count >= 2,
        "second prefetch dispatched after the error released the permit",
    )
    .await;
    let got = tokio::time::timeout(GUARD, store.get_stored_block(real_hash))
        .await
        .expect("get after error must complete")
        .unwrap();
    assert_eq!(got, real);
    store.close().await.unwrap();
}

/// Flush drains the enqueued-but-undispatched backlog and releases held
/// budget, so parked dispatch tasks abandon instead of fetching stale hashes.
#[tokio::test(start_paused = true)]
async fn flush_drains_undispatched_backlog() {
    let mem = MemBlobStore::new("", true);
    let a = make_block(11);
    let b = make_block(22);
    seed_block(&mem, &a).await;
    seed_block(&mem, &b).await;

    // Budget = 1 permit: A's prefetch dispatches (unknown size → 1 permit) and
    // holds the whole budget; B's prefetch stays parked.
    let store = store_with_budget(&mem, 1).await;
    let (ha, hb) = (a.block_index.block_hash, b.block_index.block_hash);

    store.preflight_get(&[ha]).await.unwrap();
    wait_until(|| store.stats().get_count >= 1, "A's prefetch dispatched").await;
    store.preflight_get(&[hb]).await.unwrap();

    // Drain: clears B from the backlog and drops A's unconsumed entry
    // (releasing its permit, which wakes B's parked dispatcher to abandon).
    store.flush().await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        store.stats().get_count,
        1,
        "the drained backlog must not fetch B in the background"
    );

    // Both blocks remain demand-fetchable after the drain.
    let got_b = tokio::time::timeout(GUARD, store.get_stored_block(hb))
        .await
        .expect("get B after flush")
        .unwrap();
    assert_eq!(got_b, b);
    let got_a = tokio::time::timeout(GUARD, store.get_stored_block(ha))
        .await
        .expect("get A after flush")
        .unwrap();
    assert_eq!(got_a, a);
    assert_eq!(store.stats().get_count, 3, "A refetched, B fetched once");
    store.close().await.unwrap();
}

/// Demand-vs-parked-prefetch at budget = exactly one block's estimate: the
/// first prefetch holds the entire budget, the second parks; demand gets for
/// BOTH complete (coalesce with the dispatched one, claim the parked one) and
/// the later-freed dispatcher does not duplicate the claimed fetch.
#[tokio::test(start_paused = true)]
async fn demand_get_bypasses_parked_prefetch_at_one_block_budget() {
    let mem = MemBlobStore::new("", true);
    let store = store_with_budget(&mem, 60).await; // exactly one block's estimate
    let a = make_block(31);
    let b = make_block(42);
    store.put_stored_block(a.clone()).await.unwrap();
    store.put_stored_block(b.clone()).await.unwrap();
    store.flush().await.unwrap();
    let (ha, hb) = (a.block_index.block_hash, b.block_index.block_hash);

    store.preflight_get(&[ha, hb]).await.unwrap();
    // Exactly one of the two dispatches (60 of 60 permits); the other parks
    // (order is scheduler-dependent, so wait for the first fetch only).
    wait_until(|| store.stats().get_count >= 1, "first prefetch dispatched").await;

    // Demand gets for BOTH must complete without touching the budget.
    let got_b = tokio::time::timeout(GUARD, store.get_stored_block(hb))
        .await
        .expect("get B must not wait on the prefetch budget")
        .unwrap();
    assert_eq!(got_b, b);
    let got_a = tokio::time::timeout(GUARD, store.get_stored_block(ha))
        .await
        .expect("get A must not wait on the prefetch budget")
        .unwrap();
    assert_eq!(got_a, a);

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        store.stats().get_count,
        2,
        "each block fetched exactly once (no duplicate from the parked task)"
    );
    store.close().await.unwrap();
}

// --- deterministic coalescing under contention (a gated blob store) ---

/// Wraps [`MemBlobStore`], blocking reads of `chunks/…` block payloads until
/// the gate opens. Index reads (`store.lsi`) pass through so the index-owner
/// task never wedges.
#[derive(Debug)]
struct GatedStore {
    inner: MemBlobStore,
    gate: tokio::sync::watch::Receiver<bool>,
}

#[async_trait]
impl BlobStore for GatedStore {
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, StoreError> {
        Ok(Box::new(GatedClient {
            inner: self.inner.new_client().await?,
            gate: self.gate.clone(),
        }))
    }
    fn name(&self) -> String {
        "gated".into()
    }
}

struct GatedClient {
    inner: Box<dyn BlobClient>,
    gate: tokio::sync::watch::Receiver<bool>,
}

#[async_trait]
impl BlobClient for GatedClient {
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError> {
        Ok(Box::new(GatedObject {
            inner: self.inner.new_object(path).await?,
            gated: path.starts_with("chunks/"),
            gate: self.gate.clone(),
        }))
    }
    async fn get_objects(&self, prefix: &str) -> Result<Vec<BlobProperties>, StoreError> {
        self.inner.get_objects(prefix).await
    }
    fn supports_locking(&self) -> bool {
        self.inner.supports_locking()
    }
    fn name(&self) -> String {
        self.inner.name()
    }
}

struct GatedObject {
    inner: Box<dyn BlobObject>,
    gated: bool,
    gate: tokio::sync::watch::Receiver<bool>,
}

#[async_trait]
impl BlobObject for GatedObject {
    async fn exists(&self) -> Result<bool, StoreError> {
        self.inner.exists().await
    }
    async fn lock_write_version(&mut self) -> Result<bool, StoreError> {
        self.inner.lock_write_version().await
    }
    async fn read(&self) -> Result<Vec<u8>, StoreError> {
        if self.gated {
            let mut gate = self.gate.clone();
            gate.wait_for(|open| *open)
                .await
                .map_err(|_| StoreError::Backend("gate dropped".into()))?;
        }
        self.inner.read().await
    }
    async fn write(&mut self, data: &[u8]) -> Result<bool, StoreError> {
        self.inner.write(data).await
    }
    async fn delete(&mut self) -> Result<(), StoreError> {
        self.inner.delete().await
    }
    fn name(&self) -> String {
        self.inner.name()
    }
}

/// Coalescing under contention, deterministically: the dispatched prefetch is
/// held open at the blob layer while 8 demand gets pile onto its entry; when
/// the gate opens, all complete off the single underlying fetch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_gets_coalesce_with_dispatched_prefetch() {
    let mem = MemBlobStore::new("", true);
    let block = make_block(5);
    seed_block(&mem, &block).await;

    let (gate_tx, gate_rx) = tokio::sync::watch::channel(false);
    let gated: Arc<dyn BlobStore> = Arc::new(GatedStore {
        inner: mem,
        gate: gate_rx,
    });
    let store = Arc::new(
        RemoteBlockStore::with_prefetch_budget(gated, AccessType::ReadOnly, 4, 1024, None)
            .await
            .unwrap(),
    );
    let hash = block.block_index.block_hash;

    store.preflight_get(&[hash]).await.unwrap();
    // get_count ticks at fetch start, before the gated read: dispatched ⇒ the
    // map entry exists and is held in-flight by the gate.
    let s = store.clone();
    tokio::time::timeout(GUARD, async move {
        while s.stats().get_count < 1 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("prefetch should dispatch");

    let mut handles = Vec::new();
    for _ in 0..8 {
        let store = store.clone();
        handles.push(tokio::spawn(
            async move { store.get_stored_block(hash).await },
        ));
    }
    // Let the gets reach the coalescing point, then open the gate.
    tokio::time::sleep(Duration::from_millis(50)).await;
    gate_tx.send(true).unwrap();

    for h in handles {
        let got = tokio::time::timeout(GUARD, h)
            .await
            .expect("coalesced get must complete")
            .unwrap()
            .unwrap();
        assert_eq!(got, block);
    }
    assert_eq!(
        store.stats().get_count,
        1,
        "all 8 gets coalesce on the single dispatched fetch"
    );
    store.close().await.unwrap();
}
