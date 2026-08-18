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
pub mod compression;
mod downsync;
pub mod error;
mod fs_util;
mod get;
mod hash_util;
mod inspect;
pub mod options;
pub mod path_filter;
pub mod progress;
mod version;

pub use compression::compression_type_for_name;
pub use downsync::downsync;
pub use error::LongtailError;
pub use get::get;
pub use hash_util::{SyncHasher, make_hasher};
pub use inspect::{ValidateVersionOptions, read_version_index_from_uri, validate_version};
pub use options::{DownsyncOptions, DownsyncReport, DownsyncStoreStats, GetOptions, PhaseTiming};
pub use path_filter::RegexPathFilter;
pub use progress::{NullProgress, ProgressSink};
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
