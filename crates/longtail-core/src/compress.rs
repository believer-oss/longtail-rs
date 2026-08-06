//! Compression layer: codec registry + the compressed-block payload framing
//! codec (`docs/format-spec.md` §4, §3 "Payload").
//!
//! # Codecs
//!
//! - **LZ4** (`lz4_flex`, pure Rust): raw LZ4 *block* format via
//!   `lz4_flex::block::{compress, decompress}` — matching C's
//!   `LZ4_compress_fast(acceleration=1)` / `LZ4_decompress_safe`
//!   (`lib/lz4/longtail_lz4.c:69,:94`). The `*_prepend_size` variants are NOT
//!   used — they add a 4-byte size prefix absent from longtail's framing.
//! - **Zstd** (`zstd` crate = libzstd): the only C in `longtail-core` (user
//!   decision 2026-07-05); the pluggable [`Compressor`] trait keeps a pure-Rust
//!   swap cheap later.
//! - **Brotli** (`brotli` crate, pure Rust).
//!
//! # Registry dispatch (mirrors C exactly, `docs/format-spec.md` §4)
//!
//! LZ4 is matched by exact ID (`lib/lz4/longtail_lz4.c:14`). Zstd and brotli
//! variants are dispatched by masking off the low byte and matching the
//! 3-char-plus-NUL family prefix (`(id & 0xffffff00)`,
//! `lib/zstd/longtail_zstd.c:32`, `lib/brotli/longtail_brotli.c:41`), with the
//! low byte selecting the variant. **Every ID in the §4 table decodes** —
//! including the ones golongtail never writes (`zstd_high`/`zstd_low`, brotli
//! min/max/text). Because C dispatches by family, any id sharing a family
//! prefix decodes; an unlisted low byte falls back to that family's default
//! encode params (decode never reads them — the codec reads the frame).
//!
//! # Encode levels / params — cited from the C source
//!
//! **Zstd** (`lib/zstd/longtail_zstd.c:11-14,45-59`; the `int` levels are
//! `ZSTD_CLEVEL_DEFAULT = 3`, `ZSTD_MAX_CLEVEL = 22`):
//!
//! | ID (`m_Tag`)   | variant   | level | source                                      |
//! |----------------|-----------|-------|---------------------------------------------|
//! | `0x7A746431`   | `zstd_min`| 0     | `LONGTAIL_ZSTD_MIN_COMPRESSION_LEVEL = 0`   |
//! | `0x7A746432`   | default   | 3     | `ZSTD_CLEVEL_DEFAULT`                        |
//! | `0x7A746433`   | `zstd_max`| 22    | `ZSTD_MAX_CLEVEL`                            |
//! | `0x7A746434`   | `zstd_high`| 8    | `LONGTAIL_ZSTD_HIGH_COMPRESSION_LEVEL = 8`  |
//! | `0x7A746435`   | `zstd_low`| 22    | shadowed-macro quirk, see below             |
//!
//! Level 0 is passed through verbatim: libzstd treats level 0 as "use the
//! default" — that is what C does too (`ZSTD_compressCCtx(..., 0)`), so it is
//! **not** "fixed" here. `zstd_low`'s C level is the shadowed-macro quirk: the
//! `const int LONGTAIL_ZSTD_LOW_COMPRESSION_TYPE = 2` (longtail_zstd.c:12) is
//! shadowed from line 22 onward by the `#define ...= COMPRESSION_TYPE + '5'`, so
//! `SettingsIDToCompressionSetting`'s `case`/`return` (longtail_zstd.c:55-56)
//! both expand to the *type ID itself* (`0x7A746435`) — a nonsensical level that
//! libzstd clamps to `ZSTD_maxCLevel()`. We therefore **encode `zstd_low` at
//! level 22**. (Decode never reads the level, so every zstd ID decodes
//! identically regardless — `docs/format-spec.md` §4.)
//!
//! **Brotli** (`lib/brotli/longtail_brotli.c:17-22`; the named constants resolve
//! to `BROTLI_MIN_WINDOW_BITS = 10`, `BROTLI_DEFAULT_WINDOW = 22`,
//! `BROTLI_MAX_WINDOW_BITS = 24`, `BROTLI_MIN_QUALITY = 0`,
//! `BROTLI_DEFAULT_QUALITY = BROTLI_MAX_QUALITY = 11`; encode.h:24-63):
//!
//! | ID (`m_Tag`) | variant        | mode    | lgwin | quality |
//! |--------------|----------------|---------|-------|---------|
//! | `0x62746C30` | generic min    | GENERIC | 10    | 0       |
//! | `0x62746C31` | generic default| GENERIC | 22    | 11      |
//! | `0x62746C32` | generic max    | GENERIC | 24    | 11      |
//! | `0x62746C61` | text min       | TEXT    | 10    | 0       |
//! | `0x62746C62` | text default   | TEXT    | 22    | 11      |
//! | `0x62746C63` | text max       | TEXT    | 24    | 11      |
//!
//! Note generic-default and generic-max share quality 11 and differ only by
//! window bits (22 vs 24); text variants use `BROTLI_MODE_TEXT`.
//!
//! **Encode byte-parity is a deliberate NON-gate:** block identity is the hash
//! of the chunk-hash array, not the compressed bytes.
//! The encode gate is: C decodes every Rust-compressed payload back to identical
//! plaintext, for every ID (proved in the testkit `differential` lane).

