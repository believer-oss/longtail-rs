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
}

impl LongtailError {
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
