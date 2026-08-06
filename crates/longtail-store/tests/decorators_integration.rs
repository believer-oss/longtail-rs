//! The downsync-shaped read path in pure Rust.
//!
//! Fetch every block of a committed compressed fixture store through
//! `Compress(Cache(Remote(fs-blob)))`, then:
//! - verify each decompressed block's payload is `Σ chunk_sizes` bytes and every
//!   chunk's blake3 hash recomputes to the stored chunk hash (the codecs +
//!   the decode gate, now in pure Rust);
//! - verify the cache was populated with byte-identical `.lrb` files (the
//!   passthrough property: `.lrb` bytes == the remote `.lsb` bytes, only the
//!   extension differs).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use longtail_core::StoreIndex;
use longtail_core::hash::{BLAKE3_ID, blake3_hash};
use longtail_store::block_store::BlockStore;
use longtail_store::cache::CacheBlockStore;
use longtail_store::compress::CompressBlockStore;
use longtail_store::remote::RemoteBlockStore;
use longtail_store::{AccessType, FsBlobStore, block_path};

fn fixture_store_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/stores/comp-lz4/store")
}

/// The cache-relative `.lrb` path for a block hash (mirrors `cache_block_path`).
fn lrb_rel(hash: u64) -> String {
    let file_name = format!("0x{hash:016x}.lrb");
    let sub = &file_name[2..6];
    format!("chunks/{sub}/{file_name}")
}

/// Build a bare `CacheBlockStore(cache_dir, Remote(fs-blob fixture))` (no
/// compression layer) with the given size limit.
async fn cache_over_fixture(cache_dir: &Path, size_limit: Option<u64>) -> CacheBlockStore {
    let blob_store = Arc::new(FsBlobStore::new(fixture_store_dir(), false));
    let remote: Arc<dyn BlockStore> = Arc::new(
        RemoteBlockStore::new(blob_store, AccessType::ReadOnly, 2)
            .await
            .unwrap(),
    );
    CacheBlockStore::new(cache_dir, remote, size_limit)
        .await
        .unwrap()
}

fn fixture_index() -> StoreIndex {
    StoreIndex::from_bytes(&std::fs::read(fixture_store_dir().join("store.lsi")).unwrap()).unwrap()
}

/// A deleted (evicted) cache file must be transparently re-fetched from the
/// remote AND rewritten to disk. The C library got this wrong: its cache-dir
/// store index stayed authoritative, so a deleted block was never rewritten.
/// Our store treats that index as advisory and probes the block file directly.
#[tokio::test]
async fn cache_refetches_and_recaches_deleted_block() {
    let index = fixture_index();
    let hash = index.block_hashes[0];
    let cache_dir = tempfile::tempdir().unwrap();
    let cached = cache_over_fixture(cache_dir.path(), Some(u64::MAX)).await;
    let lrb = cache_dir.path().join(lrb_rel(hash));

    // 1. First get: cache miss → remote fetch (get_count = 1) + write-back.
    let first = cached.get_stored_block(hash).await.unwrap();
    assert!(lrb.exists(), "first get should populate the cache");
    assert_eq!(cached.stats().get_count, 1);

    // 2. Evict the cache file out from under the live store.
    std::fs::remove_file(&lrb).unwrap();
    assert!(!lrb.exists());

    // 3. Second get MUST re-fetch from the remote (get_count = 2) and rewrite
    //    the cache file — not serve a phantom hit from a stale index.
    let second = cached.get_stored_block(hash).await.unwrap();
    assert_eq!(
        first.to_bytes(),
        second.to_bytes(),
        "same block bytes after re-fetch"
    );
    assert!(
        lrb.exists(),
        "deleted cache file must be re-created on re-fetch"
    );
    assert_eq!(
        cached.stats().get_count,
        2,
        "second get must hit the remote again, not a stale-index phantom"
    );
    cached.close().await.unwrap();
}

