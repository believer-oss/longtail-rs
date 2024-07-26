mod common;

use longtail_sys::LONGTAIL_LOG_LEVEL_DEBUG;

use clap::Parser;
use longtail::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Source file uri(s)
    #[clap(name = "source-path", long)]
    source_paths: Vec<String>,

    /// Target folder path
    #[clap(name = "target-path", long)]
    target_path: String,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_file(true)
        .with_line_number(true)
        .init();
    set_longtail_loglevel(LONGTAIL_LOG_LEVEL_DEBUG);

    let args = Args::parse();
    let source_path = args.source_paths[0].clone();
    get(&source_path, &args.target_path, None).unwrap();
}
