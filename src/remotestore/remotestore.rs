use crate::{BikeshedJobAPI, BlockstoreAPI, S3Options};

pub enum AccessType {
    Init,
    ReadWrite,
    ReadOnly,
}

pub fn create_block_store_for_uri(
    uri: &str,
    optional_store_index_paths: Vec<&str>,
    job_api: BikeshedJobAPI,
    num_worker_count: i32,
    target_block_size: u32,
    max_chunks_per_block: u32,
    access_type: AccessType,
    enable_file_mapping: bool,
    opts: Option<S3Options>,
) -> Result<BlockstoreAPI, Box<dyn std::error::Error>> {
    match uri {
        s if s.starts_with("fsblob://") => {
            let fs_blob_store = longtailstorelib::new_fs_blob_store(&uri[9..], true)?;
            let fs_block_store = new_remote_block_store(
                job_api,
                fs_blob_store,
                optional_store_index_paths,
                num_worker_count,
                access_type,
                opts,
            )?;
            Ok(longtaillib::create_block_store_api(fs_block_store))
        }
        s if s.starts_with("s3://") => {
            let s3_blob_store = longtailstorelib::new_s3_blob_store(uri, opts)?;
            let s3_block_store = new_remote_block_store(
                job_api,
                s3_blob_store,
                optional_store_index_paths,
                num_worker_count,
                access_type,
                opts,
            )?;
            Ok(longtaillib::create_block_store_api(s3_block_store))
        }
        s if s.starts_with("file://") => Ok(longtaillib::create_fs_block_store(
            job_api,
            longtaillib::create_fs_storage_api(),
            &uri[7..],
            ".lsb",
            enable_file_mapping,
        )),
        _ => Ok(longtaillib::create_fs_block_store(
            job_api,
            longtaillib::create_fs_storage_api(),
            uri,
            ".lsb",
            enable_file_mapping,
        )),
    }
}
