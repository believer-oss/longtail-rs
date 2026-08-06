//! HPCDC content-defined chunker (`docs/format-spec.md` §6).
//!
//! Source of truth: `lib/hpcdcchunker/longtail_hpcdcchunker.c`
//! (`Longtail_HPCDCNextChunk`, hpcdcchunker.c:224-307). This is an exact port of
//! the **streaming** boundary algorithm — the canonical path for port parity
//! (golongtail defaults `--enable-file-mapping` to false, so production `.lvi`
//! data was chunked this way). One boundary algorithm is used for all I/O forms;
//! the reader-driven feed buffer (`max_feed = max*4`) is an I/O batching detail
//! that never affects boundary placement (`docs/format-spec.md` §6, "Streaming
//! refill contract"), so a whole-slice implementation is boundary-identical to
//! the C feeder path.
//!
//! [`SeedMode::Buffer`] reproduces the upstream mmap/buffer entry point
//! (`HPCDCChunker_NextChunkFromBuffer`, hpcdcchunker.c:452-534), which seeds the
//! rolling hash over the first 48 bytes of each scope instead of the 48 bytes
//! preceding `min` and can therefore DIVERGE when `min > 48`. It exists solely
//! as a labeled differential target for the committed `*.buffer.json` tables and
//! is **not** used by any production path.
//!
//! Bench note: [`HpcdcChunker::chunk`] allocates only the output `Vec`
//! and a fixed 48-byte stack window — no per-chunk allocation — so the inner
//! loop can be benchmarked without refactoring.

use thiserror::Error;

use crate::hash::Hash;

/// Rolling-hash window size (`ChunkerWindowSize`, hpcdcchunker.c:12). Doubles as
/// the absolute floor for `min` and the size at/below which the remaining data
/// becomes a single final chunk.
pub const WINDOW_SIZE: usize = 48;

/// Ceiling on the derived `avg` accepted by the constructor. Above this the C
/// discriminator expression's quotient leaves `u32` range (at `avg ≈ 9_309_388`)
/// and, past the denominator's pole (`avg ≈ 9_324_556`), goes negative — there
/// C's `(uint32_t)` cast is undefined (C17 §6.3.1.4), so there is no compatible
/// behavior to mirror. Every real target sits far below (avg = target/2; even a
/// 16 MiB target → avg = 2²³ ≈ 8.4M).
pub const MAX_AVG: u32 = 9_309_387;