/// A cache hit stamps the block file's mtime to now, so the LRU sweep sees it as
/// recently used (only when a size limit is configured).
#[tokio::test]
async fn cache_hit_touches_mtime_when_limited() {
    let index = fixture_index();
    let hash = index.block_hashes[0];
    let cache_dir = tempfile::tempdir().unwrap();
    let cached = cache_over_fixture(cache_dir.path(), Some(u64::MAX)).await;
    let lrb = cache_dir.path().join(lrb_rel(hash));

    // Populate, then backdate the file far into the past.
    cached.get_stored_block(hash).await.unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&lrb)
        .unwrap()
        .set_modified(old)
        .unwrap();

    // A hit must bump the mtime forward (touch-on-hit, awaited).
    cached.get_stored_block(hash).await.unwrap();
    let after = std::fs::metadata(&lrb).unwrap().modified().unwrap();
    assert!(
        after > old,
        "cache hit should advance the block file's mtime"
    );
    cached.close().await.unwrap();
}

/// Closing a size-limited cache evicts down to the budget. The fixture's large
/// block alone exceeds a small cap, so it must be dropped and the small one kept.
#[tokio::test]
async fn cache_evicts_to_limit_on_close() {
    let index = fixture_index();
    assert_eq!(index.block_count(), 2, "fixture assumption: two blocks");
    let cache_dir = tempfile::tempdir().unwrap();
    // 300 KiB budget: holds the ~219 KB block but not the ~1.5 MB one.
    let cached = cache_over_fixture(cache_dir.path(), Some(300_000)).await;

    // Populate both blocks.
    let mut sizes = Vec::new();
    for &h in &index.block_hashes {
        cached.get_stored_block(h).await.unwrap();
        let lrb = cache_dir.path().join(lrb_rel(h));
        sizes.push((h, std::fs::metadata(&lrb).unwrap().len()));
    }
    let (small_hash, small_size) = *sizes.iter().min_by_key(|(_, s)| *s).unwrap();
    let (large_hash, large_size) = *sizes.iter().max_by_key(|(_, s)| *s).unwrap();
    assert!(
        large_size > 300_000 && small_size <= 300_000,
        "size assumption"
    );

    // Make the large block the LRU (oldest) so it is evicted first; the small
    // block, being newer, is reached only after we're already under budget.
    let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    for (h, mtime) in [
        (large_hash, base),
        (small_hash, base + Duration::from_secs(10)),
    ] {
        std::fs::OpenOptions::new()
            .write(true)
            .open(cache_dir.path().join(lrb_rel(h)))
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    // Close → eviction runs.
    cached.close().await.unwrap();

    assert!(
        cache_dir.path().join(lrb_rel(small_hash)).exists(),
        "the block that fits must survive"
    );
    assert!(
        !cache_dir.path().join(lrb_rel(large_hash)).exists(),
        "the oversized block must be evicted"
    );
}

#[tokio::test]
async fn downsync_read_path_compress_cache_remote() {
    let store_dir = fixture_store_dir();
    let index_bytes = std::fs::read(store_dir.join("store.lsi")).unwrap();
    let index = StoreIndex::from_bytes(&index_bytes).unwrap();
    assert_eq!(index.hash_identifier, BLAKE3_ID, "fixture must be blake3");
    assert!(index.block_count() >= 1);

    // Compress(Cache(Remote(fs-blob))) — compression outermost. Locking is
    // DISABLED for this read-only fixture store: fs reads with locking enabled
    // create `._lck` files (never unlinked, by design), which must not pollute
    // the committed read-only fixtures. A read-only downsync needs no write CAS.
    let blob_store = Arc::new(FsBlobStore::new(&store_dir, false));
    let remote: Arc<dyn BlockStore> = Arc::new(
        RemoteBlockStore::new(blob_store, AccessType::ReadOnly, 4)
            .await
            .unwrap(),
    );
    let cache_dir = tempfile::tempdir().unwrap();
    let cached: Arc<dyn BlockStore> = Arc::new(
        CacheBlockStore::new(cache_dir.path(), remote, None)
            .await
            .unwrap(),
    );
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap(),
    );
    let store = CompressBlockStore::new(cached, pool);

    for b in 0..index.block_count() as usize {
        let bi = index.block_index_at(b).unwrap();
        let block = store.get_stored_block(bi.block_hash).await.unwrap();

        // Decompressed payload is exactly Σ chunk_sizes bytes.
        let total: usize = bi.chunk_sizes.iter().map(|&s| s as usize).sum();
        assert_eq!(
            block.payload.len(),
            total,
            "block {:#x}: decompressed payload length",
            bi.block_hash
        );

        // Every chunk's blake3 hash recomputes.
        let mut offset = 0usize;
        for (i, &size) in bi.chunk_sizes.iter().enumerate() {
            let chunk = &block.payload[offset..offset + size as usize];
            assert_eq!(
                blake3_hash(chunk),
                bi.chunk_hashes[i],
                "block {:#x} chunk {i}: hash mismatch",
                bi.block_hash
            );
            offset += size as usize;
        }
    }

    store.flush().await.unwrap();

    // Cache populated with byte-identical `.lrb` files.
    for b in 0..index.block_count() as usize {
        let hash = index.block_hashes[b];
        let lsb = store_dir.join(block_path("chunks", hash));
        let file_name = format!("0x{hash:016x}.lrb");
        let sub = &file_name[2..6];
        let lrb = cache_dir.path().join(format!("chunks/{sub}/{file_name}"));
        let lsb_bytes = std::fs::read(&lsb).unwrap();
        let lrb_bytes = std::fs::read(&lrb)
            .unwrap_or_else(|e| panic!("cache .lrb missing at {}: {e}", lrb.display()));
        assert_eq!(
            lrb_bytes, lsb_bytes,
            "cache .lrb must be byte-identical to the remote .lsb"
        );
    }

    store.close().await.unwrap();
}