use std::io::Cursor;

use brotli::enc::BrotliEncoderParams;
use brotli::enc::backward_references::BrotliEncoderMode;
use thiserror::Error;

// ---- IDs (docs/format-spec.md §4) ----------------------------------------

/// LZ4 block codec — `"lz42"` (single ID, no variants).
pub const LZ4_ID: u32 = 0x6c7a_3432;
/// Zstd family prefix (`"ztd\0"`); the low byte selects the level variant.
pub const ZSTD_FAMILY: u32 = 0x7a74_6400;
/// Brotli family prefix (`"btl\0"`); the low byte selects the variant.
pub const BROTLI_FAMILY: u32 = 0x6274_6c00;
/// Mask isolating the 3-char family prefix (drops the low variant byte).
pub const FAMILY_MASK: u32 = 0xffff_ff00;

/// Errors from the compression layer (codec registry + payload framing).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum CompressError {
    /// The tag/ID does not name any known codec (and is not `0` = raw).
    #[error("unknown compression identifier {id:#010x}")]
    UnknownCompressionId { id: u32 },

    /// A compressed (`tag != 0`) payload is shorter than its 8-byte
    /// `[uncompressed u32][compressed u32]` frame header.
    #[error("compressed payload frame header truncated: {len} bytes (need 8)")]
    TruncatedFrameHeader { len: usize },

    /// The frame's declared `compressed_size` does not equal the number of bytes
    /// following the header. **Stricter than C** (whose decoder ignores trailing
    /// bytes, passing only `compressed_size` bytes to the codec) — consistent
    /// with the format layer's trailing-bytes rejection policy so round-trips are sound.
    #[error("frame compressed_size {declared} != payload body length {actual}")]
    CompressedSizeMismatch { declared: usize, actual: usize },

    /// The frame's declared plaintext size exceeds what the caller knows the
    /// block can hold. Rejected before any memory is committed, so a lie about
    /// the size costs nothing to refuse.
    #[error("declared uncompressed_size {declared} exceeds the block's {max} bytes")]
    DeclaredSizeTooLarge { declared: usize, max: usize },

    /// The codec produced a different number of bytes than the frame's declared
    /// `uncompressed_size`. This is C's one integrity check (EBADF-equivalent,
    /// `compressblockstore.c:328`).
    #[error("decoded length {actual} != declared uncompressed_size {expected}")]
    DecodedLengthMismatch { expected: usize, actual: usize },

    /// The underlying codec failed to compress.
    #[error("codec {id:#010x} compression failed")]
    Compress { id: u32 },

    /// The underlying codec failed to decompress (malformed compressed bytes).
    #[error("codec {id:#010x} decompression failed")]
    Decompress { id: u32 },
}

/// A pluggable compression codec. Decode is the compatibility gate; encode
/// byte-parity is a deliberate non-gate (block identity = hash of the chunk-hash
/// array, not compressed bytes).
pub trait Compressor: std::fmt::Debug {
    /// The compression ID this codec was resolved for.
    fn id(&self) -> u32;
    /// Compress `data` into a fresh buffer.
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressError>;
    /// Decompress `data`, whose plaintext is known to be `uncompressed_size`
    /// bytes (from the frame header).
    fn decompress(&self, data: &[u8], uncompressed_size: usize) -> Result<Vec<u8>, CompressError>;
}

// ---- LZ4 -----------------------------------------------------------------

#[derive(Debug)]
struct Lz4;

impl Compressor for Lz4 {
    fn id(&self) -> u32 {
        LZ4_ID
    }
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressError> {
        // Raw LZ4 block, no size prefix (matches LZ4_compress_fast framing).
        Ok(lz4_flex::block::compress(data))
    }
    fn decompress(&self, data: &[u8], uncompressed_size: usize) -> Result<Vec<u8>, CompressError> {
        lz4_flex::block::decompress(data, uncompressed_size)
            .map_err(|_| CompressError::Decompress { id: LZ4_ID })
    }
}

