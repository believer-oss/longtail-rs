//! The async download-path orchestration (`ChangeVersion2` semantics; mirrors
//! `cmd_downsync.go` + the ffi `commands.rs` map).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use longtail_core::{
    StoreIndex, VersionIndex, create_version_diff, get_required_chunk_hashes, merge_version_index,
};
use longtail_store::AccessType;
use longtail_store::block_store::BlockStore;
use longtail_store::uri::{BlockStoreOpts, create_block_store_for_uri_with_budget};
use tokio_util::sync::CancellationToken;

use crate::apply::change_version2;
use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};
use crate::hash_util::make_hasher;
use crate::options::{DownsyncOptions, DownsyncReport, PhaseTiming};
use crate::path_filter::RegexPathFilter;
use crate::progress::{NullProgress, ProgressSink, RateLimited};
use crate::version::create_version_index_from_folder;

const CACHE_INDEX_NAME: &str = ".longtail.index.cache.lvi";

/// Downsync one or more source versions into a target folder. See
/// [`DownsyncOptions`]. Runs on the caller's ambient tokio runtime.
#[tracing::instrument(
    name = "downsync",
    skip_all,
    fields(
        storage_uri = %opts.storage_uri,
        sources = opts.source_paths.len(),
        worker_count = opts.worker_count,
        remote_worker_count = opts.remote_worker_count,
    )
)]
pub async fn downsync(opts: DownsyncOptions) -> Result<DownsyncReport, LongtailError> {
    if opts.use_legacy_write {
        return Err(LongtailError::LegacyWriteUnsupported);
    }

    // Non-empty source paths (cmd_downsync.go:87-94).
    let sources: Vec<String> = opts
        .source_paths
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect();
    if sources.is_empty() {
        return Err(LongtailError::InvalidArgument(
            "please provide at least one source path uri".into(),
        ));
    }

    let filter = RegexPathFilter::new(
        opts.include_filter_regex.as_deref(),
        opts.exclude_filter_regex.as_deref(),
    )?;

    // Resolve the target folder (cmd_downsync.go:101).
    let target_string = match opts.target_path.as_deref() {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => derive_target_path(&sources[0])?,
    };
    let target_root = PathBuf::from(&target_string);

    // Target-index caching four-step semantics (cmd_downsync.go:120-135).
    let mut cache_target_index = opts.cache_target_index;
    let explicit_target_index = opts
        .target_index_path
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    if explicit_target_index.is_some() {
        cache_target_index = false;
    }
    let cache_index_path = target_root.join(CACHE_INDEX_NAME);
    // Effective target index: explicit path, or the cache file if it exists. The
    // two are not equally trusted — see `read_target_index` below.
    let effective_target_index: Option<String> = if let Some(t) = &explicit_target_index {
        Some(t.clone())
    } else if cache_target_index && fs_util::file_exists(&cache_index_path) {
        Some(cache_index_path.to_string_lossy().into_owned())
    } else {
        None
    };
    let target_index_is_cache = explicit_target_index.is_none();

    let progress: Arc<dyn ProgressSink> = opts
        .progress
        .clone()
        .unwrap_or_else(|| Arc::new(NullProgress));
    let progress = Arc::new(RateLimited::new(progress));
    let cancel = opts.cancel.clone().unwrap_or_default();

    let pool = match &opts.pool {
        Some(p) => p.clone(),
        None => Arc::new(crate::version::build_pool(opts.worker_count)?),
    };

    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let mut phases: Vec<PhaseTiming> = Vec::new();
    let mut phase = PhaseTimer::new();

    // Read + merge source version index(es) (cmd_downsync.go:142-164). This is
    // the first (often remote) fetch, so label it — otherwise the run appears to
    // hang here with no phase shown.
    check_cancel(&cancel)?;
    progress.phase("Reading version index");
    let source_version = read_merged_source(&sources, &s3).await?;
    let hash_id = source_version.hash_identifier;
    let target_chunk_size = source_version.target_chunk_size;
    let hasher = make_hasher(hash_id)?;
    phases.push(phase.lap("read_source_index"));

    // Build the current target index (explicit/cached file, scan, or empty).
    progress.phase("Indexing version");
    let preloaded = read_target_index(effective_target_index.as_deref(), target_index_is_cache)?;
    let used_preloaded = preloaded.is_some();
    let target_index = if let Some(vi) = preloaded {
        vi
    } else if opts.scan_target {
        let on_scan = crate::version::scan_progress_forwarder(progress.clone());
        create_version_index_from_folder(
            &target_root,
            &filter,
            hasher.as_ref(),
            target_chunk_size,
            0, // NoCompressionType for target scanning (cmd_downsync.go:176)
            &pool,
            &cancel,
            Some(&on_scan),
        )?
    } else {
        empty_version_index(hash_id, target_chunk_size)
    };
    phases.push(phase.lap("build_target_index"));

    // A repair that reads a cached target index cannot repair anything: the cache
    // is trusted as the target's state, so the diff is empty and the run exits 0
    // having written nothing. The two options are orthogonal and the combination
    // is legal — a no-delete upgrade wants exactly this — but it is also the shape
    // a "repair" button gets wired as by accident, and the failure is silent.
    // Keyed on the index actually being used, so a cache that was rejected above
    // does not draw a warning about a trust that is no longer being placed.
    if !opts.delete_removed && used_preloaded {
        tracing::warn!(
            "delete_removed is off while a cached or explicit target index is in use; damage on \
             disk will not be detected — set cache_target_index = false to scan the target"
        );
    }

    // Build the ReadOnly store-index override from version-local-store-index
    // paths (remotestore.go:1897; on any failure → None → the store reads its
    // own index instead). Reading the override is itself a (possibly remote)
    // step, so label it rather than leaving the stale "Indexing version" up.
    check_cancel(&cancel)?;
    progress.phase("Reading store index");
    let override_index =
        load_store_index_override(&opts.version_local_store_index_paths, &s3).await;

    // Without an override the store reads its own index — a list of `store*.lsi`
    // and a merge of every shard — on the first block query below, and this phase
    // is still the one on screen when that happens. Re-label so the slow branch
    // is named rather than hiding behind a label that also covers the cheap one.
    // Fires whether the override failed or was never supplied: the work, and so
    // the honest label, is the same either way.
    if override_index.is_none() {
        progress.phase("Reading full store index");
    }

    // Compose the block store (Compress(Cache(Remote))), ReadOnly. The apply
    // loop's block-task concurrency shares the store's resolved worker count
    // (one knob — no separate apply setting).
    let apply_concurrency =
        longtail_store::resolved_worker_count(&opts.storage_uri, opts.remote_worker_count);
    let opts_store = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: opts.cache_path.clone(),
        pool: pool.clone(),
        version_local_store_index: override_index,
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    // `max_prefetch_bytes` is the deadlock-regression test knob (None in
    // production → the 512 MiB default). Liveness must never depend on it.
    let store: Arc<dyn BlockStore> = create_block_store_for_uri_with_budget(
        &opts.storage_uri,
        opts_store,
        opts.max_prefetch_bytes,
        opts.cache_size_limit,
    )
    .await?;
    phases.push(phase.lap("open_store"));

    // A second hasher instance for the opt-in chunk verification: the apply tasks
    // are spawned, so it has to be shared rather than borrowed. Constructing one is
    // trivial (a unit struct), so this is cheaper than reshaping the scan's hasher.
    let verify_hasher: Option<Arc<dyn longtail_core::Hash + Send + Sync>> = if opts.verify_chunks {
        Some(Arc::from(make_hasher(hash_id)?))
    } else {
        None
    };

    // Everything that can fail between opening the store and closing it runs
    // inside this block, so the flush + close below happen on a cancel or a
    // failure too — the store's write-backs and the cache-budget sweep both hang
    // off `close()`, and cancelling is a routine way to end a download.
    let applied = async {
        // Diff (from = current target, to = desired source), required chunks,
        // retargetted store index (min_block_usage_percent = 0,
        // cmd_downsync.go:266).
        let diff = create_version_diff(&target_index, &source_version);
        let required = get_required_chunk_hashes(&source_version, &diff);
        let store_index = store.get_existing_content(&required, 0).await?;
        phases.push(phase.lap("diff_and_retarget"));

        // Delete the cache index before mutating the target (cmd_downsync.go:274).
        if cache_target_index {
            fs_util::delete_local(&cache_index_path)?;
        }

        // Apply.
        let apply_stats = change_version2(
            &store,
            &target_root,
            &source_version,
            &target_index,
            &diff,
            &store_index,
            opts.retain_permissions,
            opts.delete_removed,
            verify_hasher,
            apply_concurrency,
            &progress,
            &cancel,
        )
        .await?;
        phases.push(phase.lap("apply"));
        Ok::<_, LongtailError>(apply_stats)
    }
    .await;

    // Flush + close the store chain before resolving (obligation #6; warm-cache
    // write-backs must complete — cmd_downsync.go:324).
    let apply_stats = crate::store_lifecycle::finish_store(&store, applied).await?;
    let store_stats = store.stats();
    phases.push(phase.lap("flush"));

    // Optional post-downsync validation (cmd_downsync.go:380-456).
    if opts.validate {
        progress.phase("Validating version");
        validate_target(
            &target_root,
            &filter,
            hasher.as_ref(),
            target_chunk_size,
            &source_version,
            opts.retain_permissions,
            &pool,
            &cancel,
        )?;
        phases.push(phase.lap("validate"));
    }

    // Cache the SOURCE version index for next time (cmd_downsync.go:458).
    if cache_target_index {
        fs_util::write_local(&cache_index_path, &source_version.to_bytes())?;
    }

    Ok(DownsyncReport {
        target_path: target_string,
        phases,
        store_stats: store_stats.into(),
        bytes_written: apply_stats.bytes_written,
        assets_written: apply_stats.assets_written,
        assets_removed: apply_stats.assets_removed,
        blocks_fetched: store_stats.get_count,
    })
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), LongtailError> {
    if cancel.is_cancelled() {
        Err(LongtailError::Cancelled)
    } else {
        Ok(())
    }
}

