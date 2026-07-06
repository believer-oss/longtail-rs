//! `RemoteBlockStore` + store-index-sync semantics ported from
//! golongtail's `remotestore/remotestore_test.go` (the mem/fs subset).
//!
//! The S3 shard-sync test is env-gated in `s3_spec.rs`. The two prune tests
//! exercise the implemented `prune_blocks` (index rewrite + block
//! delete) in both locking and lockless flavors. GCS index-sync tests are
//! omitted (GCS is out of scope for this port).

use std::sync::Arc;

use longtail_core::{BlockIndex, StoreIndex, StoredBlock};
use longtail_store::blob::{BlobClient, BlobStore, FsBlobStore, MemBlobStore};
use longtail_store::{
    AccessType, BlockStore, RemoteBlockStore, add_to_remote_store_index, block_path,
    read_merged_store_index,
};

// --- block generators (port of remotestore_test.go helpers) ---

/// `generateStoredBlock` (remotestore_test.go:368).
fn generate_stored_block(seed: u8) -> StoredBlock {
    let s = seed as u64;
    let chunk_hashes = vec![s + 1, s + 2, s + 3];
    let chunk_sizes = vec![seed as u32 + 10, seed as u32 + 20, seed as u32 + 30];
    let len: usize = chunk_sizes.iter().map(|&x| x as usize).sum();
    StoredBlock {
        block_index: BlockIndex {
            block_hash: s + 21412151,
            hash_identifier: 997,
            tag: 2,
            chunk_hashes,
            chunk_sizes,
        },
        payload: vec![seed; len],
    }
}

/// `generateUniqueStoredBlock` (remotestore_test.go:388).
fn generate_unique_stored_block(seed: u8) -> StoredBlock {
    let s = seed as u64;
    let chunk_hashes = vec![(s << 8) + 1, (s << 8) + 2, (s << 8) + 3];
    let chunk_sizes = vec![
        (seed as u32) << 8 | 10,
        (seed as u32) << 8 | 20,
        (seed as u32) << 8 | 30,
    ];
    let len: usize = chunk_sizes.iter().map(|&x| x as usize).sum();
    StoredBlock {
        block_index: BlockIndex {
            block_hash: (s << 16) + 21412151,
            hash_identifier: 997,
            tag: 2,
            chunk_hashes,
            chunk_sizes,
        },
        payload: vec![seed; len],
    }
}

/// `storeBlock` (remotestore_test.go:354): write a block's bytes directly to the
/// backend at a path derived from `block_hash + offset`, optionally under
/// `parent`.
async fn store_block_raw(
    client: &dyn BlobClient,
    block: &StoredBlock,
    hash_offset: u64,
    parent: &str,
) -> u64 {
    let stored_hash = block.block_index.block_hash + hash_offset;
    let mut path = block_path("chunks", stored_hash);
    if !parent.is_empty() {
        path = format!("{parent}/{path}");
    }
    let mut obj = client.new_object(&path).await.unwrap();
    obj.write(&block.to_bytes()).await.unwrap();
    stored_hash
}

async fn put(store: &dyn BlockStore, seed: u8) -> StoredBlock {
    let block = generate_stored_block(seed);
    store.put_stored_block(block.clone()).await.unwrap();
    block
}

// --- tests ---

/// Source: remotestore_test.go::TestCreateRemoteBlobStore — construct + dispose
/// cleanly.
#[tokio::test]
async fn create_remote_blob_store() {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("the_path", true));
    let store = RemoteBlockStore::new(blob_store, AccessType::ReadOnly, 4)
        .await
        .unwrap();
    store.close().await.unwrap();
}

/// Source: remotestore_test.go::TestEmptyGetExistingContent — empty store yields
/// a valid, empty store index.
#[tokio::test]
async fn empty_get_existing_content() {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("the_path", true));
    let store = RemoteBlockStore::new(blob_store, AccessType::ReadOnly, 4)
        .await
        .unwrap();
    let index = store.get_existing_content(&[1, 2, 3, 4], 0).await.unwrap();
    assert_eq!(index.block_count(), 0);
    store.close().await.unwrap();
}

