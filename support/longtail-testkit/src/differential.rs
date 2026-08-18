//! C-backed differential helpers (feature `differential`).
//!
//! Everything here drives the reference C library through `longtail-ffi`. It is
//! used both to GENERATE the committed goldens (boundary tables) and to
//! REPRODUCE them in the Stage 1 self-validation suite — proving the C library
//! deterministically reproduces every golden before any pure-Rust port exists.

use std::path::Path;

use longtail_ffi::{
    ChunkerAPI, CompressionRegistry, HashAPI, HashRegistry, HashType, StoreIndex, VersionIndex,
};

use crate::boundary::{
    BoundaryTable, ChunkEntry, PATH_BUFFER, PATH_STREAMING, chunker_params_for_target,
};
use crate::fixture_manifest::sha256_hex;

/// Build a blake3 hash API (kept alive by the returned registry).
pub fn blake3_hash() -> (HashRegistry, HashAPI) {
    let registry = HashRegistry::new();
    let hash = registry
        .get_hash_api(HashType::Blake3)
        .expect("blake3 hash api");
    (registry, hash)
}

/// The 64-bit longtail hash of `data` computed by the reference C library for a
/// given [`HashType`] (the `HashBuffer` call used everywhere in CreateVersionIndex).
pub fn c_hash(hash_type: HashType, data: &[u8]) -> u64 {
    let registry = HashRegistry::new();
    let hash = registry.get_hash_api(hash_type).expect("hash api");
    hash.hash_buffer(data).expect("c hash_buffer")
}

/// Compress `data` through the reference C codec registry for compression ID
/// `id` (raw codec bytes, no framing header).
pub fn c_compress(id: u32, data: &[u8]) -> Result<Vec<u8>, i32> {
    let registry = CompressionRegistry::new();
    registry.compress_buffer(id, data)
}

/// Decompress `compressed` (raw codec bytes) through the reference C codec
/// registry for compression ID `id` into a buffer of `uncompressed_size` bytes.
pub fn c_decompress(id: u32, compressed: &[u8], uncompressed_size: usize) -> Result<Vec<u8>, i32> {
    let registry = CompressionRegistry::new();
    registry.decompress_buffer(id, compressed, uncompressed_size)
}

fn hash_hex(hash: &HashAPI, bytes: &[u8]) -> String {
    format!("{:016x}", hash.hash_buffer(bytes).expect("hash chunk"))
}

/// Compute a boundary table for `data` at `target` using the streaming chunker
/// entry point (canonical) and blake3 chunk hashes.
pub fn boundary_table_streaming(input_id: &str, data: &[u8], target: u32) -> BoundaryTable {
    boundary_table(input_id, data, target, true)
}

/// Compute a boundary table using the buffer/mmap chunker entry point (labeled
/// `buffer`; boundaries may diverge from streaming when `min > 48`).
pub fn boundary_table_buffer(input_id: &str, data: &[u8], target: u32) -> BoundaryTable {
    boundary_table(input_id, data, target, false)
}

fn boundary_table(input_id: &str, data: &[u8], target: u32, streaming: bool) -> BoundaryTable {
    let chunker = ChunkerAPI::new();
    let (_registry, hash) = blake3_hash();
    let (min, avg, max) = chunker_params_for_target(target);

    let spans = if streaming {
        chunker.chunk_streaming(data, min, avg, max)
    } else {
        chunker.chunk_from_buffer(data, min, avg, max)
    }
    .expect("chunk buffer");

    let chunks = spans
        .into_iter()
        .map(|s| {
            let start = s.offset as usize;
            let end = start + s.size as usize;
            ChunkEntry {
                offset: s.offset,
                size: s.size,
                hash_hex: hash_hex(&hash, &data[start..end]),
            }
        })
        .collect();

    BoundaryTable {
        input_id: input_id.to_string(),
        input_sha256: sha256_hex(data),
        target_chunk_size: target,
        chunker_path: if streaming {
            PATH_STREAMING
        } else {
            PATH_BUFFER
        }
        .to_string(),
        hash_algorithm: "blake3".to_string(),
        chunks,
    }
}

