//! `longtail` CLI — a drop-in replacement for the golongtail download-path
//! commands (`downsync`, `get`, `ls`, `validate-version`, `print-version`).
//! Flag names match the pinned golongtail v0.4.5 `--help`.

#![forbid(unsafe_code)]

mod progress;

use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use longtail::{
    DownsyncOptions, GetOptions, ValidateVersionOptions, downsync, get,
    read_version_index_from_uri, validate_version,
};
use longtail_core::VersionIndex;

use crate::progress::CliProgress;

#[derive(Parser)]
#[command(
    name = "longtail",
    version,
    about = "Pure-Rust longtail CLI (drop-in for golongtail)"
)]
struct Cli {
    /// CPU worker count (chunk/hash); 0 = logical CPUs.
    #[arg(long, global = true, default_value_t = 0)]
    worker_count: usize,
    /// Remote block-I/O worker count; 0 = scheme default.
    #[arg(long, global = true, default_value_t = 0)]
    remote_worker_count: usize,
    /// Log level (accepted for parity; the CLI logs to stderr on error).
    #[arg(long, global = true, default_value = "warn")]
    log_level: String,
    /// Print store/time stats after the operation.
    #[arg(long, global = true, default_value_t = false)]
    show_stats: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Download a version into a target folder.
    Downsync(DownsyncArgs),
    /// Read a get-config JSON and download the referenced version(s).
    Get(GetArgs),
    /// List the contents of a path inside a version index.
    Ls(LsArgs),
    /// Confirm the store covers everything a version needs.
    ValidateVersion(ValidateArgs),
    /// Print a summary of a version index.
    PrintVersion(PrintArgs),
    /// Upload a folder into a store, writing the version index.
    Upsync(UpsyncArgs),
    /// Upsync a folder and write a get-config JSON (path-defaulting).
    Put(PutArgs),
    /// Open/create a remote store and force-rebuild the store index.
    InitRemoteStore(InitArgs),
    /// Create a store index optimized for a version index.
    CreateVersionStoreIndex(CreateVsiArgs),
    /// Prune the store index and delete orphan blocks for a version set.
    PruneStore(PruneStoreArgs),
    /// Prune only the store index for a version set.
    PruneStoreIndex(PruneStoreIndexArgs),
    /// Delete block files not referenced by the store index.
    PruneStoreBlocks(PruneStoreBlocksArgs),
    /// Clone versions from one store to another (materialize + re-upload).
    CloneStore(CloneStoreArgs),
    /// Print info about a store index.
    PrintStore(PrintStoreArgs),
    /// Show block usage and asset fragmentation for a version.
    PrintVersionUsage(PrintVersionUsageArgs),
    /// List all asset paths inside a version index.
    DumpVersionAssets(DumpVersionAssetsArgs),
    /// Copy one asset out of a version into a local file.
    Cp(CpArgs),
}

#[derive(Args)]
struct DownsyncArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    source_path: Option<String>,
    #[arg(long, value_delimiter = '|')]
    source_paths: Vec<String>,
    #[arg(long)]
    target_path: Option<String>,
    #[arg(long)]
    target_index_path: Option<String>,
    #[arg(long)]
    cache_path: Option<String>,
    /// Cap the local block cache; LRU-evict after the download (e.g. `2GiB`, `500MB`).
    #[arg(long, value_parser = parse_size)]
    cache_size_limit: Option<u64>,
    #[arg(long, default_value_t = false)]
    retain_permissions: bool,
    #[arg(long, default_value_t = false)]
    no_retain_permissions: bool,
    #[arg(long, default_value_t = false)]
    validate: bool,
    #[arg(long)]
    version_local_store_index_path: Option<String>,
    #[arg(long, value_delimiter = '|')]
    version_local_store_index_paths: Vec<String>,
    #[arg(long)]
    include_filter_regex: Option<String>,
    #[arg(long)]
    exclude_filter_regex: Option<String>,
    #[arg(long, default_value_t = false)]
    scan_target: bool,
    #[arg(long, default_value_t = false)]
    no_scan_target: bool,
    #[arg(long, default_value_t = false)]
    cache_target_index: bool,
    #[arg(long, default_value_t = false)]
    no_cache_target_index: bool,
    #[arg(long, default_value_t = false)]
    enable_file_mapping: bool,
    #[arg(long, default_value_t = false)]
    use_legacy_write: bool,
}