/// The 256-entry gear table, copied **verbatim** from
/// `lib/hpcdcchunker/longtail_hpcdcchunker.c:22-95` (cross-checked against that
/// C source, not just the spec).
#[rustfmt::skip]
static GEAR: [u32; 256] = [
    0x458be752, 0xc10748cc, 0xfbbcdbb8, 0x6ded5b68,
    0xb10a82b5, 0x20d75648, 0xdfc5665f, 0xa8428801,
    0x7ebf5191, 0x841135c7, 0x65cc53b3, 0x280a597c,
    0x16f60255, 0xc78cbc3e, 0x294415f5, 0xb938d494,
    0xec85c4e6, 0xb7d33edc, 0xe549b544, 0xfdeda5aa,
    0x882bf287, 0x3116737c, 0x05569956, 0xe8cc1f68,
    0x0806ac5e, 0x22a14443, 0x15297e10, 0x50d090e7,
    0x4ba60f6f, 0xefd9f1a7, 0x5c5c885c, 0x82482f93,
    0x9bfd7c64, 0x0b3e7276, 0xf2688e77, 0x8fad8abc,
    0xb0509568, 0xf1ada29f, 0xa53efdfe, 0xcb2b1d00,
    0xf2a9e986, 0x6463432b, 0x95094051, 0x5a223ad2,
    0x9be8401b, 0x61e579cb, 0x1a556a14, 0x5840fdc2,
    0x9261ddf6, 0xcde002bb, 0x52432bb0, 0xbf17373e,
    0x7b7c222f, 0x2955ed16, 0x9f10ca59, 0xe840c4c9,
    0xccabd806, 0x14543f34, 0x1462417a, 0x0d4a1f9c,
    0x087ed925, 0xd7f8f24c, 0x7338c425, 0xcf86c8f5,
    0xb19165cd, 0x9891c393, 0x325384ac, 0x0308459d,
    0x86141d7e, 0xc922116a, 0xe2ffa6b6, 0x53f52aed,
    0x2cd86197, 0xf5b9f498, 0xbf319c8f, 0xe0411fae,
    0x977eb18c, 0xd8770976, 0x9833466a, 0xc674df7f,
    0x8c297d45, 0x8ca48d26, 0xc49ed8e2, 0x7344f874,
    0x556f79c7, 0x6b25eaed, 0xa03e2b42, 0xf68f66a4,
    0x8e8b09a2, 0xf2e0e62a, 0x0d3a9806, 0x9729e493,
    0x8c72b0fc, 0x160b94f6, 0x450e4d3d, 0x7a320e85,
    0xbef8f0e1, 0x21d73653, 0x4e3d977a, 0x1e7b3929,
    0x1cc6c719, 0xbe478d53, 0x8d752809, 0xe6d8c2c6,
    0x275f0892, 0xc8acc273, 0x4cc21580, 0xecc4a617,
    0xf5f7be70, 0xe795248a, 0x375a2fe9, 0x425570b6,
    0x8898dcf8, 0xdc2d97c4, 0x0106114b, 0x364dc22f,
    0x1e0cad1f, 0xbe63803c, 0x5f69fac2, 0x4d5afa6f,
    0x1bc0dfb5, 0xfb273589, 0x0ea47f7b, 0x3c1c2b50,
    0x21b2a932, 0x6b1223fd, 0x2fe706a8, 0xf9bd6ce2,
    0xa268e64e, 0xe987f486, 0x3eacf563, 0x1ca2018c,
    0x65e18228, 0x2207360a, 0x57cf1715, 0x34c37d2b,
    0x1f8f3cde, 0x93b657cf, 0x31a019fd, 0xe69eb729,
    0x8bca7b9b, 0x4c9d5bed, 0x277ebeaf, 0xe0d8f8ae,
    0xd150821c, 0x31381871, 0xafc3f1b0, 0x927db328,
    0xe95effac, 0x305a47bd, 0x426ba35b, 0x1233af3f,
    0x686a5b83, 0x50e072e5, 0xd9d3bb2a, 0x8befc475,
    0x487f0de6, 0xc88dff89, 0xbd664d5e, 0x971b5d18,
    0x63b14847, 0xd7d3c1ce, 0x7f583cf3, 0x72cbcb09,
    0xc0d0a81c, 0x7fa3429b, 0xe9158a1b, 0x225ea19a,
    0xd8ca9ea3, 0xc763b282, 0xbb0c6341, 0x020b8293,
    0xd4cd299d, 0x58cfa7f8, 0x91b4ee53, 0x37e4d140,
    0x95ec764c, 0x30f76b06, 0x5ee68d24, 0x679c8661,
    0xa41979c2, 0xf2b61284, 0x4fac1475, 0x0adb49f9,
    0x19727a23, 0x15a7e374, 0xc43a18d5, 0x3fb1aa73,
    0x342fc615, 0x924c0793, 0xbee2d7f0, 0x8a279de9,
    0x4aa2d70c, 0xe24dd37f, 0xbe862c0b, 0x177c22c2,
    0x5388e5ee, 0xcd8a7510, 0xf901b4fd, 0xdbc13dbc,
    0x6c0bae5b, 0x64efe8c7, 0x48b02079, 0x80331a49,
    0xca3d8ae6, 0xf3546190, 0xfed7108b, 0xc49b941b,
    0x32baf4a9, 0xeb833a4a, 0x88a3f1a5, 0x3a91ce0a,
    0x3cc27da1, 0x7112e684, 0x4a3096b1, 0x3794574c,
    0xa3c8b6f3, 0x1d213941, 0x6e0a2e00, 0x233479f1,
    0x0f4cd82f, 0x6093edd2, 0x5d7d209e, 0x464fe319,
    0xd4dcac9e, 0x0db845cb, 0xfb5e4bc3, 0xe0256ce1,
    0x09fb4ed1, 0x0914be1e, 0xa5bdb2c3, 0xc6eb57bb,
    0x30320350, 0x3f397e91, 0xa67791bc, 0x86bc0e2c,
    0xefa0a7e2, 0xe9ff7543, 0xe733612c, 0xd185897b,
    0x329e5388, 0x91dd236b, 0x2ecb0d93, 0xf4d82a3d,
    0x35b5c03f, 0xe4e606f0, 0x05b21843, 0x37b45964,
    0x5eff22f4, 0x6027f4cc, 0x77178b3c, 0xae507131,
    0x7bf7cabc, 0xf9c18d66, 0x593ade65, 0xd95ddf11,
];