// ---- Zstd ----------------------------------------------------------------

#[derive(Debug)]
struct Zstd {
    id: u32,
    level: i32,
}

/// Map a zstd-family ID's low byte to the C encode level (see module docs).
/// Unlisted low bytes fall back to `0` (C's `default:` case), which libzstd
/// treats as its default level — decode never reads the level regardless.
fn zstd_level(id: u32) -> i32 {
    match id {
        0x7a74_6431 => 0,  // zstd_min  -> LONGTAIL_ZSTD_MIN_COMPRESSION_LEVEL
        0x7a74_6432 => 3,  // default   -> ZSTD_CLEVEL_DEFAULT
        0x7a74_6433 => 22, // zstd_max  -> ZSTD_MAX_CLEVEL
        0x7a74_6434 => 8,  // zstd_high -> LONGTAIL_ZSTD_HIGH_COMPRESSION_LEVEL
        0x7a74_6435 => 22, // zstd_low  -> shadowed-macro quirk clamps to max
        _ => 0,
    }
}

impl Compressor for Zstd {
    fn id(&self) -> u32 {
        self.id
    }
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressError> {
        zstd::bulk::compress(data, self.level).map_err(|_| CompressError::Compress { id: self.id })
    }
    fn decompress(&self, data: &[u8], uncompressed_size: usize) -> Result<Vec<u8>, CompressError> {
        zstd::bulk::decompress(data, uncompressed_size)
            .map_err(|_| CompressError::Decompress { id: self.id })
    }
}

// ---- Brotli --------------------------------------------------------------

#[derive(Debug)]
struct Brotli {
    id: u32,
    quality: i32,
    lgwin: i32,
    text: bool,
}

/// Map a brotli-family ID to its encode params (see module docs). Unlisted low
/// bytes fall back to generic-default params; decode ignores params entirely.
fn brotli_params(id: u32) -> (i32, i32, bool) {
    // (quality, lgwin, text)
    match id {
        0x6274_6c30 => (0, 10, false),  // generic min
        0x6274_6c31 => (11, 22, false), // generic default
        0x6274_6c32 => (11, 24, false), // generic max
        0x6274_6c61 => (0, 10, true),   // text min
        0x6274_6c62 => (11, 22, true),  // text default
        0x6274_6c63 => (11, 24, true),  // text max
        _ => (11, 22, false),
    }
}

impl Compressor for Brotli {
    fn id(&self) -> u32 {
        self.id
    }
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>, CompressError> {
        let mut params = BrotliEncoderParams {
            quality: self.quality,
            lgwin: self.lgwin,
            ..Default::default()
        };
        params.mode = if self.text {
            BrotliEncoderMode::BROTLI_MODE_TEXT
        } else {
            BrotliEncoderMode::BROTLI_MODE_GENERIC
        };
        let mut out = Vec::new();
        let mut reader = data;
        brotli::BrotliCompress(&mut reader, &mut out, &params)
            .map_err(|_| CompressError::Compress { id: self.id })?;
        Ok(out)
    }
    fn decompress(&self, data: &[u8], uncompressed_size: usize) -> Result<Vec<u8>, CompressError> {
        use std::io::Read;
        // Decompress through a `Read` adapter with a hard ceiling rather than
        // `BrotliDecompress` into a bare `Vec`. Brotli's window makes a ~1000:1
        // expansion trivial, so a stream is free to decode to far more than the
        // frame header claims; an unbounded `Vec` on a codec worker turns that
        // into an OOM rather than an error. Reading one byte past the declared
        // size is what makes the lie detectable instead of silently truncated.
        let ceiling = uncompressed_size.saturating_add(1);
        let mut out = Vec::with_capacity(initial_capacity(uncompressed_size));
        brotli::Decompressor::new(Cursor::new(data), BROTLI_READ_BUFFER)
            .take(ceiling as u64)
            .read_to_end(&mut out)
            .map_err(|_| CompressError::Decompress { id: self.id })?;
        if out.len() > uncompressed_size {
            return Err(CompressError::DecodedLengthMismatch {
                expected: uncompressed_size,
                actual: out.len(),
            });
        }
        Ok(out)
    }
}

/// Read-buffer size for the brotli streaming decoder.
const BROTLI_READ_BUFFER: usize = 8192;