#[derive(Args)]
struct GetArgs {
    #[arg(long)]
    source_path: Option<String>,
    #[arg(long, value_delimiter = '|')]
    source_paths: Vec<String>,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    target_path: Option<String>,
    #[arg(long)]
    target_index_path: Option<String>,
    #[arg(long)]
    cache_path: Option<String>,
    /// Cap the local block cache; LRU-evict after the download (e.g. `2GiB`, `500MB`).
    #[arg(long, value_parser = parse_size)]
    cache_size_limit: Option<u64>,
    #[arg(long, default_value_t = false)]
    retain_permissions: bool,
    #[arg(long, default_value_t = false)]
    no_retain_permissions: bool,
    #[arg(long, default_value_t = false)]
    validate: bool,
    /// Accepted-but-ignored (parity with golongtail; lsi comes from the config).
    #[arg(long)]
    version_local_store_index_path: Option<String>,
    #[arg(long)]
    include_filter_regex: Option<String>,
    #[arg(long)]
    exclude_filter_regex: Option<String>,
    #[arg(long, default_value_t = false)]
    scan_target: bool,
    #[arg(long, default_value_t = false)]
    no_scan_target: bool,
    #[arg(long, default_value_t = false)]
    cache_target_index: bool,
    #[arg(long, default_value_t = false)]
    no_cache_target_index: bool,
    #[arg(long, default_value_t = false)]
    enable_file_mapping: bool,
    #[arg(long, default_value_t = false)]
    use_legacy_write: bool,
}

#[derive(Args)]
struct LsArgs {
    #[arg(long)]
    version_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    /// Directory inside the version to list (`.` or empty = root).
    path: Option<String>,
}

#[derive(Args)]
struct ValidateArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    version_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
}

#[derive(Args)]
struct PrintArgs {
    #[arg(long)]
    version_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long, default_value_t = false)]
    compact: bool,
}

#[derive(Args)]
struct UpsyncArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    source_path: String,
    #[arg(long)]
    source_index_path: Option<String>,
    #[arg(long)]
    target_path: String,
    #[arg(long)]
    version_local_store_index_path: Option<String>,
    #[arg(long, default_value_t = 32768)]
    target_chunk_size: u32,
    #[arg(long, default_value_t = 1024)]
    max_chunks_per_block: u32,
    #[arg(long, default_value_t = 8388608)]
    target_block_size: u32,
    #[arg(long, default_value_t = 80)]
    min_block_usage_percent: u32,
    #[arg(long, default_value = "zstd")]
    compression_algorithm: String,
    #[arg(long, default_value = "blake3")]
    hash_algorithm: String,
    #[arg(long)]
    include_filter_regex: Option<String>,
    #[arg(long)]
    exclude_filter_regex: Option<String>,
    #[arg(long, default_value_t = false)]
    enable_file_mapping: bool,
    #[arg(long, default_value_t = false)]
    use_legacy_write: bool,
}

#[derive(Args)]
struct PutArgs {
    #[arg(long)]
    target_path: String,
    #[arg(long)]
    target_version_index_path: Option<String>,
    #[arg(long)]
    version_local_store_index_path: Option<String>,
    #[arg(long)]
    storage_uri: Option<String>,
    #[arg(long, default_value_t = false)]
    no_version_local_store_index: bool,
    #[arg(long)]
    source_path: String,
    #[arg(long)]
    source_index_path: Option<String>,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long, default_value_t = 32768)]
    target_chunk_size: u32,
    #[arg(long, default_value_t = 8388608)]
    target_block_size: u32,
    #[arg(long, default_value_t = 1024)]
    max_chunks_per_block: u32,
    #[arg(long, default_value = "zstd")]
    compression_algorithm: String,
    #[arg(long, default_value = "blake3")]
    hash_algorithm: String,
    #[arg(long)]
    include_filter_regex: Option<String>,
    #[arg(long)]
    exclude_filter_regex: Option<String>,
    #[arg(long, default_value_t = 80)]
    min_block_usage_percent: u32,
    #[arg(long, default_value_t = false)]
    enable_file_mapping: bool,
}

#[derive(Args)]
struct InitArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long, default_value = "blake3")]
    hash_algorithm: String,
}

#[derive(Args)]
struct CreateVsiArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    source_path: String,
    #[arg(long)]
    version_local_store_index_path: String,
}

