//! Hash throughput micro-bench: blake3-Rust vs blake3-C and blake2s-Rust vs
//! blake2s-C over {4 KiB, 64 KiB, 1 MiB} buffers — chunk-sized workloads, the
//! real shape (longtail hashes per-chunk, not giant streams).
//!
//! The stored longtail hash is the little-endian u64 of the first 8 digest
//! bytes; both the Rust and C sides compute exactly that, so this is a like-for
//! -like throughput comparison. C runs in-process (differential lane).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use longtail_core::{Blake2s, Blake3, Hash};
use std::hint::black_box;

const SIZES: [(&str, usize); 3] = [("4k", 4 * 1024), ("64k", 64 * 1024), ("1m", 1024 * 1024)];

fn bench_hash(c: &mut Criterion) {
    for (label, size) in SIZES {
        let data = longtail_bench::incompressible(&format!("hash-{label}"), size);
        let mut group = c.benchmark_group(format!("hash/{label}"));
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_function("blake3_rust", |b| {
            b.iter(|| black_box(Blake3.hash(black_box(&data))))
        });
        group.bench_function("blake2s_rust", |b| {
            b.iter(|| black_box(Blake2s.hash(black_box(&data))))
        });

        #[cfg(feature = "differential")]
        {
            use longtail_ffi::{HashRegistry, HashType};
            let reg = HashRegistry::new();
            let b3 = reg.get_hash_api(HashType::Blake3).expect("c blake3");
            let b2 = reg.get_hash_api(HashType::Blake2).expect("c blake2");
            group.bench_function("blake3_c", |b| {
                b.iter(|| black_box(b3.hash_buffer(black_box(&data)).expect("c blake3 buf")))
            });
            group.bench_function("blake2s_c", |b| {
                b.iter(|| black_box(b2.hash_buffer(black_box(&data)).expect("c blake2 buf")))
            });
        }
        group.finish();
    }
}

criterion_group!(benches, bench_hash);
criterion_main!(benches);