/// Derive the target folder from a source URI: basename (last `/`-segment) of the
/// normalized path, truncated at the **first** dot (cmd_downsync.go:101-108).
fn derive_target_path(source: &str) -> Result<String, LongtailError> {
    let normalized = source.replace('\\', "/");
    let last = normalized.rsplit('/').next().unwrap_or("");
    let stem = last.split('.').next().unwrap_or("");
    if stem.is_empty() {
        return Err(LongtailError::InvalidArgument(format!(
            "unable to resolve target path using `{source}` as base"
        )));
    }
    Ok(stem.to_string())
}

async fn read_merged_source(
    sources: &[String],
    s3: &S3OptionsArg,
) -> Result<VersionIndex, LongtailError> {
    let mut merged: Option<VersionIndex> = None;
    for path in sources {
        let bytes = fs_util::read_from_uri(path, s3).await?;
        let vi = VersionIndex::from_bytes(&bytes)?;
        merged = Some(match merged {
            None => vi,
            Some(base) => merge_version_index(&base, &vi)?,
        });
    }
    merged.ok_or_else(|| LongtailError::InvalidArgument("no source version index".into()))
}

fn read_version_index_local(path: &Path) -> Result<VersionIndex, LongtailError> {
    let bytes = std::fs::read(path).map_err(|e| LongtailError::io(format!("read {path:?}"), e))?;
    Ok(VersionIndex::from_bytes(&bytes)?)
}