#[derive(Args)]
struct PruneStoreArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    /// Path to a text file listing source version-index URIs (one per line).
    #[arg(long)]
    source_paths: String,
    /// Path to a text file listing version-local store-index URIs (one per line).
    #[arg(long)]
    version_local_store_index_paths: Option<String>,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    #[arg(long, default_value_t = false)]
    write_version_local_store_index: bool,
    #[arg(long, default_value_t = false)]
    validate_versions: bool,
    #[arg(long, default_value_t = false)]
    skip_invalid_versions: bool,
}

#[derive(Args)]
struct PruneStoreIndexArgs {
    #[arg(long)]
    store_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    source_paths: String,
    #[arg(long)]
    version_local_store_index_paths: Option<String>,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
    #[arg(long, default_value_t = false)]
    write_version_local_store_index: bool,
    #[arg(long, default_value_t = false)]
    validate_versions: bool,
    #[arg(long, default_value_t = false)]
    skip_invalid_versions: bool,
}

#[derive(Args)]
struct PruneStoreBlocksArgs {
    #[arg(long)]
    store_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    blocks_root_path: String,
    #[arg(long, default_value = ".lsb")]
    block_extension: String,
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Args)]
struct CloneStoreArgs {
    #[arg(long)]
    source_storage_uri: String,
    #[arg(long)]
    target_storage_uri: String,
    #[arg(long)]
    source_s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    target_s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    target_path: String,
    #[arg(long)]
    source_paths: String,
    #[arg(long)]
    target_paths: String,
    #[arg(long)]
    source_zip_paths: Option<String>,
    #[arg(long, default_value_t = false)]
    create_version_local_store_index: bool,
    #[arg(long, default_value_t = false)]
    skip_validate: bool,
    #[arg(long)]
    cache_path: Option<String>,
    #[arg(long, default_value_t = false)]
    retain_permissions: bool,
    #[arg(long, default_value_t = false)]
    no_retain_permissions: bool,
    #[arg(long, default_value_t = 1024)]
    max_chunks_per_block: u32,
    #[arg(long, default_value_t = 8388608)]
    target_block_size: u32,
    #[arg(long, default_value_t = 80)]
    min_block_usage_percent: u32,
    /// Accepted for parity; ignored (the hash comes from the source version).
    #[arg(long, default_value = "blake3")]
    hash_algorithm: String,
    /// Accepted for parity; ignored (compression comes from the source version).
    #[arg(long, default_value = "zstd")]
    compression_algorithm: String,
    #[arg(long, default_value_t = false)]
    enable_file_mapping: bool,
    #[arg(long, default_value_t = false)]
    use_legacy_write: bool,
}

#[derive(Args)]
struct PrintStoreArgs {
    #[arg(long)]
    store_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long, default_value_t = false)]
    compact: bool,
    #[arg(long, default_value_t = false)]
    details: bool,
}

#[derive(Args)]
struct PrintVersionUsageArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    version_index_path: String,
    #[arg(long)]
    cache_path: Option<String>,
}

#[derive(Args)]
struct DumpVersionAssetsArgs {
    #[arg(long)]
    version_index_path: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long, default_value_t = false)]
    details: bool,
}

#[derive(Args)]
struct CpArgs {
    #[arg(long)]
    storage_uri: String,
    #[arg(long)]
    s3_endpoint_resolver_uri: Option<String>,
    #[arg(long)]
    version_index_path: String,
    #[arg(long)]
    cache_path: Option<String>,
    #[arg(long, default_value_t = false)]
    enable_file_mapping: bool,
    /// Asset path inside the version index.
    source_path: String,
    /// Destination file path/URI.
    target_path: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to build runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(&cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Render the full source chain: LongtailError's top-level Display is
            // a category (e.g. "store error"); the cause hangs off `#[source]`.
            eprintln!("error: {}", e.full_chain());
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: &Cli) -> Result<(), longtail::LongtailError> {
    match &cli.command {
        Command::Downsync(a) => run_downsync(cli, a).await,
        Command::Get(a) => run_get(cli, a).await,
        Command::Ls(a) => run_ls(a).await,
        Command::ValidateVersion(a) => run_validate(cli, a).await,
        Command::PrintVersion(a) => run_print(a).await,
        Command::Upsync(a) => run_upsync(cli, a).await,
        Command::Put(a) => run_put(cli, a).await,
        Command::InitRemoteStore(a) => run_init(cli, a).await,
        Command::CreateVersionStoreIndex(a) => run_create_vsi(cli, a).await,
        Command::PruneStore(a) => run_prune_store(cli, a).await,
        Command::PruneStoreIndex(a) => run_prune_store_index(a).await,
        Command::PruneStoreBlocks(a) => run_prune_store_blocks(a).await,
        Command::CloneStore(a) => run_clone_store(cli, a).await,
        Command::PrintStore(a) => run_print_store(a).await,
        Command::PrintVersionUsage(a) => run_print_version_usage(cli, a).await,
        Command::DumpVersionAssets(a) => run_dump_version_assets(a).await,
        Command::Cp(a) => run_cp(cli, a).await,
    }
}

/// Read a text file of one URI per line into a `Vec<String>` (golongtail's
/// `--source-paths`/`--target-paths` list files).
fn read_lines_file(path: &str) -> Result<Vec<String>, longtail::LongtailError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        longtail::LongtailError::InvalidArgument(format!("read list file {path}: {e}"))
    })?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

