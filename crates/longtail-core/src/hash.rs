//! Longtail hash layer (`docs/format-spec.md` §5).
//!
//! Longtail stores a 64-bit hash for every chunk, asset (content), path, and
//! block. All hash algorithms produce an **8-byte** digest whose bytes are
//! interpreted **little-endian** into the stored `u64` — equivalently, the
//! on-disk index bytes for a hash field are exactly the first 8 digest bytes
//! (`docs/format-spec.md` §5, "Digest-bytes → u64 mapping"). Verified against
//! `lib/blake2/longtail_blake2.c`, `lib/blake3/longtail_blake3.c`, and
//! `lib/meowhash/longtail_meowhash.c`.
//!
//! Three algorithm IDs exist (4 ASCII chars packed big-endian into a `u32`):
//!
//! | Hash    | ID           | ASCII    | Support                                  |
//! |---------|--------------|----------|------------------------------------------|
//! | BLAKE3  | `0x626c6b33` | `"blk3"` | full (production / golongtail default)   |
//! | BLAKE2s | `0x626c6b32` | `"blk2"` | full (read-capable priority)             |
//! | Meow    | `0x6d656f77` | `"meow"` | recognized, **verification unsupported** |
//!
//! Meow is *recognized* (indexes carrying it parse fine) but
//! its digest cannot be reproduced in the pure-Rust port (it needs x86 AES-NI
//! intrinsics, i.e. `unsafe`, and this crate is `#![forbid(unsafe_code)]`), so
//! [`hasher`] returns a typed [`HashError::UnsupportedHash`] per the plan's
//! parse-without-verify decision.

use blake2::Blake2sVar;
use blake2::digest::{Update, VariableOutput};
use thiserror::Error;

/// BLAKE3 hash ID — `"blk3"`, `(b'b'<<24)|(b'l'<<16)|(b'k'<<8)|b'3'`.
pub const BLAKE3_ID: u32 = 0x626c_6b33;
/// BLAKE2s hash ID — `"blk2"`.
pub const BLAKE2S_ID: u32 = 0x626c_6b32;
/// Meow hash ID — `"meow"`.
pub const MEOW_ID: u32 = 0x6d65_6f77;

/// Errors from the hash registry ([`hasher`]).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum HashError {
    /// The ID names a hash the pure-Rust port recognizes but cannot compute.
    /// Currently only Meow (`0x6d656f77`): its digest needs x86 AES-NI
    /// intrinsics, so the port parses meow-hashed indexes but cannot verify or
    /// produce meow hashes.
    #[error("meow hash verification unsupported in the pure-Rust port (id {id:#010x})")]
    UnsupportedHash { id: u32 },

    /// The ID does not name any known longtail hash.
    #[error("unknown hash identifier {id:#010x}")]
    UnknownHashId { id: u32 },
}

/// A longtail hash algorithm: maps a byte buffer to the stored 64-bit hash.
///
/// The mapping is exactly what `Longtail_Hash_HashBuffer` computes for the
/// algorithm — the little-endian interpretation of the first 8 digest bytes.
pub trait Hash: std::fmt::Debug {
    /// The 4-ASCII-char algorithm identifier stored in indexes/blocks.
    fn id(&self) -> u32;
    /// The 64-bit longtail hash of `data`.
    fn hash(&self, data: &[u8]) -> u64;
}

/// BLAKE3, taking the **first 8 bytes** of the standard 32-byte digest
/// (identical to the first 8 XOF bytes; `longtail_blake3.c:76,:100`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Blake3;

impl Hash for Blake3 {
    fn id(&self) -> u32 {
        BLAKE3_ID
    }
    fn hash(&self, data: &[u8]) -> u64 {
        blake3_hash(data)
    }
}

/// BLAKE2s parameterized to an **8-byte digest** (`blake2s_init(state, 8)` /
/// `blake2s(out, 8, data, len, 0, 0)`, `longtail_blake2.c:43,111`). The digest
/// length is part of BLAKE2's parameter block, so this is NOT the same as
/// truncating a 32-byte BLAKE2s digest.
#[derive(Debug, Clone, Copy, Default)]
pub struct Blake2s;

impl Hash for Blake2s {
    fn id(&self) -> u32 {
        BLAKE2S_ID
    }
    fn hash(&self, data: &[u8]) -> u64 {
        blake2s_hash(data)
    }
}

