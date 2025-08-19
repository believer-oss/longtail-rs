use crate::file_cache::FileCacheData;
use crate::{
    create_block_store_for_uri, get_files_recursively, normalize_file_system_path, read_from_uri,
    AccessType, BikeshedJobAPI, BlockstoreAPI, ChunkerAPI, CompressionRegistry,
    ConcurrentChunkWriteAPI, FolderScanner, HashRegistry, HashType, PathFilterAPIProxy,
    ProgressAPI, ProgressAPIProxy, RegexPathFilter, S3Options, StorageAPI, StoreIndex, VersionDiff,
    VersionIndex, VersionIndexReader, LONGTAIL_NO_COMPRESSION_TYPE,
};

use crate::error::LongtailError;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};

#[allow(clippy::too_many_arguments)]
pub fn downsync(
    workers: usize,
    storage_uri: &str,
    s3_endpoint_resolver_url: Option<String>,
    s3_transfer_acceleration: Option<bool>,
    source_paths: &[String],
    target_path: &str,
    target_index_path: &str,
    cache_path: Option<&Path>,
    retain_permissions: bool,
    validate: bool,
    version_local_store_index_paths: Option<Vec<String>>,
    include_filter_regex: Option<String>,
    exclude_filter_regex: Option<String>,
    _scan_target: bool,
    cache_target_index: bool,
    enable_file_mapping: bool,
    _use_legacy_write: bool,
    progress_api: Option<Box<dyn ProgressAPI>>,
) -> Result<(), LongtailError> {
    // Setup the longtail environment
    let jobs = BikeshedJobAPI::new(workers as u32, 1);

    let s3_options = Some(S3Options::new(
        s3_endpoint_resolver_url,
        s3_transfer_acceleration,
    ));

    // TODO: Validate source-path
    // if sourceFilePath != "" {
    //  sourceFilePaths = []string{sourceFilePath}
    // }
    //
    // if len(sourceFilePaths) < 1 {
    //  err := fmt.Errorf("please provide at least one source path uri")
    //  return storeStats, timeStats, errors.Wrap(err, fname)
    // }
    // if sourceFilePaths[0] == "" {
    //  err := fmt.Errorf("please provide at least one source path uri")
    //  return storeStats, timeStats, errors.Wrap(err, fname)
    // }

    // This creates a callback function that is used to check if a given file should
    // be included or excluded from the recursive file scan.
    let path_filter = match RegexPathFilter::new(include_filter_regex, exclude_filter_regex) {
        Ok(regex_path_filter) => {
            info!("Using regex path filter");
            PathFilterAPIProxy::new(Box::new(regex_path_filter))
        }
        Err(e) => {
            error!("Failed to create regex path filter: {}", e);
            return Err(Into::into(-1));
        }
    };

    // If passed a source path but no target path, use the base of the source path
    // (without file extension) as the target path.
    // FIXME: This path handling should be converted to (dunce)[https://crates.io/crates/dunce]
    let resolved_target_folder_path = if target_path.is_empty() {
        let path = normalize_file_system_path(source_paths[0].clone())
            .replace('\\', "/")
            .split('/')
            .next_back()
            .and_then(|v| v.split('.').next().map(|v| v.to_owned()));
        match path {
            Some(path) if !path.is_empty() => Ok(path),
            _ => {
                error!(
                    "Unable to resolve target path using `{}` as base",
                    source_paths[0]
                );
                Err(-1)
            }
        }?
    } else {
        target_path.to_owned()
    };

    let fs = StorageAPI::new_fs();

    // TODO: This is ugly - If we pass a target_index_path, force cache_target_index
    // to false.
    let cache_target_index = if !target_index_path.is_empty() {
        false
    } else {
        cache_target_index
    };

    // TODO: Replace this with PathBuf handling?
    let cache_target_index_path = normalize_file_system_path(
        resolved_target_folder_path.to_owned() + "/.longtail.index.cache.lvi",
    );

    // TODO: This is ugly, and I'm not sure why this is needed
    let target_index_path = if cache_target_index {
        if fs.file_exists(&cache_target_index_path) {
            info!("Using cached target index");
            &cache_target_index_path
        } else {
            info!("Using target index path");
            target_index_path
        }
    } else {
        info!("Using target index path - cache_target_index is false");
        target_index_path
    };

    info!(
        "Resolved target folder path: {}",
        resolved_target_folder_path
    );
    // Recursively scan the target folder.
    // TODO: This is async in golongtail, and is contingent on `scan_target &&
    // target_index_path == ""`
    let target_path_scanner =
        FolderScanner::scan(&resolved_target_folder_path, &path_filter, &fs, &jobs)?;
    info!("Scanned target path");

    let hash_registry = HashRegistry::new();

    // TODO: Handle multiple source paths
    //  var sourceVersionIndex longtaillib.Longtail_VersionIndex
    //  for index, sourceFilePath := range sourceFilePaths {
    //  oneVersionIndex, err := readVersionIndex(sourceFilePath,
    // longtailutils.WithS3EndpointResolverURI(s3EndpointResolverURI))
    //  if err != nil {
    //    err = errors.Wrapf(err, "Cant read version index from `%s`",
    // sourceFilePath)    return storeStats, timeStats, errors.Wrap(err, fname)
    //  }
    //  if index == 0 {
    //    sourceVersionIndex = oneVersionIndex
    //    continue
    //  }
    //  mergedVersionIndex, err := longtaillib.MergeVersionIndex(sourceVersionIndex,
    // oneVersionIndex)  if err != nil {
    //    sourceVersionIndex.Dispose()
    //    oneVersionIndex.Dispose()
    //    err = errors.Wrapf(err, "Cant mnerge version index from `%s`",
    // sourceFilePath)    return storeStats, timeStats, errors.Wrap(err, fname)
    //  }
    //  sourceVersionIndex.Dispose()
    //  oneVersionIndex.Dispose()
    //  sourceVersionIndex = mergedVersionIndex
    // }
    // defer sourceVersionIndex.Dispose()

    let source_version_index = {
        // TODO: Handle multiple source paths
        match source_paths.first() {
            Some(uri) => {
                info!("Reading version index from object: {}", uri);
                let mut buf = read_from_uri(uri, s3_options.clone()).map_err(|err| {
                    let err = format!("failed to read object: {err}");
                    error!("{}", err);
                    1
                })?;
                VersionIndex::new_from_buffer(&mut buf).map_err(|err| {
                    let err = format!("failed to create version index: {err}");
                    error!("{}", err);
                    1
                })?
            }
            None => {
                error!("No source paths provided");
                return Err(Into::into(-1));
            }
        }
    };
    debug!("Source version index: {:?}", source_version_index);

    // Find the hash type and target chunk size of the source version index
    let hash_id = HashType::from_repr(source_version_index.get_hash_identifier() as usize)
        .expect("Failed to get hash type");
    let target_chunk_size = source_version_index.get_target_chunk_size();

    // This builds an index of the current target directory, which is used to
    // compare against the source version index.
    info!("Building target index");
    let target_index_reader = VersionIndexReader::get_folder_index(
        &resolved_target_folder_path,
        target_index_path,
        target_chunk_size,
        LONGTAIL_NO_COMPRESSION_TYPE,
        hash_id as u32,
        &path_filter,
        &fs,
        &jobs,
        &hash_registry,
        enable_file_mapping,
        &target_path_scanner,
    )?;

    // Setup prerequisites for local file writing
    info!("Setting up local file writing");
    let creg = CompressionRegistry::new();
    let localfs = StorageAPI::new_fs();

    // MaxBlockSize and MaxChunksPerBlock are just temporary values until we get the
    // remote index settings remoteIndexStore, err :=
    // remotestore.CreateBlockStoreForURI(blobStoreURI, versionLocalStoreIndexPaths,
    // jobs, numWorkerCount, 8388608, 1024, remotestore.ReadOnly, enableFileMapping,
    // longtailutils.WithS3EndpointResolverURI(s3EndpointResolverURI)) if err !=
    // nil {  return storeStats, timeStats, errors.Wrap(err, fname)
    // }
    // defer remoteIndexStore.Dispose()
    // let fake_remotefs = BlockstoreAPI::new_fs(
    //     &jobs,
    //     &localfs,
    //     storage_uri,
    //     Some(".lsb"),
    //     enable_file_mapping,
    // );

    // TODO: Handle multiple source paths
    let remote_index_store = create_block_store_for_uri(
        storage_uri,
        version_local_store_index_paths,
        Some(&jobs),
        1,
        AccessType::ReadOnly,
        enable_file_mapping,
        s3_options,
    )
    .map_err(|err| {
        error!("Failed to create block store: {}", err);
        -1
    })?;

    let compress_block_store = match cache_path {
        None => BlockstoreAPI::new_compressed(Box::new(remote_index_store), &creg),
        Some(cache_path) => {
            let local_index_store =
                BlockstoreAPI::new_fs(&jobs, &localfs, cache_path, "", enable_file_mapping);
            let cache_block_store =
                BlockstoreAPI::new_cached(&jobs, &local_index_store, &remote_index_store);
            BlockstoreAPI::new_compressed(Box::new(cache_block_store), &creg)
        }
    };

    // TODO: disabled these for now...
    // // Assuming we're not using legacy writes here.
    // let lru_block_store = BlockstoreAPI::new_lru(&compress_block_store, 32);
    // let index_store = BlockstoreAPI::new_share(&lru_block_store);
    let index_store = BlockstoreAPI::new_share(&compress_block_store);
    // let index_store = compress_block_store;

    // this appears to just be validating that we can get the hash id
    let _hash = hash_registry
        .get_hash_api(hash_id)
        .expect("Failed to get hash API");

    // Use the computed index to diff against the source index
    info!("Diffing source and target indexes");
    let target_index_version = target_index_reader.version_index;
    let target_hash = target_index_reader.hash_api;
    let version_diff =
        VersionDiff::diff(&target_hash, &target_index_version, &source_version_index)
            .expect("Failed to diff versions");

    debug!("Source version index: {:?}", source_version_index);
    debug!("Target version index: {:?}", target_index_version);
    debug!("Version diff: {:?}", version_diff);

    let chunk_hashes = version_diff
        .get_required_chunk_hashes(&source_version_index)
        .expect("Failed to get required chunk hashes");

    let retargetted_version_store_index =
        StoreIndex::get_existing_store_index_sync(&index_store, chunk_hashes, 0).map_err(
            |err| {
                error!("Failed to retarget version store index: {}", err);
                -1
            },
        )?;
    debug!(
        "Retargetted version store index: {:?}",
        retargetted_version_store_index
    );
    debug!(
        "Retargetted version store index ptr: {:p}",
        std::ptr::addr_of!(retargetted_version_store_index)
    );

    if cache_target_index && localfs.file_exists(&cache_target_index_path) {
        localfs.delete_file(&cache_target_index_path)?;
    }

    // Setup prerequisites for writing to the target folder
    info!("Setting up target folder writing");

    let progress_api = progress_api.unwrap_or_else(|| {
        struct ProgressHandler {}
        impl ProgressAPI for ProgressHandler {
            fn on_progress(&self, _total_count: u32, _done_count: u32) {
                info!("Downsync Progress: {}/{}", _done_count, _total_count);
            }
        }
        Box::new(ProgressHandler {})
    });
    let progress = ProgressAPIProxy::new(progress_api);

    let concurrent_chunk_write_api = ConcurrentChunkWriteAPI::new(
        &localfs,
        &source_version_index,
        &version_diff,
        &resolved_target_folder_path,
    );

    info!("Writing to target folder");
    index_store.change_version(
        &localfs,
        &concurrent_chunk_write_api,
        &target_hash,
        &jobs,
        &progress,
        &retargetted_version_store_index,
        &target_index_version,
        &source_version_index,
        &version_diff,
        &resolved_target_folder_path,
        true,
    )?;

    // TODO: FlushStoresSync index_store, cache_block_store, local_index_store

    if validate {
        // Validate the target folder
        info!("Validating target folder");
        let validate_file_infos =
            get_files_recursively(&localfs, &jobs, &path_filter, &resolved_target_folder_path)?;
        let chunker = ChunkerAPI::new();

        struct ProgressHandler {}
        impl ProgressAPI for ProgressHandler {
            fn on_progress(&self, _total_count: u32, _done_count: u32) {
                info!("Validate Progress: {}/{}", _done_count, _total_count);
            }
        }
        let progress = ProgressAPIProxy::new(Box::new(ProgressHandler {}));

        let validate_version_index = VersionIndex::new_from_fileinfos(
            &localfs,
            &target_hash,
            &chunker,
            &jobs,
            &progress,
            &resolved_target_folder_path,
            validate_file_infos,
            target_chunk_size,
            enable_file_mapping,
        )?;

        if validate_version_index.get_asset_count() != source_version_index.get_asset_count() {
            error!("Validation failed: asset count mismatch");
            return Err(Into::into(-1));
        }

        let validate_asset_sizes = validate_version_index.get_asset_sizes();
        let validate_asset_hashes = validate_version_index.get_asset_hashes();

        let source_asset_sizes = source_version_index.get_asset_sizes();
        let source_asset_hashes = source_version_index.get_asset_hashes();

        let mut asset_size_lookup = HashMap::new();
        let mut asset_hash_lookup = HashMap::new();
        let mut asset_permissions_lookup = HashMap::new();

        for (i, size) in source_asset_sizes.iter().enumerate() {
            let path = source_version_index.get_asset_path(i as u32);
            asset_size_lookup.insert(path.clone(), size);
            asset_hash_lookup.insert(path.clone(), source_asset_hashes[i]);
            asset_permissions_lookup
                .insert(path, source_version_index.get_asset_permissions(i as u32));
        }
        info!("Source asset sizes loaded");
        for (i, validate_size) in validate_asset_sizes.iter().enumerate() {
            let validate_path = validate_version_index.get_asset_path(i as u32);
            let validate_hash = validate_asset_hashes[i];
            let size = asset_size_lookup.get(&validate_path);
            if let Some(size) = size {
                if validate_size != *size {
                    error!(
                        "Validation failed: asset size mismatch for `{}`",
                        validate_path
                    );
                    error!("Expected: {}, Got: {}", size, validate_size);
                    // return Err(-1);
                }
                if validate_hash != asset_hash_lookup[&validate_path] {
                    error!(
                        "Validation failed: asset hash mismatch for `{}`",
                        validate_path
                    );
                    error!(
                        "Expected: {}, Got: {}",
                        asset_hash_lookup[&validate_path], validate_hash
                    );

                    // return Err(-1);
                }
                if retain_permissions {
                    let permissions = asset_permissions_lookup[&validate_path];
                    let file_permissions = validate_version_index.get_asset_permissions(i as u32);
                    if file_permissions != permissions {
                        // error!(
                        //     "Validation failed: asset permissions mismatch
                        // for `{}`",     validate_path
                        // );
                        // error!("Expected: {:o}, Got: {:o}", permissions,
                        // file_permissions);
                        // return Err(-1);
                    }
                }
            } else {
                error!(
                    "Validation failed: asset `{}` not found in source index",
                    validate_path
                );
                return Err(Into::into(-1));
            }
            info!("Validation passed for {}", validate_path);
        }
    }

    // Cache the source target index locally
    if cache_target_index {
        localfs.write_version_index(&source_version_index, &cache_target_index_path)?;
    }

    info!("Downsync complete");
    Ok(())
}

