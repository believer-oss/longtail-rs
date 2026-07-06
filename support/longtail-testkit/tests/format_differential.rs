//! Differential suite (differential lane, via `longtail-ffi`):
//!
//! 1. **Parse-equivalence** — every committed fixture parsed by the pure-Rust
//!    `longtail-core` and by the reference C reader agree on every field/array.
//! 2. **Write-compat** — random logical indexes serialized by Rust are read by
//!    the C reader and re-serialized by the C writer byte-identically (C
//!    read→write is a memcpy, so this proves Rust emits a canonical C buffer).
//! 3. **Merge** — `StoreIndex::merge` matches `Longtail_MergeStoreIndex`
//!    byte-for-byte over generated shared-identifier pairs and the committed
//!    `sharded/` shard pair (both orders), with the mismatched-identifier and
//!    empty×empty edge cases asserted explicitly.
//!
//! Compiles to nothing without the `differential` feature.
#![cfg(feature = "differential")]

use std::path::{Path, PathBuf};

use longtail_core as core;
use longtail_ffi as ffi;
use longtail_testkit::paths::fixtures_dir;
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

fn collect(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).min_depth(1).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some(ext)
        {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}

// ------------------------------------------------------------------ 1. parse

#[test]
fn version_index_parse_equivalence() {
    let files = collect(&fixtures_dir(), "lvi");
    assert_eq!(files.len(), 16);
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        let rust = core::VersionIndex::from_bytes(&orig)
            .unwrap_or_else(|e| panic!("{}: rust parse: {e}", f.display()));
        let mut buf = orig.clone();
        let c = ffi::VersionIndex::new_from_buffer(&mut buf)
            .unwrap_or_else(|e| panic!("{}: c parse: {e}", f.display()));
        let ctx = f.display();
        assert_eq!(
            core::VERSION_INDEX_VERSION,
            c.get_version(),
            "{ctx} version"
        );
        assert_eq!(
            rust.hash_identifier,
            c.get_hash_identifier(),
            "{ctx} hashid"
        );
        assert_eq!(
            rust.target_chunk_size,
            c.get_target_chunk_size(),
            "{ctx} tcs"
        );
        assert_eq!(rust.asset_count(), c.get_asset_count(), "{ctx} A");
        assert_eq!(rust.chunk_count(), c.get_chunk_count(), "{ctx} C");
        assert_eq!(
            rust.asset_chunk_index_count(),
            c.get_asset_chunk_index_count(),
            "{ctx} ACI"
        );
        assert_eq!(rust.path_hashes, c.get_path_hashes(), "{ctx} path_hashes");
        assert_eq!(
            rust.content_hashes,
            c.get_asset_hashes(),
            "{ctx} content_hashes"
        );
        assert_eq!(rust.asset_sizes, c.get_asset_sizes(), "{ctx} asset_sizes");
        assert_eq!(
            rust.asset_chunk_counts,
            c.get_asset_chunk_counts(),
            "{ctx} asset_chunk_counts"
        );
        assert_eq!(
            rust.asset_chunk_index_starts,
            c.get_asset_chunk_index_starts(),
            "{ctx} acis"
        );
        assert_eq!(
            rust.asset_chunk_indexes,
            c.get_asset_chunk_indexes(),
            "{ctx} aci_map"
        );
        assert_eq!(
            rust.chunk_hashes,
            c.get_chunk_hashes(),
            "{ctx} chunk_hashes"
        );
        assert_eq!(rust.chunk_sizes, c.get_chunk_sizes(), "{ctx} chunk_sizes");
        assert_eq!(rust.chunk_tags, c.get_chunk_tags(), "{ctx} chunk_tags");
        assert_eq!(
            rust.name_offsets,
            c.get_name_offsets(),
            "{ctx} name_offsets"
        );
        let rust_perms: Vec<u16> = rust.permissions.iter().map(|p| p.bits()).collect();
        assert_eq!(rust_perms, c.get_permissions(), "{ctx} permissions");
        assert_eq!(
            rust.name_data.len() as u32,
            c.get_name_data_size(),
            "{ctx} name_data_size"
        );
        for i in 0..rust.asset_count() as usize {
            assert_eq!(
                rust.path(i).unwrap(),
                c.get_asset_path(i as u32),
                "{ctx} path[{i}]"
            );
        }
    }
    eprintln!(
        "version index parse-equivalence OK for {} files",
        files.len()
    );
}