/// Errors from constructing an [`HpcdcChunker`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ChunkerError {
    /// `min` is below the 48-byte window floor (`params.min >= ChunkerWindowSize`,
    /// hpcdcchunker.c:139).
    #[error("min chunk size {min} is below the window floor {window}")]
    MinBelowWindow { min: u32, window: u32 },

    /// The invariant `min <= avg <= max` (hpcdcchunker.c:140-142) is violated.
    #[error("invalid chunker sizes: require min ({min}) <= avg ({avg}) <= max ({max})")]
    InvalidSizes { min: u32, avg: u32, max: u32 },

    /// The derived `avg` exceeds [`MAX_AVG`]; above it the C discriminator
    /// expression is UB (see [`MAX_AVG`]).
    #[error("avg {avg} exceeds the discriminator ceiling {ceiling}")]
    AvgTooLarge { avg: u32, ceiling: u32 },
}

/// Which entry point's rolling-hash seed window an [`HpcdcChunker`] reproduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    /// Canonical streaming path: seed over `scope[min-48 .. min)`
    /// (`Longtail_HPCDCNextChunk`). The only mode production data used.
    Streaming,
    /// Upstream mmap/buffer path: seed over `scope[0 .. 48)`
    /// (`HPCDCChunker_NextChunkFromBuffer`). Labeled differential target only —
    /// never a production path.
    Buffer,
}

/// A single chunk boundary: absolute byte offset within the input and its
/// length in bytes. Matches the boundary-table / ffi `ChunkSpan` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSpan {
    pub offset: u64,
    pub size: u32,
}

/// A content-defined chunker: maps a byte slice to ordered chunk boundaries.
/// Implemented by [`HpcdcChunker`] (the compat-critical port) and, behind the
/// `fastcdc` feature, [`FastCdcChunker`] (benchmarking only — NOT held to
/// HPCDC's constants and with no compat gates).
pub trait Chunker {
    /// Chunk `data` into ordered, contiguous, input-covering boundaries.
    fn chunk(&self, data: &[u8]) -> Vec<ChunkSpan>;
}

impl Chunker for HpcdcChunker {
    fn chunk(&self, data: &[u8]) -> Vec<ChunkSpan> {
        HpcdcChunker::chunk(self, data)
    }
}

/// FastCDC chunker (feature `fastcdc`) — a **benchmarking alternative** to HPCDC
/// via the `fastcdc` crate (`v2020`). It implements the same [`Chunker`] trait
/// but produces DIFFERENT boundaries; it is not a compat target and reviewers
/// should not hold it to HPCDC's gear table / discriminator.
#[cfg(feature = "fastcdc")]
#[derive(Debug, Clone, Copy)]
pub struct FastCdcChunker {
    min: u32,
    avg: u32,
    max: u32,
}

#[cfg(feature = "fastcdc")]
impl FastCdcChunker {
    /// Build from explicit `(min, avg, max)` sizes.
    pub fn new(min: u32, avg: u32, max: u32) -> Self {
        FastCdcChunker { min, avg, max }
    }

    /// Build from a target chunk size using the same `(min, avg, max)`
    /// derivation as HPCDC, so benchmark comparisons use matched parameters.
    pub fn from_target(target_chunk_size: u32) -> Self {
        let window = WINDOW_SIZE as u32;
        FastCdcChunker {
            min: window.max(target_chunk_size / 8),
            avg: window.max(target_chunk_size / 2),
            max: window.max(target_chunk_size.saturating_mul(2)),
        }
    }
}