/// How much to reserve up front for a decode of `declared` bytes.
///
/// `declared` comes from the frame header, which is attacker-controlled, so
/// reserving it outright hands over an allocation of any size — and in Rust an
/// allocation failure aborts the process rather than returning an error. Reserve
/// a bounded amount and let the buffer grow into the real output, which the
/// caller's own ceiling already limits.
fn initial_capacity(declared: usize) -> usize {
    const MAX_RESERVE: usize = 1 << 20; // 1 MiB
    declared.min(MAX_RESERVE)
}

// ---- Registry ------------------------------------------------------------

/// Resolve a compression ID to a codec, mirroring C's registry dispatch. `0`
/// (no compression) is not a codec here — the framing codec
/// ([`decode_block_payload`]) handles `tag == 0` directly; calling this with `0`
/// returns [`CompressError::UnknownCompressionId`].
pub fn compressor_for(id: u32) -> Result<Box<dyn Compressor>, CompressError> {
    if id == LZ4_ID {
        Ok(Box::new(Lz4))
    } else if id & FAMILY_MASK == ZSTD_FAMILY {
        Ok(Box::new(Zstd {
            id,
            level: zstd_level(id),
        }))
    } else if id & FAMILY_MASK == BROTLI_FAMILY {
        let (quality, lgwin, text) = brotli_params(id);
        Ok(Box::new(Brotli {
            id,
            quality,
            lgwin,
            text,
        }))
    } else {
        Err(CompressError::UnknownCompressionId { id })
    }
}

// ---- Payload framing codec (docs/format-spec.md §3 "Payload") -------------

/// Bytes of the compressed-payload frame header (`[uncompressed u32][compressed u32]`).
const FRAME_HEADER_SIZE: usize = 8;

/// Decode a stored-block payload into the raw concatenated chunk bytes.
///
/// - `tag == 0` (uncompressed): the payload is already the raw chunk bytes;
///   returned as-is (`docs/format-spec.md` §3).
/// - `tag != 0` (compressed): the payload is
///   `[uncompressed_size u32][compressed_size u32][compressed bytes]`
///   (`compressblockstore.c:135-136`). Validates (a) the header is present,
///   (b) `compressed_size` equals the body length exactly (stricter than C —
///   see [`CompressError::CompressedSizeMismatch`]), then decodes and validates
///   (c) the decoded length equals `uncompressed_size` (C's only integrity
///   check, `compressblockstore.c:328`).
///
/// Never panics on malformed input — every failure is a typed [`CompressError`].
pub fn decode_block_payload(
    tag: u32,
    payload: &[u8],
    max_uncompressed: usize,
) -> Result<Vec<u8>, CompressError> {
    if tag == 0 {
        return Ok(payload.to_vec());
    }
    // Resolve the codec first, mirroring C's DecompressBlock, which calls
    // GetCompressionAPI before touching the frame header (compressblockstore.c:287).
    let codec = compressor_for(tag)?;
    if payload.len() < FRAME_HEADER_SIZE {
        return Err(CompressError::TruncatedFrameHeader { len: payload.len() });
    }
    let uncompressed_size =
        u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    // Checked before the codec runs, so a lie costs nothing: `uncompressed_size`
    // is attacker-controlled and is otherwise handed straight to a codec as an
    // allocation size. The caller knows the real bound — a block's plaintext is
    // exactly the chunks its index claims — so a frame declaring more than that
    // is malformed regardless of what it would decode to.
    if uncompressed_size > max_uncompressed {
        return Err(CompressError::DeclaredSizeTooLarge {
            declared: uncompressed_size,
            max: max_uncompressed,
        });
    }
    let compressed_size =
        u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
    let body = &payload[FRAME_HEADER_SIZE..];
    if body.len() != compressed_size {
        return Err(CompressError::CompressedSizeMismatch {
            declared: compressed_size,
            actual: body.len(),
        });
    }
    let decoded = codec.decompress(body, uncompressed_size)?;
    if decoded.len() != uncompressed_size {
        return Err(CompressError::DecodedLengthMismatch {
            expected: uncompressed_size,
            actual: decoded.len(),
        });
    }
    Ok(decoded)
}

