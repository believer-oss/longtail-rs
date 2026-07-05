//! `longtail` CLI — a drop-in replacement for the golongtail download-path
//! commands (`downsync`, `get`, `ls`, `validate-version`, `print-version`).
//! Flag names match the pinned v0.4.5 `--help` (rust-port-1-results.md §5).

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use longtail::{
    DownsyncOptions, GetOptions, ValidateVersionOptions, downsync, get,
    read_version_index_from_uri, validate_version,
};
use longtail_core::VersionIndex;

#[derive(Parser)]
#[command(
    name = "longtail",
    version,
    about = "Pure-Rust longtail — download-path CLI (drop-in for golongtail)"
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
            eprintln!("error: {e}");
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
    }
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

async fn run_downsync(cli: &Cli, a: &DownsyncArgs) -> Result<(), longtail::LongtailError> {
    let sources = merge_paths(&a.source_path, &a.source_paths);
    let mut opts = DownsyncOptions::new(sources, a.storage_uri.clone(), String::new());
    opts.target_path = a.target_path.clone();
    opts.target_index_path = a.target_index_path.clone();
    opts.cache_path = a.cache_path.clone().map(Into::into);
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
    let report = downsync(opts).await?;
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
    let report = get(opts).await?;
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