/// Ordered `(chunk_hash, chunk_size)` pairs for one asset in a version index,
/// looked up by its root-relative path. Returns `None` if the path is absent.
///
/// This is the independent anchor for boundary tables: for a single-file corpus
/// case, the streaming boundary table's chunk hashes+sizes must equal the
/// chunks golongtail recorded for that file in its `.lvi` (which golongtail
/// produced via the same streaming path).
pub fn asset_chunks(vi: &VersionIndex, path: &str) -> Option<Vec<(u64, u32)>> {
    let asset_count = vi.get_asset_count();
    let counts = vi.get_asset_chunk_counts();
    let starts = vi.get_asset_chunk_index_starts();
    let indexes = vi.get_asset_chunk_indexes();
    let chunk_hashes = vi.get_chunk_hashes();
    let chunk_sizes = vi.get_chunk_sizes();

    for i in 0..asset_count {
        if vi.get_asset_path(i) == path {
            let start = starts[i as usize] as usize;
            let count = counts[i as usize] as usize;
            let mut out = Vec::with_capacity(count);
            for j in 0..count {
                let chunk_idx = indexes[start + j] as usize;
                out.push((chunk_hashes[chunk_idx], chunk_sizes[chunk_idx]));
            }
            return Some(out);
        }
    }
    None
}

/// Parse a store-index buffer via the C reader and return its block hashes,
/// sorted. Two store indexes describing the same blocks have equal sorted
/// block-hash lists regardless of block ordering (a block hash uniquely
/// identifies its chunk-hash array). Used by the freshness check to compare
/// store indexes semantically, tolerating golongtail's non-deterministic block
/// ordering while still catching real drift.
pub fn store_index_block_hashes_sorted(bytes: &[u8]) -> Result<Vec<u64>, i32> {
    let si = StoreIndex::new_from_buffer(bytes)?;
    let mut hashes = si.get_block_hashes();
    hashes.sort_unstable();
    Ok(hashes)
}

/// Read a `.lvi` file and parse it into a [`VersionIndex`] via the C reader.
pub fn read_version_index(path: &Path) -> VersionIndex {
    let mut bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    VersionIndex::new_from_buffer(&mut bytes)
        .unwrap_or_else(|e| panic!("parse version index {}: {e}", path.display()))
}

/// Downsync a single version from `storage_uri` into `target_dir` via the C
/// library, with `enable_file_mapping = false` (matching golongtail's default
/// and how the fixtures were chunked). `target_dir` is created if missing.
#[allow(clippy::too_many_arguments)]
pub fn downsync_version(
    storage_uri: &str,
    source_lvi: &Path,
    target_dir: &Path,
    version_local_store_index_paths: Option<Vec<String>>,
    cache_path: Option<&Path>,
) -> Result<(), String> {
    std::fs::create_dir_all(target_dir).map_err(|e| format!("mkdir target: {e}"))?;
    // The bikeshed job system needs several workers to make progress on its job
    // dependency graph (a single worker can deadlock). The downsynced tree is
    // deterministic regardless of worker count, so this does not affect the
    // golden comparison.
    longtail_ffi::downsync(
        8,
        storage_uri,
        None,
        None,
        &[source_lvi.to_string_lossy().into_owned()],
        &target_dir.to_string_lossy(),
        "",
        cache_path,
        true,  // retain_permissions
        false, // validate (we compare tree manifests ourselves)
        version_local_store_index_paths,
        None,
        None,
        true,  // scan_target
        false, // cache_target_index
        false, // enable_file_mapping — canonical streaming target scan
        false, // use_legacy_write — ChangeVersion2
        None,
    )
    .map_err(|e| format!("downsync failed: {e:?}"))
}
