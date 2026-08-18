//! merge_mem — isolate the store-index union-merge peak (finding 7).
//!
//! Builds two large, **distinct**, canonical synthetic store indexes (the
//! "two-file steady state" a month-end shard reaches) and runs either the
//! allocating [`StoreIndex::merge`] or the consuming `merge_consuming`. Run each
//! under `/usr/bin/time -v` and compare "Maximum resident set size":
//!
//! - `merge`     holds `local + remote + fresh output`  (~3 shards)
//! - `consuming` reuses `local` as the output           (~2 shards + remote)
//!
//! so the consuming path should peak ~one shard lower. This is a pure,
//! network-free micro-measurement of the merge itself — no S3, no feature flags.
//!
//! Usage: `merge_mem <merge|consuming> [blocks]`
//!   `blocks` defaults to 2_000_000 (~200 MiB/shard at 8 chunks/block).

use longtail_bench::synthetic_store_index;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "merge".into());
    let blocks: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2_000_000);
    let cpb = 8usize;

    eprintln!("building 2 × {blocks}-block synthetic store indexes ({cpb} chunks/block)...");
    let a = synthetic_store_index(blocks, cpb);
    let mut b = synthetic_store_index(blocks, cpb);
    // Make `b` disjoint from `a` (same seed → identical hashes otherwise) so the
    // union is the full 2× with no dedup overlap — the real two-shard case.
    const K: u64 = 0x8000_0000_0000_0001;
    for h in &mut b.block_hashes {
        *h = h.wrapping_add(K);
    }
    for h in &mut b.chunk_hashes {
        *h = h.wrapping_add(K);
    }
    // Estimated serialized/in-memory size, computed WITHOUT allocating a
    // to_bytes() buffer (which would itself inflate the measured peak).
    let bytes = 16 + blocks * 20 + blocks * cpb * 12;
    eprintln!(
        "  each: {} blocks, {} chunks, ~{:.0} MiB",
        a.block_count(),
        a.chunk_count(),
        bytes as f64 / 1048576.0
    );

    let merged = match mode.as_str() {
        "merge" => a.merge(&b).expect("merge"),
        "consuming" => a.merge_consuming(&b).expect("merge_consuming"),
        other => {
            eprintln!("unknown mode {other:?}; use merge|consuming");
            std::process::exit(2);
        }
    };
    // Touch the result so it can't be optimized away; print a checksum.
    let sum = merged
        .block_hashes
        .iter()
        .fold(0u64, |acc, &h| acc.wrapping_add(h));
    println!(
        "{mode}: union {} blocks, hashsum {sum:#018x}",
        merged.block_count()
    );
}
