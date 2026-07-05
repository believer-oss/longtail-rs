//! Chunker differential (differential lane, ladder 8-9):
//!
//! - **Ladder 8** — random buffers of varied sizes (`< 48`, `≈min`, `≈max`,
//!   `> max*4`) at the four standard targets {1024, 32768, 131072, 1048576}:
//!   the pure-Rust streaming chunker + pure blake3 produce identical
//!   `(offset, size, hash)` boundaries to the ffi `chunk_streaming` +
//!   `hash_buffer`. The committed corpus tables only cover target 32768; this is
//!   what demonstrates the all-four-targets coverage exit criterion.
//! - **Ladder 9** — the discriminator `d` exhaustively matches the committed C
//!   shim over `avg ∈ [48, 9_309_387]` (the range where C's `(uint32_t)` cast is
//!   defined; above it the expression is UB — see `rust-port-3.md` Task 6).
//!
//! Compiles to nothing without the `differential` feature.
#![cfg(feature = "differential")]

use longtail_core::chunker::{HpcdcChunker, MAX_AVG, discriminator_from_avg};
use longtail_core::hash::Blake3;
use longtail_ffi::ChunkerAPI;
use longtail_testkit::boundary::chunker_params_for_target;
use longtail_testkit::differential::blake3_hash;
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

// The committed HPCDC discriminator shim (`shim/hpcdc_discriminator_shim.c`,
// compiled by build.rs under the `differential` feature). The `unsafe extern`
// binding lives here in `tests/` — testkit's `src/` is `#![forbid(unsafe_code)]`
// and cannot host it.
unsafe extern "C" {
    fn longtail_shim_discriminator_from_avg(avg: f64) -> u32;
}

const TARGETS: [u32; 4] = [1024, 32768, 131072, 1048576];

fn seeded(target: u32, size: usize) -> Vec<u8> {
    let mut rng = ChaCha8Rng::seed_from_u64((target as u64) << 32 | size as u64);
    let mut buf = vec![0u8; size];
    rng.fill_bytes(&mut buf);
    buf
}

/// The sizes to exercise for a given (min, max): boundary-sensitive values plus
/// a couple of larger multiples of max (covering `> max*4`).
fn sizes_for(min: u32, max: u32) -> Vec<usize> {
    let min = min as usize;
    let max = max as usize;
    let mut v = vec![
        0,
        1,
        47,
        48,
        49,
        min.saturating_sub(1),
        min,
        min + 1,
        max - 1,
        max,
        max + 1,
        2 * max,
        4 * max + 1, // > max*4
    ];
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn streaming_boundaries_and_hashes_match_c_all_targets() {
    let chunker_api = ChunkerAPI::new();
    let (_reg, c_hash_api) = blake3_hash();

    let mut total_chunks = 0usize;
    for target in TARGETS {
        let (min, avg, max) = chunker_params_for_target(target);
        let rust = HpcdcChunker::from_target(target).unwrap();
        assert_eq!(rust.params(), (min, avg, max));

        for size in sizes_for(min, max) {
            let data = seeded(target, size);

            // Pure Rust: composed (offset, size, hash).
            let pure = rust.chunk_hashed(&data, &Blake3);

            // C via ffi: streaming spans, hashed with the C blake3.
            let c_spans = chunker_api
                .chunk_streaming(&data, min, avg, max)
                .expect("ffi chunk_streaming");

            assert_eq!(
                pure.len(),
                c_spans.len(),
                "chunk count differs at target {target}, size {size}"
            );
            for (p, cs) in pure.iter().zip(&c_spans) {
                assert_eq!(p.offset, cs.offset, "offset @ target {target} size {size}");
                assert_eq!(p.size, cs.size, "size @ target {target} size {size}");
                let start = cs.offset as usize;
                let end = start + cs.size as usize;
                let c_h = c_hash_api.hash_buffer(&data[start..end]).unwrap();
                assert_eq!(p.hash, c_h, "hash @ target {target} size {size}");
                total_chunks += 1;
            }
        }
    }
    eprintln!(
        "chunker+hash parity vs C confirmed over {} chunks across {} targets",
        total_chunks,
        TARGETS.len()
    );
}

#[test]
fn discriminator_exhaustive_matches_c_shim() {
    // Every avg in [48, MAX_AVG]. Above MAX_AVG the C cast is UB, so there is no
    // behavior to be compatible with (the Rust chunker rejects such avg — see
    // `discriminator_ceiling_rejected`). ~9.3M iterations; cheap.
    let lo = 48u32;
    let hi = MAX_AVG;
    let mut mismatches = 0u64;
    let mut first_mismatch: Option<(u32, u32, u32)> = None;
    for avg in lo..=hi {
        let rust = discriminator_from_avg(avg);
        let c = unsafe { longtail_shim_discriminator_from_avg(avg as f64) };
        if rust != c {
            mismatches += 1;
            if first_mismatch.is_none() {
                first_mismatch = Some((avg, rust, c));
            }
        }
    }
    assert_eq!(
        mismatches, 0,
        "discriminator mismatches vs C shim: {mismatches} (first: {first_mismatch:?})"
    );
    eprintln!(
        "discriminator exhaustive match vs C shim over avg [{lo}, {hi}] ({} values)",
        hi - lo + 1
    );
}

#[test]
fn discriminator_ceiling_rejected() {
    // At the ceiling the quotient is still in u32 range; one above, the C
    // expression's quotient leaves u32 range (UB in C) so the constructor rejects.
    assert!(HpcdcChunker::new(48, MAX_AVG, MAX_AVG).is_ok());
    assert!(HpcdcChunker::new(48, MAX_AVG + 1, MAX_AVG + 1).is_err());
}
