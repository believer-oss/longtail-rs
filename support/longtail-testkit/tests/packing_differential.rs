//! Differential suite (differential lane, via `longtail-ffi`) for the
//! new upload-path algebra in `longtail-core`:
//!
//! 1. **CreateStoreIndex (packing)** — `pack::create_store_index` is
//!    byte-identical to `Longtail_CreateStoreIndex` over the committed corpus
//!    version indexes (the greedy block-fill loop + block hashing).
//! 2. **PruneStoreIndex** — `StoreIndex::prune` is byte-identical to
//!    `Longtail_PruneStoreIndex` over the committed store indexes for a range of
//!    keep-sets (subset / all / none / with-absent-hashes).
//!
//! `CreateMissingContent` is exercised end-to-end by the upsync byte-gate (it is
//! `DiffHashes` + `CreateStoreIndex`, and its output reproduces the committed
//! version-local `.lsi` byte-for-byte). Compiles to nothing without
//! `differential`.
#![cfg(feature = "differential")]

use longtail_core as core;
use longtail_core::pack::create_store_index;
use longtail_ffi as ffi;
use longtail_testkit::differential::{self, read_version_index};
use longtail_testkit::paths::fixtures_dir;

const MAX_BLOCK_SIZE: u32 = 8 * 1024 * 1024;
const MAX_CHUNKS_PER_BLOCK: u32 = 1024;

/// Packing: `create_store_index` over a version's chunks == C `CreateStoreIndex`.
fn assert_packing(lvi_rel: &str) {
    let path = fixtures_dir().join(lvi_rel);
    // C side.
    let (_reg, hash) = differential::blake3_hash();
    let vi_ffi = read_version_index(&path);
    let c_store = ffi::StoreIndex::new_from_version_index(
        &hash,
        &vi_ffi,
        MAX_BLOCK_SIZE,
        MAX_CHUNKS_PER_BLOCK,
    )
    .expect("C CreateStoreIndex");
    let c_bytes = c_store.write_to_buffer().expect("C write store index");

    // Rust side.
    let bytes = std::fs::read(&path).unwrap();
    let vi = core::VersionIndex::from_bytes(&bytes).unwrap();
    let rust = create_store_index(
        &vi.chunk_hashes,
        &vi.chunk_sizes,
        &vi.chunk_tags,
        MAX_BLOCK_SIZE,
        MAX_CHUNKS_PER_BLOCK,
        &core::Blake3,
    )
    .unwrap();
    let rust_bytes = rust.to_bytes();

    assert_eq!(
        rust_bytes,
        c_bytes,
        "CreateStoreIndex byte mismatch for {lvi_rel} (rust {} bytes vs C {} bytes)",
        rust_bytes.len(),
        c_bytes.len()
    );
}

#[test]
fn packing_matches_c_over_corpus() {
    // blake3 cells across compression + chunk-size variants (packing is
    // compression-independent; the block hash is over chunk hashes).
    for cell in [
        "stores/comp-none/zoo.lvi",
        "stores/comp-zstd_max/zoo.lvi",
        "stores/chunk-1024/zoo.lvi",
        "stores/chunk-131072/zoo.lvi",
        "stores/default/zoo.lvi",
        "stores/default/chain-v1.lvi",
        "stores/default/chain-v2.lvi",
        "stores/default/chain-v3.lvi",
    ] {
        assert_packing(cell);
    }
}

/// Prune: `StoreIndex::prune` == C `Longtail_PruneStoreIndex` for several keep-sets.
fn assert_prune(lsi_rel: &str) {
    let path = fixtures_dir().join(lsi_rel);
    let bytes = std::fs::read(&path).unwrap();
    let core_si = core::StoreIndex::from_bytes(&bytes).unwrap();
    let all: Vec<u64> = core_si.block_hashes.clone();

    let keep_sets: Vec<Vec<u64>> = vec![
        Vec::new(),                               // none
        all.clone(),                              // all
        all.iter().step_by(2).copied().collect(), // every other
        all.iter().take(1).copied().collect(),    // first
        // with an absent hash mixed in (must be ignored)
        {
            let mut v: Vec<u64> = all.iter().take(2).copied().collect();
            v.push(0xdead_beef_dead_beef);
            v
        },
    ];

    for keep in keep_sets {
        let c_store = ffi::StoreIndex::new_from_buffer(&bytes).expect("C read store index");
        let c_pruned = c_store.prune(&keep).expect("C prune");
        let c_bytes = c_pruned.write_to_buffer().expect("C write pruned");

        let rust_bytes = core_si.prune(&keep).to_bytes();
        assert_eq!(
            rust_bytes,
            c_bytes,
            "PruneStoreIndex byte mismatch for {lsi_rel} keep={} (rust {} vs C {})",
            keep.len(),
            rust_bytes.len(),
            c_bytes.len()
        );
    }
}

#[test]
fn prune_matches_c() {
    for cell in [
        "stores/default/store/store.lsi",
        "stores/comp-none/store/store.lsi",
        "stores/chunk-1024/store/store.lsi",
    ] {
        assert_prune(cell);
    }
}
