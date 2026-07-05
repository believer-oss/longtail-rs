//! Read-only inspection entry points used by `ls` / `print-version` /
//! `validate-version`.

use std::sync::Arc;

use longtail_core::{VersionIndex, validate_store};
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
fn default_s3() -> S3OptionsArg {}

/// Read + parse a version index from a URI (local path, `file://`, or `s3://`).
pub async fn read_version_index_from_uri(uri: &str) -> Result<VersionIndex, LongtailError> {
    let bytes = fs_util::read_from_uri(uri, &default_s3()).await?;
    Ok(VersionIndex::from_bytes(&bytes)?)
}

/// Options for [`validate_version`].
pub struct ValidateVersionOptions {
    pub storage_uri: String,
    pub version_index_path: String,
    pub remote_worker_count: usize,
    #[cfg(feature = "s3")]
    pub s3_options: S3OptionsArg,
}

impl ValidateVersionOptions {
    pub fn new(storage_uri: impl Into<String>, version_index_path: impl Into<String>) -> Self {
        ValidateVersionOptions {
            storage_uri: storage_uri.into(),
            version_index_path: version_index_path.into(),
            remote_worker_count: 0,
            #[cfg(feature = "s3")]
            s3_options: default_s3(),
        }
    }
}

/// `validate-version`: confirm the store covers every chunk the version needs
/// (`GetExistingStoreIndex(all chunks, min-usage 0)` + `ValidateStore`,
/// cmd_validateversion.go:61-74).
pub async fn validate_version(opts: ValidateVersionOptions) -> Result<(), LongtailError> {
    let vi = read_version_index_from_uri(&opts.version_index_path).await?;
    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .map_err(|e| LongtailError::InvalidArgument(format!("rayon pool: {e}")))?,
    );
    let store_opts = BlockStoreOpts {
        access_type: AccessType::ReadOnly,
        worker_count: opts.remote_worker_count,
        cache_dir: None,
        pool,
        version_local_store_index: None,
        #[cfg(feature = "s3")]
        s3_options: opts.s3_options,
    };
    let store: Arc<dyn BlockStore> =
        create_block_store_for_uri(&opts.storage_uri, store_opts).await?;
    let store_index = store.get_existing_content(&vi.chunk_hashes, 0).await?;
    store.close().await?;
    validate_store(&store_index, &vi).map_err(LongtailError::from)
}
