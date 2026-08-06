//! `clone-store` (`cmd_clonestore.go`): for each `(source .lvi, target .lvi)`
//! pair, **materialize** the version into the local `--target-path` via the
//! downsync path (reusing the source `.lvi`, no re-chunk), then **re-upload**
//! from that folder into the target store with the upsync machinery
//! (`GetExistingStoreIndex(target) → CreateMissingContent → WriteContent`).
//! clone-store never copies blocks store-to-store.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use longtail_core::{VersionIndex, create_missing_content, validate_store};
use longtail_store::AccessType;
use longtail_store::block_store::BlockStore;
use longtail_store::uri::{BlockStoreOpts, create_block_store_for_uri};
use tokio_util::sync::CancellationToken;

use crate::downsync::downsync;
use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};
use crate::hash_util::make_hasher;
use crate::options::DownsyncOptions;
use crate::progress::{NullProgress, ProgressSink, RateLimited};
use crate::upsync::write_content;

/// Options for [`clone_store`].
#[non_exhaustive]
pub struct CloneStoreOptions {
    pub source_storage_uri: String,
    pub target_storage_uri: String,
    /// Local folder each version is materialized into.
    pub target_path: String,
    /// Source version-index (`.lvi`) URIs (parallel to `target_paths`).
    pub source_paths: Vec<String>,
    /// Target version-index (`.lvi`) URIs to write (parallel to `source_paths`).
    pub target_paths: Vec<String>,
    /// Optional per-version zip fallback URIs (parallel).
    pub source_zip_paths: Vec<String>,
    pub create_version_local_store_index: bool,
    pub skip_validate: bool,
    pub cache_path: Option<PathBuf>,
    pub retain_permissions: bool,
    pub max_chunks_per_block: u32,
    pub target_block_size: u32,
    pub min_block_usage_percent: u32,
    pub enable_file_mapping: bool,
    pub use_legacy_write: bool,
    pub remote_worker_count: usize,
    pub worker_count: usize,
    /// Optional progress sink (drives both the materialize and re-upload halves).
    pub progress: Option<Arc<dyn ProgressSink>>,
    #[cfg(feature = "s3")]
    pub source_s3_options: S3OptionsArg,
    #[cfg(feature = "s3")]
    pub target_s3_options: S3OptionsArg,
}

impl CloneStoreOptions {
    pub fn new(
        source_storage_uri: impl Into<String>,
        target_storage_uri: impl Into<String>,
        target_path: impl Into<String>,
        source_paths: Vec<String>,
        target_paths: Vec<String>,
    ) -> Self {
        CloneStoreOptions {
            source_storage_uri: source_storage_uri.into(),
            target_storage_uri: target_storage_uri.into(),
            target_path: target_path.into(),
            source_paths,
            target_paths,
            source_zip_paths: Vec::new(),
            create_version_local_store_index: false,
            skip_validate: false,
            cache_path: None,
            retain_permissions: true,
            max_chunks_per_block: crate::upsync::DEFAULT_MAX_CHUNKS_PER_BLOCK,
            target_block_size: crate::upsync::DEFAULT_TARGET_BLOCK_SIZE,
            min_block_usage_percent: crate::upsync::DEFAULT_MIN_BLOCK_USAGE_PERCENT,
            enable_file_mapping: false,
            use_legacy_write: false,
            remote_worker_count: 0,
            worker_count: 0,
            progress: None,
            #[cfg(feature = "s3")]
            source_s3_options: S3OptionsArg::default(),
            #[cfg(feature = "s3")]
            target_s3_options: S3OptionsArg::default(),
        }
    }
}