#[test]
fn store_index_parse_equivalence() {
    let files = collect(&fixtures_dir(), "lsi");
    assert_eq!(files.len(), 29);
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        let rust = core::StoreIndex::from_bytes(&orig)
            .unwrap_or_else(|e| panic!("{}: rust parse: {e}", f.display()));
        let c = ffi::StoreIndex::new_from_buffer(&orig)
            .unwrap_or_else(|e| panic!("{}: c parse: {e}", f.display()));
        let ctx = f.display();
        assert_eq!(core::STORE_INDEX_VERSION, c.get_version(), "{ctx} version");
        assert_eq!(
            rust.hash_identifier,
            c.get_hash_identifier(),
            "{ctx} hashid"
        );
        assert_eq!(rust.block_count(), c.get_block_count(), "{ctx} B");
        assert_eq!(rust.chunk_count(), c.get_chunk_count(), "{ctx} C");
        assert_eq!(
            rust.block_hashes,
            c.get_block_hashes(),
            "{ctx} block_hashes"
        );
        assert_eq!(
            rust.chunk_hashes,
            c.get_chunk_hashes(),
            "{ctx} chunk_hashes"
        );
        assert_eq!(
            rust.block_chunks_offsets,
            c.get_block_chunks_offsets(),
            "{ctx} offsets"
        );
        assert_eq!(
            rust.block_chunk_counts,
            c.get_block_chunk_counts(),
            "{ctx} counts"
        );
        assert_eq!(rust.block_tags, c.get_block_tags(), "{ctx} tags");
        assert_eq!(rust.chunk_sizes, c.get_chunk_sizes(), "{ctx} chunk_sizes");
    }
    eprintln!("store index parse-equivalence OK for {} files", files.len());
}

#[test]
fn stored_block_parse_equivalence() {
    let files = collect(&fixtures_dir(), "lsb");
    assert!(!files.is_empty());
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        let rust = core::StoredBlock::from_bytes(&orig)
            .unwrap_or_else(|e| panic!("{}: rust parse: {e}", f.display()));
        let mut buf = orig.clone();
        let c = ffi::StoredBlock::new_from_buffer(&mut buf)
            .unwrap_or_else(|e| panic!("{}: c parse: {e}", f.display()));
        let cbi = c.get_block_index();
        let ctx = f.display();
        assert_eq!(
            rust.block_index.block_hash,
            cbi.get_block_hash(),
            "{ctx} block_hash"
        );
        assert_eq!(
            rust.block_index.hash_identifier,
            cbi.get_hash_identifier(),
            "{ctx} hashid"
        );
        assert_eq!(rust.block_index.tag, cbi.get_tag(), "{ctx} tag");
        assert_eq!(
            rust.block_index.chunk_count(),
            cbi.get_chunk_count(),
            "{ctx} n"
        );
        assert_eq!(
            rust.block_index.chunk_hashes,
            cbi.get_chunk_hashes(),
            "{ctx} chunk_hashes"
        );
        assert_eq!(
            rust.block_index.chunk_sizes.as_slice(),
            cbi.get_chunk_sizes(),
            "{ctx} chunk_sizes"
        );
    }
    eprintln!(
        "stored block parse-equivalence OK for {} files",
        files.len()
    );
}

// ----------------------------------------------------------- 2. write-compat

fn u32v(r: &mut ChaCha8Rng, n: usize) -> Vec<u32> {
    (0..n).map(|_| r.next_u32()).collect()
}
fn u64v(r: &mut ChaCha8Rng, n: usize) -> Vec<u64> {
    (0..n).map(|_| r.next_u64()).collect()
}

