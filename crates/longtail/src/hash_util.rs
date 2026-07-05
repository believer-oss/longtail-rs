//! Hasher construction returning a `Send + Sync` trait object (for rayon).
//!
//! `longtail_core::hash::hasher` returns `Box<dyn Hash>` (not `Sync`); the
//! parallel chunk pipeline needs a `Sync` hasher. Only blake3/blake2s are
//! supported (both ZSTs, trivially `Send + Sync`); meow / unknown ids error.

use longtail_core::hash::{BLAKE2S_ID, BLAKE3_ID, HashError, MEOW_ID};
use longtail_core::{Blake2s, Blake3, Hash};

/// A hasher usable across rayon worker threads.
pub type SyncHasher = Box<dyn Hash + Send + Sync>;

/// Resolve a hash identifier to a `Send + Sync` hasher (three-state, mirroring
/// `longtail_core::hash::hasher`).
pub fn make_hasher(id: u32) -> Result<SyncHasher, HashError> {
    match id {
        BLAKE3_ID => Ok(Box::new(Blake3)),
        BLAKE2S_ID => Ok(Box::new(Blake2s)),
        MEOW_ID => Err(HashError::UnsupportedHash { id }),
        other => Err(HashError::UnknownHashId { id: other }),
    }
}