// Present the previous API for usage without a cache
pub fn get(
    url: &str,
    target_path: &str,
    progress_api: Option<Box<dyn ProgressAPI>>,
) -> Result<(), LongtailError> {
    get_with_cache(url, target_path, progress_api, None)
}

pub struct CacheControl {
    path: PathBuf,
    max_size_bytes: u64,
}

impl CacheControl {
    pub fn new(path: &Path, max_size_bytes: u64) -> Self {
        Self {
            path: path.to_path_buf(),
            max_size_bytes,
        }
    }
}

pub fn get_with_cache(
    url: &str,
    target_path: &str,
    progress_api: Option<Box<dyn ProgressAPI>>,
    cache: Option<CacheControl>,
) -> Result<(), LongtailError> {
    // Hardcoding here for now, to keep the API stable
    let s3_transfer_acceleration = Some(true);
    let s3_endpoint_resolver_url = None;

    let s3_options = Some(S3Options::new(
        s3_endpoint_resolver_url.clone(),
        s3_transfer_acceleration,
    ));

    let buf = read_from_uri(url, s3_options).map_err(LongtailError::Misc)?;
    let s = std::str::from_utf8(&buf).map_err(LongtailError::UTF8Error)?;
    let json = serde_json::from_str::<serde_json::Value>(s).map_err(LongtailError::JSONError)?;

    let source_path = json["source-path"]
        .as_str()
        .ok_or(LongtailError::JSONInvalid(String::from(
            "source-path missing or invalid",
        )))?;
    let storage_uri = json["storage-uri"]
        .as_str()
        .ok_or(LongtailError::JSONInvalid(String::from(
            "storage-uri missing or invalid",
        )))?;
    let version_local_store_index_path =
        json["version-local-store-index-path"]
            .as_str()
            .ok_or(LongtailError::JSONInvalid(String::from(
                "version-local-store-index-path missing or invalid",
            )))?;

    let cache_path = cache.as_ref().map(|cache| cache.path.clone());

    downsync(
        32,
        storage_uri,
        s3_endpoint_resolver_url,
        s3_transfer_acceleration,
        &[source_path.to_string()],
        target_path,
        "",
        cache_path.as_deref(),
        false,
        false,
        Some(vec![version_local_store_index_path.to_string()]),
        None,
        None,
        false, // This defaults to true in golongtail
        false, // This defaults to true in golongtail
        false,
        false,
        progress_api,
    )?;

    if let Some(cache) = &cache {
        let mut all_files = FileCacheData::collect(&cache.path.join("chunks"));
        let total_size = all_files.iter().fold(0, |acc, entry| acc + entry.size);

        if total_size > cache.max_size_bytes {
            info!(
                "File cache total size {} is over threshold {} by {} bytes. Purging old chunks...",
                total_size,
                cache.max_size_bytes,
                total_size - cache.max_size_bytes
            );
            all_files.sort_by(|a, b| -> Ordering {
                let time_ord = a.timestamp.partial_cmp(&b.timestamp);
                if time_ord == Some(Ordering::Equal) {
                    return a.size.partial_cmp(&b.size).unwrap();
                }
                time_ord.unwrap()
            });

            let mut current_size = total_size;
            for f in all_files.iter() {
                info!(
                    "Deleting chunk {:?} with size {} (total {} -> {}, threshold {})",
                    f.path,
                    f.size,
                    current_size,
                    current_size - f.size,
                    cache.max_size_bytes
                );
                if let Err(e) = std::fs::remove_file(&f.path) {
                    warn!("Unable to delete file {:?}: {:?}", &f.path, e);
                }
                current_size -= f.size;
                if current_size <= cache.max_size_bytes {
                    break;
                }
            }
        }
    }
    Ok(())
}
