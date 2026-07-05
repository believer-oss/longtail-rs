//! Stage 4 differential suite (differential lane, via `longtail-ffi`) for the
//! store-index algebra added to `longtail-core`:
//!
//! 1. **CreateStoreIndexFromBlocks** — `StoreIndex::from_block_indexes` is
//!    byte-identical to `Longtail_CreateStoreIndexFromBlocks` over random block
//!    sets (concatenation is deterministic; C's own order is the input order).
//! 2. **GetExistingStoreIndex (byte-identity)** — on single-chunk-per-block
//!    store indexes (where every block's chunk offset equals its block index),
//!    `StoreIndex::get_existing_store_index` is byte-identical to
//!    `Longtail_GetExistingStoreIndex`.
//! 3. **GetExistingStoreIndex (semantic)** — on multi-chunk store indexes, every
//!    output field EXCEPT `block_tags` matches C byte-for-byte. `block_tags`
//!    diverge because `Longtail_GetExistingStoreIndex` indexes `m_BlockTags` with
//!    the *chunk* offset (longtail.c:7307) — a latent C bug that reads the wrong
//!    (or out-of-bounds) slot when a kept block's chunk offset differs from its
//!    block index. The Rust port emits the correct tag (matching
//!    `Longtail_MakeBlockIndex`, longtail.c:9145). Documented divergence.
//!
//! Compiles to nothing without the `differential` feature.
#![cfg(feature = "differential")]

use longtail_core as core;
use longtail_ffi as ffi;
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

// ---------------------------------------------------------- CreateStoreIndexFromBlocks

fn gen_block(r: &mut ChaCha8Rng, hash_id: u32) -> core::BlockIndex {
    let n = 1 + (r.next_u32() % 5) as usize;
    core::BlockIndex {
        block_hash: r.next_u64(),
        hash_identifier: hash_id,
        tag: r.next_u32(),
        chunk_hashes: (0..n).map(|_| r.next_u64()).collect(),
        chunk_sizes: (0..n).map(|_| 1 + r.next_u32() % 4096).collect(),
    }
}

fn c_from_blocks(blocks: &[core::BlockIndex]) -> Vec<u8> {
    // Keep the serialized block-index buffers alive until after
    // `write_to_buffer`: the C block index points into these bytes, and
    // `CreateStoreIndexFromBlocks` copies from them.
    let mut buffers: Vec<Vec<u8>> = blocks.iter().map(|b| b.to_bytes()).collect();
    let c_blocks: Vec<ffi::BlockIndex> = buffers
        .iter_mut()
        .map(|bytes| ffi::BlockIndex::new_from_buffer(bytes).expect("C reads block index"))
        .collect();
    let si = ffi::StoreIndex::new_from_blocks(c_blocks).expect("C create-from-blocks");
    let out = si.write_to_buffer().expect("C writes store index");
    drop(buffers);
    out
}

#[test]
fn create_store_index_from_blocks_byte_identity() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5734_0001);
    for _ in 0..300 {
        let hash_id = 0x626c6b33; // blk3 — a realistic shared identifier
        let block_count = (r.next_u32() % 8) as usize;
        let blocks: Vec<core::BlockIndex> = (0..block_count)
            .map(|_| gen_block(&mut r, hash_id))
            .collect();
        let rust = core::StoreIndex::from_block_indexes(&blocks).unwrap();
        if block_count == 0 {
            // C requires block_count==0 → empty; our empty(0) serializes the bare
            // header. (The C wrapper needs at least the call to succeed.)
            assert_eq!(rust.to_bytes().len(), 16);
            continue;
        }
        assert_eq!(
            c_from_blocks(&blocks),
            rust.to_bytes(),
            "from-blocks bytes differ"
        );
    }
}

// ---------------------------------------------------------- GetExistingStoreIndex

fn c_get_existing(index_bytes: &[u8], chunks: &[u64], pct: u32) -> Vec<u8> {
    let c = ffi::StoreIndex::new_from_buffer(index_bytes).expect("C reads store index");
    let e = c
        .get_existing_store_index(chunks.to_vec(), pct)
        .expect("C get-existing");
    e.write_to_buffer().expect("C writes existing")
}

/// Single chunk per block ⇒ each block's chunk offset equals its block index, so
/// C's tag-offset bug reads the correct slot and byte-identity holds.
fn gen_single_chunk_index(r: &mut ChaCha8Rng, hash_id: u32) -> core::StoreIndex {
    let b = 1 + (r.next_u32() % 10) as usize;
    let mut si = core::StoreIndex::empty(hash_id);
    for i in 0..b {
        si.block_hashes.push(r.next_u64()); // unique-ish block hashes
        si.block_tags.push(r.next_u32());
        si.block_chunks_offsets.push(i as u32);
        si.block_chunk_counts.push(1);
        si.chunk_hashes.push((r.next_u32() % 20) as u64); // small pool → query hits
        si.chunk_sizes.push(1 + r.next_u32() % 1000);
    }
    si
}

fn gen_multi_chunk_index(r: &mut ChaCha8Rng, hash_id: u32) -> core::StoreIndex {
    let b = 1 + (r.next_u32() % 6) as usize;
    let mut si = core::StoreIndex::empty(hash_id);
    for _ in 0..b {
        si.block_hashes.push(r.next_u64());
        si.block_tags.push(r.next_u32());
        si.block_chunks_offsets.push(si.chunk_hashes.len() as u32);
        let n = 1 + (r.next_u32() % 4) as usize;
        si.block_chunk_counts.push(n as u32);
        for _ in 0..n {
            si.chunk_hashes.push((r.next_u32() % 30) as u64);
            si.chunk_sizes.push(1 + r.next_u32() % 1000);
        }
    }
    si
}

fn gen_query(r: &mut ChaCha8Rng, pool: u64) -> Vec<u64> {
    let n = (r.next_u32() % 12) as usize;
    (0..n).map(|_| r.next_u64() % pool).collect()
}

#[test]
fn get_existing_store_index_byte_identity_single_chunk() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5734_0002);
    for _ in 0..400 {
        let si = gen_single_chunk_index(&mut r, 0x626c6b33);
        let query = gen_query(&mut r, 25);
        for pct in [0u32, 25, 50, 80, 100] {
            let rust = si.get_existing_store_index(&query, pct).to_bytes();
            let c = c_get_existing(&si.to_bytes(), &query, pct);
            assert_eq!(
                rust, c,
                "get-existing single-chunk bytes differ (pct={pct})"
            );
        }
    }
}

#[test]
fn get_existing_store_index_semantic_multi_chunk() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5734_0003);
    for _ in 0..400 {
        let si = gen_multi_chunk_index(&mut r, 0x626c6b33);
        let query = gen_query(&mut r, 35);
        for pct in [0u32, 40, 80, 100] {
            let rust = si.get_existing_store_index(&query, pct);
            let c_bytes = c_get_existing(&si.to_bytes(), &query, pct);
            let c = core::StoreIndex::from_bytes(&c_bytes).unwrap();
            // Every field EXCEPT block_tags matches C exactly (see the C tag bug
            // note above).
            assert_eq!(rust.hash_identifier, c.hash_identifier, "hash id");
            assert_eq!(rust.block_hashes, c.block_hashes, "block selection/order");
            assert_eq!(rust.block_chunk_counts, c.block_chunk_counts, "counts");
            assert_eq!(rust.block_chunks_offsets, c.block_chunks_offsets, "offsets");
            assert_eq!(rust.chunk_hashes, c.chunk_hashes, "chunk hashes");
            assert_eq!(rust.chunk_sizes, c.chunk_sizes, "chunk sizes");
        }
    }
}
