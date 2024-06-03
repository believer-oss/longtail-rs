use longtail::*;
use std::io::Read;

fn main() {
    setup_logging();

    let file = "test-data/target-path/testdir.lvi";
    // let file = "/home/chris/tmp/tmp.PVTbfclbJ3/.longtail.index.cache.lvi";

    println!("Opening {} for reading", file);
    let mut f = std::fs::File::open(file).unwrap();
    let metadata = f.metadata().unwrap();

    let mut buffer = vec![0u8; metadata.len() as usize];
    println!("Reading {} bytes", metadata.len());
    f.read_exact(&mut buffer).unwrap();
    // println!("Bytes read: {:?}", buffer);

    let result = VersionIndex::read_version_index_from_buffer(&mut buffer);
    // println!("Result: {:?}", result);

    let version_index = result.unwrap();
    // println!("Version index debugdisplay: {:?}", version_index);

    let chunk_sizes = version_index.get_chunk_sizes();
    let larget_chunk_size = chunk_sizes.iter().max().unwrap();
    let smallest_chunk_size = chunk_sizes.iter().min().unwrap();
    let average_chunk_size = chunk_sizes.iter().sum::<u32>() / chunk_sizes.len() as u32;
    let total_chunk_size = chunk_sizes.iter().sum::<u32>();

    let total_asset_size = version_index.get_asset_sizes().iter().sum::<u64>();

    println!("Version: {}", version_index.get_version());
    // TODO(cm): Add reverse lookup of hashing type
    // https://github.com/DanEngelbrecht/golongtail/blob/main/longtailutils/longtailutils.go#L517
    println!("Hash identifier: {}", version_index.get_hash_identifier());
    println!(
        "Target Chunk Size: {}",
        version_index.get_target_chunk_size()
    );
    println!("Asset Count: {}", version_index.get_asset_count());
    println!("Asset Total Size: {}", total_asset_size);
    println!("Chunk Count: {}", version_index.get_chunk_count());
    println!("Chunk Total Size: {}", total_chunk_size);
    println!("Average Chunk Size: {}", average_chunk_size);
    println!("Smallest Chunk Size: {}", smallest_chunk_size);
    println!("Largest Chunk Size: {}", larget_chunk_size);
}
