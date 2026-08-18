//! `LongtailError` — the facade-level error tree.
//!
//! Wraps the sibling per-crate enums (`FormatError`/`HashError`/`CompressError`/
//! `ChunkerError`/`StoreError` + the core additions) with `#[from]` and
//! source chains, so the launcher gets structured causes rather than strings.

use longtail_core::{
    ChunkerError, CompressError, FormatError, HashError, MergeVersionError, ValidateError,
};
use longtail_store::StoreError;

/// The unified error surfaced by the download-path facade API.
///
/// The top-level `Display` of each variant is deliberately a *category*
/// (`"store error"`, `"format error"`, …); the actionable detail hangs off the
/// `#[source]` chain. A consumer that renders only `format!("{e}")` therefore
/// loses the cause — render [`LongtailError::full_chain`] (or walk
/// [`std::error::Error::source`] yourself) to surface it.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LongtailError {
    /// A `.lvi`/`.lsi`/`.lsb` codec error.
    #[error("format error")]
    Format(#[from] FormatError),

    /// A hash-algorithm error (unknown / unsupported id).
    #[error("hash error")]
    Hash(#[from] HashError),

    /// A compression/decompression error.
    #[error("compression error")]
    Compress(#[from] CompressError),

    /// A chunker-construction error (bad target chunk size).
    #[error("chunker error")]
    Chunker(#[from] ChunkerError),

    /// A version-index merge error (multi-source downsync / multi-config get).
    #[error("version merge error")]
    Merge(#[from] MergeVersionError),

    /// A store-covers-version validation failure (`validate-version`).
    #[error("store does not cover version")]
    Validate(#[from] ValidateError),

    /// A block/blob store error.
    #[error("store error")]
    Store(#[from] StoreError),

    /// The `--validate` post-downsync target rescan disagreed with the source
    /// version index.
    #[error("downsync validation failed: {0}")]
    ValidationMismatch(String),

    /// The operation was cancelled via the caller's `CancellationToken`. The
    /// target is left resumable (a follow-up downsync completes and matches).
    #[error("operation cancelled")]
    Cancelled,

    /// Legacy `--use-legacy-write` was requested; the pure-Rust port implements
    /// only `ChangeVersion2`.
    #[error("legacy write path (--use-legacy-write) is not supported in the pure-Rust port")]
    LegacyWriteUnsupported,

    /// A get-config JSON was missing a required key or was malformed.
    #[error("invalid get-config: {0}")]
    InvalidGetConfig(String),

    /// A caller-supplied argument was invalid (empty source path, unresolvable
    /// target path, etc.).
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A URI scheme the facade cannot read from.
    #[error("unsupported uri `{uri}`: {reason}")]
    UnsupportedUri { uri: String, reason: String },

    /// An asset path from a version index could not be materialised under the
    /// target root without escaping it. The path is reported verbatim so the
    /// offending store can be identified; see `fs_util::safe_join`.
    #[error("unsafe asset path `{path}`: {reason}")]
    UnsafeAssetPath { path: String, reason: &'static str },

    /// A filesystem I/O error, with the operation/path for context.
    #[error("io error ({context})")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    /// A bug or an unreachable state — currently only a panic in a worker task,
    /// which is surfaced as an error rather than propagated. Distinct from
    /// [`LongtailError::Io`] because a panicking task is not a disk problem, and
    /// telling a user to check their disk for one is worse than saying nothing.
    #[error("internal error: {0}")]
    Internal(String),
}

/// What a caller should *do* about a [`LongtailError`], as opposed to what it
/// says.
///
/// The error enums are deliberately detailed — three crates' worth of variants
/// reachable through `LongtailError` — but a UI has only a handful of distinct
/// responses. Classifying here means every consumer gets the same answer instead
/// of hand-writing a match over the whole tree and guessing at retryability.
///
/// `#[non_exhaustive]`: new classes are expected as the error tree grows, so
/// callers must carry a fallback arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// The caller asked to stop. Not a failure; nothing to report or retry.
    Cancelled,
    /// The named version, block or object is not in the store.
    NotFound,
    /// Credentials were rejected. Re-authenticate and retry; retrying as-is
    /// cannot succeed.
    Unauthorized,
    /// A transport or contention failure that is expected to clear on its own.
    /// Safe to retry the whole operation.
    Transient,
    /// The request itself is wrong — a bad URI, an unusable option combination,
    /// an unsupported feature, an asset path that cannot be materialised. Retry
    /// will not help; the caller must change what it asked for.
    InvalidInput,
    /// Data read from the store did not decode, verify, or agree with its index.
    /// The store or the transfer is damaged, not the request.
    Corrupt,
    /// A local filesystem failure — out of space, permission denied, a path that
    /// vanished. Actionable by the operator, but on this machine rather than in
    /// the store. Inspect the underlying [`std::io::ErrorKind`] to say which.
    Io,
    /// A bug, or a state that should be unreachable. Worth reporting.
    Internal,
}

impl LongtailError {
    /// Classify this error by the response it calls for.
    ///
    /// This is the programmatic half of the store-error classes named in the
    /// crate docs: it holds across the whole error tree, including errors raised
    /// per block, which is where a caller most needs to tell "re-authenticate"
    /// from "retry later".
    pub fn class(&self) -> ErrorClass {
        match self {
            LongtailError::Cancelled => ErrorClass::Cancelled,

            // Malformed or disagreeing data, wherever it was decoded.
            LongtailError::Format(_)
            | LongtailError::Hash(_)
            | LongtailError::Compress(_)
            | LongtailError::Chunker(_)
            | LongtailError::Merge(_)
            | LongtailError::Validate(_)
            | LongtailError::ValidationMismatch(_) => ErrorClass::Corrupt,

            // The caller asked for something impossible or unsupported.
            LongtailError::LegacyWriteUnsupported
            | LongtailError::InvalidGetConfig(_)
            | LongtailError::InvalidArgument(_)
            | LongtailError::UnsupportedUri { .. }
            | LongtailError::UnsafeAssetPath { .. } => ErrorClass::InvalidInput,

            LongtailError::Io { .. } => ErrorClass::Io,
            LongtailError::Internal(_) => ErrorClass::Internal,

            LongtailError::Store(e) => match e {
                StoreError::NotFound(_) => ErrorClass::NotFound,
                StoreError::NotAuthorized(_) => ErrorClass::Unauthorized,
                // A lost CAS is the signal to re-run the read-merge-write loop.
                StoreError::Network(_) | StoreError::GenerationMismatch(_) => ErrorClass::Transient,
                StoreError::BadFormat(_) | StoreError::Format(_) | StoreError::Compress(_) => {
                    ErrorClass::Corrupt
                }
                StoreError::LockingNotSupported(_)
                | StoreError::NotSupported(_)
                | StoreError::InvalidUri { .. }
                | StoreError::AccessViolation => ErrorClass::InvalidInput,
                StoreError::Io { .. } => ErrorClass::Io,
                StoreError::WorkerGone => ErrorClass::Internal,
                // Unclassified by the backend; it carries a message but no
                // decision. Treated as a bug rather than silently "retryable".
                StoreError::Backend(_) => ErrorClass::Internal,
                // `StoreError` is `#[non_exhaustive]`, so this match cannot be
                // exhaustive from outside its crate. A new variant reaching here
                // is a classification that was never written, which is a bug in
                // this table rather than something to guess at. The compile-time
                // tripwire lives with the enum: `clone_store_error` matches it
                // exhaustively inside `longtail-store`, so adding a variant
                // still fails a build.
                _ => ErrorClass::Internal,
            },
        }
    }

    /// Render this error joined with its full `source()` chain.
    ///
    /// The top-level `Display` is a category, not the detail (see the type
    /// docs); this walks [`std::error::Error::source`] and joins each level
    /// with `": "`, so a single string carries the whole cause — e.g.
    /// `"store error: backend error: not authorized: …"`. Convenience for
    /// consumers (and the CLI) that want the detail without reimplementing the
    /// walk.
    pub fn full_chain(&self) -> String {
        use std::error::Error;
        let mut out = self.to_string();
        let mut src = self.source();
        while let Some(e) = src {
            out.push_str(": ");
            out.push_str(&e.to_string());
            src = e.source();
        }
        out
    }

    /// Attach filesystem context to an `io::Error`.
    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> LongtailError {
        LongtailError::Io {
            context: context.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The classification a consumer dispatches on. Pinned because the point of
    /// the class is that it is stable: a caller writes one match and expects the
    /// same answer regardless of which crate raised the error.
    #[test]
    fn classes_map_to_the_response_they_call_for() {
        let cases: &[(LongtailError, ErrorClass)] = &[
            (LongtailError::Cancelled, ErrorClass::Cancelled),
            (
                LongtailError::Store(StoreError::NotFound("v.lvi".into())),
                ErrorClass::NotFound,
            ),
            (
                LongtailError::Store(StoreError::NotAuthorized("403".into())),
                ErrorClass::Unauthorized,
            ),
            (
                LongtailError::Store(StoreError::Network("timeout".into())),
                ErrorClass::Transient,
            ),
            (
                LongtailError::Store(StoreError::GenerationMismatch("store.lsi".into())),
                ErrorClass::Transient,
            ),
            (
                LongtailError::Store(StoreError::BadFormat("block hash".into())),
                ErrorClass::Corrupt,
            ),
            (
                LongtailError::Store(StoreError::AccessViolation),
                ErrorClass::InvalidInput,
            ),
            (
                LongtailError::Store(StoreError::WorkerGone),
                ErrorClass::Internal,
            ),
            (
                LongtailError::InvalidArgument("empty source".into()),
                ErrorClass::InvalidInput,
            ),
            (
                LongtailError::UnsafeAssetPath {
                    path: "../x".into(),
                    reason: "escapes",
                },
                ErrorClass::InvalidInput,
            ),
            (
                LongtailError::ValidationMismatch("tree differs".into()),
                ErrorClass::Corrupt,
            ),
            (
                LongtailError::io("write", std::io::Error::other("disk full")),
                ErrorClass::Io,
            ),
            (
                LongtailError::Internal("task panicked".into()),
                ErrorClass::Internal,
            ),
        ];
        for (err, want) in cases {
            assert_eq!(err.class(), *want, "wrong class for {err:?}");
        }
    }

    /// A panicking worker task is a bug, not a disk problem. Telling a user to
    /// check their disk because of one is worse than saying nothing.
    #[test]
    fn a_panicking_task_is_internal_not_io() {
        let e = LongtailError::Internal("apply block task panicked: …".into());
        assert_eq!(e.class(), ErrorClass::Internal);
        assert_ne!(e.class(), ErrorClass::Io);
    }
}