#[cfg(feature = "fastcdc")]
impl Chunker for FastCdcChunker {
    fn chunk(&self, data: &[u8]) -> Vec<ChunkSpan> {
        if data.is_empty() {
            return Vec::new();
        }
        fastcdc::v2020::FastCDC::new(
            data,
            self.min as usize,
            self.avg as usize,
            self.max as usize,
        )
        .map(|c| ChunkSpan {
            offset: c.offset as u64,
            size: c.length as u32,
        })
        .collect()
    }
}

/// A chunk boundary plus its longtail hash (the composed chunk+hash shape wanted
/// by the version-build/upload paths and every boundary/hash test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkHash {
    pub offset: u64,
    pub size: u32,
    pub hash: u64,
}

/// A configured HPCDC chunker. Cheap to construct and `Copy`; carries only the
/// derived `(min, avg, max)`, the precomputed discriminator, and the seed mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HpcdcChunker {
    min: u32,
    avg: u32,
    max: u32,
    discriminator: u32,
    seed_mode: SeedMode,
}

/// The discriminator, computed in **f64** exactly as C
/// (`HPCDCDiscriminatorFromAvg`, hpcdcchunker.c:126-129):
/// `(uint32_t)(avg / (-1.42888852e-7*avg + 1.33237515))`. The `avg` integer is
/// widened to `f64` before the division; the operator shape and constants are
/// reproduced bit-for-bit. Callers must ensure `avg <= MAX_AVG` so the quotient
/// stays in `u32` range (the `as u32` cast then equals C's truncating cast).
pub fn discriminator_from_avg(avg: u32) -> u32 {
    let avg = avg as f64;
    (avg / (-1.42888852e-7 * avg + 1.33237515)) as u32
}

impl HpcdcChunker {
    /// Derive `(min, avg, max)` from a caller-supplied `target_chunk_size`
    /// exactly as the C callers do (`docs/format-spec.md` §6):
    /// `min = max(48, target/8)`, `avg = max(48, target/2)`,
    /// `max = max(48, target*2)`. Streaming (canonical) seed mode.
    pub fn from_target(target_chunk_size: u32) -> Result<Self, ChunkerError> {
        let window = WINDOW_SIZE as u32;
        let min = window.max(target_chunk_size / 8);
        let avg = window.max(target_chunk_size / 2);
        let max = window.max(target_chunk_size.saturating_mul(2));
        Self::new(min, avg, max)
    }

    /// Construct from explicit `(min, avg, max)` in streaming (canonical) seed
    /// mode. Validates `min >= 48`, `min <= avg <= max`, and `avg <= MAX_AVG`.
    pub fn new(min: u32, avg: u32, max: u32) -> Result<Self, ChunkerError> {
        Self::with_seed_mode(min, avg, max, SeedMode::Streaming)
    }

    /// Construct in the [`SeedMode::Buffer`] variant. **Labeled differential
    /// target only** — reproduces the upstream mmap-path seed window and must not
    /// be used by production chunking. Kept `pub` so the testkit's pure-lane
    /// `*.buffer.json` golden can drive it without a native library.
    pub fn new_buffer(min: u32, avg: u32, max: u32) -> Result<Self, ChunkerError> {
        Self::with_seed_mode(min, avg, max, SeedMode::Buffer)
    }

    fn with_seed_mode(
        min: u32,
        avg: u32,
        max: u32,
        seed_mode: SeedMode,
    ) -> Result<Self, ChunkerError> {
        let window = WINDOW_SIZE as u32;
        if min < window {
            return Err(ChunkerError::MinBelowWindow { min, window });
        }
        if !(min <= avg && avg <= max) {
            return Err(ChunkerError::InvalidSizes { min, avg, max });
        }
        if avg > MAX_AVG {
            return Err(ChunkerError::AvgTooLarge {
                avg,
                ceiling: MAX_AVG,
            });
        }
        Ok(HpcdcChunker {
            min,
            avg,
            max,
            discriminator: discriminator_from_avg(avg),
            seed_mode,
        })
    }