/// Clone each `(source, target)` version pair. Returns the number of versions
/// actually cloned (versions skipped by the already-cloned check are not
/// counted).
pub async fn clone_store(opts: CloneStoreOptions) -> Result<u32, LongtailError> {
    if opts.use_legacy_write {
        return Err(LongtailError::LegacyWriteUnsupported);
    }
    #[cfg(feature = "s3")]
    let src_s3: S3OptionsArg = opts.source_s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let src_s3: S3OptionsArg = ();
    #[cfg(feature = "s3")]
    let tgt_s3: S3OptionsArg = opts.target_s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let tgt_s3: S3OptionsArg = ();

    let cancel = CancellationToken::new();
    let mut cloned = 0u32;

    // The re-upload half's rate-limited sink (same pattern as downsync/upsync).
    // The materialize half is downsync, which wraps `opts.progress` in its own
    // RateLimited internally (see `materialize`).
    let raw_progress: Arc<dyn ProgressSink> = opts
        .progress
        .clone()
        .unwrap_or_else(|| Arc::new(NullProgress));
    let progress = Arc::new(RateLimited::new(raw_progress));
    let version_total = opts.source_paths.len();

    for (i, source_lvi) in opts.source_paths.iter().enumerate() {
        if source_lvi.is_empty() {
            continue;
        }
        let target_lvi = match opts.target_paths.get(i) {
            Some(t) if !t.is_empty() => t.clone(),
            _ => break, // targets ran out (cmd_clonestore.go:655-657)
        };

        // Already-cloned check — INTENDED (unswapped) semantics: the v0.4.5
        // call site swaps args (:398) so this never fires; we implement what it
        // was meant to do and record the divergence.
        if let Some(target_version) = try_read_version_index(&target_lvi, &tgt_s3).await? {
            if opts.skip_validate {
                continue; // target exists and we trust it
            }
            match validate_target_covers(&opts, &target_version, &tgt_s3).await {
                Ok(()) => continue,      // already fully cloned → skip
                Err(e) => return Err(e), // exists but invalid → hard error
            }
        }

        // Read the SOURCE version index (its hash + tags drive the re-upload).
        let source_bytes = fs_util::read_from_uri(source_lvi, &src_s3).await?;
        let source_version = VersionIndex::from_bytes(&source_bytes)?;

        // 1. Materialize into target_path via downsync (reuses source .lvi).
        materialize(&opts, source_lvi).await?;
        progress.phase(&format!("Cloning version {}/{version_total}", i + 1));

        // 2. Re-upload from the materialized folder into the target store.
        let store: Arc<dyn BlockStore> = create_block_store_for_uri(
            &opts.target_storage_uri,
            BlockStoreOpts {
                access_type: AccessType::ReadWrite,
                worker_count: opts.remote_worker_count,
                cache_dir: None,
                pool: Arc::new(crate::version::build_pool(opts.worker_count)?),
                version_local_store_index: None,
                max_block_bytes: None,
                #[cfg(feature = "s3")]
                s3_options: opts.target_s3_options.clone(),
            },
        )
        .await?;

        // Fallible work inside the block so the flush + close below run on a
        // cancel or failure too: a partial re-upload still owes its write-backs.
        let uploaded = async {
            let hasher = make_hasher(source_version.hash_identifier)?;
            let existing = store
                .get_existing_content(&source_version.chunk_hashes, opts.min_block_usage_percent)
                .await?;
            let missing = create_missing_content(
                hasher.as_ref(),
                &existing,
                &source_version,
                opts.target_block_size,
                opts.max_chunks_per_block,
            )?;
            if missing.block_count() > 0 {
                write_content(
                    &store,
                    Path::new(&opts.target_path),
                    &source_version,
                    &missing,
                    &progress,
                    &cancel,
                )
                .await?;
            }
            Ok::<_, LongtailError>((existing, missing))
        }
        .await;
        let (existing, missing) = crate::store_lifecycle::finish_store(&store, uploaded).await?;

        // 3. Write the target .lvi (= the source version index).
        fs_util::write_to_uri(&target_lvi, source_version.to_bytes().into(), &tgt_s3).await?;

        // 4. Optional version-local .lsi (target .lvi path with .lvi→.lsi).
        if opts.create_version_local_store_index {
            // All-occurrence replacement is what golongtail does
            // (`strings.Replace(targetFilePath, ".lvi", ".lsi", -1)` in
            // cmd_clonestore.go), so a target that derives to a different path
            // derives to the *same* different path in both tools — including odd
            // ones like `archive.lvi.d/v1.lvi`. Diverging there would put the two
            // tools' outputs in different places for the same input.
            let lsi_path = target_lvi.replace(".lvi", ".lsi");
            // What is not worth matching is the case with no `.lvi` to replace:
            // the derivation returns the path unchanged, and the store index lands
            // on top of the version index written moments earlier through the same
            // truncating write, destroying it while reporting success. Refuse
            // instead — the version index above is already on disk and intact.
            if lsi_path == target_lvi {
                return Err(LongtailError::InvalidArgument(format!(
                    "target path `{target_lvi}` contains no `.lvi`, so the version-local store \
                     index would be written over the version index; name the target with a `.lvi` \
                     extension or drop --create-version-local-store-index"
                )));
            }
            let version_local = existing.merge(&missing)?;
            fs_util::write_to_uri(&lsi_path, version_local.to_bytes().into(), &tgt_s3).await?;
        }

        cloned += 1;
    }

    Ok(cloned)
}

