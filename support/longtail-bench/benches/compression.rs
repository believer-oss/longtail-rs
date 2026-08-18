//! Compression throughput micro-bench: per codec × {compress, decompress} ×
//! {zstd_default, zstd_max, lz4, brotli_default}, Rust vs C, on ~8 MiB
//! block-shaped payloads (compressible + incompressible).
//!
//! Encode byte-parity is a deliberate NON-gate (block identity = hash of the
//! chunk-hash array), so decompress uses the Rust-compressed bytes as the common
//! valid-frame input for both sides — a fair like-for-like decode workload.
//!
//! Sample count is capped (brotli q11 compress of 8 MiB is intentionally slow):
//! the point is a throughput ratio vs C, not a tight CI number.

use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use longtail_core::compressor_for;
use std::hint::black_box;

const PAYLOAD: usize = 8 * 1024 * 1024;

/// (label, id) for the four codecs under test.
const CODECS: [(&str, u32); 4] = [
    ("zstd_default", 0x7a74_6432),
    ("zstd_max", 0x7a74_6433),
    ("lz4", 0x6c7a_3432),
    ("brotli_default", 0x6274_6c31),
];

fn bench_compression(c: &mut Criterion) {
    let payloads = [
        (
            "compressible",
            longtail_bench::compressible("comp-payload", PAYLOAD),
        ),
        (
            "incompressible",
            longtail_bench::incompressible("incomp-payload", PAYLOAD),
        ),
    ];

    for (codec_name, id) in CODECS {
        let rust_codec = compressor_for(id).expect("rust codec");
        #[cfg(feature = "differential")]
        let c_reg = longtail_ffi::CompressionRegistry::new();

        for (pname, data) in &payloads {
            // Common compressed bytes (Rust-produced valid frame) for decode.
            let compressed = rust_codec.compress(data).expect("rust compress");

            let mut group = c.benchmark_group(format!("compress/{codec_name}/{pname}"));
            group.throughput(Throughput::Bytes(PAYLOAD as u64));
            group.sample_size(10);
            group.warm_up_time(Duration::from_secs(1));

            group.bench_function("compress_rust", |b| {
                b.iter(|| black_box(rust_codec.compress(black_box(data)).unwrap().len()))
            });
            #[cfg(feature = "differential")]
            group.bench_function("compress_c", |b| {
                b.iter(|| black_box(c_reg.compress_buffer(id, black_box(data)).unwrap().len()))
            });
            group.finish();

            let mut dgroup = c.benchmark_group(format!("decompress/{codec_name}/{pname}"));
            dgroup.throughput(Throughput::Bytes(PAYLOAD as u64));
            dgroup.sample_size(20);
            dgroup.warm_up_time(Duration::from_secs(1));
            dgroup.bench_function("decompress_rust", |b| {
                b.iter(|| {
                    black_box(
                        rust_codec
                            .decompress(black_box(&compressed), PAYLOAD)
                            .unwrap()
                            .len(),
                    )
                })
            });
            #[cfg(feature = "differential")]
            dgroup.bench_function("decompress_c", |b| {
                b.iter(|| {
                    black_box(
                        c_reg
                            .decompress_buffer(id, black_box(&compressed), PAYLOAD)
                            .unwrap()
                            .len(),
                    )
                })
            });
            dgroup.finish();
        }
    }
}

criterion_group!(benches, bench_compression);
criterion_main!(benches);