    /// The derived `(min, avg, max)`.
    pub fn params(&self) -> (u32, u32, u32) {
        (self.min, self.avg, self.max)
    }

    /// The precomputed discriminator `d`.
    pub fn discriminator(&self) -> u32 {
        self.discriminator
    }

    /// Chunk `data` into ordered boundaries. Boundary-identical to C's streaming
    /// chunker (or, in [`SeedMode::Buffer`], to the mmap path). No per-chunk
    /// allocation beyond the output `Vec`.
    pub fn chunk(&self, data: &[u8]) -> Vec<ChunkSpan> {
        let mut spans = Vec::new();
        self.chunk_with(data, |span| spans.push(span));
        spans
    }

    /// Chunk `data`, invoking `emit` for each boundary in order. The zero-alloc
    /// core the other APIs build on.
    pub fn chunk_with(&self, data: &[u8], mut emit: impl FnMut(ChunkSpan)) {
        let n = data.len();
        let min = self.min as usize;
        let max = self.max as usize;
        let d = self.discriminator;
        // `d == 0` cannot occur for a valid chunker: avg >= 48 gives d >= 35.
        // Guard anyway so a hand-built value can never divide by zero.
        let d = if d == 0 { 1 } else { d };
        let target = d - 1;

        let mut off = 0usize;
        while off < n {
            let left = n - off;
            if left <= min {
                // Less than min-size left: emit it all as the final chunk, no
                // hashing (hpcdcchunker.c:258-263).
                emit(ChunkSpan {
                    offset: off as u64,
                    size: left as u32,
                });
                off += left; // == n; loop then exits
                continue;
            }

            let scope = &data[off..]; // length `left`
            let data_len = left.min(max); // roll upper bound (cap at max)

            // Seed the rolling hash over the 48-byte window. Streaming seeds over
            // scope[min-48 .. min); Buffer seeds over scope[0 .. 48).
            let seed_start = match self.seed_mode {
                SeedMode::Streaming => min - WINDOW_SIZE,
                SeedMode::Buffer => 0,
            };
            let mut hash: u32 = 0;
            let mut window = [0u8; WINDOW_SIZE];
            for i in 0..WINDOW_SIZE {
                let b = scope[seed_start + i];
                let rot = ((WINDOW_SIZE - i - 1) & 31) as u32;
                hash ^= GEAR[b as usize].rotate_left(rot);
                window[i] = b;
            }

            // Roll forward from pos = min up to data_len.
            let mut pos = min;
            let mut idx = 0usize;
            while pos < data_len {
                let in_b = scope[pos];
                pos += 1;
                let out_b = window[idx];
                window[idx] = in_b;
                idx += 1;
                // Evicted byte's table entry is rotated by the CONSTANT
                // ChunkerWindowSize & 31 = 16 (not the loop position).
                hash = hash.rotate_left(1)
                    ^ GEAR[out_b as usize].rotate_left(16)
                    ^ GEAR[in_b as usize];
                if hash % d == target {
                    break;
                }
                if idx == WINDOW_SIZE {
                    idx = 0;
                }
            }
            emit(ChunkSpan {
                offset: off as u64,
                size: pos as u32,
            });
            off += pos;
        }
    }

