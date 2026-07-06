//! Index-codec micro-bench — the **owned-struct revisit trigger**.
//! The core chose owned-struct `.lvi`/`.lsi` codecs
//! (`Vec<u64>`/`Vec<u32>` …) for correctness/simplicity; this bench measures
//! parse + serialize at realistic scale to decide whether zero-copy views are
//! worth revisiting. Verdict expectation: microseconds against seconds of I/O.
//!
//! Cells: the largest committed `.lvi` / `.lsi`, AND a synthetic large index
//! (~100k assets / ~500k chunks; ~50k blocks) built in-memory. Reports ns/op and
//! derived MiB/s (via `Throughput::Bytes`).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use longtail_core::{StoreIndex, VersionIndex};
use longtail_testkit::paths::fixtures_dir;
use std::hint::black_box;

fn bench_version_index(c: &mut Criterion, label: &str, bytes: &[u8]) {
    let parsed = VersionIndex::from_bytes(bytes).expect("parse lvi");
    let reser = parsed.to_bytes();
    let mut group = c.benchmark_group(format!("version_index/{label}"));
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("parse", |b| {
        b.iter(|| black_box(VersionIndex::from_bytes(black_box(bytes)).unwrap()))
    });
    group.bench_function("serialize", |b| b.iter(|| black_box(parsed.to_bytes())));
    group.finish();
    assert_eq!(reser, bytes, "{label}: round-trip must be byte-identical");
}

fn bench_store_index(c: &mut Criterion, label: &str, bytes: &[u8]) {
    let parsed = StoreIndex::from_bytes(bytes).expect("parse lsi");
    let mut group = c.benchmark_group(format!("store_index/{label}"));
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("parse", |b| {
        b.iter(|| black_box(StoreIndex::from_bytes(black_box(bytes)).unwrap()))
    });
    group.bench_function("serialize", |b| b.iter(|| black_box(parsed.to_bytes())));
    group.finish();
}

fn bench_index_codec(c: &mut Criterion) {
    // Largest committed fixtures.
    let lvi_path = fixtures_dir().join("stores/chunk-1024/zoo.lvi");
    let lsi_path = fixtures_dir().join("stores/chunk-1024/store/store.lsi");
    if let Ok(bytes) = std::fs::read(&lvi_path) {
        bench_version_index(c, "committed_largest", &bytes);
    }
    if let Ok(bytes) = std::fs::read(&lsi_path) {
        bench_store_index(c, "committed_largest", &bytes);
    }

    // Synthetic large indexes (in-memory).
    let big_vi = longtail_bench::synthetic_version_index(100_000, 500_000);
    let big_vi_bytes = big_vi.to_bytes();
    bench_version_index(c, "synthetic_100k_assets", &big_vi_bytes);

    let big_si = longtail_bench::synthetic_store_index(50_000, 10); // 500k chunks
    let big_si_bytes = big_si.to_bytes();
    bench_store_index(c, "synthetic_50k_blocks", &big_si_bytes);
}

criterion_group!(benches, bench_index_codec);
criterion_main!(benches);