fn merge_paths(single: &Option<String>, multi: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(s) = single
        && !s.is_empty()
    {
        out.push(s.clone());
    }
    out.extend(multi.iter().filter(|s| !s.is_empty()).cloned());
    out
}

/// clap value parser for `--cache-size-limit`: a human-readable byte size
/// (`2GiB`, `500MB`, `1.5GiB`, or a bare byte count) parsed to bytes.
fn parse_size(s: &str) -> Result<u64, String> {
    s.parse::<bytesize::ByteSize>().map(|b| b.as_u64())
}

async fn run_downsync(cli: &Cli, a: &DownsyncArgs) -> Result<(), longtail::LongtailError> {
    let sources = merge_paths(&a.source_path, &a.source_paths);
    let mut opts = DownsyncOptions::new(sources, a.storage_uri.clone(), String::new());
    opts.target_path = a.target_path.clone();
    opts.target_index_path = a.target_index_path.clone();
    opts.cache_path = a.cache_path.clone().map(Into::into);
    opts.cache_size_limit = a.cache_size_limit;
    opts.retain_permissions = !a.no_retain_permissions;
    opts.validate = a.validate;
    opts.version_local_store_index_paths = merge_paths(
        &a.version_local_store_index_path,
        &a.version_local_store_index_paths,
    );
    opts.include_filter_regex = a.include_filter_regex.clone();
    opts.exclude_filter_regex = a.exclude_filter_regex.clone();
    opts.scan_target = !a.no_scan_target;
    opts.cache_target_index = !a.no_cache_target_index;
    opts.enable_file_mapping = a.enable_file_mapping;
    opts.use_legacy_write = a.use_legacy_write;
    opts.worker_count = cli.worker_count;
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let progress = Arc::new(CliProgress::new());
    opts.progress = Some(progress.clone());
    let result = downsync(opts).await;
    progress.finish(); // clear the bar before stats / error output
    let report = result?;
    if cli.show_stats {
        print_stats(&report);
    }
    Ok(())
}

async fn run_get(cli: &Cli, a: &GetArgs) -> Result<(), longtail::LongtailError> {
    let configs = merge_paths(&a.source_path, &a.source_paths);
    let mut opts = GetOptions::new(configs, String::new());
    opts.target_path = a.target_path.clone();
    opts.target_index_path = a.target_index_path.clone();
    opts.cache_path = a.cache_path.clone().map(Into::into);
    opts.cache_size_limit = a.cache_size_limit;
    opts.retain_permissions = !a.no_retain_permissions;
    opts.validate = a.validate;
    opts.include_filter_regex = a.include_filter_regex.clone();
    opts.exclude_filter_regex = a.exclude_filter_regex.clone();
    opts.scan_target = !a.no_scan_target;
    opts.cache_target_index = !a.no_cache_target_index;
    opts.enable_file_mapping = a.enable_file_mapping;
    opts.use_legacy_write = a.use_legacy_write;
    opts.worker_count = cli.worker_count;
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let progress = Arc::new(CliProgress::new());
    opts.progress = Some(progress.clone());
    let result = get(opts).await;
    progress.finish(); // clear the bar before stats / error output
    let report = result?;
    if cli.show_stats {
        print_stats(&report);
    }
    Ok(())
}

async fn run_ls(a: &LsArgs) -> Result<(), longtail::LongtailError> {
    let vi = read_version_index_from_uri(&a.version_index_path).await?;
    let search = match a.path.as_deref() {
        Some(".") | Some("") | None => String::new(),
        Some(p) => p.trim_end_matches('/').to_string(),
    };
    for line in ls_entries(&vi, &search)? {
        println!("{line}");
    }
    Ok(())
}

