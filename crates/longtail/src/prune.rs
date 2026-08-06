//! The three prune commands (`cmd_prunestore*.go`). No confirmation prompts —
//! the safety surface is `--dry-run` (distinct outputs) plus, for `prune-store`,
//! the index-overwrite-BEFORE-block-delete ordering enforced in the store layer.
//!
//! - [`prune_store`]: rewrites the store index AND deletes orphan blocks.
//! - [`prune_store_index`]: rewrites only the store index file.
//! - [`prune_store_blocks`]: deletes only orphan block files (keep-set = the
//!   store index's block hashes).

use std::collections::HashSet;
use std::sync::Arc;

use longtail_core::{StoreIndex, VersionIndex, validate_store};
use longtail_store::AccessType;
use longtail_store::blob::create_blob_store_for_uri;
use longtail_store::uri::{BlockStoreOpts, create_block_store_for_uri};

use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};

#[cfg(feature = "s3")]
fn default_s3() -> S3OptionsArg {
    longtail_store::S3Options::default()
}
#[cfg(not(feature = "s3"))]
#[allow(dead_code)]
fn default_s3() -> S3OptionsArg {}

fn pool() -> Result<Arc<rayon::ThreadPool>, LongtailError> {
    // Via `build_pool` for the shared panic handler.
    Ok(Arc::new(crate::version::build_pool(1)?))
}

/// Common versions-driven prune options shared by `prune-store` and
/// `prune-store-index`.
struct GatherOptions {
    source_version_index_paths: Vec<String>,
    version_local_store_index_paths: Vec<String>,
    validate_versions: bool,
    skip_invalid_versions: bool,
    write_version_local_store_index: bool,
    dry_run: bool,
}

fn check_lsi_len(g: &GatherOptions) -> Result<(), LongtailError> {
    if !g.version_local_store_index_paths.is_empty()
        && g.version_local_store_index_paths.len() != g.source_version_index_paths.len()
    {
        return Err(LongtailError::InvalidArgument(
            "prune: number of version-local-store-index paths does not match source paths".into(),
        ));
    }
    Ok(())
}

/// Resolve one version's kept-block set. `resolve_existing` maps a version's
/// chunk hashes to the covering store index (either the remote store, for
/// `prune-store`, or an in-memory master index, for `prune-store-index`).
async fn keep_for_version<F, Fut>(
    g: &GatherOptions,
    idx: usize,
    version: &VersionIndex,
    resolve_existing: F,
    s3: &S3OptionsArg,
) -> Result<Option<Vec<u64>>, LongtailError>
where
    F: FnOnce(Vec<u64>) -> Fut,
    Fut: std::future::Future<Output = Result<StoreIndex, LongtailError>>,
{
    let lsi_path = g
        .version_local_store_index_paths
        .get(idx)
        .filter(|s| !s.is_empty());

    // Supplied LSI (and NOT rewriting it): read + validate + use directly.
    if let Some(lsi) = lsi_path
        && !g.write_version_local_store_index
    {
        let bytes = fs_util::read_from_uri(lsi, s3).await?;
        let existing = StoreIndex::from_bytes(&bytes)?;
        // Hard error on validation failure regardless of skip flag (matches
        // readPruneVersion, cmd_prunestore.go:49-55).
        validate_store(&existing, version)?;
        return Ok(Some(existing.block_hashes.clone()));
    }

    // Otherwise resolve against the store/master index at usage percent 0.
    let existing = resolve_existing(version.chunk_hashes.clone()).await?;
    if g.validate_versions
        && let Err(e) = validate_store(&existing, version)
    {
        if g.skip_invalid_versions {
            // Contribute no blocks; the version's data becomes prune-eligible.
            return Ok(None);
        }
        return Err(LongtailError::from(e));
    }

    // Rewrite the version-local store index if requested (not in dry-run).
    if let Some(lsi) = lsi_path
        && g.write_version_local_store_index
        && !g.dry_run
    {
        fs_util::write_to_uri(lsi, existing.to_bytes().into(), s3).await?;
    }

    Ok(Some(existing.block_hashes.clone()))
}