/// BLAKE3 longtail hash: little-endian `u64` of the first 8 digest bytes.
pub fn blake3_hash(data: &[u8]) -> u64 {
    let digest = blake3::hash(data);
    let bytes = digest.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// BLAKE2s-with-8-byte-digest longtail hash: little-endian `u64` of the 8
/// output bytes. `Blake2sVar::new(8)` sets the digest length in BLAKE2's
/// parameter block, matching `blake2s(out, 8, ...)` in C.
pub fn blake2s_hash(data: &[u8]) -> u64 {
    let mut hasher = Blake2sVar::new(8).expect("8 is a valid BLAKE2s digest length");
    hasher.update(data);
    let mut out = [0u8; 8];
    hasher
        .finalize_variable(&mut out)
        .expect("8-byte output buffer matches configured digest length");
    u64::from_le_bytes(out)
}

/// Resolve a hash ID to a working hasher (three-state, mirroring the C hash
/// registry): a supported algorithm, a recognized-but-unsupported error
/// ([`HashError::UnsupportedHash`], meow), or [`HashError::UnknownHashId`].
pub fn hasher(id: u32) -> Result<Box<dyn Hash>, HashError> {
    match id {
        BLAKE3_ID => Ok(Box::new(Blake3)),
        BLAKE2S_ID => Ok(Box::new(Blake2s)),
        MEOW_ID => Err(HashError::UnsupportedHash { id }),
        other => Err(HashError::UnknownHashId { id: other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known-answer vectors: the little-endian first-8-byte longtail hash for a
    // few fixed inputs. These constants are cross-checked against the reference
    // C library by the testkit's differential tests
    // (`hash_differential.rs::hash_kats_match_c`) — a red differential there is
    // the real gate; this pure test freezes the values so a run without the C
    // library has
    // hash coverage without the native library.
    const BLAKE3_EMPTY: u64 = 0xa6a1f9f5b94913af;
    const BLAKE3_LONGTAIL: u64 = 0xefb32f524ca47d58;
    const BLAKE2S_EMPTY: u64 = 0x9cda80dd788b2aef;
    const BLAKE2S_LONGTAIL: u64 = 0x1a838bcb644c5a58;

    #[test]
    fn ids_are_ascii_packed() {
        assert_eq!(BLAKE3_ID, u32::from_be_bytes(*b"blk3"));
        assert_eq!(BLAKE2S_ID, u32::from_be_bytes(*b"blk2"));
        assert_eq!(MEOW_ID, u32::from_be_bytes(*b"meow"));
        assert_eq!(Blake3.id(), BLAKE3_ID);
        assert_eq!(Blake2s.id(), BLAKE2S_ID);
    }

    #[test]
    fn blake3_known_answers() {
        assert_eq!(blake3_hash(b""), BLAKE3_EMPTY);
        assert_eq!(blake3_hash(b"longtail"), BLAKE3_LONGTAIL);
        // Trait and free function agree.
        assert_eq!(Blake3.hash(b"longtail"), BLAKE3_LONGTAIL);
    }

    #[test]
    fn blake2s_known_answers() {
        assert_eq!(blake2s_hash(b""), BLAKE2S_EMPTY);
        assert_eq!(blake2s_hash(b"longtail"), BLAKE2S_LONGTAIL);
        assert_eq!(Blake2s.hash(b"longtail"), BLAKE2S_LONGTAIL);
    }

    #[test]
    fn blake2s_is_parameterized_not_truncated() {
        // Sanity: the 8-byte-parameterized digest must differ from truncating a
        // full 32-byte BLAKE2s digest (different parameter block).
        use blake2::Blake2s256;
        use blake2::Digest;
        let full = Blake2s256::digest(b"longtail");
        let truncated = u64::from_le_bytes([
            full[0], full[1], full[2], full[3], full[4], full[5], full[6], full[7],
        ]);
        assert_ne!(
            blake2s_hash(b"longtail"),
            truncated,
            "8-byte-param digest must not equal a truncated 32-byte digest"
        );
    }

    #[test]
    fn registry_three_state() {
        assert_eq!(hasher(BLAKE3_ID).unwrap().id(), BLAKE3_ID);
        assert_eq!(hasher(BLAKE2S_ID).unwrap().id(), BLAKE2S_ID);
        assert_eq!(
            hasher(MEOW_ID).unwrap_err(),
            HashError::UnsupportedHash { id: MEOW_ID }
        );
        assert_eq!(
            hasher(0xdead_beef).unwrap_err(),
            HashError::UnknownHashId { id: 0xdead_beef }
        );
    }
}