async fn run_validate(cli: &Cli, a: &ValidateArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = ValidateVersionOptions::new(a.storage_uri.clone(), a.version_index_path.clone());
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    validate_version(opts).await?;
    println!("Version index `{}` is valid", a.version_index_path);
    Ok(())
}

async fn run_print(a: &PrintArgs) -> Result<(), longtail::LongtailError> {
    let vi = read_version_index_from_uri(&a.version_index_path).await?;
    print_version_index(&a.version_index_path, &vi, a.compact);
    Ok(())
}

async fn run_upsync(cli: &Cli, a: &UpsyncArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::UpsyncOptions::new(
        a.source_path.clone(),
        a.storage_uri.clone(),
        a.target_path.clone(),
    );
    opts.source_index_path = a.source_index_path.clone();
    opts.version_local_store_index_path = a.version_local_store_index_path.clone();
    opts.target_chunk_size = a.target_chunk_size;
    opts.max_chunks_per_block = a.max_chunks_per_block;
    opts.target_block_size = a.target_block_size;
    opts.min_block_usage_percent = a.min_block_usage_percent;
    opts.compression_algorithm = a.compression_algorithm.clone();
    opts.hash_algorithm = a.hash_algorithm.clone();
    opts.include_filter_regex = a.include_filter_regex.clone();
    opts.exclude_filter_regex = a.exclude_filter_regex.clone();
    opts.enable_file_mapping = a.enable_file_mapping;
    opts.use_legacy_write = a.use_legacy_write;
    opts.worker_count = cli.worker_count;
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let progress = Arc::new(CliProgress::new());
    opts.progress = Some(progress.clone());
    let result = longtail::upsync(opts).await;
    progress.finish();
    let report = result?;
    if cli.show_stats {
        eprintln!(
            "upsync complete: {} blocks written, {} bytes, target {}",
            report.blocks_written, report.bytes_written, report.target_path
        );
    }
    Ok(())
}

async fn run_put(cli: &Cli, a: &PutArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::PutOptions::new(a.target_path.clone(), a.source_path.clone());
    opts.target_version_index_path = a.target_version_index_path.clone();
    opts.version_local_store_index_path = a.version_local_store_index_path.clone();
    opts.storage_uri = a.storage_uri.clone();
    opts.no_version_local_store_index = a.no_version_local_store_index;
    opts.source_index_path = a.source_index_path.clone();
    opts.s3_endpoint_resolver_uri = a.s3_endpoint_resolver_uri.clone();
    opts.target_chunk_size = a.target_chunk_size;
    opts.target_block_size = a.target_block_size;
    opts.max_chunks_per_block = a.max_chunks_per_block;
    opts.compression_algorithm = a.compression_algorithm.clone();
    opts.hash_algorithm = a.hash_algorithm.clone();
    opts.include_filter_regex = a.include_filter_regex.clone();
    opts.exclude_filter_regex = a.exclude_filter_regex.clone();
    opts.min_block_usage_percent = a.min_block_usage_percent;
    opts.enable_file_mapping = a.enable_file_mapping;
    opts.worker_count = cli.worker_count;
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let progress = Arc::new(CliProgress::new());
    opts.progress = Some(progress.clone());
    let result = longtail::put(opts).await;
    progress.finish();
    let report = result?;
    if cli.show_stats {
        eprintln!(
            "put complete: {} blocks written, get-config {}",
            report.blocks_written, a.target_path
        );
    }
    Ok(())
}

async fn run_init(cli: &Cli, a: &InitArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::InitRemoteStoreOptions::new(a.storage_uri.clone());
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let _ = &a.hash_algorithm; // accepted for parity; rebuild derives it from blocks
    longtail::init_remote_store(opts).await?;
    Ok(())
}

async fn run_create_vsi(cli: &Cli, a: &CreateVsiArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::CreateVersionStoreIndexOptions::new(
        a.source_path.clone(),
        a.version_local_store_index_path.clone(),
        a.storage_uri.clone(),
    );
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    longtail::create_version_store_index(opts).await
}

