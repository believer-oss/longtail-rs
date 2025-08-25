// use std::{collections::HashMap, path::Path, ptr::null_mut, sync::Mutex};
use std::{path::Path, ptr::null_mut};

use crate::{
    AsyncFlushAPIProxy, AsyncGetExistingContentAPIProxy, AsyncGetStoredBlockAPI,
    AsyncGetStoredBlockAPIProxy, AsyncPreflightStartedAPIProxy, AsyncPruneBlocksAPIProxy,
    AsyncPutStoredBlockAPI, AsyncPutStoredBlockAPIProxy, BikeshedJobAPI, BlobClient, BlobStore,
    BlockIndex, Blockstore, BlockstoreAPI, BlockstoreAPIProxy, FsBlobStore, S3BlobStore, S3Options,
    StorageAPI, StoreIndex, StoredBlock,
    async_apis::{AsyncGetExistingContentAPI, AsyncPreflightStartedAPI},
    read_blob, read_from_uri,
};

use http::Uri;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum AccessType {
    Init,
    ReadWrite,
    ReadOnly,
}

// pub struct pending_prefetch_blocks {
//     stored_block: StoredBlock,
//     complete_callbacks: Vec<Box<dyn AsyncGetStoredBlockAPI>>,
//     err: i32,
// }

#[derive(Debug)]
pub struct RemoteBlockStore<S> {
    // job_api: BikeshedJobAPI,
    blob_store: S,
    // blob_store_options: Option<S3Options>,
    worker_count: i32,
    // prefetch_mem: i64,
    // max_prefetch_mem: i64,
    // prefetchBlocks: Mutex<HashMap<u64, pending_prefetch_blocks>>,
    store_index_paths: Option<Vec<String>>,
    access_type: AccessType,
}

impl<S: BlobStore> Blockstore for RemoteBlockStore<S> {
    fn put_stored_block(
        &self,
        stored_block: &StoredBlock,
        async_complete_api: AsyncPutStoredBlockAPIProxy,
    ) -> Result<(), i32> {
        if self.access_type == AccessType::ReadOnly {
            error!("Attempted to write to a read-only store");
            async_complete_api.on_complete(1);
            return Ok(());
        }
        info!("put_stored_block: {:?}", stored_block);
        let stored_block = stored_block.clone();
        self.put_stored_block(stored_block, async_complete_api);
        Ok(())
    }