/// Load the target index named by `path`, if any.
///
/// An explicitly supplied `--target-index-path` is a hard error when it will not
/// read: the caller named that file and silently substituting a scan would give
/// them a different operation than the one they asked for.
///
/// `.longtail.index.cache.lvi` is the opposite. It is an optimisation this code
/// wrote for itself, and its worst case has to be "slower", never "the download
/// stops". A rejected cache returns `None` so the caller scans the target, which
/// is exactly what would have happened had the file never existed.
///
/// It reaches an unreadable state by ordinary means, no crash required. The file
/// lives inside the target, so a folder that has been downsynced once and then
/// upsynced carries it into the version index as an asset — golongtail v0.4.5
/// indexes it the same way, so existing stores already hold such versions. Apply
/// then re-materialises it like any other asset: pre-created at its final size by
/// `create_file_sized`, hence zero-filled until its blocks land. A run that fails
/// or is cancelled in between leaves those zeros, and the post-apply cache write
/// that would have replaced them never happens. Before this, the next run read
/// `0x00000000` as a format version and refused a target that was merely stale.
///
/// A rejected file is left alone rather than deleted: the run deletes it before
/// mutating the target anyway, and a successful run replaces it.
fn read_target_index(
    path: Option<&str>,
    is_cache: bool,
) -> Result<Option<VersionIndex>, LongtailError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let path = Path::new(path);
    match read_version_index_local(path) {
        Ok(vi) => Ok(Some(vi)),
        Err(e) if is_cache => {
            tracing::warn!(
                cache = %path.display(),
                error = %e,
                "the cached target index could not be read and is being ignored; scanning the \
                 target instead — this run is slower, not wrong"
            );
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

fn empty_version_index(hash_id: u32, target_chunk_size: u32) -> VersionIndex {
    VersionIndex {
        hash_identifier: hash_id,
        target_chunk_size,
        path_hashes: Vec::new(),
        content_hashes: Vec::new(),
        asset_sizes: Vec::new(),
        asset_chunk_counts: Vec::new(),
        asset_chunk_index_starts: Vec::new(),
        asset_chunk_indexes: Vec::new(),
        chunk_hashes: Vec::new(),
        chunk_sizes: Vec::new(),
        chunk_tags: Vec::new(),
        name_offsets: Vec::new(),
        permissions: Vec::new(),
        name_data: Vec::new(),
    }
}

/// Read + merge the version-local store index override paths; `None` on any
/// read/merge failure (falls back to the store's own index, remotestore.go:1897).
async fn load_store_index_override(paths: &[String], s3: &S3OptionsArg) -> Option<StoreIndex> {
    if paths.is_empty() {
        return None;
    }
    // Any failure here falls back to reading the store's own index — a list of
    // `store*.lsi` plus a merge of every shard, covering the whole store rather
    // than just this version's blocks. That produces no progress of its own, so
    // the download simply appears to stall. Warn with the URI that caused it,
    // otherwise the difference between "slow store" and "your override path is
    // wrong" is invisible from the outside.
    let mut acc: Option<StoreIndex> = None;
    for p in paths {
        let bytes = match fs_util::read_from_uri(p, s3).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    uri = %p,
                    error = %e,
                    "could not read version-local store index; falling back to reading the whole store index"
                );
                return None;
            }
        };
        let si = match StoreIndex::from_bytes(&bytes) {
            Ok(si) => si,
            Err(e) => {
                tracing::warn!(
                    uri = %p,
                    error = %e,
                    "version-local store index did not parse; falling back to reading the whole store index"
                );
                return None;
            }
        };
        acc = Some(match acc {
            None => si,
            Some(a) => match a.merge(&si) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(
                        uri = %p,
                        error = %e,
                        "version-local store indexes did not merge; falling back to reading the whole store index"
                    );
                    return None;
                }
            },
        });
    }
    acc
}

