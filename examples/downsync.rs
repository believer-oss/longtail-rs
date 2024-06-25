mod common;

use clap::Parser;
use common::version_index_from_file;
use longtail::*;
use tracing::info;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Storage URI (local file system and S3 bucket(soon!) URI supported)
    #[clap(name = "storage-uri", long)]
    storage_uri: String,

    /// Optional URI for S3 endpoint resolver
    // #[clap(name = "s3-endpoint-resolver-url", long)]
    // s3_endpoint_resolver_url: Option<String>,

    /// Source file uri(s)
    #[clap(name = "source-path", long)]
    source_path: Vec<String>,

    /// Target folder path
    #[clap(name = "target-path", long)]
    target_path: String,

    /// Optional pre-computed index of target-path
    #[clap(name = "target-index-path", long)]
    target_index_path: Option<String>,

    /// Location for cached blocks
    #[clap(name = "cache-path", long)]
    cache_path: Option<String>,

    /// Set permission on file/directories from source
    #[clap(long, default_value = "true")]
    retain_permissions: bool,

    /// Validate target path once completed
    #[clap(long, default_value = "false")]
    validate: bool,

    /// Path(s) to an optimized store index matching the source. If any of the
    /// file(s) cant be read it will fall back to the master store index
    #[clap(name = "version-local-store-index-path", long)]
    version_local_store_index_path: Vec<String>,

    /// Optional include regex filter for assets in --target-path on downsync.
    #[clap(name = "include-filter-regex", long)]
    include_filter_regex: Option<String>,

    /// Optional exclude regex filter for assets in --target-path on downsync.
    #[clap(name = "exclude-filter-regex", long)]
    exclude_filter_regex: Option<String>,

    /// Enables scanning of target folder before write. Disable it to only add/write content to a
    /// folder.
    #[clap(name = "scan-target", long, default_value = "true")]
    scan_target: bool,

    /// Stores a copy version index for the target folder and uses it if it exists, skipping folder scanning
    #[clap(name = "cache-target-index", long, default_value = "true")]
    cache_target_index: bool,

    /// Enabled memory mapped file for file reads and writes
    #[clap(name = "enable-file-mapping", long, default_value = "true")]
    enable_file_mapping: bool,

    /// Uses legacy algorithm to update a version
    #[clap(name = "use-legacy-write", long, default_value = "false")]
    use_legacy_write: bool,
}