    fn preflight_get(
        &self,
        block_hashes: Vec<u64>,
        async_complete_api: Option<AsyncPreflightStartedAPIProxy>,
    ) -> Result<(), i32> {
        warn!("preflight_get not implemented");
        if let Some(async_complete_api) = async_complete_api {
            async_complete_api.on_complete(block_hashes, 0);
        }
        Ok(())
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn get_stored_block(
        &self,
        block_hash: u64,
        async_complete_api: *mut AsyncGetStoredBlockAPIProxy,
    ) -> Result<(), i32> {
        info!("get_stored_block: {:?}", block_hash);
        let stored_block = self.get_stored_block(block_hash);
        tracing::debug!("stored_block: {:?}", stored_block);
        tracing::debug!("async_complete_api: {:p}", async_complete_api);
        tracing::debug!("context: {:?}", unsafe {
            async_complete_api
                .as_ref()
                .expect("Failed to get async_complete_api")
                .get_context()
        });
        match stored_block {
            Ok(stored_block) => unsafe {
                async_complete_api
                    .as_ref()
                    .expect("Failed to get async_complete_api")
                    .on_complete(*stored_block, 0)
            },
            Err(e) => {
                error!("Failed to get stored block: {:?}", e);
                unsafe {
                    async_complete_api
                        .as_ref()
                        .expect("Failed to get async_complete_api")
                        .on_complete(*StoredBlock::new_from_lt(null_mut()), 1)
                };
            }
        }
        Ok(())
    }

    fn get_existing_content(
        &self,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
        mut async_complete_api: AsyncGetExistingContentAPIProxy,
    ) -> Result<(), i32> {
        debug!("get_existing_content: {:?}", chunk_hashes);
        debug!(
            "get_existing_content min_block_usage_percent: {:?}",
            min_block_usage_percent
        );
        debug!(
            "get_existing_content async_complete_api: {:?}",
            async_complete_api
        );
        let store_index = StoreIndex::new_null_index();
        let added_block_indexes = Vec::new();
        let (store_index, _additional_store_indexes) = self.get_current_store_index(
            &self.store_index_paths,
            &self.access_type,
            store_index,
            added_block_indexes,
        )?;
        let existing_store_index = store_index
            .get_existing_store_index(chunk_hashes, min_block_usage_percent)
            .map_err(|e| {
                error!("Failed to get existing store index: {}", e);
                1
            })?;

        assert!(existing_store_index.is_valid());
        unsafe { async_complete_api.on_complete(*existing_store_index, 0) };
        Ok(())
    }

    fn prune_blocks(
        &self,
        _block_keep_hashes: Vec<u64>,
        _async_complete_api: AsyncPruneBlocksAPIProxy,
    ) -> Result<(), i32> {
        todo!()
    }

    fn get_stats(&self) -> Result<crate::Longtail_BlockStore_Stats, i32> {
        info!("get_stats not implemented");
        Ok(crate::Longtail_BlockStore_Stats { m_StatU64: [0; 22] })
    }

    fn flush(&self, mut async_complete_api: AsyncFlushAPIProxy) -> Result<(), i32> {
        info!("flush not implemented");
        if let Some(on_complete) = async_complete_api.api.OnComplete {
            unsafe { on_complete(&mut async_complete_api.api, 0) };
        }
        Ok(())
    }
}

impl<S: BlobStore> RemoteBlockStore<S> {
    pub fn new_remote_block_store(
        // job_api: BikeshedJobAPI,
        blob_store: S,
        store_index_paths: Option<Vec<String>>,
        worker_count: i32,
        access_type: AccessType,
        // opts: Option<S3Options>,
    ) -> Result<RemoteBlockStore<S>, Box<dyn std::error::Error>> {
        // let max_prefetch_mem = 512 * 1024 * 1024;
        Ok(RemoteBlockStore {
            // job_api,
            blob_store,
            // blob_store_options: opts,
            worker_count,
            // prefetch_mem: 0,
            // max_prefetch_mem,
            // prefetchBlocks: Mutex::new(HashMap::new()),
            store_index_paths,
            access_type,
        })
    }

    fn get_store_index_from_blocks(
        _blob_store: &impl BlobStore,
        _worker_count: i32,
        block_paths: Vec<String>,
    ) -> Result<StoreIndex, Box<dyn std::error::Error>> {
        // let store_index = StoreIndex::new();
        let mut blocks = Vec::new();
        // Iterate over block_paths and merge them into store_index
        for block_path in block_paths {
            // TODO: Add retries
            let mut blob = read_from_uri(block_path.as_str(), None)?;
            let block_index = BlockIndex::new_from_buffer(blob.as_mut_slice())
                .expect("Failed to create block index from buffer");
            let computed_block_path = block_index.get_block_path(Path::new("chunks"));
            if computed_block_path != block_path {
                warn!(
                    "Block {} name does not match content hash, expected name {}",
                    computed_block_path, block_path
                );
            } else {
                blocks.push(block_index);
            }
        }
        let blocks = blocks
            .into_iter()
            .filter(|block| block.is_valid())
            .collect();
        StoreIndex::new_from_blocks(blocks)
            .map_err(|e| format!("Failed to create store index: {e}").into())
    }

    fn read_store_index_from_uri(
        added_store_index: &str,
    ) -> Result<StoreIndex, Box<dyn std::error::Error>> {
        // TODO: Add retries
        let buf = read_from_uri(added_store_index, None)?;
        StoreIndex::new_from_buffer(buf.as_slice())
            .map_err(|e| format!("Failed to create store index from buffer: {e}").into())
    }

    fn get_store_store_indexes(
        client: Box<dyn BlobClient>,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        // TODO: Add retries
        let blobs = client
            .get_objects("store".to_string())
            .inspect_err(|e| {
                error!("Failed to get store blobs: {}", e);
            })
            .inspect(|r| tracing::debug!("Results: {:?}", r))?;

        Ok(blobs
            .into_iter()
            .filter(|blob| blob.size > 0 && blob.name.ends_with(".lsi"))
            .map(|blob| blob.name)
            .collect())
    }

