use longtail::*;
use longtail_sys::LONGTAIL_LOG_LEVEL_INFO;

mod common;
use common::version_index_from_file;

fn display_file(filename: &str) {
    let version_index = version_index_from_file(filename);
    let chunk_sizes = version_index.get_chunk_sizes();
    let larget_chunk_size = chunk_sizes.iter().max().unwrap();
    let smallest_chunk_size = chunk_sizes.iter().min().unwrap();
    let average_chunk_size =
        chunk_sizes.iter().fold(0u64, |sum, i| sum + (*i as u64)) / chunk_sizes.len() as u64;
    let total_chunk_size = chunk_sizes.iter().fold(0u64, |sum, i| sum + (*i as u64));
    let total_asset_size = version_index.get_asset_sizes().iter().sum::<u64>();
    println!("Debug: {version_index:?}");
    println!("Version: {}", version_index.get_version());
    println!(
        "Hash identifier: {:?}",
        &HashType::from_repr(version_index.get_hash_identifier().try_into().unwrap())
            .expect("Failed to get hash type")
    );
    println!(
        "Target Chunk Size: {}",
        version_index.get_target_chunk_size()
    );
    println!("Asset Count: {}", version_index.get_asset_count());
    println!("Asset Total Size: {total_asset_size}");
    println!("Chunk Count: {}", version_index.get_chunk_count());
    println!("Chunk Total Size: {total_chunk_size}");
    println!("Average Chunk Size: {average_chunk_size}");
    println!("Smallest Chunk Size: {smallest_chunk_size}");
    println!("Largest Chunk Size: {larget_chunk_size}");
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    set_longtail_loglevel(LONGTAIL_LOG_LEVEL_INFO);

    let file = std::env::args().nth(1).expect("No file provided");
    display_file(&file);
}
