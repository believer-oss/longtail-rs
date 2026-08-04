//! Shared inputs for the benchmarks: deterministic seeded buffer
//! generators, a synthetic large-index builder (owned-struct revisit trigger),
//! and the ~1 GiB parametric dataset + churn generators for the e2e harness.
//!
//! # Determinism
//!
//! All content is generated with `rand_chacha::ChaCha20Rng` (algorithmically
//! stable across releases — never `StdRng`), seeded from
//! `blake3("longtail-bench:" + tag)`, exactly like testkit's corpus. testkit's
//! own `random_bytes`/`compressible_text` are private and fixed-size; these are
//! the size-parameterized bench-crate equivalents (new code, per the work
//! order) — nothing in testkit is disturbed.
//!
//! # Bench artifacts
//!
//! Everything the harness writes lives under `target/bench/` (git-ignored via
//! `/target`), so `fixtures/` is provably untouched and there are no collisions
//! with testkit temp dirs.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use longtail_core::{Blake3, Hash};
use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;

/// The per-tree bench artifact root: `<workspace>/target/bench`.
pub fn bench_root() -> PathBuf {
    longtail_testkit::paths::workspace_root().join("target/bench")
}

/// A ChaCha20 RNG seeded deterministically from a tag (matches testkit).
fn seeded_rng(tag: &str) -> ChaCha20Rng {
    let mut input = b"longtail-bench:".to_vec();
    input.extend_from_slice(tag.as_bytes());
    let seed: [u8; 32] = *blake3::hash(&input).as_bytes();
    ChaCha20Rng::from_seed(seed)
}

/// `n` deterministic pseudo-random (incompressible) bytes for `tag`.
pub fn incompressible(tag: &str, n: usize) -> Vec<u8> {
    let mut rng = seeded_rng(tag);
    let mut v = vec![0u8; n];
    rng.fill_bytes(&mut v);
    v
}

/// `n` bytes of word-like, highly compressible pseudo-text for `tag` (the same
/// shape as testkit's `compressible_text`, size-parameterized).
pub fn compressible(tag: &str, n: usize) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "the",
        "quick",
        "brown",
        "fox",
        "jumps",
        "over",
        "lazy",
        "dog",
        "longtail",
        "chunk",
        "block",
        "store",
        "index",
        "version",
        "hash",
        "delta",
        "content",
        "asset",
        "folder",
        "compress",
        "downsync",
        "upsync",
        "boundary",
        "gear",
        "window",
        "discriminator",
    ];
    let mut rng = seeded_rng(tag);
    let mut out = Vec::with_capacity(n + 16);
    while out.len() < n {
        let w = WORDS[(rng.next_u32() as usize) % WORDS.len()];
        out.extend_from_slice(w.as_bytes());
        out.push(b' ');
        if (rng.next_u32() & 0x0f) == 0 {
            out.push(b'\n');
        }
    }
    out.truncate(n);
    out
}

// -------------------------------------------------------------------------
// Synthetic large index (owned-struct revisit trigger)
// -------------------------------------------------------------------------