    fn read_store_store_index_from_path<'a>(
        client: &(dyn BlobClient + 'a),
        item: &str,
    ) -> Result<StoreIndex, Box<dyn std::error::Error>> {
        let buf = read_blob(client, item)
            .inspect(|i| tracing::debug!("Read {:?} bytes for {:?}", i.len(), item))?;
        StoreIndex::new_from_buffer(buf.as_slice())
            .map_err(|e| format!("Failed to create store index from buffer: {e}").into())
    }

    fn merge_store_index_items(
        client: Box<dyn BlobClient>,
        items: Vec<String>,
    ) -> Result<(StoreIndex, Vec<String>), Box<dyn std::error::Error>> {
        tracing::debug!("Merging store index items: {:?}", items);
        let mut used_items = Vec::new();
        let mut store_index = StoreIndex::new_null_index();
        for item in items {
            let tmp_store_index = Self::read_store_store_index_from_path(client.as_ref(), &item)
                .inspect_err(|e| tracing::debug!("Error: {:?}", e))?;
            if !store_index.is_valid() {
                warn!("Store index is invalid");
                store_index = tmp_store_index;
                used_items.push(item);
                continue;
            }
            let merged_store_index = store_index
                .merge_store_index(&tmp_store_index)
                .map_err(|e| format!("Failed to merge store indexes: {e}"))?;
            store_index = merged_store_index;
            used_items.push(item);
        }
        Ok((store_index, used_items))
    }

    fn read_store_store_index_with_items(
        store: &impl BlobStore,
    ) -> Result<StoreIndex, Box<dyn std::error::Error>> {
        loop {
            let client = store.new_client()?;
            let items = Self::get_store_store_indexes(client)?;
            tracing::debug!("Found {} store index items", items.len());
            if items.is_empty() {
                warn!("No store index found");
                return Ok(StoreIndex::new_null_index());
            }
            let client = store.new_client()?;
            let (store_index, used_items) = Self::merge_store_index_items(client, items)?;
            if used_items.is_empty() {
                warn!("The underlying index files changed as we were scanning them, trying again");
                continue;
            } else {
                debug!("Merged {} store indexes", used_items.len());
            }
            if store_index.is_valid() {
                return Ok(store_index);
            }
            warn!("Failed to merge store indexes, retrying");
        }
    }

    #[allow(unreachable_code)]
    fn read_remote_store_index(
        added_store_index_paths: &Option<Vec<String>>,
        blob_store: &impl BlobStore,
        access_type: &AccessType,
        _worker_count: i32,
    ) -> Result<StoreIndex, Box<dyn std::error::Error>> {
        let mut store_index = StoreIndex::new_null_index();
        match access_type {
            AccessType::Init => {
                // TODO: This is only used in for new remote stores, skipping for now
                todo!();
                info!("building store index from blocks");
                // inlining buildStoreIndexFromStoreBlocks
                let client = blob_store.new_client()?;
                let blobs = client.get_objects("".to_string())?;
                let blobs = blobs
                    .into_iter()
                    .filter(|blob| blob.size > 0 && blob.name.ends_with(".lsb"))
                    .map(|blob| blob.name)
                    .collect();
                let _result = Self::get_store_index_from_blocks(blob_store, _worker_count, blobs);

                info!(
                    "rebuilt remote index with {} blocks",
                    store_index.get_block_hashes().len()
                );
                // let new_store_index = add_to_remote_store_index(client, store_index)?;
                // if new_store_index.is_valid() {
                //     store_index = new_store_index
                // }
                return Ok(store_index);
            }
            AccessType::ReadOnly => {
                if let Some(added_store_index_paths) = added_store_index_paths {
                    for (i, added_store_index) in added_store_index_paths.iter().enumerate() {
                        let one_store_index = Self::read_store_index_from_uri(added_store_index)?;
                        if i == 0 {
                            store_index = one_store_index;
                        } else {
                            store_index = store_index
                                .merge_store_index(&one_store_index)
                                .map_err(|e| format!("Failed to merge store index: {e}"))?
                        }
                    }
                }
            }
            AccessType::ReadWrite => {}
        }
        if !store_index.is_valid() {
            store_index = match Self::read_store_store_index_with_items(blob_store) {
                Ok(store_index) => store_index,
                Err(e) => {
                    error!("Failed to read store index: {:?}", e);
                    return Err(e);
                }
            }
        }
        Ok(store_index)
    }

