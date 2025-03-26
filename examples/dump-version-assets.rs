mod common;

use clap::Parser;
use common::version_index_from_file;
use longtail::*;
use longtail_sys::{permissions_to_string, LONGTAIL_LOG_LEVEL_DEBUG};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    // Optional URI for S3 endpoint resolver
    #[clap(name = "s3-endpoint-resolver-url", long)]
    s3_endpoint_resolver_url: Option<String>,

    /// URI to version index (local file system and S3 bucket URI (soon!)
    /// supported)
    #[clap(name = "version-index-path", long)]
    version_index_path: String,

    /// Show details about assets
    #[clap(name = "details", long, default_value = "false")]
    details: bool,
}

fn dump_version_assets(version_index_path: String, details: bool) {
    let version_index = version_index_from_file(&version_index_path);
    let asset_count = version_index.get_asset_count();
    let mut biggest_asset = 0;
    for i in 0..asset_count {
        let asset_size = version_index.get_asset_size(i);
        if asset_size > biggest_asset {
            biggest_asset = asset_size;
        }
    }
    let size_padding = format!("{}", biggest_asset).len();
    for i in 0..asset_count {
        let path = version_index.get_asset_path(i);
        if details {
            let size = version_index.get_asset_size(i);
            let permission = version_index.get_asset_permissions(i);
            let dirbit = if path.ends_with('/') { 'd' } else { '-' };
            let path = path.strip_suffix('/').unwrap_or(&path);
            println!(
                "{}{} {:size_padding$} {}",
                dirbit,
                permissions_to_string(permission),
                size,
                path
            );
        } else {
            println!("{}", path);
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    set_longtail_loglevel(LONGTAIL_LOG_LEVEL_DEBUG);

    let args = Args::parse();

    dump_version_assets(args.version_index_path, args.details);
}
