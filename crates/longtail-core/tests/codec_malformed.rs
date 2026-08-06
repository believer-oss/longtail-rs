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
            decode_block_payload(LZ4, &payload, usize::MAX),
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
        decode_block_payload(LZ4, &framed, usize::MAX),
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
    let err = decode_block_payload(LZ4, &framed, usize::MAX).unwrap_err();
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
        decode_block_payload(0xdead_beef, &payload, usize::MAX),
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
    let res = decode_block_payload(LZ4, &framed, usize::MAX);
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
        prop_assert_eq!(decode_block_payload(tag, &framed, data.len()).unwrap(), data);
        // Now corrupt one byte and just require no panic.
        if !framed.is_empty() {
            let mut m = framed.clone();
            let i = idx.index(m.len());
            m[i] ^= xor;
            let _ = decode_block_payload(tag, &m, usize::MAX); // must not panic
        }
    }
}

// --- allocation is bounded by the caller's knowledge, not the frame ---------

/// Build a frame header for a body of `body`, declaring `uncompressed`.
fn frame_with_declared(uncompressed: u32, tag_body: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + tag_body.len());
    v.extend_from_slice(&uncompressed.to_le_bytes());
    v.extend_from_slice(&(tag_body.len() as u32).to_le_bytes());
    v.extend_from_slice(tag_body);
    v
}

/// A frame declaring more plaintext than the block can hold is refused before
/// any memory is committed.
///
/// `uncompressed_size` is read straight out of the payload and was handed to the
/// codec as an allocation size. A block's plaintext is exactly the chunks its
/// index claims, so the caller always knows the real bound; without passing it
/// in, a `u32::MAX` declaration is a 4 GiB request, and an allocation failure in
/// Rust aborts the process rather than returning an error.
#[test]
fn a_frame_declaring_more_than_the_block_holds_is_refused() {
    const BROTLI_DEFAULT: u32 = 0x6274_6c31;
    let framed = frame_with_declared(u32::MAX, b"whatever");

    let err = decode_block_payload(BROTLI_DEFAULT, &framed, 4096)
        .expect_err("a 4 GiB declaration against a 4 KiB block must be refused");
    assert!(
        matches!(err, CompressError::DeclaredSizeTooLarge { .. }),
        "wrong error: {err:?}"
    );
}

/// The refusal comes before the body is examined at all.
///
/// Codec resolution deliberately happens first, mirroring C's `DecompressBlock`.
/// What matters is that nothing is *allocated* on the strength of the declared
/// size: this frame also has an inconsistent `compressed_size`, and the size
/// refusal is what surfaces, so the check sits ahead of the body handling and
/// therefore ahead of the decode.
#[test]
fn the_size_refusal_precedes_any_body_handling() {
    let mut framed = frame_with_declared(u32::MAX, b"body");
    framed[4..8].copy_from_slice(&9999u32.to_le_bytes()); // wrong compressed_size too
    let err = decode_block_payload(LZ4, &framed, 16).unwrap_err();
    assert!(
        matches!(err, CompressError::DeclaredSizeTooLarge { .. }),
        "expected the size refusal ahead of body handling, got {err:?}"
    );
}

/// A brotli stream that expands far beyond its declared size must be cut off
/// rather than grown into memory.
///
/// Brotli's window makes a ~1000:1 expansion trivial, so the declared size is no
/// guarantee of what the stream actually decodes to. The decoder reads through a
/// bounded sink; the length check that catches the lie has to come *after* a
/// bounded read, not after an unbounded one.
#[test]
fn a_brotli_stream_that_outgrows_its_declaration_is_cut_off() {
    const BROTLI_DEFAULT: u32 = 0x6274_6c31;
    // 8 MiB of zeros compresses to a tiny brotli stream.
    let plaintext = vec![0u8; 8 << 20];
    let real = encode_block_payload(BROTLI_DEFAULT, &plaintext).unwrap();
    let body = &real[8..]; // strip the header we are about to lie in

    // Declare 1 KiB while the stream actually decodes to 8 MiB.
    let lying = frame_with_declared(1024, body);
    let err = decode_block_payload(BROTLI_DEFAULT, &lying, 8 << 20)
        .expect_err("a stream outgrowing its declared size must not be accepted");
    assert!(
        matches!(err, CompressError::DecodedLengthMismatch { .. }),
        "wrong error: {err:?}"
    );

    // Control: the honest frame still decodes, so the bound is not just
    // rejecting brotli outright.
    let ok = decode_block_payload(BROTLI_DEFAULT, &real, 8 << 20).expect("honest frame decodes");
    assert_eq!(ok, plaintext);
}
