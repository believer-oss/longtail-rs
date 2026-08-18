//! Malformed-input tests for the framing codec (pure lane):
//! truncated frame header, `compressed_size` mismatch, wrong decoded length, and
//! unknown tag all return typed [`CompressError`]s and never panic. Plus a
//! fuzz-ish proptest that mutates a valid compressed payload and only asserts the
//! decoder does not panic.

use longtail_core::compress::{CompressError, decode_block_payload, encode_block_payload};
use proptest::prelude::*;

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: if cfg!(miri) { 8 } else { 256 },
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

// Use LZ4 (pure Rust) as the framing test codec so this whole file runs under
// miri (zstd is the only FFI codec — miri can't call it). The framing header
// logic is codec-independent; the differential lane proves every codec ID.
const LZ4: u32 = 0x6c7a_3432;

fn sample() -> Vec<u8> {
    b"the quick brown fox jumps over the lazy dog. ".repeat(32)
}

/// Tags to fuzz. zstd (FFI) is excluded under miri; brotli/lz4 are pure Rust.
fn fuzz_tags() -> Vec<u32> {
    if cfg!(miri) {
        vec![0u32, 0x6c7a_3432] // raw + lz4 (fast + pure)
    } else {
        vec![0u32, 0x6c7a_3432, 0x7a74_6432, 0x6274_6c31] // raw, lz4, zstd, brotli
    }
}

#[test]
fn truncated_frame_header() {
    // A compressed tag with fewer than 8 payload bytes is a truncated header.
    for len in 0..8usize {
        let payload = vec![0u8; len];
        assert_eq!(
            decode_block_payload(LZ4, &payload),
            Err(CompressError::TruncatedFrameHeader { len })
        );
    }
}

#[test]
fn compressed_size_mismatch() {
    let data = sample();
    let mut framed = encode_block_payload(LZ4, &data).unwrap();
    // Append a stray trailing byte: body length no longer equals compressed_size.
    let declared = framed.len() - 8;
    framed.push(0xAB);
    assert_eq!(
        decode_block_payload(LZ4, &framed),
        Err(CompressError::CompressedSizeMismatch {
            declared,
            actual: declared + 1
        })
    );
}

#[test]
fn wrong_declared_uncompressed_size() {
    let data = sample();
    let mut framed = encode_block_payload(LZ4, &data).unwrap();
    // Corrupt header[0] (uncompressed_size) to a wrong-but-plausible value. The
    // codec will decode to the real length, tripping DecodedLengthMismatch (or a
    // codec decode error if the smaller output buffer is exceeded — both typed).
    let wrong = (data.len() as u32) - 1;
    framed[0..4].copy_from_slice(&wrong.to_le_bytes());
    let err = decode_block_payload(LZ4, &framed).unwrap_err();
    assert!(
        matches!(
            err,
            CompressError::DecodedLengthMismatch { .. } | CompressError::Decompress { .. }
        ),
        "unexpected error: {err:?}"
    );
}

#[test]
fn unknown_tag_rejected() {
    let payload = vec![0u8; 16];
    assert_eq!(
        decode_block_payload(0xdead_beef, &payload),
        Err(CompressError::UnknownCompressionId { id: 0xdead_beef })
    );
}

#[test]
fn corrupt_compressed_body_is_typed_error() {
    let data = sample();
    let mut framed = encode_block_payload(LZ4, &data).unwrap();
    // Flip bytes in the compressed body (keep sizes consistent) — must be a
    // typed decode error, never a panic.
    for b in framed[8..].iter_mut() {
        *b ^= 0xFF;
    }
    let res = decode_block_payload(LZ4, &framed);
    assert!(res.is_err(), "corrupt body should not decode");
}

proptest! {
    #![proptest_config(config())]

    /// Mutating a valid compressed payload never panics the decoder — it either
    /// decodes or returns a typed error.
    #[test]
    fn mutated_payload_never_panics(
        seed in any::<u64>(),
        idx in any::<prop::sample::Index>(),
        xor in 1u8..=255,
        tag in prop::sample::select(fuzz_tags()),
    ) {
        // Build a small deterministic input and frame it.
        let n = 1 + (seed % 300) as usize;
        let data: Vec<u8> = (0..n).map(|i| (seed.wrapping_mul(i as u64 + 1) >> 13) as u8).collect();
        let framed = encode_block_payload(tag, &data).unwrap();
        // Round-trip the pristine frame.
        prop_assert_eq!(decode_block_payload(tag, &framed).unwrap(), data);
        // Now corrupt one byte and just require no panic.
        if !framed.is_empty() {
            let mut m = framed.clone();
            let i = idx.index(m.len());
            m[i] ^= xor;
            let _ = decode_block_payload(tag, &m); // must not panic
        }
    }
}