/// Build a synthetic large [`longtail_core::VersionIndex`] with `assets` assets
/// and roughly `chunks` unique chunks, all fields internally consistent so it
/// re-serializes byte-identically. Used by the index-codec micro-bench to
/// measure parse+serialize at realistic scale (~100k assets / ~500k chunks).
pub fn synthetic_version_index(assets: usize, chunks: usize) -> longtail_core::VersionIndex {
    use longtail_core::{Permissions, VersionIndex};
    let mut rng = seeded_rng(&format!("vindex:{assets}:{chunks}"));

    let mut path_hashes = Vec::with_capacity(assets);
    let mut content_hashes = Vec::with_capacity(assets);
    let mut asset_sizes = Vec::with_capacity(assets);
    let mut asset_chunk_counts = Vec::with_capacity(assets);
    let mut asset_chunk_index_starts = Vec::with_capacity(assets);
    let mut asset_chunk_indexes = Vec::with_capacity(chunks);
    let mut name_offsets = Vec::with_capacity(assets);
    let mut permissions = Vec::with_capacity(assets);
    let mut name_data: Vec<u8> = Vec::new();

    // Distribute `chunks` chunk-index references across `assets` assets as
    // contiguous runs (each asset owns ~chunks/assets chunk indexes). The
    // asset_chunk_indexes point into the chunk arrays 0..chunks.
    let per_asset = (chunks / assets.max(1)).max(1);
    let mut next_chunk = 0usize;
    for i in 0..assets {
        path_hashes.push(rng.next_u64());
        content_hashes.push(rng.next_u64());
        asset_sizes.push((per_asset as u64) * 32768);
        let start = asset_chunk_indexes.len() as u32;
        asset_chunk_index_starts.push(start);
        let count = per_asset.min(chunks.saturating_sub(next_chunk).max(1));
        for _ in 0..count {
            asset_chunk_indexes.push((next_chunk % chunks.max(1)) as u32);
            next_chunk = next_chunk.wrapping_add(1);
        }
        asset_chunk_counts.push(count as u32);
        name_offsets.push(name_data.len() as u32);
        let name = format!("dir{}/file_{i:06}.bin\0", i % 512);
        name_data.extend_from_slice(name.as_bytes());
        permissions.push(Permissions(0o644));
    }

    // The chunk arrays hold `chunks` unique chunks. asset_chunk_index_count
    // (== asset_chunk_indexes.len()) must be >= chunk_count (Rust-side rule);
    // per_asset*assets >= chunks by construction, so pad the index map if a
    // rounding shortfall left it below chunk_count.
    while asset_chunk_indexes.len() < chunks {
        asset_chunk_indexes.push((asset_chunk_indexes.len() % chunks.max(1)) as u32);
    }

    let mut chunk_hashes = Vec::with_capacity(chunks);
    let mut chunk_sizes = Vec::with_capacity(chunks);
    let mut chunk_tags = Vec::with_capacity(chunks);
    for _ in 0..chunks {
        chunk_hashes.push(rng.next_u64());
        chunk_sizes.push(1024 + (rng.next_u32() % 64512)); // 1 KiB .. 64 KiB
        chunk_tags.push(0x7a74_6432); // zstd default
    }

    VersionIndex {
        hash_identifier: longtail_core::hash::BLAKE3_ID,
        target_chunk_size: 32768,
        path_hashes,
        content_hashes,
        asset_sizes,
        asset_chunk_counts,
        asset_chunk_index_starts,
        asset_chunk_indexes,
        chunk_hashes,
        chunk_sizes,
        chunk_tags,
        name_offsets,
        permissions,
        name_data,
    }
}

/// Build a synthetic large [`longtail_core::StoreIndex`] with `blocks` blocks and
/// `blocks * chunks_per_block` chunk entries, internally consistent.
pub fn synthetic_store_index(blocks: usize, chunks_per_block: usize) -> longtail_core::StoreIndex {
    use longtail_core::StoreIndex;
    let mut rng = seeded_rng(&format!("sindex:{blocks}:{chunks_per_block}"));
    let total = blocks * chunks_per_block;
    let mut block_hashes = Vec::with_capacity(blocks);
    let mut block_chunks_offsets = Vec::with_capacity(blocks);
    let mut block_chunk_counts = Vec::with_capacity(blocks);
    let mut block_tags = Vec::with_capacity(blocks);
    let mut chunk_hashes = Vec::with_capacity(total);
    let mut chunk_sizes = Vec::with_capacity(total);
    for b in 0..blocks {
        block_hashes.push(rng.next_u64());
        block_chunks_offsets.push((b * chunks_per_block) as u32);
        block_chunk_counts.push(chunks_per_block as u32);
        block_tags.push(0x7a74_6432);
        for _ in 0..chunks_per_block {
            chunk_hashes.push(rng.next_u64());
            chunk_sizes.push(1024 + (rng.next_u32() % 64512));
        }
    }
    StoreIndex {
        hash_identifier: longtail_core::hash::BLAKE3_ID,
        block_hashes,
        chunk_hashes,
        block_chunks_offsets,
        block_chunk_counts,
        block_tags,
        chunk_sizes,
    }
}

/// Write a synthetic `store.lsi` of approximately `size_mb` MiB to `path` — a
/// valid, internally-consistent [`longtail_core::StoreIndex`] standing in for a
/// large accumulated store index (Fellowship's ~1.29 GB shards). Seeding a store
/// with this and upsyncing a small delta exercises the read-merge-flush path
/// (the upsync OOM vector) without generating millions of real blocks. Returns
/// the block count actually written.
pub fn write_synthetic_store_lsi(
    path: &Path,
    size_mb: usize,
    chunks_per_block: usize,
) -> std::io::Result<usize> {
    // On-disk StoreIndex size ≈ 20·B (block_hash + 3× u32 per block) + 12·B·K
    // (u64 chunk_hash + u32 chunk_size per chunk); solve for the block count B.
    let target = size_mb.max(1) * MIB;
    let per_block = 20 + 12 * chunks_per_block.max(1);
    let blocks = (target / per_block).max(1);
    let si = synthetic_store_index(blocks, chunks_per_block.max(1));
    let bytes = si.to_bytes();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &bytes)?;
    Ok(blocks)
}