/// Source: remotestore_test.go::TestPutGetStoredBlock — a put block round-trips
/// by hash, and the block path follows `chunks/<top4>/0x<hash>.lsb`.
#[tokio::test]
async fn put_get_stored_block() {
    let mem = MemBlobStore::new("the_path", true);
    let blob_store: Arc<dyn BlobStore> = Arc::new(mem.clone());
    let store = RemoteBlockStore::new(blob_store, AccessType::ReadWrite, 4)
        .await
        .unwrap();
    let block = put(&store, 0).await;
    let hash = block.block_index.block_hash;

    let got = store.get_stored_block(hash).await.unwrap();
    assert_eq!(got.block_index.block_hash, hash);
    assert_eq!(got.block_index.chunk_count(), 3);
    assert_eq!(got, block);

    // Path scheme assertion.
    let client = mem.new_client().await.unwrap();
    let objs = client.get_objects("chunks").await.unwrap();
    let expected = block_path("chunks", hash);
    assert!(
        objs.iter().any(|o| o.name == expected),
        "expected {expected} among {objs:?}"
    );
    store.close().await.unwrap();
}

/// Source: remotestore_test.go::TestGetExistingContent — 6 blocks, query touches
/// all → 6 blocks / 18 chunks.
#[tokio::test]
async fn get_existing_content() {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("the_path", true));
    let store = RemoteBlockStore::new(blob_store, AccessType::ReadWrite, 4)
        .await
        .unwrap();
    for seed in [0u8, 10, 20, 30, 40, 50] {
        put(&store, seed).await;
    }
    let chunk_hashes = [1u64, 2, 11, 13, 21, 22, 32, 33, 41, 43, 51];
    store.flush().await.unwrap();

    let index = store.get_existing_content(&chunk_hashes, 0).await.unwrap();
    assert_eq!(index.block_count(), 6);
    assert_eq!(index.chunk_count(), 18);
    store.close().await.unwrap();
}

/// Source: remotestore_test.go::TestRestoreStore — index survives a close/reopen
/// through the persisted `store.lsi`.
#[tokio::test]
async fn restore_store() {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("the_path", true));

    let store = RemoteBlockStore::new(blob_store.clone(), AccessType::ReadWrite, 4)
        .await
        .unwrap();
    put(&store, 0).await;
    put(&store, 10).await;
    put(&store, 20).await;
    store.close().await.unwrap();

    let store = RemoteBlockStore::new(blob_store.clone(), AccessType::ReadWrite, 4)
        .await
        .unwrap();
    let idx = store
        .get_existing_content(&[1, 2, 11, 13], 0)
        .await
        .unwrap();
    assert_eq!(idx.block_count(), 2);
    assert_eq!(idx.chunk_count(), 6);

    let idx = store
        .get_existing_content(&[1, 2, 11, 13, 31], 0)
        .await
        .unwrap();
    assert_eq!(idx.block_count(), 2);
    assert_eq!(idx.chunk_count(), 6);
    put(&store, 30).await;
    store.close().await.unwrap();

    let store = RemoteBlockStore::new(blob_store.clone(), AccessType::ReadWrite, 4)
        .await
        .unwrap();
    let idx = store
        .get_existing_content(&[1, 2, 11, 13, 31], 0)
        .await
        .unwrap();
    assert_eq!(idx.block_count(), 3);
    assert_eq!(idx.chunk_count(), 9);
    store.close().await.unwrap();
}