/// Encode raw concatenated chunk bytes into a stored-block payload for `tag`.
///
/// - `tag == 0`: returns `raw` unchanged (no header).
/// - `tag != 0`: compresses `raw` and prepends the
///   `[uncompressed_size u32][compressed_size u32]` header
///   (`compressblockstore.c:135-136`).
///
/// The output bytes are **not** required to match C's byte-for-byte (encode
/// parity is a deliberate non-gate); the gate is that C can decode this back to
/// `raw` (proved in the differential lane).
pub fn encode_block_payload(tag: u32, raw: &[u8]) -> Result<Vec<u8>, CompressError> {
    if tag == 0 {
        return Ok(raw.to_vec());
    }
    let codec = compressor_for(tag)?;
    let compressed = codec.compress(raw)?;
    let mut out = Vec::with_capacity(FRAME_HEADER_SIZE + compressed.len());
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

/// Convenience: decode a parsed [`crate::StoredBlock`]'s payload into the raw
/// concatenated chunk bytes, using the tag from its block index. The result has
/// length `Σ chunk_sizes` (verified by [`decode_block_payload`] against the
/// frame's `uncompressed_size` for compressed blocks).
pub fn decode_stored_block(block: &crate::StoredBlock) -> Result<Vec<u8>, CompressError> {
    let max = block
        .block_index
        .uncompressed_len()
        .try_into()
        .unwrap_or(usize::MAX);
    decode_block_payload(block.block_index.tag, &block.payload, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    // zstd is the only FFI codec (libzstd). Under miri, foreign calls are
    // unsupported, so the miri lane exercises the pure codecs (lz4_flex, brotli)
    // + the registry/framing logic and excludes zstd. The non-miri lane covers
    // every ID.
    const PURE_IDS: &[u32] = &[
        LZ4_ID,
        0x6274_6c30,
        0x6274_6c31,
        0x6274_6c32,
        0x6274_6c61,
        0x6274_6c62,
        0x6274_6c63,
    ];
    const ZSTD_IDS: &[u32] = &[
        0x7a74_6431,
        0x7a74_6432,
        0x7a74_6433,
        0x7a74_6434,
        0x7a74_6435,
    ];
    // Under miri, keep only the fast codecs: lz4 + the quality-0 brotli variants.
    // brotli quality 11 (the default/max variants) is far too slow interpreted,
    // and it exercises the same encode/decode code path as quality 0. zstd is FFI
    // (excluded). The non-miri and differential lanes cover every ID at full params.
    const MIRI_IDS: &[u32] = &[LZ4_ID, 0x6274_6c30, 0x6274_6c61];

    fn test_ids() -> Vec<u32> {
        if cfg!(miri) {
            return MIRI_IDS.to_vec();
        }
        let mut v = PURE_IDS.to_vec();
        v.extend_from_slice(ZSTD_IDS);
        v
    }

    fn sample() -> Vec<u8> {
        // Keep the input tiny under miri (interpreted codecs are slow).
        let reps = if cfg!(miri) { 2 } else { 64 };
        b"the quick brown fox jumps over the lazy dog. ".repeat(reps)
    }

    #[test]
    fn family_constants_are_ascii() {
        assert_eq!(LZ4_ID, u32::from_be_bytes(*b"lz42"));
        assert_eq!(ZSTD_FAMILY, u32::from_be_bytes([b'z', b't', b'd', 0]));
        assert_eq!(BROTLI_FAMILY, u32::from_be_bytes([b'b', b't', b'l', 0]));
    }

    #[test]
    fn every_id_round_trips_pure() {
        let data = sample();
        for &id in &test_ids() {
            let codec = compressor_for(id).unwrap();
            let c = codec.compress(&data).unwrap();
            let d = codec.decompress(&c, data.len()).unwrap();
            assert_eq!(d, data, "codec {id:#010x} round-trip");
        }
    }

    #[test]
    fn unknown_id_rejected() {
        assert_eq!(
            compressor_for(0xdead_beef).unwrap_err(),
            CompressError::UnknownCompressionId { id: 0xdead_beef }
        );
        // 0 is not a codec (framing handles raw).
        assert!(compressor_for(0).is_err());
    }

    #[test]
    fn framing_round_trip_all_ids_and_raw() {
        let data = sample();
        for &tag in std::iter::once(&0u32).chain(test_ids().iter()) {
            let framed = encode_block_payload(tag, &data).unwrap();
            let back = decode_block_payload(tag, &framed, data.len()).unwrap();
            assert_eq!(back, data, "framing round-trip tag {tag:#010x}");
        }
    }

    #[test]
    fn framing_raw_is_passthrough() {
        let data = sample();
        let framed = encode_block_payload(0, &data).unwrap();
        assert_eq!(framed, data, "raw payload has no header");
    }

    #[test]
    fn zstd_levels_match_citations() {
        assert_eq!(zstd_level(0x7a74_6431), 0);
        assert_eq!(zstd_level(0x7a74_6432), 3);
        assert_eq!(zstd_level(0x7a74_6433), 22);
        assert_eq!(zstd_level(0x7a74_6434), 8);
        assert_eq!(zstd_level(0x7a74_6435), 22); // shadowed-macro quirk
    }
}