// -------------------------------------------------------------------------
// The synthetic ~1 GiB dataset + v1→v2 churn (e2e harness / dedup driver)
// -------------------------------------------------------------------------

/// A file plan entry: relative path, byte size, and whether it is compressible
/// text or incompressible random.
#[derive(Debug, Clone)]
pub struct FilePlan {
    pub rel: String,
    pub size: usize,
    pub compressible: bool,
}

/// Deterministic file layout summing to ~`total_bytes`: a mix of a few large
/// assets (up to 256 MiB — capped so RSS measures the pipeline,
/// not one giant buffer), medium assets, and many small assets, split between
/// incompressible (random) and compressible (text) content. Stable for a given
/// `total_bytes` so v1 and its churned v2 share a base layout.
pub fn dataset_plan(total_bytes: usize) -> Vec<FilePlan> {
    let mut plan = Vec::new();
    let mut remaining = total_bytes as i64;
    let mut idx = 0usize;

    // A helper closure that pushes a file and decrements the budget.
    let mut push =
        |plan: &mut Vec<FilePlan>, remaining: &mut i64, dir: &str, size: usize, comp: bool| {
            if *remaining <= 0 {
                return;
            }
            let size = (size as i64).min(*remaining) as usize;
            plan.push(FilePlan {
                rel: format!("{dir}/asset_{idx:05}.bin"),
                size,
                compressible: comp,
            });
            idx += 1;
            *remaining -= size as i64;
        };

    // Large assets: up to 256 MiB, alternating compressible / incompressible.
    // Take ~55% of the budget in large files.
    let large_budget = (total_bytes as i64 * 55) / 100;
    let mut large_spent = 0i64;
    let large_sizes = [256 * MIB, 192 * MIB, 128 * MIB, 96 * MIB, 64 * MIB];
    let mut li = 0;
    while large_spent < large_budget && remaining > 0 {
        let size = large_sizes[li % large_sizes.len()];
        let comp = li % 2 == 0;
        let before = remaining;
        push(&mut plan, &mut remaining, "large", size, comp);
        large_spent += before - remaining;
        li += 1;
    }

    // Medium assets: 1..16 MiB, ~35% of the budget.
    let medium_budget = (total_bytes as i64 * 35) / 100;
    let mut medium_spent = 0i64;
    let medium_sizes = [4 * MIB, 8 * MIB, 2 * MIB, 16 * MIB, MIB];
    let mut mi = 0;
    while medium_spent < medium_budget && remaining > 0 {
        let size = medium_sizes[mi % medium_sizes.len()];
        let comp = mi % 3 != 0;
        let before = remaining;
        push(&mut plan, &mut remaining, "medium", size, comp);
        medium_spent += before - remaining;
        mi += 1;
    }

    // Small assets: 4..64 KiB, whatever budget remains (multi-file / packing).
    let small_sizes = [64 * KIB, 16 * KIB, 4 * KIB, 32 * KIB];
    let mut si = 0;
    while remaining > 0 {
        let size = small_sizes[si % small_sizes.len()];
        let comp = si % 2 == 0;
        push(&mut plan, &mut remaining, "small", size, comp);
        si += 1;
        if plan.len() > 100_000 {
            break; // guard against pathological tiny totals
        }
    }
    plan
}