    pub fn get_current_store_index(
        &self,
        added_store_index_paths: &Option<Vec<String>>,
        access_type: &AccessType,
        store_index: StoreIndex,
        added_block_indexes: Vec<BlockIndex>,
    ) -> Result<(StoreIndex, Option<StoreIndex>), i32> {
        if !store_index.is_valid() {
            let store_index = Self::read_remote_store_index(
                added_store_index_paths,
                &self.blob_store,
                access_type,
                self.worker_count,
            )
            .map_err(|e| {
                error!("Failed to read remote store index: {}", e);
                1
            })?;
            return Ok((store_index, None));
        };
        if added_block_indexes.is_empty() {
            return Ok((store_index, None));
        }
        let updated_store_index = store_index.add_blocks(added_block_indexes)?;
        Ok((store_index, Some(updated_store_index)))
    }

    // TODO: The on_complete should take the result, instead of the call returning
    // Result for all of these methods
    pub fn put_stored_block(
        &self,
        stored_block: StoredBlock,
        complete_callback: AsyncPutStoredBlockAPIProxy,
    ) {
        let block_index = stored_block.get_block_index();
        let block_hash = block_index.get_block_hash();
        let key = StoredBlock::get_block_path(Path::new("chunks"), block_hash);

        let blob_client = match self.blob_store.new_client() {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to create blob client: {}", e);
                complete_callback.on_complete(1);
                return;
            }
        };
        let obj_handle = match blob_client.new_object(key) {
            Ok(obj) => obj,
            Err(e) => {
                error!("Failed to create object handle: {}", e);
                complete_callback.on_complete(1);
                return;
            }
        };

        match obj_handle.exists() {
            Ok(true) => {
                info!("Block already exists");
                complete_callback.on_complete(0);
            }
            Ok(false) => {
                let blob = stored_block
                    .write_to_buffer()
                    .expect("Failed to write block to buffer");
                // TODO: Add retries
                match obj_handle.write(blob.as_slice()) {
                    Ok(_) => (),
                    Err(e) => {
                        error!("Failed to write block to object: {}", e);
                        complete_callback.on_complete(1);
                        return;
                    }
                };
                complete_callback.on_complete(0);
            }
            Err(e) => {
                error!("Failed to check if object exists: {}", e);
                complete_callback.on_complete(1);
            }
        }
    }

    // NOTE: We don't signal the completion here, since it's handled by the
    // pendingPrefetchedBlock struct and the fetch_block method... or at least
    // it should be
    pub fn get_stored_block(
        &self,
        block_hash: u64,
    ) -> Result<StoredBlock, Box<dyn std::error::Error>> {
        let block_path = StoredBlock::get_block_path(Path::new("chunks"), block_hash);
        tracing::debug!("get_stored_block: {:?}", block_path);
        // TODO: Add retries
        let client = self.blob_store.new_client()?;
        let mut blob = read_blob(client.as_ref(), block_path.as_str())?;
        let stored_block = StoredBlock::new_from_buffer(blob.as_mut_slice())
            .expect("Failed to create stored block from buffer");
        let block_index = stored_block.get_block_index();
        if block_index.get_block_hash() != block_hash {
            return Err("Block hash mismatch".into());
        };
        Ok(stored_block)
    }

    // TODO: Add prefetching
    // pub fn fetch_block(&self, block_hash: u64, complete_callback: Box<dyn
    // AsyncGetStoredBlockAPI>) {     let mut _prefetch_blocks =
    // self.prefetchBlocks.lock().unwrap();     // if there's an entry, we've
    // already fetched the block     let prefetched_block =
    // _prefetch_blocks.remove(&block_hash);     if let Some(prefetched_block) =
    // prefetched_block {         let stored_block =
    // &prefetched_block.stored_block;         complete_callback.on_complete(**
    // stored_block, 0);         return;
    //     }
    //     let prefetched_block = pending_prefetch_blocks {
    //         stored_block: StoredBlock::new(),
    //         complete_callbacks: vec![complete_callback],
    //         err: 0,
    //     };
    //     _prefetch_blocks.insert(block_hash, prefetched_block);
    //     drop(_prefetch_blocks);
    //     let stored_block = self.get_stored_block(block_hash);
    //     let mut _prefetch_blocks = self.prefetchBlocks.lock().unwrap();
    //     let prefetched_block = _prefetch_blocks.remove(&block_hash);
    //     drop(_prefetch_blocks);
    //     if let Some(prefetched_block) = prefetched_block {
    //         let stored_block = &prefetched_block.stored_block;
    //         for callback in prefetched_block.complete_callbacks {
    //             callback.on_complete(**stored_block, 0);
    //         }
    //         return;
    //     }
    // }