#[allow(unused_variables, clippy::too_many_arguments)]
pub fn downsync(
    workers: usize,
    storage_uri: &str,
    // s3_endpoint_resolver_url: &str,
    source_paths: &[String],
    target_path: &str,
    target_index_path: &str,
    cache_path: &str,
    retain_permissions: bool,
    validate: bool,
    version_local_store_index_paths: &[String],
    include_filter_regex: Option<String>,
    exclude_filter_regex: Option<String>,
    scan_target: bool,
    cache_target_index: bool,
    enable_file_mapping: bool,
    use_legacy_write: bool,
) -> Result<(), i32> {
    // Setup the longtail environment
    let jobs = BikeshedJobAPI::new(workers as u32, 1);

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

    // This creates a callback function that is used to check if a given file should be included or
    // excluded from the recursive file scan.
    let regex_path_filter = RegexPathFilter::new(include_filter_regex, exclude_filter_regex)
        .map_err(|err| {
            let err = format!("failed to create regex path filter: {}", err);
            err
        })
        .unwrap();
    let path_filter = PathFilterAPIProxy::new(Box::new(regex_path_filter));

    // TODO: Fixup target-path
    // if targetFolderPath == "" {
    //  normalizedSourceFilePath := longtailstorelib.NormalizeFileSystemPath(sourceFilePaths[0])
    //  normalizedSourceFilePath = strings.ReplaceAll(normalizedSourceFilePath, "\\", "/")
    //  urlSplit := strings.Split(normalizedSourceFilePath, "/")
    //  sourceName := urlSplit[len(urlSplit)-1]
    //  sourceNameSplit := strings.Split(sourceName, ".")
    //  resolvedTargetFolderPath = sourceNameSplit[0]
    //  if resolvedTargetFolderPath == "" {
    //    err = fmt.Errorf("unable to resolve target path using `%s` as base", sourceFilePaths[0])
    //    return storeStats, timeStats, errors.Wrap(err, fname)
    //  }
    // } else {
    //  resolvedTargetFolderPath = targetFolderPath
    // }
    let resolved_target_folder_path = target_path;

    let fs = StorageAPI::new_fs();

    // TODO: This is ugly
    let cache_target_index = if !target_index_path.is_empty() {
        false
    } else {
        cache_target_index
    };

    // TODO: Replace this with PathBuf handling?
    let cache_target_index_path = NormalizeFileSystemPath(
        resolved_target_folder_path.to_owned() + "/.longtail.index.cache.lvi",
    );

    // TODO: This is ugly, and I'm not sure why this is needed
    let target_index_path = if cache_target_index {
        if fs.file_exists(&cache_target_index_path) {
            &cache_target_index_path
        } else {
            target_index_path
        }
    } else {
        target_index_path
    };

    info!(
        "Resolved target folder path: {}",
        resolved_target_folder_path
    );
    // Recursively scan the target folder. TODO: This is async in golongtail
    let mut target_path_scanner = FolderScanner::new();
    target_path_scanner.scan(resolved_target_folder_path, &path_filter, &fs, &jobs);
    info!("Scanned target path");

    let hash_registry = HashRegistry::new();

    // TODO: Handle multiple source paths
    //  var sourceVersionIndex longtaillib.Longtail_VersionIndex
    //  for index, sourceFilePath := range sourceFilePaths {
    //  oneVersionIndex, err := readVersionIndex(sourceFilePath, longtailutils.WithS3EndpointResolverURI(s3EndpointResolverURI))
    //  if err != nil {
    //    err = errors.Wrapf(err, "Cant read version index from `%s`", sourceFilePath)
    //    return storeStats, timeStats, errors.Wrap(err, fname)
    //  }
    //  if index == 0 {
    //    sourceVersionIndex = oneVersionIndex
    //    continue
    //  }
    //  mergedVersionIndex, err := longtaillib.MergeVersionIndex(sourceVersionIndex, oneVersionIndex)
    //  if err != nil {
    //    sourceVersionIndex.Dispose()
    //    oneVersionIndex.Dispose()
    //    err = errors.Wrapf(err, "Cant mnerge version index from `%s`", sourceFilePath)
    //    return storeStats, timeStats, errors.Wrap(err, fname)
    //  }
    //  sourceVersionIndex.Dispose()
    //  oneVersionIndex.Dispose()
    //  sourceVersionIndex = mergedVersionIndex
    // }
    // defer sourceVersionIndex.Dispose()
    let source_filename = source_paths.first().unwrap();
    info!("Reading version index from file: {}", source_filename);
    let source_version_index = version_index_from_file(source_filename);

    // Find the hash type and target chunk size of the source version index
    let hash_id = HashType::from_repr(source_version_index.get_hash_identifier() as usize)
        .expect("Failed to get hash type");
    let target_chunk_size = source_version_index.get_target_chunk_size();

    // This builds an index of the current target directory, which is used to compare against the
    // source version index.
    info!("Building target index");
    let target_index_reader = VersionIndexReader::get_folder_index(
        resolved_target_folder_path,
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
    )
    .unwrap();

    // Setup prerequisites for local file writing
    info!("Setting up local file writing");
    let creg = CompressionRegistry::create_full_compression_registry();
    let localfs = StorageAPI::new_fs();
    // MaxBlockSize and MaxChunksPerBlock are just temporary values until we get the remote index settings
    // remoteIndexStore, err := remotestore.CreateBlockStoreForURI(blobStoreURI, versionLocalStoreIndexPaths, jobs, numWorkerCount, 8388608, 1024, remotestore.ReadOnly, enableFileMapping, longtailutils.WithS3EndpointResolverURI(s3EndpointResolverURI))
    // if err != nil {
    //  return storeStats, timeStats, errors.Wrap(err, fname)
    // }
    // defer remoteIndexStore.Dispose()
    let fake_remotefs = BlockstoreAPI::new_fs(
        &jobs,
        &localfs,
        storage_uri,
        Some(".lsb"),
        enable_file_mapping,
    );

    let (compress_block_store, cache_block_store, local_index_store) = match cache_path.is_empty() {
        true => {
            let block_store = BlockstoreAPI::new_compressed(&fake_remotefs, &creg);
            (block_store, None, None)
        }
        false => {
            let local_index_store =
                BlockstoreAPI::new_fs(&jobs, &localfs, cache_path, Some(""), enable_file_mapping);
            let cache_block_store = BlockstoreAPI::new_compressed(&local_index_store, &creg);
            let block_store = BlockstoreAPI::new_compressed(&cache_block_store, &creg);
            (
                block_store,
                Some(cache_block_store),
                Some(local_index_store),
            )
        }
    };

    // Assuming we're not using legacy writes here.
    let lru_block_store = BlockstoreAPI::new_lru(&compress_block_store, 32);
    let index_store = BlockstoreAPI::new_share(&lru_block_store);

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

    let chunk_hashes = version_diff
        .get_required_chunk_hashes(&source_version_index)
        .expect("Failed to get required chunk hashes");

    let retargetted_version_store_index =
        StoreIndex::get_existing_store_index(&index_store, chunk_hashes, 0).unwrap();

    if cache_target_index && localfs.file_exists(&cache_target_index_path) {
        localfs.delete_file(&cache_target_index_path)?;
    }

    // Setup prerequisites for writing to the target folder
    info!("Setting up target folder writing");
    struct ProgressHandler {}
    impl ProgressAPI for ProgressHandler {
        fn on_progress(&self, _total_count: u32, _done_count: u32) {
            info!("Downsync Progress: {}/{}", _done_count, _total_count);
        }
    }
    let progress = ProgressAPIProxy::new(Box::new(ProgressHandler {}));

    let concurrent_chunk_write_api = ConcurrentChunkWriteAPI::new(
        &localfs,
        &source_version_index,
        &version_diff,
        resolved_target_folder_path,
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
        resolved_target_folder_path,
        true,
    )?;

    // TODO: Handle validate
    // TODO: Handle cache_target_index

    info!("Downsync complete");
    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    set_longtail_loglevel(LONGTAIL_LOG_LEVEL_DEBUG);

    let args = Args::parse();
    downsync(
        1,
        &args.storage_uri,
        // &args.s3_endpoint_resolver_url.unwrap_or_default(),
        &args.source_path,
        &args.target_path,
        &args.target_index_path.unwrap_or_default(),
        &args.cache_path.unwrap_or_default(),
        args.retain_permissions,
        args.validate,
        &args.version_local_store_index_path,
        args.include_filter_regex,
        args.exclude_filter_regex,
        args.scan_target,
        args.cache_target_index,
        args.enable_file_mapping,
        args.use_legacy_write,
    )
    .unwrap();
}
