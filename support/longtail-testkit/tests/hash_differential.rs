//! Hash differential (differential lane, via `longtail-ffi`):
//! the pure-Rust blake3/blake2s hashes equal the reference C library's over a
//! range of inputs, and the frozen known-answer constants used in the pure lane
//! are cross-checked against C here (the real gate for those KATs).
//!
//! Compiles to nothing without the `differential` feature.
#![cfg(feature = "differential")]

use longtail_core::hash::{Blake2s, Blake3, Hash, blake2s_hash, blake3_hash};
use longtail_ffi::HashType;
use longtail_testkit::corpus;
use longtail_testkit::differential::c_hash;
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

/// The frozen KAT constants that the pure-lane `longtail_core::hash` unit test
/// asserts — cross-checked against C here so they are anchored to the reference
/// implementation, not to the pure code that also produces them.
#[test]
fn frozen_kats_match_c() {
    let cases: [(&[u8], u64, u64); 3] = [
        (b"", 0xa6a1f9f5b94913af, 0x9cda80dd788b2aef),
        (b"longtail", 0xefb32f524ca47d58, 0x1a838bcb644c5a58),
        (
            b"the quick brown fox",
            0x0ad886dd9a538965,
            0x12763f8910862397,
        ),
    ];
    for (input, b3, b2) in cases {
        assert_eq!(blake3_hash(input), b3, "blake3 KAT constant");
        assert_eq!(blake2s_hash(input), b2, "blake2s KAT constant");
        assert_eq!(c_hash(HashType::Blake3, input), b3, "blake3 KAT vs C");
        assert_eq!(c_hash(HashType::Blake2, input), b2, "blake2s KAT vs C");
    }
}

#[test]
fn blake3_matches_c_over_corpus_and_random() {
    // Every single-file corpus case.
    for name in corpus::zoo_all() {
        if let Some(bytes) = corpus::case_bytes(name) {
            assert_eq!(
                blake3_hash(&bytes),
                c_hash(HashType::Blake3, &bytes),
                "blake3 mismatch for corpus case {name}"
            );
            assert_eq!(Blake3.hash(&bytes), c_hash(HashType::Blake3, &bytes));
        }
    }
    // Random buffers of varied sizes (seeded, deterministic).
    let mut rng = ChaCha8Rng::seed_from_u64(0xB3B3_B3B3);
    for _ in 0..200 {
        let n = (rng.next_u32() % 5000) as usize;
        let mut buf = vec![0u8; n];
        rng.fill_bytes(&mut buf);
        assert_eq!(
            blake3_hash(&buf),
            c_hash(HashType::Blake3, &buf),
            "blake3 random n={n}"
        );
    }
}

#[test]
fn blake2s_matches_c_over_corpus_and_random() {
    for name in corpus::zoo_all() {
        if let Some(bytes) = corpus::case_bytes(name) {
            assert_eq!(
                blake2s_hash(&bytes),
                c_hash(HashType::Blake2, &bytes),
                "blake2s mismatch for corpus case {name}"
            );
            assert_eq!(Blake2s.hash(&bytes), c_hash(HashType::Blake2, &bytes));
        }
    }
    let mut rng = ChaCha8Rng::seed_from_u64(0xB2B2_B2B2);
    for _ in 0..200 {
        let n = (rng.next_u32() % 5000) as usize;
        let mut buf = vec![0u8; n];
        rng.fill_bytes(&mut buf);
        assert_eq!(
            blake2s_hash(&buf),
            c_hash(HashType::Blake2, &buf),
            "blake2s random n={n}"
        );
    }
}
