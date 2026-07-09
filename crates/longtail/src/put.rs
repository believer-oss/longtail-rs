//! `put` (`cmd_put.go`): derive default storage/`.lvi`/`.lsi` paths from the
//! get-config `--target-path`, upsync, then write the get-config JSON.

use std::sync::Arc;

use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};
use crate::options::{UpsyncOptions, UpsyncReport};
use crate::progress::ProgressSink;
use crate::upsync::upsync;

/// Options for [`crate::put`].
pub struct PutOptions {
    /// The get-config JSON URI (`--target-path`). Defaults for storage/`.lvi`/
    /// `.lsi` are derived from its parent dir + basename-without-extension.
    pub get_config_uri: String,
    /// Explicit target `.lvi` URI; defaults to
    /// `<parent>/version-data/version-index/<name>.lvi`.
    pub target_version_index_path: Option<String>,
    /// Explicit version-local `.lsi` URI; defaults to
    /// `<parent>/version-data/version-store-index/<name>.lsi`.
    pub version_local_store_index_path: Option<String>,
    /// Explicit storage URI; defaults to `<parent>/store`.
    pub storage_uri: Option<String>,
    /// Suppress the version-local `.lsi` entirely (error if combined with an
    /// explicit `version_local_store_index_path`).
    pub no_version_local_store_index: bool,
    /// Source folder to upload.
    pub source_path: String,
    pub source_index_path: Option<String>,
    pub s3_endpoint_resolver_uri: Option<String>,
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
    /// Optional progress sink (forwarded to the underlying upsync).
    pub progress: Option<Arc<dyn ProgressSink>>,
    #[cfg(feature = "s3")]
    pub s3_options: longtail_store::S3Options,
}

impl PutOptions {
    pub fn new(get_config_uri: impl Into<String>, source_path: impl Into<String>) -> PutOptions {
        PutOptions {
            get_config_uri: get_config_uri.into(),
            target_version_index_path: None,
            version_local_store_index_path: None,
            storage_uri: None,
            no_version_local_store_index: false,
            source_path: source_path.into(),
            source_index_path: None,
            s3_endpoint_resolver_uri: None,
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
            #[cfg(feature = "s3")]
            s3_options: longtail_store::S3Options::default(),
        }
    }
}

/// Split a URI at the last `/` or `\` (cmd_put.go:64-70). `("."`, whole) when
/// there is no delimiter.
fn split_parent(uri: &str) -> (String, String) {
    match uri.rfind(['/', '\\']) {
        Some(i) => (uri[..i].to_string(), uri[i + 1..].to_string()),
        None => (".".to_string(), uri.to_string()),
    }
}

/// Strip the last `.`-extension (cmd_put.go:71-75).
fn strip_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) => name[..i].to_string(),
        None => name.to_string(),
    }
}

/// `put`: derive default paths, upsync, then write the get-config JSON.
pub async fn put(opts: PutOptions) -> Result<UpsyncReport, LongtailError> {
    let (parent, target_name) = split_parent(&opts.get_config_uri);
    let config_name = strip_extension(&target_name);

    // storage-uri default (cmd_put.go:77-79).
    let storage_uri = match opts.storage_uri.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => format!("{parent}/store"),
    };
    // target .lvi default (cmd_put.go:81-83).
    let target_lvi = match opts
        .target_version_index_path
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => format!("{parent}/version-data/version-index/{config_name}.lvi"),
    };
    // .lsi default + conflict error (cmd_put.go:85-91).
    let explicit_lsi = opts
        .version_local_store_index_path
        .as_deref()
        .filter(|s| !s.is_empty());
    let lsi_path: Option<String> = match explicit_lsi {
        None => {
            if opts.no_version_local_store_index {
                None
            } else {
                Some(format!(
                    "{parent}/version-data/version-store-index/{config_name}.lsi"
                ))
            }
        }
        Some(path) => {
            if opts.no_version_local_store_index {
                return Err(LongtailError::InvalidArgument(format!(
                    "put: conflicting options for version local store index, \
                     --no-version-local-store-index is set together with path `{path}`"
                )));
            }
            Some(path.to_string())
        }
    };

    let mut up = UpsyncOptions::new(&opts.source_path, &storage_uri, &target_lvi);
    up.source_index_path = opts.source_index_path.clone();
    up.version_local_store_index_path = lsi_path.clone();
    up.target_chunk_size = opts.target_chunk_size;
    up.max_chunks_per_block = opts.max_chunks_per_block;
    up.target_block_size = opts.target_block_size;
    up.min_block_usage_percent = opts.min_block_usage_percent;
    up.compression_algorithm = opts.compression_algorithm.clone();
    up.hash_algorithm = opts.hash_algorithm.clone();
    up.include_filter_regex = opts.include_filter_regex.clone();
    up.exclude_filter_regex = opts.exclude_filter_regex.clone();
    up.worker_count = opts.worker_count;
    up.remote_worker_count = opts.remote_worker_count;
    up.enable_file_mapping = opts.enable_file_mapping;
    up.use_legacy_write = opts.use_legacy_write;
    up.progress = opts.progress.clone();
    #[cfg(feature = "s3")]
    {
        up.s3_options = opts.s3_options.clone();
    }

    let report = upsync(up).await?;

    // Write the get-config JSON (cmd_put.go:115-146). Only on success.
    let mut map = serde_json::Map::new();
    map.insert("storage-uri".to_string(), storage_uri.clone().into());
    if let Some(ep) = opts
        .s3_endpoint_resolver_uri
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        map.insert("s3-endpoint-resolver-uri".to_string(), ep.into());
    }
    // The get-config `source-path` key = the target `.lvi` URI (cmd_put.go:124).
    map.insert("source-path".to_string(), target_lvi.clone().into());
    if let Some(lsi) = &lsi_path {
        map.insert(
            "version-local-store-index-path".to_string(),
            lsi.clone().into(),
        );
    }
    let json = serde_json::Value::Object(map);
    // viper writes pretty JSON with sorted keys; serde_json's Map preserves
    // insertion order, but the get-config is consumed key-by-key (get.rs reads
    // named keys), so ordering is not compat-critical here.
    let bytes = serde_json::to_vec_pretty(&json)
        .map_err(|e| LongtailError::InvalidArgument(format!("serialize get-config: {e}")))?;

    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();
    fs_util::write_to_uri(&opts.get_config_uri, &bytes, &s3).await?;

    Ok(report)
}
