//! Golden hash/decode tests (pure lane, ladder 3-5): recompute the hashes stored
//! in every committed fixture with the pure-Rust hash layer and framing codec and
//! assert they equal the stored values. No native library involved.
//!
//! - **Ladder 3** — `.lvi`: path hashes (path string, no NUL) and content hashes
//!   (the asset's chunk-hash sub-array) recompute to the stored values, for the
//!   blake3 cells and the blake2 cell; the meow cell asserts the typed
//!   unsupported error.
//! - **Ladder 4** — block hashes: every `.lsi` block entry and every `.lsb`
//!   block index has `block_hash == hash(chunk-hash array)` under the index's
//!   hash ID.
//! - **Ladder 5** — decode gate ④: every `.lsb` decompresses per its tag to
//!   `Σ chunk_sizes` bytes and **every chunk hash verifies** against the decoded
//!   bytes.

use std::path::{Path, PathBuf};

use longtail_core::compress::decode_stored_block;
use longtail_core::hash::{self, Hash, HashError, MEOW_ID};
use longtail_core::{StoreIndex, StoredBlock, VersionIndex};
use longtail_testkit::paths::fixtures_dir;

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

/// Hash a `u64` array as its little-endian bytes — the input shape for content
/// hashes and block hashes (`sizeof(u64) * n` bytes of already-computed hashes).
fn hash_u64_array(hasher: &dyn Hash, hashes: &[u64]) -> u64 {
    let mut buf = Vec::with_capacity(hashes.len() * 8);
    for h in hashes {
        buf.extend_from_slice(&h.to_le_bytes());
    }
    hasher.hash(&buf)
}

#[test]
fn version_index_path_and_content_hashes_recompute() {
    let files = collect(&fixtures_dir(), "lvi");
    assert_eq!(files.len(), 16, "expected 16 committed .lvi fixtures");
    let mut checked_assets = 0usize;
    let mut meow_seen = false;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let vi = VersionIndex::from_bytes(&bytes).unwrap();
        let hasher = match hash::hasher(vi.hash_identifier) {
            Ok(h) => h,
            Err(HashError::UnsupportedHash { id }) => {
                assert_eq!(id, MEOW_ID, "{}: only meow is unsupported", f.display());
                meow_seen = true;
                continue; // cannot recompute meow hashes in the pure port
            }
            Err(e) => panic!("{}: unexpected hash error {e}", f.display()),
        };
        let a = vi.asset_count() as usize;
        for i in 0..a {
            // Path hash — the path string, no NUL.
            let path = vi.path_bytes(i).unwrap();
            assert_eq!(
                hasher.hash(path),
                vi.path_hashes[i],
                "{}: path hash for asset {i} ({:?})",
                f.display(),
                String::from_utf8_lossy(path)
            );
            // Content hash — the asset's chunk-hash sub-array.
            let start = vi.asset_chunk_index_starts[i] as usize;
            let count = vi.asset_chunk_counts[i] as usize;
            let asset_chunk_hashes: Vec<u64> = (0..count)
                .map(|j| {
                    let chunk_idx = vi.asset_chunk_indexes[start + j] as usize;
                    vi.chunk_hashes[chunk_idx]
                })
                .collect();
            assert_eq!(
                hash_u64_array(hasher.as_ref(), &asset_chunk_hashes),
                vi.content_hashes[i],
                "{}: content hash for asset {i}",
                f.display()
            );
            checked_assets += 1;
        }
    }
    assert!(meow_seen, "expected a meow-hashed .lvi in the fixture set");
    eprintln!("recomputed path+content hashes for {checked_assets} assets across .lvi fixtures");
}

