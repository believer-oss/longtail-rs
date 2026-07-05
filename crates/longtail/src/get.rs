//! `get` — parse get-config JSON(s) and downsync the referenced version(s).
//! Mirrors `cmd_get.go` defensively: unknown keys ignored; required keys are
//! `storage-uri` + `source-path` only; `version-local-store-index-path` optional.

use crate::downsync::downsync;
use crate::error::LongtailError;
use crate::fs_util::{self, S3OptionsArg};
use crate::options::{DownsyncOptions, DownsyncReport, GetOptions};

/// Read a get-config JSON and downsync it (see [`GetOptions`]).
pub async fn get(opts: GetOptions) -> Result<DownsyncReport, LongtailError> {
    let configs: Vec<String> = opts
        .get_config_paths
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect();
    if configs.is_empty() {
        return Err(LongtailError::InvalidGetConfig(
            "source-path is missing".into(),
        ));
    }

    #[cfg(feature = "s3")]
    let s3: S3OptionsArg = opts.s3_options.clone();
    #[cfg(not(feature = "s3"))]
    let s3: S3OptionsArg = ();

    let mut storage_uri: Option<String> = None;
    let mut source_paths: Vec<String> = Vec::new();
    let mut lsi_paths: Vec<String> = Vec::new();

    for cfg_path in &configs {
        let bytes = fs_util::read_from_uri(cfg_path, &s3).await?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            LongtailError::InvalidGetConfig(format!("get-config `{cfg_path}` is not UTF-8"))
        })?;
        let json: serde_json::Value = serde_json::from_str(text).map_err(|e| {
            LongtailError::InvalidGetConfig(format!("get-config `{cfg_path}` parse error: {e}"))
        })?;

        // storage-uri: required; all configs must agree (cmd_get.go:83-92).
        let su = json
            .get("storage-uri")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                LongtailError::InvalidGetConfig(format!(
                    "missing storage-uri in get-config `{cfg_path}`"
                ))
            })?
            .to_string();
        match &storage_uri {
            None => storage_uri = Some(su),
            Some(first) if *first != su => {
                return Err(LongtailError::InvalidGetConfig(format!(
                    "storage-uri in get-config `{cfg_path}` does not match initial storage-uri `{first}`"
                )));
            }
            Some(_) => {}
        }

        // source-path: required (cmd_get.go:94-99).
        let sp = json
            .get("source-path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                LongtailError::InvalidGetConfig(format!(
                    "missing source-path in get-config `{cfg_path}`"
                ))
            })?
            .to_string();
        source_paths.push(sp);

        // version-local-store-index-path: optional (cmd_get.go:101-106).
        if let Some(lsi) = json
            .get("version-local-store-index-path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            lsi_paths.push(lsi.to_string());
        }
    }

    // If not every config supplied an lsi path, drop them all (cmd_get.go:109-111).
    if lsi_paths.len() != source_paths.len() {
        lsi_paths.clear();
    }

    let storage_uri =
        storage_uri.ok_or_else(|| LongtailError::InvalidGetConfig("missing storage-uri".into()))?;

    // get's own --version-local-store-index-path flag is accepted-but-ignored
    // (cmd_get.go:143-159) — lsi paths come only from the config files.
    let mut ds = DownsyncOptions::new(
        source_paths,
        storage_uri,
        opts.target_path.clone().unwrap_or_default(),
    );
    ds.target_path = opts.target_path;
    ds.version_local_store_index_paths = lsi_paths;
    ds.cache_path = opts.cache_path;
    ds.retain_permissions = opts.retain_permissions;
    ds.validate = opts.validate;
    ds.scan_target = opts.scan_target;
    ds.cache_target_index = opts.cache_target_index;
    ds.target_index_path = opts.target_index_path;
    ds.include_filter_regex = opts.include_filter_regex;
    ds.exclude_filter_regex = opts.exclude_filter_regex;
    ds.worker_count = opts.worker_count;
    ds.remote_worker_count = opts.remote_worker_count;
    ds.enable_file_mapping = opts.enable_file_mapping;
    ds.use_legacy_write = opts.use_legacy_write;
    ds.progress = opts.progress;
    ds.cancel = opts.cancel;
    ds.pool = opts.pool;
    #[cfg(feature = "s3")]
    {
        ds.s3_options = opts.s3_options;
    }

    downsync(ds).await
}