/// Try to read a version index at a URI; `Ok(None)` if it does not exist.
async fn try_read_version_index(
    uri: &str,
    s3: &S3OptionsArg,
) -> Result<Option<VersionIndex>, LongtailError> {
    match fs_util::read_from_uri(uri, s3).await {
        Ok(bytes) => Ok(Some(VersionIndex::from_bytes(&bytes)?)),
        Err(LongtailError::Io { .. }) => Ok(None),
        Err(LongtailError::Store(e)) if e.is_not_found() => Ok(None),
        Err(e) => Err(e),
    }
}

/// `validateOneVersion` body (intended): confirm the target store covers every
/// chunk the target version needs.
async fn validate_target_covers(
    opts: &CloneStoreOptions,
    target_version: &VersionIndex,
    #[allow(unused)] tgt_s3: &S3OptionsArg,
) -> Result<(), LongtailError> {
    let store = create_block_store_for_uri(
        &opts.target_storage_uri,
        BlockStoreOpts {
            access_type: AccessType::ReadOnly,
            worker_count: opts.remote_worker_count,
            cache_dir: None,
            pool: Arc::new(crate::version::build_pool(1)?),
            version_local_store_index: None,
            max_block_bytes: None,
            #[cfg(feature = "s3")]
            s3_options: opts.target_s3_options.clone(),
        },
    )
    .await?;
    let fetched = store
        .get_existing_content(&target_version.chunk_hashes, 0)
        .await
        .map_err(LongtailError::from);
    let existing = crate::store_lifecycle::finish_store(&store, fetched).await?;
    validate_store(&existing, target_version).map_err(LongtailError::from)
}

/// Materialize `source_lvi` from the source store into `--target-path` via the
/// standard downsync path (no re-chunk of source content).
async fn materialize(opts: &CloneStoreOptions, source_lvi: &str) -> Result<(), LongtailError> {
    let mut ds = DownsyncOptions::new(
        vec![source_lvi.to_string()],
        opts.source_storage_uri.clone(),
        opts.target_path.clone(),
    );
    ds.retain_permissions = opts.retain_permissions;
    ds.cache_path = opts.cache_path.clone();
    ds.scan_target = true;
    // Do not scatter a cache index into the materialization folder.
    ds.cache_target_index = false;
    ds.enable_file_mapping = opts.enable_file_mapping;
    ds.worker_count = opts.worker_count;
    ds.remote_worker_count = opts.remote_worker_count;
    // downsync wraps this raw sink in its own RateLimited (downsync.rs); the
    // clone-store re-upload half uses a separate RateLimited over the same sink.
    ds.progress = opts.progress.clone();
    #[cfg(feature = "s3")]
    {
        ds.s3_options = opts.source_s3_options.clone();
    }
    downsync(ds).await?;
    Ok(())
}