async fn run_prune_store(cli: &Cli, a: &PruneStoreArgs) -> Result<(), longtail::LongtailError> {
    let sources = read_lines_file(&a.source_paths)?;
    let mut opts = longtail::PruneStoreOptions::new(a.storage_uri.clone(), sources);
    if let Some(p) = &a.version_local_store_index_paths {
        opts.version_local_store_index_paths = read_lines_file(p)?;
    }
    opts.dry_run = a.dry_run;
    opts.write_version_local_store_index = a.write_version_local_store_index;
    opts.validate_versions = a.validate_versions;
    opts.skip_invalid_versions = a.skip_invalid_versions;
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let r = longtail::prune_store(opts).await?;
    if r.dry_run {
        println!("Prune would keep {} blocks", r.keep_blocks);
    } else if cli.show_stats {
        eprintln!("Pruned {} blocks", r.pruned_blocks);
    }
    Ok(())
}

async fn run_prune_store_index(a: &PruneStoreIndexArgs) -> Result<(), longtail::LongtailError> {
    let sources = read_lines_file(&a.source_paths)?;
    let mut opts = longtail::PruneStoreIndexOptions::new(a.store_index_path.clone(), sources);
    if let Some(p) = &a.version_local_store_index_paths {
        opts.version_local_store_index_paths = read_lines_file(p)?;
    }
    opts.dry_run = a.dry_run;
    opts.write_version_local_store_index = a.write_version_local_store_index;
    opts.validate_versions = a.validate_versions;
    opts.skip_invalid_versions = a.skip_invalid_versions;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let r = longtail::prune_store_index(opts).await?;
    println!(
        "Pruned {} blocks out of {}",
        r.old_block_count - r.new_block_count,
        r.old_block_count
    );
    Ok(())
}

async fn run_prune_store_blocks(a: &PruneStoreBlocksArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::PruneStoreBlocksOptions::new(
        a.store_index_path.clone(),
        a.blocks_root_path.clone(),
    );
    opts.block_extension = a.block_extension.clone();
    opts.dry_run = a.dry_run;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let r = longtail::prune_store_blocks(opts).await?;
    println!("Found {} blocks", r.found_blocks);
    println!("Found {} blocks to prune", r.blocks_to_prune);
    if !r.dry_run {
        println!("Deleted {} blocks", r.deleted_blocks);
    }
    Ok(())
}

async fn run_clone_store(cli: &Cli, a: &CloneStoreArgs) -> Result<(), longtail::LongtailError> {
    let sources = read_lines_file(&a.source_paths)?;
    let targets = read_lines_file(&a.target_paths)?;
    let mut opts = longtail::CloneStoreOptions::new(
        a.source_storage_uri.clone(),
        a.target_storage_uri.clone(),
        a.target_path.clone(),
        sources,
        targets,
    );
    if let Some(p) = &a.source_zip_paths {
        opts.source_zip_paths = read_lines_file(p)?;
    }
    opts.create_version_local_store_index = a.create_version_local_store_index;
    opts.skip_validate = a.skip_validate;
    opts.cache_path = a.cache_path.clone().map(Into::into);
    opts.retain_permissions = !a.no_retain_permissions;
    opts.max_chunks_per_block = a.max_chunks_per_block;
    opts.target_block_size = a.target_block_size;
    opts.min_block_usage_percent = a.min_block_usage_percent;
    opts.enable_file_mapping = a.enable_file_mapping;
    opts.use_legacy_write = a.use_legacy_write;
    opts.worker_count = cli.worker_count;
    opts.remote_worker_count = cli.remote_worker_count;
    let _ = (&a.hash_algorithm, &a.compression_algorithm); // ignored (source-derived)
    #[cfg(feature = "s3")]
    {
        if let Some(u) = &a.source_s3_endpoint_resolver_uri {
            opts.source_s3_options.endpoint_url = Some(u.clone());
        }
        if let Some(u) = &a.target_s3_endpoint_resolver_uri {
            opts.target_s3_options.endpoint_url = Some(u.clone());
        }
    }
    let progress = Arc::new(CliProgress::new());
    opts.progress = Some(progress.clone());
    let result = longtail::clone_store(opts).await;
    progress.finish();
    let cloned = result?;
    if cli.show_stats {
        eprintln!("clone-store complete: {cloned} versions cloned");
    }
    Ok(())
}

