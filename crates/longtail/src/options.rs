//! Public options + report types for the download-path facade API.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::progress::ProgressSink;

#[cfg(feature = "s3")]
use longtail_store::S3Options;

/// Options for [`crate::downsync`] / [`crate::downsync_blocking`].
///
/// Construct with [`DownsyncOptions::new`] (source paths + storage URI + target)
/// then set the optional fields. All flag defaults match golongtail v0.4.5:
/// `retain_permissions`/`scan_target`/`cache_target_index` default **true**.
#[non_exhaustive]
pub struct DownsyncOptions {
    /// Source version-index URIs (`.lvi`). Multiple are merged
    /// (`MergeVersionIndex`); at least one is required.
    pub source_paths: Vec<String>,
    /// The target folder. `None` derives it from the first source path's
    /// basename truncated at the first dot (cmd_downsync.go:101).
    pub target_path: Option<String>,
    /// The block store URI (`s3://…`, a path, `file://…`).
    pub storage_uri: String,
    /// Optional local cache directory (`.lrb` blocks).
    pub cache_path: Option<PathBuf>,
    /// Optional cache byte budget. When set (and `cache_path` is set), the local
    /// block cache tracks per-block access time and LRU-evicts down to this many
    /// bytes after the operation completes. `None` = unbounded.
    pub cache_size_limit: Option<u64>,
    /// Version-local store index URIs (`.lsi`) — the ReadOnly store-index
    /// override (speeds reads, must yield the same tree).
    pub version_local_store_index_paths: Vec<String>,
    /// Include filter (multi-regex separated by `**`).
    pub include_filter_regex: Option<String>,
    /// Exclude filter (multi-regex separated by `**`).
    pub exclude_filter_regex: Option<String>,
    /// Apply the source version's POSIX permissions to written files (default
    /// true).
    pub retain_permissions: bool,
    /// Delete assets the target has and the source version does not (default
    /// **true**, matching golongtail).
    ///
    /// Set false for a repair: every asset in the version is still checked and
    /// rewritten if wrong, but nothing else in the target folder is touched. That
    /// is what makes it safe to run over an install folder holding save games,
    /// logs, or user config — those live in the target but in no version, so the
    /// delete phase would otherwise claim them. Excluding them by regex does the
    /// same job but has to be kept correct; getting the pattern wrong deletes
    /// user data silently, whereas this cannot.
    ///
    /// **Pair it with `cache_target_index = false`.** On its own this only stops
    /// deletions. A cached target index short-circuits the target scan entirely,
    /// so the run diffs the cache rather than the disk, finds nothing to do, and
    /// reports success without repairing anything. A repair is
    /// `delete_removed = false` *and* `cache_target_index = false`; the
    /// combination without the second is warned about at runtime.
    pub delete_removed: bool,
    /// Re-scan the target after writing and compare to the source index.
    pub validate: bool,
    /// Scan the target folder to build its current index (default true). Skipped
    /// when a target index path (or an existing cache) supplies it.
    pub scan_target: bool,
    /// Cache the source version index as `<target>/.longtail.index.cache.lvi`
    /// and reuse it next time (default true).
    pub cache_target_index: bool,
    /// An explicit target index path (local `.lvi`); disables caching.
    pub target_index_path: Option<String>,
    /// CPU (rayon) worker count for chunk/hash; `0` = logical CPUs.
    pub worker_count: usize,
    /// Remote block-I/O worker count; `0` = the scheme default.
    pub remote_worker_count: usize,
    /// Accepted **no-op** (boundaries are identical by design).
    pub enable_file_mapping: bool,
    /// Requesting the legacy write path yields a typed
    /// [`crate::LongtailError::LegacyWriteUnsupported`].
    pub use_legacy_write: bool,
    /// Optional progress sink.
    pub progress: Option<Arc<dyn ProgressSink>>,
    /// Optional cancellation token.
    pub cancel: Option<CancellationToken>,
    /// Optional caller-supplied rayon pool (else one is built per operation).
    pub pool: Option<Arc<rayon::ThreadPool>>,
    /// Test-oriented override of the remote store's prefetch byte budget
    /// (`None` → the 512 MiB default). Exists for the deadlock
    /// regression suite — correctness must never depend on this value (the
    /// budget bounds memory held by unconsumed background prefetches, never
    /// progress). Deliberately not exposed as a CLI flag.
    #[doc(hidden)]
    pub max_prefetch_bytes: Option<usize>,
    /// S3 credential/endpoint injection (feature `s3`).
    #[cfg(feature = "s3")]
    pub s3_options: S3Options,
}