fn gen_version_index(r: &mut ChaCha8Rng) -> core::VersionIndex {
    let a = (r.next_u32() % 5) as usize;
    let c = (r.next_u32() % 5) as usize;
    let aci = c + (r.next_u32() % 4) as usize; // ACI >= C always
    let name_len = (r.next_u32() % 24) as usize;
    core::VersionIndex {
        hash_identifier: r.next_u32(),
        target_chunk_size: r.next_u32(),
        path_hashes: u64v(r, a),
        content_hashes: u64v(r, a),
        asset_sizes: u64v(r, a),
        asset_chunk_counts: u32v(r, a),
        asset_chunk_index_starts: u32v(r, a),
        asset_chunk_indexes: u32v(r, aci),
        chunk_hashes: u64v(r, c),
        chunk_sizes: u32v(r, c),
        chunk_tags: u32v(r, c),
        name_offsets: u32v(r, a),
        permissions: (0..a)
            .map(|_| core::Permissions(r.next_u32() as u16))
            .collect(),
        name_data: (0..name_len).map(|_| r.next_u32() as u8).collect(),
    }
}

/// A valid store index with cumulative offsets. Block hashes are drawn from a
/// small pool so merge pairs exercise cross-input and internal ties.
fn gen_store_index(r: &mut ChaCha8Rng, hash_id: u32) -> core::StoreIndex {
    let b = (r.next_u32() % 5) as usize;
    let mut si = core::StoreIndex::empty(hash_id);
    for _ in 0..b {
        let bh = (r.next_u32() % 6) as u64;
        let n = 1 + (r.next_u32() % 3) as usize;
        si.block_hashes.push(bh);
        si.block_tags.push(r.next_u32());
        si.block_chunks_offsets.push(si.chunk_hashes.len() as u32);
        si.block_chunk_counts.push(n as u32);
        for _ in 0..n {
            si.chunk_hashes.push(r.next_u64());
            si.chunk_sizes.push(r.next_u32());
        }
    }
    si
}

fn gen_stored_block(r: &mut ChaCha8Rng) -> core::StoredBlock {
    let n = 1 + (r.next_u32() % 6) as usize;
    let payload_len = (r.next_u32() % 40) as usize;
    core::StoredBlock {
        block_index: core::BlockIndex {
            block_hash: r.next_u64(),
            hash_identifier: r.next_u32(),
            tag: r.next_u32(),
            chunk_hashes: u64v(r, n),
            chunk_sizes: u32v(r, n),
        },
        payload: (0..payload_len).map(|_| r.next_u32() as u8).collect(),
    }
}

#[test]
fn version_index_write_compat() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5732_0001);
    for _ in 0..300 {
        let x = gen_version_index(&mut r);
        let rust_bytes = x.to_bytes();
        let mut buf = rust_bytes.clone();
        let c = ffi::VersionIndex::new_from_buffer(&mut buf).expect("C reads Rust .lvi");
        assert_eq!(
            c.write_to_buffer().expect("C writes .lvi"),
            rust_bytes,
            "C re-serialization differs from Rust"
        );
    }
}

#[test]
fn store_index_write_compat() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5732_0002);
    for _ in 0..300 {
        let hash_id = r.next_u32();
        let x = gen_store_index(&mut r, hash_id);
        let rust_bytes = x.to_bytes();
        let c = ffi::StoreIndex::new_from_buffer(&rust_bytes).expect("C reads Rust .lsi");
        assert_eq!(
            c.write_to_buffer().expect("C writes .lsi"),
            rust_bytes,
            "C re-serialization differs from Rust"
        );
    }
}

#[test]
fn stored_block_write_compat() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5732_0003);
    for _ in 0..300 {
        let x = gen_stored_block(&mut r);
        let rust_bytes = x.to_bytes();
        let mut buf = rust_bytes.clone();
        let c = ffi::StoredBlock::new_from_buffer(&mut buf).expect("C reads Rust .lsb");
        assert_eq!(
            c.to_bytes().expect("C writes .lsb"),
            rust_bytes,
            "C re-serialization differs from Rust"
        );
    }
}

// ------------------------------------------------------------------ 3. merge