    pub fn delete_block(&self, block_hash: u64) -> Result<(), Box<dyn std::error::Error>> {
        let block_path = StoredBlock::get_block_path(Path::new("chunks"), block_hash);
        let blob_client = self.blob_store.new_client()?;
        let obj_handle = blob_client.new_object(block_path)?;
        obj_handle.delete()?;
        Ok(())
    }
}

/// Parse a URI and create the corresponding block store
///
/// # Arguments
///
/// * `uri` - A URI string, either a filesystem path or "s3://", "fsblob://", or
///   "file://"
/// * `store_index_paths` - Optional location of a store index optimized for a
///   specific version
/// * `job_api` - Optional job API for longtail operations. Unused for s3 and
///   fsblob uris
/// * `num_worker_count` - Number of workers passed to the block store
/// * `access_type` - Access type for the block store
/// * `enable_file_mapping` - Enable file mapping for the block store
/// * `opts` - Optional S3 options for "s3://" uris
pub fn create_block_store_for_uri(
    uri: &str,
    store_index_paths: Option<Vec<String>>,
    job_api: Option<&BikeshedJobAPI>,
    num_worker_count: i32,
    access_type: AccessType,
    enable_file_mapping: bool,
    opts: Option<S3Options>,
) -> Result<BlockstoreAPI, Box<dyn std::error::Error>> {
    match uri {
        // Filesystem blob store in rust
        s if s.starts_with("fsblob://") => {
            let fs_blob_store = FsBlobStore::new(&uri[9..], true);
            let fs_block_store = RemoteBlockStore::new_remote_block_store(
                fs_blob_store,
                store_index_paths,
                num_worker_count,
                access_type,
            )?;
            let blockstore_apiproxy = BlockstoreAPIProxy::new(Box::new(fs_block_store));
            Ok(BlockstoreAPI::new_from_proxy(Box::new(blockstore_apiproxy)))
        }

        // S3 blob store
        s if s.starts_with("s3://") => {
            let uri = uri.parse::<Uri>().expect("could not parse uri");
            let bucket_name = uri.host().expect("could not identify host").to_string();
            let mut prefix = uri.path().to_string();
            // Strip initial slash
            if prefix[0..1] == *"/" {
                prefix = prefix[1..].to_string();
            }
            // Add trailing slash
            if !prefix.is_empty() {
                prefix.push('/');
            }
            let s3_blob_store = S3BlobStore::new(&bucket_name, &prefix, opts);
            let s3_block_store = RemoteBlockStore::new_remote_block_store(
                s3_blob_store,
                store_index_paths,
                num_worker_count,
                access_type,
            )?;
            let blockstore_apiproxy = BlockstoreAPIProxy::new(Box::new(s3_block_store));
            let block_store = BlockstoreAPI::new_from_proxy(Box::new(blockstore_apiproxy));
            Ok(block_store)
        }

        // Longtail Filesystem block store
        s if s.starts_with("file://") => Ok(unsafe {
            BlockstoreAPI::new_fs_owned(
                job_api.ok_or("Job API required for file:// uri")?,
                Box::into_raw(Box::new(StorageAPI::new_fs())),
                Path::new(&uri[7..]),
                ".lsb",
                enable_file_mapping,
            )
        }),

        // Longtail Filesystem block store
        _ => Ok(unsafe {
            BlockstoreAPI::new_fs_owned(
                job_api.ok_or("Job API required for filesystem uri")?,
                Box::into_raw(Box::new(StorageAPI::new_fs())),
                Path::new(uri),
                ".lsb",
                enable_file_mapping,
            )
        }),
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     #[ignore = "Not finished"]
//     fn test_fs_blob_store_uri() {
//         let _guard = crate::init_logging().unwrap();
//         let temp_dir = tempfile::tempdir().unwrap();
//         let temp_dir_path = temp_dir.path().to_str().unwrap();
//
//         let uri = format!("fsblob://{}", temp_dir_path);
//         let fs_block_store =
//             create_block_store_for_uri(&uri, None, None, 1,
// AccessType::ReadWrite, false, None);
//
//     }
// }