impl DownsyncOptions {
    /// Minimal options: source path(s), storage URI, and a target folder.
    pub fn new(
        source_paths: Vec<String>,
        storage_uri: impl Into<String>,
        target_path: impl Into<String>,
    ) -> DownsyncOptions {
        DownsyncOptions {
            source_paths,
            target_path: Some(target_path.into()),
            storage_uri: storage_uri.into(),
            cache_path: None,
            cache_size_limit: None,
            version_local_store_index_paths: Vec::new(),
            include_filter_regex: None,
            exclude_filter_regex: None,
            retain_permissions: true,
            delete_removed: true,
            validate: false,
            scan_target: true,
            cache_target_index: true,
            target_index_path: None,
            worker_count: 0,
            remote_worker_count: 0,
            enable_file_mapping: false,
            use_legacy_write: false,
            progress: None,
            cancel: None,
            pool: None,
            max_prefetch_bytes: None,
            #[cfg(feature = "s3")]
            s3_options: S3Options::default(),
        }
    }
}

/// One phase's wall-clock timing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PhaseTiming {
    pub phase: String,
    pub millis: u64,
}

/// Store I/O counters, mirroring `longtail_store::StatsSnapshot` but
/// serializable for the launcher/CLI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DownsyncStoreStats {
    pub get_count: u64,
    pub get_byte_count: u64,
    pub get_chunk_count: u64,
    pub get_retry_count: u64,
    pub get_fail_count: u64,
    pub put_count: u64,
    pub put_byte_count: u64,
    pub put_chunk_count: u64,
    pub put_retry_count: u64,
    pub put_fail_count: u64,
}

impl From<longtail_store::StatsSnapshot> for DownsyncStoreStats {
    fn from(s: longtail_store::StatsSnapshot) -> DownsyncStoreStats {
        DownsyncStoreStats {
            get_count: s.get_count,
            get_byte_count: s.get_byte_count,
            get_chunk_count: s.get_chunk_count,
            get_retry_count: s.get_retry_count,
            get_fail_count: s.get_fail_count,
            put_count: s.put_count,
            put_byte_count: s.put_byte_count,
            put_chunk_count: s.put_chunk_count,
            put_retry_count: s.put_retry_count,
            put_fail_count: s.put_fail_count,
        }
    }
}

/// The result of a successful [`crate::downsync`]: phase timings, store I/O
/// counters, and the change summary. Serializable so the CLI can print it and
/// the launcher can log it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DownsyncReport {
    /// Resolved target folder path.
    pub target_path: String,
    /// Per-phase wall-clock timings.
    pub phases: Vec<PhaseTiming>,
    /// Block-store I/O counters.
    pub store_stats: DownsyncStoreStats,
    /// Total bytes written to the target.
    pub bytes_written: u64,
    /// Assets created or content-rewritten.
    pub assets_written: u32,
    /// Assets removed.
    pub assets_removed: u32,
    /// Blocks fetched from the store (== store_stats.get_count).
    pub blocks_fetched: u64,
}

/// Options for [`crate::get`] / [`crate::get_blocking`].
#[non_exhaustive]
pub struct GetOptions {
    /// Get-config JSON URIs. Multiple configs are merged (their `source-path`s
    /// are read + merged; all `storage-uri`s must agree).
    pub get_config_paths: Vec<String>,
    /// Target folder (derived if `None`, as for downsync).
    pub target_path: Option<String>,
    /// Optional local cache directory.
    pub cache_path: Option<PathBuf>,
    /// Optional cache byte budget (LRU eviction after the operation); `None` =
    /// unbounded. See [`DownsyncOptions::cache_size_limit`].
    pub cache_size_limit: Option<u64>,
    pub retain_permissions: bool,
    /// See [`DownsyncOptions::delete_removed`].
    pub delete_removed: bool,
    pub validate: bool,
    pub scan_target: bool,
    pub cache_target_index: bool,
    pub target_index_path: Option<String>,
    pub include_filter_regex: Option<String>,
    pub exclude_filter_regex: Option<String>,
    pub worker_count: usize,
    pub remote_worker_count: usize,
    pub enable_file_mapping: bool,
    pub use_legacy_write: bool,
    pub progress: Option<Arc<dyn ProgressSink>>,
    pub cancel: Option<CancellationToken>,
    pub pool: Option<Arc<rayon::ThreadPool>>,
    #[cfg(feature = "s3")]
    pub s3_options: S3Options,
}