async fn run_print_store(a: &PrintStoreArgs) -> Result<(), longtail::LongtailError> {
    let si = longtail::read_store_index_from_uri(&a.store_index_path).await?;
    let s = longtail::store_index_stats(&si);
    let hash_str = hash_identifier_string(s.hash_identifier);
    if a.compact {
        let mut line = format!(
            "{}\t{}\t{}\t{}\t{}",
            a.store_index_path, s.version, hash_str, s.block_count, s.chunk_count
        );
        if a.details {
            line.push_str(&format!(
                "\t{}\t{}",
                s.stored_chunks_size, s.unique_stored_chunks_size
            ));
        }
        println!("{line}");
    } else {
        println!("Version:             {}", s.version);
        println!("Hash Identifier:     {hash_str}");
        println!(
            "Block Count:         {}   ({})",
            s.block_count,
            byte_count_decimal(s.block_count as u64)
        );
        println!(
            "Chunk Count:         {}   ({})",
            s.chunk_count,
            byte_count_decimal(s.chunk_count as u64)
        );
        if a.details {
            println!(
                "Data size:           {}   ({})",
                s.stored_chunks_size,
                byte_count_binary(s.stored_chunks_size)
            );
            println!(
                "Unique Data size:    {}   ({})",
                s.unique_stored_chunks_size,
                byte_count_binary(s.unique_stored_chunks_size)
            );
        }
    }
    Ok(())
}

async fn run_print_version_usage(
    cli: &Cli,
    a: &PrintVersionUsageArgs,
) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::PrintVersionUsageOptions::new(
        a.storage_uri.clone(),
        a.version_index_path.clone(),
    );
    opts.cache_path = a.cache_path.clone().map(Into::into);
    opts.remote_worker_count = cli.remote_worker_count;
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    let stats = longtail::print_version_usage_stats(opts).await?;
    println!("Block Usage:          {}%", stats.block_usage_percent);
    println!(
        "Asset Fragmentation:  {}%",
        stats.asset_fragmentation_percent
    );
    Ok(())
}

async fn run_dump_version_assets(a: &DumpVersionAssetsArgs) -> Result<(), longtail::LongtailError> {
    let vi = read_version_index_from_uri(&a.version_index_path).await?;
    let asset_count = vi.asset_count() as usize;
    let biggest = vi.asset_sizes.iter().copied().max().unwrap_or(0);
    let pad = biggest.to_string().len();
    for i in 0..asset_count {
        let path = vi.path(i)?;
        if a.details {
            let is_dir = path.ends_with('/');
            println!(
                "{}",
                details_string(
                    path,
                    vi.asset_sizes[i],
                    vi.permissions[i].bits(),
                    is_dir,
                    pad
                )
            );
        } else {
            println!("{path}");
        }
    }
    Ok(())
}

async fn run_cp(cli: &Cli, a: &CpArgs) -> Result<(), longtail::LongtailError> {
    let mut opts = longtail::CpOptions::new(
        a.storage_uri.clone(),
        a.version_index_path.clone(),
        a.source_path.clone(),
        a.target_path.clone(),
    );
    opts.cache_path = a.cache_path.clone().map(Into::into);
    opts.remote_worker_count = cli.remote_worker_count;
    let _ = a.enable_file_mapping; // accepted for parity; no-op
    #[cfg(feature = "s3")]
    if let Some(u) = &a.s3_endpoint_resolver_uri {
        opts.s3_options.endpoint_url = Some(u.clone());
    }
    longtail::cp(opts).await
}

// ---- ls / print-version formatting (golongtail-compatible) ----

fn hash_identifier_string(id: u32) -> String {
    match id {
        longtail_core::hash::BLAKE3_ID => "blake3".to_string(),
        longtail_core::hash::BLAKE2S_ID => "blake2".to_string(),
        longtail_core::hash::MEOW_ID => "meow".to_string(),
        other => other.to_string(),
    }
}

/// `GetDetailsString` (longtailutils/stats.go:48): `{rwx-bits} {size:>pad} {name}`.
fn details_string(name: &str, size: u64, perms: u16, is_dir: bool, pad: usize) -> String {
    let mut bits = String::with_capacity(10);
    bits.push(if is_dir { 'd' } else { '-' });
    const MASKS: [(u16, char); 9] = [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ];
    for (m, c) in MASKS {
        bits.push(if perms & m != 0 { c } else { '-' });
    }
    let size_s = size.to_string();
    let size_padded = if size_s.len() < pad {
        format!("{}{}", " ".repeat(pad - size_s.len()), size_s)
    } else {
        size_s
    };
    let name = name.trim_end_matches('/');
    format!("{bits} {size_padded} {name}")
}

