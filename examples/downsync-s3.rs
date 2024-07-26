mod common;

use longtail_sys::LONGTAIL_LOG_LEVEL_DEBUG;

use clap::Parser;
use longtail::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Storage URI (local file system and S3 bucket(soon!) URI supported)
    #[clap(name = "storage-uri", long)]
    storage_uri: String,

    /// Optional URI for S3 endpoint resolver
    #[clap(name = "s3-endpoint-resolver-url", long)]
    s3_endpoint_resolver_url: Option<String>,

    /// Source file uri(s)
    #[clap(name = "source-path", long)]
    source_paths: Vec<String>,

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
    version_local_store_index_paths: Option<Vec<String>>,

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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();
    set_longtail_loglevel(LONGTAIL_LOG_LEVEL_DEBUG);

    let args = Args::parse();
    downsync(
        10,
        &args.storage_uri,
        &args.s3_endpoint_resolver_url.unwrap_or_default(),
        &args.source_paths,
        &args.target_path,
        &args.target_index_path.unwrap_or_default(),
        &args.cache_path.unwrap_or_default(),
        args.retain_permissions,
        args.validate,
        args.version_local_store_index_paths,
        args.include_filter_regex,
        args.exclude_filter_regex,
        args.scan_target,
        args.cache_target_index,
        args.enable_file_mapping,
        args.use_legacy_write,
        None,
    )
    .unwrap();
}
