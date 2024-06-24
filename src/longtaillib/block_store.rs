use crate::{
    Longtail_API, Longtail_ArchiveIndex, Longtail_AsyncGetExistingContentAPI,
    Longtail_BlockStoreAPI, Longtail_BlockStore_GetExistingContent,
    Longtail_CompressionRegistryAPI, Longtail_CreateArchiveBlockStore,
    Longtail_CreateBlockStoreStorageAPI, Longtail_CreateCacheBlockStoreAPI,
    Longtail_CreateCompressBlockStoreAPI, Longtail_CreateFSBlockStoreAPI,
    Longtail_CreateLRUBlockStoreAPI, Longtail_CreateShareBlockStoreAPI, Longtail_DisposeAPI,
    Longtail_HashAPI, Longtail_JobAPI, Longtail_StorageAPI, Longtail_StoreIndex,
    Longtail_VersionIndex, StorageAPI,
};
use std::ops::{Deref, DerefMut};

#[repr(C)]
pub struct BlockstoreAPI {
    pub blockstore_api: *mut Longtail_BlockStoreAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for BlockstoreAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.blockstore_api).m_API as *mut Longtail_API) };
    }
}

impl Deref for BlockstoreAPI {
    type Target = *mut Longtail_BlockStoreAPI;
    fn deref(&self) -> &Self::Target {
        &self.blockstore_api
    }
}

impl DerefMut for BlockstoreAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.blockstore_api
    }
}

impl BlockstoreAPI {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_fs(
        jobs: *mut Longtail_JobAPI,
        storage_api: *mut Longtail_StorageAPI,
        contentPath: &str,
        block_extension: Option<&str>,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_content_path = std::ffi::CString::new(contentPath).unwrap();
        let c_block_extension = if let Some(block_extension) = block_extension {
            std::ffi::CString::new(block_extension).unwrap()
        } else {
            std::ffi::CString::new("").unwrap()
        };
        let blockstore_api = unsafe {
            Longtail_CreateFSBlockStoreAPI(
                jobs,
                storage_api,
                c_content_path.as_ptr(),
                c_block_extension.as_ptr(),
                enable_file_mapping as i32,
            )
        };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_cached(
        jobs: *mut Longtail_JobAPI,
        cache_blockstore: *mut Longtail_BlockStoreAPI,
        persistent_blockstore: *mut Longtail_BlockStoreAPI,
    ) -> BlockstoreAPI {
        let blockstore_api =
            Longtail_CreateCacheBlockStoreAPI(jobs, cache_blockstore, persistent_blockstore);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_compressed(
        backing_blockstore: *mut Longtail_BlockStoreAPI,
        compression_api: *mut Longtail_CompressionRegistryAPI,
    ) -> BlockstoreAPI {
        let blockstore_api =
            Longtail_CreateCompressBlockStoreAPI(backing_blockstore, compression_api);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_share(backing_blockstore: *mut Longtail_BlockStoreAPI) -> BlockstoreAPI {
        let blockstore_api = Longtail_CreateShareBlockStoreAPI(backing_blockstore);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_lru(
        backing_blockstore: *mut Longtail_BlockStoreAPI,
        max_cache_size: u32,
    ) -> BlockstoreAPI {
        let blockstore_api = Longtail_CreateLRUBlockStoreAPI(backing_blockstore, max_cache_size);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_archive(
        storage_api: *mut Longtail_StorageAPI,
        archive_path: &str,
        archive_index: *mut Longtail_ArchiveIndex,
        enable_write: bool,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_archive_path = std::ffi::CString::new(archive_path).unwrap();
        let blockstore_api = Longtail_CreateArchiveBlockStore(
            storage_api,
            c_archive_path.as_ptr(),
            archive_index,
            enable_write as i32,
            enable_file_mapping as i32,
        );
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_block_store(
        hash_api: *mut Longtail_HashAPI,
        job_api: *mut Longtail_JobAPI,
        block_store: *mut Longtail_BlockStoreAPI,
        store_index: *mut Longtail_StoreIndex,
        version_index: *mut Longtail_VersionIndex,
    ) -> StorageAPI {
        let blockstore_api = Longtail_CreateBlockStoreStorageAPI(
            hash_api,
            job_api,
            block_store,
            store_index,
            version_index,
        );
        StorageAPI::new_from_api(blockstore_api)
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn get_existing_content(
        &self,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
        async_complete_api: *mut Longtail_AsyncGetExistingContentAPI,
    ) -> Result<(), i32> {
        let result = unsafe {
            Longtail_BlockStore_GetExistingContent(
                self.blockstore_api,
                chunk_hashes.len() as u32,
                chunk_hashes.as_ptr(),
                min_block_usage_percent,
                async_complete_api,
            )
        };
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }
}
