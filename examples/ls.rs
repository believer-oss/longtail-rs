mod common;

use common::version_index_from_file;
use itertools::izip;
use longtail::*;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    set_longtail_loglevel(LONGTAIL_LOG_LEVEL_DEBUG);

    // Setup the longtail environment
    let jobs = BikeshedJobAPI::new(1, 1);
    let hash_registry = HashRegistry::new();

    // Read the version index from the file
    // let file = "test-data/target-path/testdir.lvi";
    let file = std::env::args().nth(1).expect("No file provided");

    let version_index = version_index_from_file(&file);
    let hash_id = HashType::from_repr(version_index.get_hash_identifier() as usize)
        .expect("Failed to get hash type");
    let hash = hash_registry
        .get_hash_api(hash_id)
        .expect("Failed to get hash API");

    let fake_storage_api = StorageAPI::new_inmem();

    let fake_block_store = BlockstoreAPI::new_fs(&jobs, &fake_storage_api, "store", None, false);

    // Hardcoded in golongtail
    let max_block_size = 1024 * 1024 * 1024;
    let max_chunks_per_block = 1024;

    let store_index = StoreIndex::new_from_version_index(
        &hash,
        &version_index,
        max_block_size,
        max_chunks_per_block,
    )
    .unwrap();
    let store_index = store_index;
    let block_store = BlockstoreAPI::new_block_store(
        &hash,
        &jobs,
        &fake_block_store,
        &store_index,
        &version_index,
    );

    // TODO: Need to understand if the blockstore should be recursing here... Leaving unsafe for
    // now.
    // This is the implementation of the ls command ported from golongtail. It doesn't recurse, and
    // I'm not sure why? It is/may be important because it loops through the blockstore layer.
    // The golongtail binary exhibits the same behavior.
    println!("-----------------------------");
    println!("Listing files in blockstore");
    match block_store.start_find("") {
        Ok(mut iter) => loop {
            let properties = unsafe { block_store.get_entry_properties(&mut iter) };
            if properties.is_ok() {
                println!("{}", properties.unwrap());
            }
            let result = unsafe { block_store.find_next(&mut iter) };
            if result.is_err() {
                break;
            }
        },
        Err(e) => println!("Error: {:?}", e),
    }

    println!("-----------------------------");
    println!("Listing files in version index");
    let files = version_index.get_name_data();
    let permissions = version_index.get_permissions();
    let sizes = version_index.get_asset_sizes();

    for (file, permission, size) in izip!(files, permissions, sizes) {
        let dirbit = if file.ends_with('/') { "d" } else { "-" };
        println!(
            "{}{} {:16} {}",
            dirbit,
            permissions_to_string(permission),
            size,
            file
        );
    }
}
