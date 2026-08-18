//! Codec cross-compat differential (`differential` feature, ladder 7): for **every**
//! compression ID in the `docs/format-spec.md` §4 table, Rust-encode → C-decode
//! is identical to the plaintext, and C-encode → Rust-decode is identical. This
//! covers the IDs golongtail never writes (`zstd_high`/`zstd_low`, brotli
//! min/max/text). Encode byte-parity is a deliberate non-gate; the gate is that
//! each side decodes the other's output back to identical plaintext.
//!
//! Compiles to nothing without the `differential` feature.
#![cfg(feature = "differential")]

use longtail_core::compress::compressor_for;
use longtail_testkit::differential::{c_compress, c_decompress};
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

/// Every compression ID from the §4 table (LZ4, 5 zstd variants, 6 brotli
/// variants). `0` (raw) has no codec and is exercised by the pure framing tests.
const ALL_IDS: &[u32] = &[
    0x6c7a_3432, // lz4
    0x7a74_6431, // zstd_min
    0x7a74_6432, // zstd default
    0x7a74_6433, // zstd_max
    0x7a74_6434, // zstd_high
    0x7a74_6435, // zstd_low
    0x6274_6c30, // brotli generic min
    0x6274_6c31, // brotli generic default
    0x6274_6c32, // brotli generic max
    0x6274_6c61, // brotli text min
    0x6274_6c62, // brotli text default
    0x6274_6c63, // brotli text max
];

/// Representative payloads (kept moderate so brotli-max/quality-11 stays fast):
/// highly compressible text, incompressible random, a short buffer, and empty.
fn samples() -> Vec<Vec<u8>> {
    let text = b"the quick brown fox jumps over the lazy dog. ".repeat(400); // ~18 KB
    let mut rng = ChaCha8Rng::seed_from_u64(0xC0DEC0DE);
    let mut random = vec![0u8; 16 * 1024];
    rng.fill_bytes(&mut random);
    let repetitive = vec![0xABu8; 8 * 1024];
    vec![text, random, repetitive, b"short".to_vec(), Vec::new()]
}

#[test]
fn rust_encode_c_decode_identical_every_id() {
    for &id in ALL_IDS {
        let codec = compressor_for(id).unwrap();
        for data in samples() {
            let compressed = codec.compress(&data).unwrap();
            let back = c_decompress(id, &compressed, data.len()).unwrap_or_else(|e| {
                panic!("C decode of Rust output failed for {id:#010x} (err {e})")
            });
            assert_eq!(
                back, data,
                "Rust-encode -> C-decode mismatch for {id:#010x}"
            );
        }
    }
    eprintln!(
        "Rust-encode -> C-decode identical for all {} IDs",
        ALL_IDS.len()
    );
}

#[test]
fn c_encode_rust_decode_identical_every_id() {
    for &id in ALL_IDS {
        let codec = compressor_for(id).unwrap();
        for data in samples() {
            let compressed = c_compress(id, &data)
                .unwrap_or_else(|e| panic!("C encode failed for {id:#010x} (err {e})"));
            let back = codec
                .decompress(&compressed, data.len())
                .unwrap_or_else(|e| panic!("Rust decode of C output failed for {id:#010x}: {e}"));
            assert_eq!(
                back, data,
                "C-encode -> Rust-decode mismatch for {id:#010x}"
            );
        }
    }
    eprintln!(
        "C-encode -> Rust-decode identical for all {} IDs",
        ALL_IDS.len()
    );
}

/// Full frame round-trip via the pure framing codec, decoded by C at the codec
/// level: proves the frame body Rust writes is exactly what C's codec consumes.
#[test]
fn framed_payload_body_is_c_decodable() {
    use longtail_core::compress::encode_block_payload;
    for &id in ALL_IDS {
        for data in samples() {
            let framed = encode_block_payload(id, &data).unwrap();
            // header = [uncompressed u32][compressed u32]; body follows.
            let uncompressed = u32::from_le_bytes([framed[0], framed[1], framed[2], framed[3]]);
            let compressed = u32::from_le_bytes([framed[4], framed[5], framed[6], framed[7]]);
            assert_eq!(
                uncompressed as usize,
                data.len(),
                "frame uncompressed_size {id:#010x}"
            );
            assert_eq!(
                compressed as usize,
                framed.len() - 8,
                "frame compressed_size {id:#010x}"
            );
            let body = &framed[8..];
            let back = c_decompress(id, body, data.len()).unwrap();
            assert_eq!(back, data, "C-decode of framed body mismatch {id:#010x}");
        }
    }
}