/// Materialize the dataset plan into `root` (created), each file's bytes derived
/// deterministically from its relative path. Returns the total bytes written.
pub fn write_dataset(root: &Path, plan: &[FilePlan]) -> std::io::Result<u64> {
    let mut total = 0u64;
    for f in plan {
        let path = root.join(&f.rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = if f.compressible {
            compressible(&f.rel, f.size)
        } else {
            incompressible(&f.rel, f.size)
        };
        std::fs::write(&path, &bytes)?;
        total += bytes.len() as u64;
    }
    Ok(total)
}

/// Produce v2 from v1 by applying ~10% churn that MUST include mid-file content
/// shifts (insert/delete), not only whole-file operations — otherwise CDC has
/// nothing to show. Writes v2 into `v2_root`. Returns a human-readable churn
/// summary. `plan` is v1's layout (from [`dataset_plan`]).
pub fn write_churned_v2(
    v1_root: &Path,
    v2_root: &Path,
    plan: &[FilePlan],
) -> std::io::Result<ChurnSummary> {
    let n = plan.len();
    let mut summary = ChurnSummary::default();
    // Deterministic selection: touch roughly every 10th file, rotating the kind
    // of edit so all five mutation classes appear.
    for (i, f) in plan.iter().enumerate() {
        let src = v1_root.join(&f.rel);
        let dst = v2_root.join(&f.rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let churn_this = i % 10 == 0 && f.size > 0;
        if !churn_this {
            // Unchanged file — copy verbatim (dedups fully in v2).
            std::fs::copy(&src, &dst)?;
            continue;
        }
        let mut bytes = std::fs::read(&src)?;
        match (i / 10) % 4 {
            0 => {
                // In-place modify: overwrite a mid-file region (same length →
                // only the chunks covering the region change).
                let start = bytes.len() / 3;
                let patch = incompressible(&format!("patch-modify:{}", f.rel), bytes.len() / 20);
                let end = (start + patch.len()).min(bytes.len());
                bytes[start..end].copy_from_slice(&patch[..end - start]);
                summary.modified += 1;
            }
            1 => {
                // Shifting insert: splice new bytes into the middle → every chunk
                // after the insertion point shifts (the CDC re-sync test).
                let at = bytes.len() / 2;
                let ins = incompressible(&format!("patch-insert:{}", f.rel), bytes.len() / 15);
                let mut out = Vec::with_capacity(bytes.len() + ins.len());
                out.extend_from_slice(&bytes[..at]);
                out.extend_from_slice(&ins);
                out.extend_from_slice(&bytes[at..]);
                bytes = out;
                summary.inserted += 1;
            }
            2 => {
                // Mid-file delete: remove a region → content after shifts back.
                let at = bytes.len() / 4;
                let cut = (bytes.len() / 12).max(1);
                let end = (at + cut).min(bytes.len());
                bytes.drain(at..end);
                summary.deleted_region += 1;
            }
            _ => {
                // Append: tail grows (leading chunks unchanged).
                let app = incompressible(&format!("patch-append:{}", f.rel), bytes.len() / 10);
                bytes.extend_from_slice(&app);
                summary.appended += 1;
            }
        }
        std::fs::write(&dst, &bytes)?;
    }

    // Whole-file add: a brand-new file only in v2.
    let add_rel = "added/new_asset_v2.bin";
    let add_path = v2_root.join(add_rel);
    std::fs::create_dir_all(add_path.parent().unwrap())?;
    std::fs::write(&add_path, incompressible("v2-added", 8 * MIB))?;
    summary.added += 1;

    // Whole-file delete: drop the first file of v1 from v2 (skip copying it).
    if let Some(first) = plan.first() {
        let del = v2_root.join(&first.rel);
        if del.exists() {
            std::fs::remove_file(&del)?;
        }
        summary.removed += 1;
    }

    summary.total_files = n;
    Ok(summary)
}

/// A tally of the churn applied to produce v2.
#[derive(Debug, Default, Clone)]
pub struct ChurnSummary {
    pub total_files: usize,
    pub modified: usize,
    pub inserted: usize,
    pub deleted_region: usize,
    pub appended: usize,
    pub added: usize,
    pub removed: usize,
}

// -------------------------------------------------------------------------
// Dedup measurement helpers (used by the dedup driver + reused for reporting)
// -------------------------------------------------------------------------

/// Chunk every regular file under `root` with `chunker`, returning
/// `(chunk_key, chunk_size)` for each chunk in traversal order. The chunk key is
/// the blake3 longtail hash of the chunk content, so equal content → equal key.
pub fn chunk_tree<C: longtail_core::Chunker>(
    root: &Path,
    chunker: &C,
) -> std::io::Result<Vec<(u64, u32)>> {
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(root, &mut files)?;
    files.sort();
    for path in files {
        let data = std::fs::read(&path)?;
        for span in chunker.chunk(&data) {
            let start = span.offset as usize;
            let end = start + span.size as usize;
            let key = Blake3.hash(&data[start..end]);
            out.push((key, span.size));
        }
    }
    Ok(out)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}