/// The `--validate` rescan: re-index the target with nil tags and compare each
/// asset's size/hash (+ permissions iff retaining) against the source index
/// (cmd_downsync.go:380-456).
#[allow(clippy::too_many_arguments)]
fn validate_target<H: longtail_core::Hash + Sync + ?Sized>(
    target_root: &Path,
    filter: &RegexPathFilter,
    hasher: &H,
    target_chunk_size: u32,
    source_version: &VersionIndex,
    retain_permissions: bool,
    pool: &rayon::ThreadPool,
    cancel: &CancellationToken,
) -> Result<(), LongtailError> {
    let rescan = create_version_index_from_folder(
        target_root,
        filter,
        hasher,
        target_chunk_size,
        0,
        pool,
        cancel,
        None, // validate rescan: no progress readout
    )?;
    if rescan.asset_count() != source_version.asset_count() {
        return Err(LongtailError::ValidationMismatch(format!(
            "asset count mismatch: rescanned {} vs source {}",
            rescan.asset_count(),
            source_version.asset_count()
        )));
    }
    // Build source lookups keyed by path.
    let mut size_by_path: HashMap<&[u8], u64> = HashMap::new();
    let mut hash_by_path: HashMap<&[u8], u64> = HashMap::new();
    let mut perm_by_path: HashMap<&[u8], u16> = HashMap::new();
    for i in 0..source_version.asset_count() as usize {
        let p = source_version.path_bytes(i)?;
        size_by_path.insert(p, source_version.asset_sizes[i]);
        hash_by_path.insert(p, source_version.content_hashes[i]);
        perm_by_path.insert(p, source_version.permissions[i].bits());
    }
    for i in 0..rescan.asset_count() as usize {
        let p = rescan.path_bytes(i)?;
        let path_str = String::from_utf8_lossy(p);
        match size_by_path.get(p) {
            None => {
                return Err(LongtailError::ValidationMismatch(format!(
                    "asset `{path_str}` not found in source index"
                )));
            }
            Some(&size) => {
                if rescan.asset_sizes[i] != size {
                    return Err(LongtailError::ValidationMismatch(format!(
                        "asset `{path_str}` size mismatch: {} vs {size}",
                        rescan.asset_sizes[i]
                    )));
                }
                if rescan.content_hashes[i] != hash_by_path[p] {
                    return Err(LongtailError::ValidationMismatch(format!(
                        "asset `{path_str}` content hash mismatch"
                    )));
                }
                if retain_permissions
                    && permissions_disagree(
                        rescan.permissions[i].bits(),
                        perm_by_path[p],
                        cfg!(windows),
                    )
                {
                    return Err(LongtailError::ValidationMismatch(format!(
                        "asset `{path_str}` permission mismatch"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Whether a rescanned permission set contradicts the one the version recorded.
///
/// Windows carries only a read-only flag; [`fs_util::mode_of`] synthesizes the
/// rest (format-spec §7), so a `.lvi` written on unix records POSIX bits a
/// Windows rescan cannot reproduce — `0o644` comes back as `0o666`. Comparing
/// every bit there fails `--validate` on every store authored anywhere else,
/// which is the normal case for a Windows consumer of a Linux-built store.
///
/// The writable bit is what the platform actually stores and what
/// [`fs_util::set_permissions`] writes from a recorded mode, so it is the part
/// that can honestly be checked. Taking `windows` as an argument rather than
/// reading `cfg!` keeps both branches testable from either host.
fn permissions_disagree(rescanned: u16, recorded: u16, windows: bool) -> bool {
    if windows {
        (rescanned & 0o222 != 0) != (recorded & 0o222 != 0)
    } else {
        rescanned != recorded
    }
}

/// A simple sequential phase timer.
struct PhaseTimer {
    last: Instant,
}

impl PhaseTimer {
    fn new() -> PhaseTimer {
        PhaseTimer {
            last: Instant::now(),
        }
    }
    fn lap(&mut self, name: &str) -> PhaseTiming {
        let now = Instant::now();
        let millis = now.duration_since(self.last).as_millis() as u64;
        self.last = now;
        PhaseTiming {
            phase: name.to_string(),
            millis,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::permissions_disagree;

    /// Both branches, from either host — the Windows one cannot be reached by
    /// running the suite on Linux, and it is the branch that was wrong.
    #[test]
    fn windows_validate_compares_only_the_bit_windows_can_store() {
        // Unix: an exact comparison, because every bit round-trips.
        assert!(!permissions_disagree(0o644, 0o644, false));
        assert!(permissions_disagree(0o666, 0o644, false));
        assert!(permissions_disagree(0o444, 0o644, false));

        // Windows: a rescan synthesizes 0o666 for any writable file, so a store
        // authored on unix records 0o644 and must still validate. Reading a
        // unix-authored store on Windows is the ordinary case, not a corner one.
        assert!(!permissions_disagree(0o666, 0o644, true));
        assert!(!permissions_disagree(0o666, 0o664, true));
        // Directories come back with the execute bits set; still writable.
        assert!(!permissions_disagree(0o777, 0o755, true));
        // A read-only file matches a read-only record...
        assert!(!permissions_disagree(0o444, 0o444, true));
        // ...and read-only versus writable is a real disagreement even there,
        // because `set_permissions` does write that bit.
        assert!(permissions_disagree(0o444, 0o644, true));
        assert!(permissions_disagree(0o666, 0o444, true));
    }
}