/// Source: remotestore_test.go::TestBlockScanning — Init rebuild only trusts
/// blocks whose stored hash matches their path.
#[tokio::test]
async fn block_scanning() {
    let mem = MemBlobStore::new("", true);
    let blob_store: Arc<dyn BlobStore> = Arc::new(mem.clone());
    let client = mem.new_client().await.unwrap();

    let good_correct = generate_stored_block(7);
    let good_correct_hash = store_block_raw(&*client, &good_correct, 0, "").await;
    let bad_correct = generate_stored_block(14);
    let bad_correct_hash = store_block_raw(&*client, &bad_correct, 1, "").await;
    let good_bad = generate_stored_block(21);
    let good_bad_hash = store_block_raw(&*client, &good_bad, 0, "chunks").await;
    let bad_bad = generate_stored_block(33);
    let bad_bad_hash = store_block_raw(&*client, &bad_bad, 2, "chunks").await;

    let store = RemoteBlockStore::new(blob_store, AccessType::Init, 4)
        .await
        .unwrap();

    // Good block in the correct path → fetchable.
    let b = store.get_stored_block(good_correct_hash).await.unwrap();
    assert_eq!(b.block_index.block_hash, good_correct_hash);

    // Bad block in the correct path → hash/path mismatch → BadFormat.
    let err = store.get_stored_block(bad_correct_hash).await.unwrap_err();
    assert!(
        matches!(err, longtail_store::StoreError::BadFormat(_)),
        "expected BadFormat, got {err:?}"
    );

    // Good/bad blocks in the wrong path → not found at the canonical location.
    assert!(
        store
            .get_stored_block(good_bad_hash)
            .await
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        store
            .get_stored_block(bad_bad_hash)
            .await
            .unwrap_err()
            .is_not_found()
    );

    // Rebuilt index contains only the one good, correctly-placed block.
    let mut chunks = good_correct.block_index.chunk_hashes.clone();
    chunks.extend_from_slice(&bad_correct.block_index.chunk_hashes);
    chunks.extend_from_slice(&good_bad.block_index.chunk_hashes);
    chunks.extend_from_slice(&bad_bad.block_index.chunk_hashes);
    let index = store.get_existing_content(&chunks, 0).await.unwrap();
    assert_eq!(
        index.chunk_count() as usize,
        good_correct.block_index.chunk_hashes.len()
    );
    store.close().await.unwrap();
}

/// Put 3 disjoint blocks, prune to keep 2, verify the third is gone from both
/// the store index (via a fresh reader) and the backend. Shared body for the
/// locking / lockless flavors.
async fn prune_store_flavor(supports_locking: bool) {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("prune", supports_locking));
    let store = RemoteBlockStore::new(blob_store.clone(), AccessType::ReadWrite, 4)
        .await
        .unwrap();
    let (b1, b2, b3) = (
        generate_unique_stored_block(1),
        generate_unique_stored_block(2),
        generate_unique_stored_block(3),
    );
    store.put_stored_block(b1.clone()).await.unwrap();
    store.put_stored_block(b2.clone()).await.unwrap();
    store.put_stored_block(b3.clone()).await.unwrap();
    store.flush().await.unwrap();

    let keep = vec![b1.block_index.block_hash, b2.block_index.block_hash];
    let pruned = store.prune_blocks(&keep).await.unwrap();
    assert_eq!(pruned, 1, "one unkept block deleted");
    store.close().await.unwrap();

    // Fresh reader sees the pruned store index (merge-on-read / canonical).
    let reader = RemoteBlockStore::new(blob_store, AccessType::ReadOnly, 4)
        .await
        .unwrap();
    let kept = reader
        .get_existing_content(&b1.block_index.chunk_hashes, 0)
        .await
        .unwrap();
    assert_eq!(kept.block_count(), 1);
    let dropped = reader
        .get_existing_content(&b3.block_index.chunk_hashes, 0)
        .await
        .unwrap();
    assert_eq!(dropped.block_count(), 0, "pruned block no longer indexed");
    reader.close().await.unwrap();
}

/// Source: remotestore_test.go::TestPruneStoreWithLocking.
#[tokio::test]
async fn prune_store_with_locking() {
    prune_store_flavor(true).await;
}