/// A compressed block whose decoded payload is shorter than the chunks its own
/// index claims must be rejected by the decompression decorator.
///
/// `decode_block_payload` compares the decoded length against the frame's
/// *self-declared* `uncompressed_size`; both numbers come from the same
/// untrusted bytes, so they agree with each other while saying nothing about the
/// block index — which is what the apply path slices the buffer with.
#[tokio::test]
async fn compressed_block_shorter_than_its_chunk_sizes_is_rejected() {
    use longtail_core::compress::{LZ4_ID, encode_block_payload};
    use longtail_core::{BlockIndex, StoredBlock};

    let tmp = tempfile::tempdir().unwrap();
    let store_dir = tmp.path().join("store");

    // Index claims one 4096-byte chunk; the frame carries 8 bytes.
    let block_hash = 0x0123_4567_89ab_cdefu64;
    let block_index = BlockIndex {
        block_hash,
        hash_identifier: BLAKE3_ID,
        tag: LZ4_ID,
        chunk_hashes: vec![0xfeed_face_dead_beef],
        chunk_sizes: vec![4096],
    };
    let framed = encode_block_payload(LZ4_ID, &[0u8; 8]).unwrap();
    let lsb = StoredBlock {
        block_index,
        payload: framed,
    }
    .to_bytes();

    let rel = block_path("chunks", block_hash);
    let path = store_dir.join(&rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &lsb).unwrap();

    let blob_store = Arc::new(FsBlobStore::new(store_dir, false));
    let remote: Arc<dyn BlockStore> = Arc::new(
        RemoteBlockStore::new(blob_store, AccessType::ReadOnly, 2)
            .await
            .unwrap(),
    );
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap(),
    );
    let compressed = CompressBlockStore::new(remote, pool);

    let err = compressed
        .get_stored_block(block_hash)
        .await
        .expect_err("a block decoding shorter than its chunk_sizes must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("claims"),
        "expected the block-index mismatch error, got: {msg}"
    );
}
