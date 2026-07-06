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

use std::path::PathBuf;
use std::sync::Arc;

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
        CacheBlockStore::new(cache_dir.path(), remote)
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
