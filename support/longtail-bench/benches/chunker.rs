//! Chunker throughput micro-bench: HPCDC-Rust vs HPCDC-C (differential) vs
//! FastCDC (fastcdc feature), over incompressible + compressible 64 MiB buffers
//! at targets {32768, 131072}. Buffers are generated in-memory at bench time,
//! bigger than any committed fixture so steady-state chunking dominates.
//!
//! Throughput is reported by criterion via `Throughput::Bytes` (→ MiB/s). The C
//! comparison runs in-process in this same bench binary (the `differential`
//! machinery); it is cfg'd out in the pure lane so `cargo bench --no-run`
//! compiles without the native lib.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
#[cfg(feature = "fastcdc")]
use longtail_core::Chunker;
use longtail_core::HpcdcChunker;
use std::hint::black_box;

const BUF: usize = 64 * 1024 * 1024;
const TARGETS: [u32; 2] = [32768, 131072];

fn inputs() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "incompressible64",
            longtail_bench::incompressible("chunker-incompressible", BUF),
        ),
        (
            "compressible64",
            longtail_bench::compressible("chunker-compressible", BUF),
        ),
    ]
}

fn bench_chunker(c: &mut Criterion) {
    let inputs = inputs();
    for target in TARGETS {
        let mut group = c.benchmark_group(format!("chunker/t{target}"));
        group.throughput(Throughput::Bytes(BUF as u64));
        let chunker = HpcdcChunker::from_target(target).unwrap();
        let (min, avg, max) = chunker.params();

        for (name, data) in &inputs {
            // HPCDC — pure Rust (the compat-critical port).
            group.bench_function(format!("hpcdc_rust/{name}"), |b| {
                b.iter(|| {
                    let mut n = 0usize;
                    chunker.chunk_with(black_box(data), |s| n += s.size as usize);
                    black_box(n)
                })
            });

            // HPCDC — C via longtail-ffi streaming entry point (differential).
            #[cfg(feature = "differential")]
            group.bench_function(format!("hpcdc_c/{name}"), |b| {
                let api = longtail_ffi::ChunkerAPI::new();
                b.iter(|| {
                    let spans = api
                        .chunk_streaming(black_box(data), min, avg, max)
                        .expect("c chunk_streaming");
                    black_box(spans.len())
                })
            });

            // FastCDC — benchmarking-only alternative (feature `fastcdc`).
            #[cfg(feature = "fastcdc")]
            group.bench_function(format!("fastcdc/{name}"), |b| {
                let fc = longtail_core::FastCdcChunker::from_target(target);
                b.iter(|| {
                    let spans = fc.chunk(black_box(data));
                    black_box(spans.len())
                })
            });
        }
        group.finish();
        // Silence unused-var warnings in the pure lane (no C column).
        let _ = (min, avg, max);
    }
}

criterion_group!(benches, bench_chunker);
criterion_main!(benches);
