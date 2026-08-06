//! The `longtail` facade: the launcher-facing download-path API
//! (`downsync`/`get`), the `ChangeVersion2` orchestration, the local-fs helpers,
//! the `RegexPathFilter`, progress/cancellation/stats plumbing, and the unified
//! [`LongtailError`] tree.
//!
//! # Runtime model
//!
//! [`downsync`] / [`get`] are plain `async fn`s that run on the **caller's**
//! ambient tokio runtime — the library never creates a runtime. The
//! [`downsync_blocking`] / [`get_blocking`] convenience wrappers build their own
//! multi-thread runtime and must be called from a non-async context.
//!
//! `longtail-core` chunk/hash work runs on a per-operation `rayon::ThreadPool`
//! (caller-supplied via [`DownsyncOptions::pool`], else built from
//! `worker_count`); block I/O runs on the tokio-native store's actor + workers.
//!
//! # Example
//!
//! ```no_run
//! use longtail::{DownsyncOptions, downsync};
//! # async fn run() -> Result<(), longtail::LongtailError> {
//! let opts = DownsyncOptions::new(
//!     vec!["s3://bucket/versions/game.v2.lvi".into()],
//!     "s3://bucket/store",
//!     "/games/mygame",
//! );
//! let report = downsync(opts).await?;
//! println!("wrote {} bytes across {} assets", report.bytes_written, report.assets_written);
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]

mod apply;
mod clonestore;
pub mod compression;
mod cp;
mod downsync;
pub mod error;
mod fs_util;
mod get;
mod hash_util;
mod inspect;
pub mod options;
pub mod path_filter;
pub mod progress;
mod prune;
mod put;
mod upsync;
mod version;

pub use clonestore::{CloneStoreOptions, clone_store};
pub use compression::compression_type_for_name;
pub use cp::{CpOptions, cp};
pub use downsync::downsync;
pub use error::LongtailError;
pub use get::get;
pub use hash_util::{SyncHasher, make_hasher};
pub use inspect::{
    CreateVersionStoreIndexOptions, InitRemoteStoreOptions, PrintVersionUsageOptions,
    StoreIndexStats, ValidateVersionOptions, VersionUsageStats, create_version_store_index,
    init_remote_store, print_version_usage_stats, read_store_index_from_uri,
    read_version_index_from_uri, store_index_stats, validate_version,
};
pub use options::{
    DownsyncOptions, DownsyncReport, DownsyncStoreStats, GetOptions, PhaseTiming, UpsyncOptions,
    UpsyncReport,
};
pub use path_filter::RegexPathFilter;
pub use progress::{NullProgress, Progress, ProgressSink};
// Re-exported so a caller can construct/trigger cancellation without a direct
// `tokio-util` dependency (or a version-coupling to it). Put a clone in
// `DownsyncOptions`/`GetOptions::cancel` and call `.cancel()` to stop: in-flight
// blocks finish, the partial target and its `.lrb` block cache stay valid, and
// the op returns `LongtailError::Cancelled`. This is also the pause primitive —
// "pause" = cancel and keep the target folder; "resume" = call `get`/`downsync`
// again (delta-only; already-fetched blocks come from the cache, not the store,
// though resume does re-scan the target); "cancel" = the same, then delete the
// target. Cancellation is block-granular: it stops launching the next block but
// cannot abort an already-in-flight fetch.
pub use tokio_util::sync::CancellationToken;
// Re-exported so a facade-only consumer can match on the store-error classes
// (`StoreError::NotAuthorized` / `Network` / `NotFound`) reachable through
// `LongtailError::Store(_)` without a direct `longtail-store` dependency.
pub use longtail_store::StoreError;
// The S3 configuration surface is re-exported so a crate that depends only on
// `longtail` can name the type it must construct for `DownsyncOptions`/
// `GetOptions::s3_options` without adding a direct `longtail-store` dependency.
// Rides the same `default = ["s3"]` flag. (Credential/provider types are not
// re-exported: a caller builds the provider via its own aws-config/aws-sdk-s3.)
#[cfg(feature = "s3")]
pub use fs_util::S3OptionsArg;
pub use longtail_store::S3Options;
pub use prune::{
    PruneStoreBlocksOptions, PruneStoreIndexOptions, PruneStoreOptions, prune_store,
    prune_store_blocks, prune_store_index,
};
pub use put::{PutOptions, put};
pub use upsync::upsync;
pub use version::create_version_index_from_folder;

/// Blocking convenience wrapper around [`downsync`]: builds its own multi-thread
/// tokio runtime. Call from a non-async context (the CLI, or a plain thread).
pub fn downsync_blocking(opts: DownsyncOptions) -> Result<DownsyncReport, LongtailError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            LongtailError::InvalidArgument(format!("failed to build tokio runtime: {e}"))
        })?;
    runtime.block_on(downsync(opts))
}

/// Blocking convenience wrapper around [`get`].
pub fn get_blocking(opts: GetOptions) -> Result<DownsyncReport, LongtailError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            LongtailError::InvalidArgument(format!("failed to build tokio runtime: {e}"))
        })?;
    runtime.block_on(get(opts))
}

/// Blocking convenience wrapper around [`upsync`]: builds its own multi-thread
/// tokio runtime. Call from a non-async context.
pub fn upsync_blocking(opts: options::UpsyncOptions) -> Result<UpsyncReport, LongtailError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            LongtailError::InvalidArgument(format!("failed to build tokio runtime: {e}"))
        })?;
    runtime.block_on(upsync(opts))
}

/// Blocking convenience wrapper around [`put`].
pub fn put_blocking(opts: PutOptions) -> Result<UpsyncReport, LongtailError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| {
            LongtailError::InvalidArgument(format!("failed to build tokio runtime: {e}"))
        })?;
    runtime.block_on(put(opts))
}