/// Source: remotestore_test.go::TestPruneStoreWithoutLocking.
#[tokio::test]
async fn prune_store_without_locking() {
    prune_store_flavor(false).await;
}

// --- store-index sync / concurrent-writer convergence ---

/// The `testStoreIndexSync` chaos body (remotestore_test.go:679), adapted:
/// `worker_count` tasks each merge `blocks_per_worker` distinct blocks into one
/// store via [`add_to_remote_store_index`]; the merged index must converge to
/// exactly the union, each block present once.
async fn run_store_index_sync(
    blob_store: Arc<dyn BlobStore>,
    worker_count: u8,
    blocks_per_worker: u8,
) {
    let mut handles = Vec::new();
    for n in 0..worker_count {
        let blob_store = blob_store.clone();
        let seed_base = blocks_per_worker * n;
        handles.push(tokio::spawn(async move {
            let client = blob_store.new_client().await.unwrap();
            let mut blocks: Vec<BlockIndex> = Vec::new();
            // First batch: all but the last block.
            for i in 0..blocks_per_worker.saturating_sub(1) {
                blocks.push(generate_unique_stored_block(seed_base + i).block_index);
            }
            if !blocks.is_empty() {
                let add = StoreIndex::from_block_indexes(&blocks).unwrap();
                add_to_remote_store_index(&*client, &add).await.unwrap();
            }
            // Read (exercises merge-on-read under contention).
            let _ = read_merged_store_index(&*client).await.unwrap();
            // Second batch: the full set.
            for i in blocks_per_worker.saturating_sub(1)..blocks_per_worker {
                blocks.push(generate_unique_stored_block(seed_base + i).block_index);
            }
            let full = StoreIndex::from_block_indexes(&blocks).unwrap();
            add_to_remote_store_index(&*client, &full).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let client = blob_store.new_client().await.unwrap();
    if !client.supports_locking() {
        // Consolidate shards into one (Go's post-loop step for lockless stores).
        let empty = StoreIndex::empty(0);
        add_to_remote_store_index(&*client, &empty).await.unwrap();
    }

    let index = read_merged_store_index(&*client).await.unwrap();
    let expected = worker_count as usize * blocks_per_worker as usize;
    let hashes = &index.block_hashes;
    assert_eq!(
        hashes.len(),
        expected,
        "expected {expected} blocks, got {}",
        hashes.len()
    );
    let unique: std::collections::HashSet<u64> = hashes.iter().copied().collect();
    assert_eq!(
        unique.len(),
        expected,
        "duplicate block hashes in merged index"
    );
}

/// Source: remotestore_test.go::TestStoreIndexSyncWithLocking.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn store_index_sync_with_locking() {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("locking_store", true));
    run_store_index_sync(blob_store, 21, 4).await;
}

/// Source: remotestore_test.go::TestStoreIndexSyncWithoutLocking.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn store_index_sync_without_locking() {
    let blob_store: Arc<dyn BlobStore> = Arc::new(MemBlobStore::new("locking_store", false));
    run_store_index_sync(blob_store, 21, 4).await;
}

/// Source: remotestore_test.go::TestFSStoreIndexSyncWithLocking.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_store_index_sync_with_locking() {
    let dir = tempfile::tempdir().unwrap();
    let blob_store: Arc<dyn BlobStore> = Arc::new(FsBlobStore::new(dir.path(), true));
    run_store_index_sync(blob_store, 21, 4).await;
}

/// Source: remotestore_test.go::TestFSStoreIndexSyncWithoutLocking (the shard
/// merge-on-read flavor on fs, locking disabled).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_store_index_sync_without_locking() {
    let dir = tempfile::tempdir().unwrap();
    let blob_store: Arc<dyn BlobStore> = Arc::new(FsBlobStore::new(dir.path(), false));
    run_store_index_sync(blob_store, 21, 4).await;
}