#[test]
fn store_index_block_hashes_recompute() {
    let files = collect(&fixtures_dir(), "lsi");
    assert_eq!(files.len(), 29, "expected 29 committed .lsi fixtures");
    let mut checked_blocks = 0usize;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let si = StoreIndex::from_bytes(&bytes).unwrap();
        let hasher = match hash::hasher(si.hash_identifier) {
            Ok(h) => h,
            Err(HashError::UnsupportedHash { .. }) => continue, // meow: cannot verify
            Err(e) => panic!("{}: {e}", f.display()),
        };
        let b = si.block_count() as usize;
        for blk in 0..b {
            let off = si.block_chunks_offsets[blk] as usize;
            let cnt = si.block_chunk_counts[blk] as usize;
            let block_chunk_hashes = &si.chunk_hashes[off..off + cnt];
            assert_eq!(
                hash_u64_array(hasher.as_ref(), block_chunk_hashes),
                si.block_hashes[blk],
                "{}: block hash for block {blk}",
                f.display()
            );
            checked_blocks += 1;
        }
    }
    eprintln!("recomputed block hashes for {checked_blocks} .lsi block entries");
}

#[test]
fn stored_block_hashes_recompute_and_meow_is_typed_error() {
    let files = collect(&fixtures_dir(), "lsb");
    assert_eq!(files.len(), 32, "expected 32 committed .lsb fixtures");
    let mut checked = 0usize;
    let mut meow_seen = false;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let sb = StoredBlock::from_bytes(&bytes).unwrap();
        let id = sb.block_index.hash_identifier;
        match hash::hasher(id) {
            Ok(hasher) => {
                assert_eq!(
                    hash_u64_array(hasher.as_ref(), &sb.block_index.chunk_hashes),
                    sb.block_index.block_hash,
                    "{}: block hash",
                    f.display()
                );
                checked += 1;
            }
            Err(HashError::UnsupportedHash { id: mid }) => {
                assert_eq!(mid, MEOW_ID);
                meow_seen = true;
            }
            Err(e) => panic!("{}: {e}", f.display()),
        }
    }
    assert!(meow_seen, "expected a meow-hashed .lsb (meow cell)");
    eprintln!("recomputed block hashes for {checked} .lsb fixtures (+meow typed error)");
}

/// Ladder 5 (decode gate ④): every `.lsb` decompresses per its tag to
/// `Σ chunk_sizes` bytes, and every chunk hash verifies against the decoded
/// bytes (for cells whose hash the pure port supports).
#[test]
fn every_stored_block_decodes_with_verifying_chunk_hashes() {
    let files = collect(&fixtures_dir(), "lsb");
    assert_eq!(files.len(), 32, "expected 32 committed .lsb fixtures");
    let mut chunks_verified = 0usize;
    let mut blocks_decoded = 0usize;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let sb = StoredBlock::from_bytes(&bytes).unwrap();
        let bi = &sb.block_index;
        let total: u64 = bi.chunk_sizes.iter().map(|&s| s as u64).sum();

        // Decode the payload per its tag (raw for tag==0, framed otherwise).
        let raw = decode_stored_block(&sb)
            .unwrap_or_else(|e| panic!("{}: decode failed: {e}", f.display()));
        assert_eq!(
            raw.len() as u64,
            total,
            "{}: decoded length != Σ chunk_sizes",
            f.display()
        );
        blocks_decoded += 1;

        // Verify each chunk hash against the decoded chunk bytes.
        let hasher = match hash::hasher(bi.hash_identifier) {
            Ok(h) => h,
            Err(HashError::UnsupportedHash { .. }) => continue, // meow: length only
            Err(e) => panic!("{}: {e}", f.display()),
        };
        let mut off = 0usize;
        for (i, &size) in bi.chunk_sizes.iter().enumerate() {
            let size = size as usize;
            let chunk = &raw[off..off + size];
            assert_eq!(
                hasher.hash(chunk),
                bi.chunk_hashes[i],
                "{}: chunk {i} hash mismatch",
                f.display()
            );
            off += size;
            chunks_verified += 1;
        }
    }
    eprintln!(
        "decode gate: {blocks_decoded} .lsb decoded to Σ chunk_sizes; {chunks_verified} chunk hashes verified"
    );
}