/// Options for [`crate::upsync`]. Construct with [`UpsyncOptions::new`]
/// (source folder + storage URI + target `.lvi` URI); block-packing defaults
/// match golongtail v0.4.5 (`options.go`): 32 KiB target chunk, 8 MiB target
/// block, 1024 max chunks/block, 80 min block usage percent, `zstd`/`blake3`.
#[non_exhaustive]
pub struct UpsyncOptions {
    /// Source folder to upload.
    pub source_path: String,
    /// A pre-built source version index (`.lvi`) URI; skips scan+chunk.
    pub source_index_path: Option<String>,
    /// Target version-index (`.lvi`) URI to write.
    pub target_path: String,
    /// Block store URI.
    pub storage_uri: String,
    /// Version-local store index (`.lsi`) URI to write (`merge(existing, missing)`).
    pub version_local_store_index_path: Option<String>,
    pub target_chunk_size: u32,
    pub max_chunks_per_block: u32,
    pub target_block_size: u32,
    pub min_block_usage_percent: u32,
    pub compression_algorithm: String,
    pub hash_algorithm: String,
    pub include_filter_regex: Option<String>,
    pub exclude_filter_regex: Option<String>,
    pub worker_count: usize,
    pub remote_worker_count: usize,
    pub enable_file_mapping: bool,
    pub use_legacy_write: bool,
    /// Optional progress sink.
    pub progress: Option<Arc<dyn ProgressSink>>,
    pub cancel: Option<CancellationToken>,
    pub pool: Option<Arc<rayon::ThreadPool>>,
    #[cfg(feature = "s3")]
    pub s3_options: S3Options,
}

impl UpsyncOptions {
    /// Minimal options: source folder, storage URI, target `.lvi` URI.
    pub fn new(
        source_path: impl Into<String>,
        storage_uri: impl Into<String>,
        target_path: impl Into<String>,
    ) -> UpsyncOptions {
        UpsyncOptions {
            source_path: source_path.into(),
            source_index_path: None,
            target_path: target_path.into(),
            storage_uri: storage_uri.into(),
            version_local_store_index_path: None,
            target_chunk_size: crate::upsync::DEFAULT_TARGET_CHUNK_SIZE,
            max_chunks_per_block: crate::upsync::DEFAULT_MAX_CHUNKS_PER_BLOCK,
            target_block_size: crate::upsync::DEFAULT_TARGET_BLOCK_SIZE,
            min_block_usage_percent: crate::upsync::DEFAULT_MIN_BLOCK_USAGE_PERCENT,
            compression_algorithm: "zstd".to_string(),
            hash_algorithm: "blake3".to_string(),
            include_filter_regex: None,
            exclude_filter_regex: None,
            worker_count: 0,
            remote_worker_count: 0,
            enable_file_mapping: false,
            use_legacy_write: false,
            progress: None,
            cancel: None,
            pool: None,
            #[cfg(feature = "s3")]
            s3_options: S3Options::default(),
        }
    }
}

/// The result of a successful [`crate::upsync`] / [`crate::put`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UpsyncReport {
    /// The written target `.lvi` URI.
    pub target_path: String,
    pub phases: Vec<PhaseTiming>,
    /// Blocks packed and written this upsync (== `blocks_missing`).
    pub blocks_written: u32,
    /// Blocks the version needed that were not already in the store.
    pub blocks_missing: u32,
    /// Uncompressed bytes assembled into block payloads.
    pub bytes_written: u64,
    /// Chunk entries across the written blocks.
    pub chunks_written: u32,
    /// Block-store I/O counters.
    pub store_stats: DownsyncStoreStats,
}

impl GetOptions {
    /// Minimal options: get-config URI(s) + target folder.
    pub fn new(get_config_paths: Vec<String>, target_path: impl Into<String>) -> GetOptions {
        GetOptions {
            get_config_paths,
            target_path: Some(target_path.into()),
            cache_path: None,
            cache_size_limit: None,
            retain_permissions: true,
            delete_removed: true,
            validate: false,
            scan_target: true,
            cache_target_index: true,
            target_index_path: None,
            include_filter_regex: None,
            exclude_filter_regex: None,
            worker_count: 0,
            remote_worker_count: 0,
            enable_file_mapping: false,
            use_legacy_write: false,
            progress: None,
            cancel: None,
            pool: None,
            #[cfg(feature = "s3")]
            s3_options: S3Options::default(),
        }
    }
}