/// Options for [`prune_store`].
#[non_exhaustive]
pub struct PruneStoreOptions {
    pub storage_uri: String,
    pub source_version_index_paths: Vec<String>,
    pub version_local_store_index_paths: Vec<String>,
    pub dry_run: bool,
    pub validate_versions: bool,
    pub skip_invalid_versions: bool,
    pub write_version_local_store_index: bool,
    pub remote_worker_count: usize,
    /// Proceed even when the resolved keep-set is empty. See
    /// [`guard_empty_keep_set`].
    pub allow_empty_keep_set: bool,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl PruneStoreOptions {
    pub fn new(storage_uri: impl Into<String>, source_version_index_paths: Vec<String>) -> Self {
        PruneStoreOptions {
            storage_uri: storage_uri.into(),
            source_version_index_paths,
            version_local_store_index_paths: Vec::new(),
            dry_run: false,
            validate_versions: false,
            skip_invalid_versions: false,
            write_version_local_store_index: false,
            remote_worker_count: 0,
            allow_empty_keep_set: false,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// The outcome of [`prune_store`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PruneStoreResult {
    pub dry_run: bool,
    /// Blocks the retained versions need (the keep-set size).
    pub keep_blocks: usize,
    /// Blocks actually deleted (0 in dry-run).
    pub pruned_blocks: u32,
}

/// Refuse a prune whose keep-set resolved to nothing.
///
/// Every prune command deletes what the keep-set does not name, so an empty
/// keep-set means "delete everything" — and the usual way to get one is an
/// input mistake rather than an intent: a `--source-paths` file that is empty
/// or all blank lines, which is what a failed listing command redirected to a
/// file produces. Left unguarded the result is silent, total and irreversible.
///
/// Checked in dry-run too. A dry run is the safety surface here, so it is the
/// place the mistake should surface, not the one place it is tolerated.
///
/// golongtail is equally unguarded, so this is a deliberate behaviour
/// divergence on a destructive command; `allow_empty_keep_set` is the way to ask
/// for the old behaviour.
fn guard_empty_keep_set(
    keep_len: usize,
    source_count: usize,
    allow: bool,
) -> Result<(), LongtailError> {
    if keep_len > 0 || allow {
        return Ok(());
    }
    Err(LongtailError::InvalidArgument(format!(
        "refusing to prune: the keep-set resolved to zero blocks from {source_count} source \
         path(s), which would delete every block in the store. If that is intended, pass \
         --allow-empty-keep-set"
    )))
}

/// `prune-store`: gather the keep-set from the retained versions, then (unless
/// `--dry-run`) overwrite the store index and delete orphan blocks.
pub async fn prune_store(opts: PruneStoreOptions) -> Result<PruneStoreResult, LongtailError> {
    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let g = GatherOptions {
        source_version_index_paths: opts.source_version_index_paths.clone(),
        version_local_store_index_paths: opts.version_local_store_index_paths.clone(),
        validate_versions: opts.validate_versions,
        skip_invalid_versions: opts.skip_invalid_versions,
        write_version_local_store_index: opts.write_version_local_store_index,
        dry_run: opts.dry_run,
    };
    check_lsi_len(&g)?;

    // ReadOnly store for gathering (no ._lck pollution in dry-run).
    let gather_store = create_block_store_for_uri(
        &opts.storage_uri,
        BlockStoreOpts {
            access_type: AccessType::ReadOnly,
            worker_count: opts.remote_worker_count,
            cache_dir: None,
            pool: pool()?,
            version_local_store_index: None,
            max_block_bytes: None,
            #[cfg(feature = "s3")]
            s3_options: opts.s3_options.clone(),
        },
    )
    .await?;

    let mut keep: HashSet<u64> = HashSet::new();
    // The gather loop reads one index per retained version and any of them can
    // fail; closing inside the block would leave the store open on that path.
    let gathered = async {
        for (i, path) in opts.source_version_index_paths.iter().enumerate() {
            if path.is_empty() {
                continue;
            }
            let vi = read_version_index(path, &s3).await?;
            let store = &gather_store;
            let blocks = keep_for_version(
                &g,
                i,
                &vi,
                |chunks| async move { Ok(store.get_existing_content(&chunks, 0).await?) },
                &s3,
            )
            .await?;
            if let Some(bs) = blocks {
                keep.extend(bs);
            }
        }
        Ok::<_, LongtailError>(())
    }
    .await;
    crate::store_lifecycle::finish_store(&gather_store, gathered).await?;
    guard_empty_keep_set(
        keep.len(),
        opts.source_version_index_paths.len(),
        opts.allow_empty_keep_set,
    )?;

    if opts.dry_run {
        return Ok(PruneStoreResult {
            dry_run: true,
            keep_blocks: keep.len(),
            pruned_blocks: 0,
        });
    }

    // ReadWrite store for the destructive prune (index overwrite then delete).
    let store = create_block_store_for_uri(
        &opts.storage_uri,
        BlockStoreOpts {
            access_type: AccessType::ReadWrite,
            worker_count: opts.remote_worker_count,
            cache_dir: None,
            pool: pool()?,
            version_local_store_index: None,
            max_block_bytes: None,
            #[cfg(feature = "s3")]
            s3_options: opts.s3_options.clone(),
        },
    )
    .await?;
    let keep_vec: Vec<u64> = keep.iter().copied().collect();
    let pruned = store
        .prune_blocks(&keep_vec)
        .await
        .map_err(LongtailError::from);
    let pruned = crate::store_lifecycle::finish_store(&store, pruned).await?;

    Ok(PruneStoreResult {
        dry_run: false,
        keep_blocks: keep.len(),
        pruned_blocks: pruned,
    })
}

/// Options for [`prune_store_index`].
#[non_exhaustive]
pub struct PruneStoreIndexOptions {
    pub store_index_path: String,
    pub source_version_index_paths: Vec<String>,
    pub version_local_store_index_paths: Vec<String>,
    pub dry_run: bool,
    pub validate_versions: bool,
    pub skip_invalid_versions: bool,
    pub write_version_local_store_index: bool,
    /// Proceed even when the resolved keep-set is empty. See
    /// [`guard_empty_keep_set`].
    pub allow_empty_keep_set: bool,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl PruneStoreIndexOptions {
    pub fn new(
        store_index_path: impl Into<String>,
        source_version_index_paths: Vec<String>,
    ) -> Self {
        PruneStoreIndexOptions {
            store_index_path: store_index_path.into(),
            source_version_index_paths,
            version_local_store_index_paths: Vec::new(),
            dry_run: false,
            validate_versions: false,
            skip_invalid_versions: false,
            write_version_local_store_index: false,
            allow_empty_keep_set: false,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// The outcome of [`prune_store_index`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PruneStoreIndexResult {
    pub dry_run: bool,
    pub old_block_count: u32,
    pub new_block_count: u32,
}

/// `prune-store-index`: recompute the keep-set from the master store index +
/// versions, then (unless `--dry-run`) write the pruned index back. Blocks are
/// never touched.
pub async fn prune_store_index(
    opts: PruneStoreIndexOptions,
) -> Result<PruneStoreIndexResult, LongtailError> {
    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let g = GatherOptions {
        source_version_index_paths: opts.source_version_index_paths.clone(),
        version_local_store_index_paths: opts.version_local_store_index_paths.clone(),
        validate_versions: opts.validate_versions,
        skip_invalid_versions: opts.skip_invalid_versions,
        write_version_local_store_index: opts.write_version_local_store_index,
        dry_run: opts.dry_run,
    };
    check_lsi_len(&g)?;

    let bytes = fs_util::read_from_uri(&opts.store_index_path, &s3).await?;
    let store_index = StoreIndex::from_bytes(&bytes)?;

    let mut keep: HashSet<u64> = HashSet::new();
    for (i, path) in opts.source_version_index_paths.iter().enumerate() {
        if path.is_empty() {
            continue;
        }
        let vi = read_version_index(path, &s3).await?;
        let si = &store_index;
        let blocks = keep_for_version(
            &g,
            i,
            &vi,
            |chunks| async move { Ok(si.get_existing_store_index(&chunks, 0)) },
            &s3,
        )
        .await?;
        if let Some(bs) = blocks {
            keep.extend(bs);
        }
    }

    guard_empty_keep_set(
        keep.len(),
        opts.source_version_index_paths.len(),
        opts.allow_empty_keep_set,
    )?;

    let keep_vec: Vec<u64> = keep.iter().copied().collect();
    let pruned = store_index.prune(&keep_vec);
    let old_block_count = store_index.block_count();
    let new_block_count = pruned.block_count();

    if !opts.dry_run {
        fs_util::write_to_uri(&opts.store_index_path, pruned.to_bytes().into(), &s3).await?;
    }

    Ok(PruneStoreIndexResult {
        dry_run: opts.dry_run,
        old_block_count,
        new_block_count,
    })
}

/// Options for [`prune_store_blocks`].
#[non_exhaustive]
pub struct PruneStoreBlocksOptions {
    pub store_index_path: String,
    pub blocks_root_path: String,
    pub block_extension: String,
    pub dry_run: bool,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl PruneStoreBlocksOptions {
    pub fn new(store_index_path: impl Into<String>, blocks_root_path: impl Into<String>) -> Self {
        PruneStoreBlocksOptions {
            store_index_path: store_index_path.into(),
            blocks_root_path: blocks_root_path.into(),
            block_extension: ".lsb".to_string(),
            dry_run: false,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// The outcome of [`prune_store_blocks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PruneStoreBlocksResult {
    pub dry_run: bool,
    pub found_blocks: usize,
    pub blocks_to_prune: usize,
    pub deleted_blocks: usize,
}

/// Parse a block hash out of an object name: the hex following `0x`
/// (cmd_prunestore_blocks.go:80-102).
fn parse_block_hash(name: &str, extension: &str) -> Option<u64> {
    if !name.ends_with(extension) {
        return None;
    }
    let after = name.rfind("0x").map(|i| &name[i + 2..])?;
    let hex: String = after
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.is_empty() {
        return None;
    }
    u64::from_str_radix(&hex, 16).ok()
}

/// `prune-store-blocks`: delete block files under `blocks_root_path` whose hash
/// is not referenced by the store index. The store index is never rewritten.
pub async fn prune_store_blocks(
    opts: PruneStoreBlocksOptions,
) -> Result<PruneStoreBlocksResult, LongtailError> {
    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let store_index_bytes = fs_util::read_from_uri(&opts.store_index_path, &s3).await?;
    let store_index = StoreIndex::from_bytes(&store_index_bytes)?;
    let used: HashSet<u64> = store_index.block_hashes.iter().copied().collect();

    let blob_store = create_blob_store_for_uri(&opts.blocks_root_path)?;
    let client = blob_store.new_client().await?;
    let objects = client.get_objects("").await?;

    let mut found: Vec<(u64, String)> = Vec::new();
    for obj in objects {
        if let Some(h) = parse_block_hash(&obj.name, &opts.block_extension) {
            found.push((h, obj.name));
        }
    }
    let found_blocks = found.len();

    let unused: Vec<(u64, String)> = found
        .into_iter()
        .filter(|(h, _)| !used.contains(h))
        .collect();
    let blocks_to_prune = unused.len();

    let mut deleted = 0usize;
    if !opts.dry_run {
        for (_, name) in &unused {
            if let Ok(mut obj) = client.new_object(name).await
                && obj.delete().await.is_ok()
            {
                deleted += 1;
            }
        }
    }

    Ok(PruneStoreBlocksResult {
        dry_run: opts.dry_run,
        found_blocks,
        blocks_to_prune,
        deleted_blocks: deleted,
    })
}

async fn read_version_index(uri: &str, s3: &S3OptionsArg) -> Result<VersionIndex, LongtailError> {
    let bytes = fs_util::read_from_uri(uri, s3).await?;
    Ok(VersionIndex::from_bytes(&bytes)?)
}
