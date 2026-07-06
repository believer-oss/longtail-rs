//! Shard-naming assertion against the committed
//! `fixtures/stores/sharded/` shards.
//!
//! The lockless shard name is byte-defined: `store_<sha256hex-of-serialized-
//! bytes>.lsi` (remotestore.go:1213). This proves (a) the committed fixtures
//! obey that rule, (b) `StoreIndex` round-trips those bytes byte-identically,
//! so re-deriving the name from the parsed+reserialized index yields
//! the same file name, and (c) the lockless write path
//! ([`add_to_remote_store_index`] on a non-locking backend) writes each fixture
//! index under exactly its committed name.

use std::path::PathBuf;

use longtail_core::StoreIndex;
use longtail_store::blob::{BlobStore, MemBlobStore};
use longtail_store::{add_to_remote_store_index, block_path};
use sha2::{Digest, Sha256};

fn sharded_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/stores/sharded")
}

fn shard_key(bytes: &[u8]) -> String {
    format!("store_{:x}.lsi", Sha256::digest(bytes))
}

#[test]
fn sharded_fixture_names_match_sha256_of_bytes() {
    let dir = sharded_dir();
    let mut shard_count = 0;
    for entry in std::fs::read_dir(&dir).expect("read sharded dir") {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("store_") && name.ends_with(".lsi")) {
            continue;
        }
        shard_count += 1;
        let bytes = std::fs::read(entry.path()).unwrap();

        // (a) The committed name is sha256 over the exact bytes.
        assert_eq!(shard_key(&bytes), name, "raw-bytes sha256 name mismatch");

        // (b) Parse → re-serialize is byte-identical, so the name
        //     re-derives from the parsed index.
        let idx = StoreIndex::from_bytes(&bytes).expect("parse shard");
        let reserialized = idx.to_bytes();
        assert_eq!(reserialized, bytes, "round-trip not byte-identical");
        assert_eq!(shard_key(&reserialized), name, "reserialized name mismatch");
    }
    assert_eq!(shard_count, 2, "expected exactly 2 committed shards");
}

/// The lockless write path reproduces a fixture shard's committed name: writing
/// a fixture index into a fresh non-locking store produces exactly
/// `store_<that-sha256>.lsi`.
#[tokio::test]
async fn lockless_write_reproduces_fixture_shard_name() {
    let dir = sharded_dir();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("store_") && name.ends_with(".lsi")) {
            continue;
        }
        let bytes = std::fs::read(entry.path()).unwrap();
        let idx = StoreIndex::from_bytes(&bytes).unwrap();

        // Fresh non-locking mem store → lockless shard path.
        let store = MemBlobStore::new("", false);
        let client = store.new_client().await.unwrap();
        // First write merges empty ⊕ idx (canonicalizing); a canonical fixture
        // index is unchanged, so the shard key equals the committed name.
        add_to_remote_store_index(&*client, &idx).await.unwrap();

        let objs = client.get_objects("store").await.unwrap();
        let shard_names: Vec<&str> = objs.iter().map(|o| o.name.as_str()).collect();
        assert!(
            shard_names.contains(&name.as_str()),
            "lockless write produced {shard_names:?}, expected {name}"
        );
    }
}

/// The block-path scheme matches the committed `chunks/<top4>/0x<hash>.lsb`
/// layout under `fixtures/stores/sharded/chunks`.
#[test]
fn block_path_matches_committed_lsb_layout() {
    let chunks = sharded_dir().join("chunks");
    let mut found = 0;
    for sub in std::fs::read_dir(&chunks).unwrap() {
        let sub = sub.unwrap();
        if !sub.file_type().unwrap().is_dir() {
            continue;
        }
        for f in std::fs::read_dir(sub.path()).unwrap() {
            let f = f.unwrap();
            let fname = f.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".lsb") {
                continue;
            }
            found += 1;
            // Parse the 0x<16hex>.lsb name back to a hash and re-derive the path.
            let hex = fname.trim_start_matches("0x").trim_end_matches(".lsb");
            let hash = u64::from_str_radix(hex, 16).unwrap();
            let expected = block_path("chunks", hash);
            let actual = format!("chunks/{}/{}", sub.file_name().to_string_lossy(), fname);
            assert_eq!(actual, expected);
        }
    }
    assert!(
        found >= 2,
        "expected committed .lsb blocks in the sharded fixture"
    );
}