/// Merge two Rust store indexes via C and return the C-produced bytes.
fn c_merge_bytes(a: &core::StoreIndex, b: &core::StoreIndex) -> Result<Vec<u8>, i32> {
    let a_bytes = a.to_bytes();
    let b_bytes = b.to_bytes();
    let ca = ffi::StoreIndex::new_from_buffer(&a_bytes).expect("C reads a");
    let cb = ffi::StoreIndex::new_from_buffer(&b_bytes).expect("C reads b");
    let cm = ca.merge_store_index(&cb)?;
    Ok(cm.write_to_buffer().expect("C writes merged"))
}

#[test]
fn merge_byte_identity_generated_pairs() {
    let mut r = ChaCha8Rng::seed_from_u64(0x5732_4000);
    // Shared hash identifier: C EINVALs on mismatch when both are non-empty, so
    // an unconstrained pair would make the byte-identity assertion vacuous.
    let hash_id = 0x626c_6b33; // "blk3"
    let mut compared = 0;
    for _ in 0..400 {
        let a = gen_store_index(&mut r, hash_id);
        let b = gen_store_index(&mut r, hash_id);
        let rust = a.merge(&b).expect("rust merge");
        let c_bytes = c_merge_bytes(&a, &b).expect("c merge");
        assert_eq!(c_bytes, rust.to_bytes(), "merge bytes differ");
        compared += 1;
    }
    eprintln!("merge byte-identity confirmed for {compared} generated pairs");
}

#[test]
fn merge_fixture_shards_byte_identity() {
    let sharded = fixtures_dir().join("stores/sharded");
    let shards = collect(&sharded, "lsi");
    assert_eq!(shards.len(), 2, "expected two sharded/ shards");
    let a = core::StoreIndex::from_bytes(&std::fs::read(&shards[0]).unwrap()).unwrap();
    let b = core::StoreIndex::from_bytes(&std::fs::read(&shards[1]).unwrap()).unwrap();
    // Both orders (merge is order-sensitive: local blocks come first).
    assert_eq!(
        a.merge(&b).unwrap().to_bytes(),
        c_merge_bytes(&a, &b).unwrap(),
        "shard merge a,b differs"
    );
    assert_eq!(
        b.merge(&a).unwrap().to_bytes(),
        c_merge_bytes(&b, &a).unwrap(),
        "shard merge b,a differs"
    );
    eprintln!("sharded shard-pair merge byte-identity confirmed (both orders)");
}

#[test]
fn merge_mismatched_identifiers_both_error() {
    let a = gen_store_index(&mut ChaCha8Rng::seed_from_u64(1), 0x1111_1111);
    let b = gen_store_index(&mut ChaCha8Rng::seed_from_u64(2), 0x2222_2222);
    // Ensure both are non-empty (only then is the conflict check reached).
    let a = if a.block_count() == 0 {
        gen_nonempty(0x1111_1111, 7)
    } else {
        a
    };
    let b = if b.block_count() == 0 {
        gen_nonempty(0x2222_2222, 9)
    } else {
        b
    };
    assert!(a.merge(&b).is_err(), "rust must reject mismatched ids");
    assert!(
        c_merge_bytes(&a, &b).is_err(),
        "C must reject mismatched ids"
    );
}

fn gen_nonempty(hash_id: u32, block_hash: u64) -> core::StoreIndex {
    let mut s = core::StoreIndex::empty(hash_id);
    s.block_hashes.push(block_hash);
    s.block_tags.push(0);
    s.block_chunks_offsets.push(0);
    s.block_chunk_counts.push(1);
    s.chunk_hashes.push(block_hash);
    s.chunk_sizes.push(42);
    s
}

#[test]
fn merge_empty_times_empty_identifier_zero() {
    let a = core::StoreIndex::empty(0x1111_1111);
    let b = core::StoreIndex::empty(0x2222_2222);
    let rust = a.merge(&b).unwrap();
    assert_eq!(
        rust.hash_identifier, 0,
        "empty×empty must yield identifier 0"
    );
    // C agrees byte-for-byte (routes through CreateStoreIndexFromBlocks).
    assert_eq!(c_merge_bytes(&a, &b).unwrap(), rust.to_bytes());
}
