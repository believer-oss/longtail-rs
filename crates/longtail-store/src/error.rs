//! `StoreError` — the store-layer error tree.
//!
//! Follows Stage 3's sibling-per-crate error pattern (`rust-port-4.md`
//! Preconditions): a single `StoreError` for `longtail-store`, with source
//! chaining where real I/O is involved. [`FormatError`] (Stage 2) and
//! [`CompressError`] (Stage 3) are wrapped rather than flattened, so the caller
//! keeps the precise codec/format diagnosis.

use longtail_core::{CompressError, FormatError};

/// Errors surfaced by the blob layer, store-index sync, block stores, and URI
/// dispatch.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The requested blob / block object does not exist. This is the retry
    /// short-circuit signal (`longtaillib.IsNotExist`): reads never retry a
    /// not-found, and `get_existing_content` treats it as an empty store.
    #[error("not found: {0}")]
    NotFound(String),

    /// The block's serialized hash did not match the path it was read from
    /// (`BadFormatErr`, remotestore.go:238-242) — a corrupt or misplaced block.
    #[error("bad format: {0}")]
    BadFormat(String),

    /// A conditional (generation-locked) write/delete was attempted on a backend
    /// whose client reports `supports_locking() == false`.
    #[error("locking not supported by {0}")]
    LockingNotSupported(String),

    /// A write/delete lost its optimistic-locking CAS (generation changed under
    /// it). Distinct from an error: the caller retries the read-merge-write loop.
    #[error("generation lock mismatch for {0}")]
    GenerationMismatch(String),

    /// The block store is read-only (`AccessViolationErr`) and a put/prune was
    /// attempted.
    #[error("access violation: store is read-only")]
    AccessViolation,

    /// A feature intentionally not carried into the pure-Rust port: `gs://`
    /// blob stores (planning §6), Azure, or `prune_blocks` (Stage 7).
    #[error("not supported: {0}")]
    NotSupported(String),

    /// A URI could not be dispatched to a backend.
    #[error("invalid store uri `{uri}`: {reason}")]
    InvalidUri { uri: String, reason: String },

    /// The index-owner actor task has stopped (its channel closed) — the store
    /// was closed or the task panicked.
    #[error("block store worker is gone")]
    WorkerGone,

    /// A `.lvi`/`.lsi`/`.lsb` codec error from `longtail-core`.
    #[error("format error")]
    Format(#[from] FormatError),

    /// A compression/decompression error from `longtail-core`.
    #[error("compression error")]
    Compress(#[from] CompressError),

    /// A filesystem I/O error (fs blob backend).
    #[error("io error: {context}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    /// A backend-specific error (S3 SDK, etc.) that has no more specific variant.
    #[error("backend error: {0}")]
    Backend(String),
}

impl StoreError {
    /// True if this is a not-found signal — the read retry ladder and store-index
    /// scan short-circuit on it (`longtaillib.IsNotExist`).
    pub fn is_not_found(&self) -> bool {
        matches!(self, StoreError::NotFound(_))
    }

    /// Helper to attach filesystem context to an `io::Error`.
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> StoreError {
        StoreError::Io {
            context: context.into(),
            source,
        }
    }
}