    /// Chunk `data` and hash each chunk with `hasher`, yielding the composed
    /// `(offset, size, hash)` shape. `hasher` may be a concrete type or a
    /// `&dyn Hash`.
    pub fn chunk_hashed<H: Hash + ?Sized>(&self, data: &[u8], hasher: &H) -> Vec<ChunkHash> {
        let mut out = Vec::new();
        self.chunk_with(data, |span| {
            let start = span.offset as usize;
            let end = start + span.size as usize;
            out.push(ChunkHash {
                offset: span.offset,
                size: span.size,
                hash: hasher.hash(&data[start..end]),
            });
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Golden `d` table for the four standard targets:
    /// independently computed by IEEE-f64 evaluation of the exact expression with
    /// avg = target/2. Any float drift fails here, loudly and locally.
    #[test]
    fn golden_discriminator_table() {
        // (target, avg, expected d)
        let table = [
            (1024u32, 512u32, 384u32),
            (32768, 16384, 12318),
            (131072, 65536, 49535),
            (1048576, 524288, 416942),
        ];
        for (target, avg, d) in table {
            assert_eq!(discriminator_from_avg(avg), d, "d for avg {avg}");
            let c = HpcdcChunker::from_target(target).unwrap();
            assert_eq!(c.discriminator(), d, "chunker d for target {target}");
            assert_eq!(c.params().1, avg, "avg for target {target}");
        }
    }

    #[test]
    fn params_derivation_matches_spec() {
        assert_eq!(
            HpcdcChunker::from_target(32768).unwrap().params(),
            (4096, 16384, 65536)
        );
        assert_eq!(
            HpcdcChunker::from_target(1024).unwrap().params(),
            (128, 512, 2048)
        );
        assert_eq!(HpcdcChunker::from_target(1).unwrap().params(), (48, 48, 48));
    }

    #[test]
    fn rejects_bad_params() {
        assert_eq!(
            HpcdcChunker::new(16, 32, 64).unwrap_err(),
            ChunkerError::MinBelowWindow {
                min: 16,
                window: 48
            }
        );
        assert!(matches!(
            HpcdcChunker::new(100, 50, 200).unwrap_err(),
            ChunkerError::InvalidSizes { .. }
        ));
        // avg just above the ceiling is rejected; at the ceiling it is accepted.
        assert!(HpcdcChunker::new(48, MAX_AVG, MAX_AVG).is_ok());
        assert_eq!(
            HpcdcChunker::new(48, MAX_AVG + 1, MAX_AVG + 1).unwrap_err(),
            ChunkerError::AvgTooLarge {
                avg: MAX_AVG + 1,
                ceiling: MAX_AVG
            }
        );
    }

    #[test]
    fn tiny_and_empty_inputs() {
        let c = HpcdcChunker::from_target(32768).unwrap();
        assert!(c.chunk(&[]).is_empty(), "empty input yields no chunks");
        // < min: one tail chunk covering everything.
        let data = vec![0u8; 100];
        let spans = c.chunk(&data);
        assert_eq!(
            spans,
            vec![ChunkSpan {
                offset: 0,
                size: 100
            }]
        );
    }

    #[test]
    fn spans_are_contiguous_and_cover_input() {
        let c = HpcdcChunker::from_target(1024).unwrap();
        // Deterministic pseudo-random-ish data.
        let data: Vec<u8> = (0..50_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let spans = c.chunk(&data);
        let mut off = 0u64;
        for s in &spans {
            assert_eq!(s.offset, off);
            assert!(
                s.size as usize <= c.params().2 as usize,
                "chunk exceeds max"
            );
            off += s.size as u64;
        }
        assert_eq!(off as usize, data.len(), "spans cover the whole input");
    }

    #[cfg(feature = "fastcdc")]
    #[test]
    fn fastcdc_smoke_chunks_and_covers() {
        let c = FastCdcChunker::from_target(1024);
        let data: Vec<u8> = (0..50_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        let spans = c.chunk(&data);
        assert!(!spans.is_empty());
        let mut off = 0u64;
        for s in &spans {
            assert_eq!(s.offset, off);
            off += s.size as u64;
        }
        assert_eq!(
            off as usize,
            data.len(),
            "fastcdc spans cover the whole input"
        );
        assert!(c.chunk(&[]).is_empty());
    }

    #[test]
    fn chunk_hashed_composes() {
        use crate::hash::Blake3;
        let c = HpcdcChunker::from_target(1024).unwrap();
        let data: Vec<u8> = (0..20_000u32)
            .map(|i| (i.wrapping_mul(40503) >> 16) as u8)
            .collect();
        let spans = c.chunk(&data);
        let hashed = c.chunk_hashed(&data, &Blake3);
        assert_eq!(spans.len(), hashed.len());
        for (s, h) in spans.iter().zip(&hashed) {
            assert_eq!(s.offset, h.offset);
            assert_eq!(s.size, h.size);
            let start = s.offset as usize;
            let end = start + s.size as usize;
            assert_eq!(h.hash, Blake3.hash(&data[start..end]));
        }
    }
}
