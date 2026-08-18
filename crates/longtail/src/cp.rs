//! `cp` (`cmd_cp.go`): extract a single asset from a version by fetching only
//! the blocks that cover its chunks and assembling the file. No
//! blockstorestorage port — a targeted block fetch + assemble.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use longtail_store::AccessType;
use longtail_store::block_store::BlockStore;
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

/// Options for [`cp`].
#[non_exhaustive]
pub struct CpOptions {
    pub storage_uri: String,
    pub version_index_path: String,
    /// Asset path INSIDE the version index.
    pub source_path: String,
    /// Destination URI for the assembled file.
    pub target_path: String,
    pub cache_path: Option<PathBuf>,
    pub remote_worker_count: usize,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl CpOptions {
    pub fn new(
        storage_uri: impl Into<String>,
        version_index_path: impl Into<String>,
        source_path: impl Into<String>,
        target_path: impl Into<String>,
    ) -> Self {
        CpOptions {
            storage_uri: storage_uri.into(),
            version_index_path: version_index_path.into(),
            source_path: source_path.into(),
            target_path: target_path.into(),
            cache_path: None,
            remote_worker_count: 0,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// Copy one asset out of a version into `target_path`.
pub async fn cp(opts: CpOptions) -> Result<(), LongtailError> {
    let vi = crate::inspect::read_version_index_from_uri(
        &opts.version_index_path,
        &crate::s3_arg!(opts),
    )
    .await?;

    // Locate the asset by its in-version path.
    let want = opts.source_path.trim_end_matches('/');
    let mut asset: Option<usize> = None;
    for i in 0..vi.asset_count() as usize {
        let p = vi.path(i)?;
        if fs_util::strip_trailing_slash(p) == want {
            asset = Some(i);
            break;
        }
    }
    let asset = asset.ok_or_else(|| {
        LongtailError::InvalidArgument(format!(
            "asset `{}` not found in version index",
            opts.source_path
        ))
    })?;

    // The asset's chunk list (hashes + sizes), in asset order.
    //
    // Sized and iterated from the map itself rather than from the header count.
    // `from_bytes` validates the asset→chunk map, but `VersionIndex`'s fields are
    // `pub`, so a hand-built index reaches here without that check — and
    // `Vec::with_capacity` of an attacker-chosen `u32` is an allocation failure,
    // which aborts the process rather than returning an error.
    let start = vi.asset_chunk_index_starts[asset] as usize;
    let count = vi.asset_chunk_counts[asset] as usize;
    let bad_map = || {
        LongtailError::InvalidArgument(format!(
            "version index asset `{}` names chunks outside the index",
            opts.source_path
        ))
    };
    let indexes = vi
        .asset_chunk_indexes
        .get(start..)
        .and_then(|tail| tail.get(..count))
        .ok_or_else(bad_map)?;
    let mut chunk_hashes: Vec<u64> = Vec::with_capacity(indexes.len());
    let mut chunk_sizes: Vec<u32> = Vec::with_capacity(indexes.len());
    for &ci in indexes {
        let ci = ci as usize;
        chunk_hashes.push(*vi.chunk_hashes.get(ci).ok_or_else(bad_map)?);
        chunk_sizes.push(*vi.chunk_sizes.get(ci).ok_or_else(bad_map)?);
    }

    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: opts.cache_path.clone(),
        pool: Arc::new(crate::version::build_pool(1)?),
        version_local_store_index: None,
        max_block_bytes: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options.clone(),
    };
    let store: Arc<dyn BlockStore> =
        create_block_store_for_uri(&opts.storage_uri, store_opts).await?;

    // Fallible work runs inside this block so the close below happens on a
    // failure too; `cp` shares the block cache and write-back obligations of
    // any other read.
    let assembled = async {
        // Retarget: the store index limited to the blocks covering these chunks.
        let store_index = store.get_existing_content(&chunk_hashes, 0).await?;

        // chunk_hash → (block_hash, byte offset within the decompressed block).
        //
        // A block whose chunk range runs off the arrays is skipped, matching the
        // same walk in `apply.rs`. Both are fed `get_existing_content`, whose
        // output is canonical by construction, so neither guard should ever fire
        // — but the two walks are the same shape over the same public-fielded
        // struct, and having one checked and the other bare reads as an oversight
        // in whichever file you open first.
        let mut location: HashMap<u64, (u64, u64)> = HashMap::new();
        for b in 0..store_index.block_count() as usize {
            let bcount = store_index.block_chunk_counts[b] as usize;
            let boff = store_index.block_chunks_offsets[b] as usize;
            let Some(end) = boff.checked_add(bcount) else {
                continue;
            };
            if end > store_index.chunk_hashes.len() || end > store_index.chunk_sizes.len() {
                continue;
            }
            let mut within: u64 = 0;
            for k in boff..end {
                let ch = store_index.chunk_hashes[k];
                location
                    .entry(ch)
                    .or_insert((store_index.block_hashes[b], within));
                within += store_index.chunk_sizes[k] as u64;
            }
        }

        // Fetch each needed block once (decompressed), then assemble in asset order.
        let mut block_cache: HashMap<u64, Arc<Vec<u8>>> = HashMap::new();
        let mut out: Vec<u8> = Vec::new();
        for (k, &ch) in chunk_hashes.iter().enumerate() {
            let (block_hash, within) = *location.get(&ch).ok_or_else(|| {
                LongtailError::InvalidArgument(format!(
                    "chunk {ch:#018x} of `{}` is not present in the store",
                    opts.source_path
                ))
            })?;
            if let std::collections::hash_map::Entry::Vacant(e) = block_cache.entry(block_hash) {
                let sb = store.get_stored_block(block_hash).await?;
                e.insert(Arc::new(sb.payload));
            }
            let payload = &block_cache[&block_hash];
            let s = within as usize;
            let e = s + chunk_sizes[k] as usize;
            if e > payload.len() {
                return Err(LongtailError::InvalidArgument(format!(
                    "block {block_hash:#018x} shorter than indexed chunk range"
                )));
            }
            out.extend_from_slice(&payload[s..e]);
        }
        Ok::<_, LongtailError>(out)
    }
    .await;
    let out = crate::store_lifecycle::finish_store(&store, assembled).await?;

    fs_util::write_to_uri(&opts.target_path, out.into(), &s3).await?;
    Ok(())
}