/// List the single directory level under `search` inside `vi`.
fn ls_entries(vi: &VersionIndex, search: &str) -> Result<Vec<String>, longtail::LongtailError> {
    let mut out = Vec::new();
    for i in 0..vi.asset_count() as usize {
        let raw = vi.path(i)?;
        let is_dir = raw.ends_with('/');
        let stripped = raw.trim_end_matches('/');
        let name = if search.is_empty() {
            if stripped.contains('/') {
                continue;
            }
            stripped
        } else {
            let prefix = format!("{search}/");
            match stripped.strip_prefix(&prefix) {
                Some(rem) if !rem.contains('/') => rem,
                _ => continue,
            }
        };
        out.push(details_string(
            name,
            vi.asset_sizes[i],
            vi.permissions[i].bits(),
            is_dir,
            16,
        ));
    }
    Ok(out)
}

fn byte_count_decimal(n: u64) -> String {
    let unit = 1000u64;
    if n < unit {
        return n.to_string();
    }
    let mut div = unit;
    let mut exp = 0usize;
    let mut v = n / unit;
    while v >= unit {
        div *= unit;
        v /= unit;
        exp += 1;
    }
    let suffix = ['k', 'M', 'G', 'T', 'P', 'E'][exp];
    format!("{:.1} {}", n as f64 / div as f64, suffix)
}

fn byte_count_binary(n: u64) -> String {
    let unit = 1024u64;
    if n < unit {
        return format!("{n} B");
    }
    let mut div = unit;
    let mut exp = 0usize;
    let mut v = n / unit;
    while v >= unit {
        div *= unit;
        v /= unit;
        exp += 1;
    }
    let suffix = ['K', 'M', 'G', 'T', 'P', 'E'][exp];
    format!("{:.1} {}B", n as f64 / div as f64, suffix)
}

fn print_version_index(path: &str, vi: &VersionIndex, compact: bool) {
    let version = longtail_core::VERSION_INDEX_VERSION;
    let hash_str = hash_identifier_string(vi.hash_identifier);
    let tcs = vi.target_chunk_size;
    let asset_count = vi.asset_count();
    let total_asset_size: u64 = vi.asset_sizes.iter().sum();
    let chunk_count = vi.chunk_count();
    let total_chunk_size: u64 = vi.chunk_sizes.iter().map(|&s| s as u64).sum();
    let (mut smallest, mut largest) = (u32::MAX, 0u32);
    for &s in &vi.chunk_sizes {
        smallest = smallest.min(s);
        largest = largest.max(s);
    }
    if vi.chunk_sizes.is_empty() {
        smallest = 0;
    }
    let avg = if chunk_count == 0 {
        0
    } else {
        (total_chunk_size / chunk_count as u64) as u32
    };

    if compact {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            path,
            version,
            hash_str,
            tcs,
            asset_count,
            total_asset_size,
            chunk_count,
            total_chunk_size,
            avg,
            smallest,
            largest
        );
    } else {
        println!("Version:             {version}");
        println!("Hash Identifier:     {hash_str}");
        println!("Target Chunk Size:   {tcs}");
        println!(
            "Asset Count:         {asset_count}   ({})",
            byte_count_decimal(asset_count as u64)
        );
        println!(
            "Asset Total Size:    {total_asset_size}   ({})",
            byte_count_binary(total_asset_size)
        );
        println!(
            "Chunk Count:         {chunk_count}   ({})",
            byte_count_decimal(chunk_count as u64)
        );
        println!(
            "Chunk Total Size:    {total_chunk_size}   ({})",
            byte_count_binary(total_chunk_size)
        );
        println!(
            "Average Chunk Size:  {avg}   ({})",
            byte_count_binary(avg as u64)
        );
        println!(
            "Smallest Chunk Size: {smallest}   ({})",
            byte_count_binary(smallest as u64)
        );
        println!(
            "Largest Chunk Size:  {largest}   ({})",
            byte_count_binary(largest as u64)
        );
    }
}

fn print_stats(report: &longtail::DownsyncReport) {
    eprintln!(
        "downsync complete: {} assets written, {} removed, {} bytes, {} blocks fetched",
        report.assets_written, report.assets_removed, report.bytes_written, report.blocks_fetched
    );
    for p in &report.phases {
        eprintln!("  phase {:<20} {} ms", p.phase, p.millis);
    }
}
