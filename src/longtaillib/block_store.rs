use crate::{
    BikeshedJobAPI, CompressionRegistry, ConcurrentChunkWriteAPI, HashAPI, Longtail_API,
    Longtail_ArchiveIndex, Longtail_AsyncGetExistingContentAPI, Longtail_BlockStoreAPI,
    Longtail_BlockStore_GetExistingContent, Longtail_CancelAPI, Longtail_CancelAPI_CancelToken,
    Longtail_ChangeVersion2, Longtail_CreateArchiveBlockStore, Longtail_CreateBlockStoreStorageAPI,
    Longtail_CreateCacheBlockStoreAPI, Longtail_CreateCompressBlockStoreAPI,
    Longtail_CreateFSBlockStoreAPI, Longtail_CreateLRUBlockStoreAPI,
    Longtail_CreateShareBlockStoreAPI, Longtail_DisposeAPI, Longtail_ProgressAPI,
    Longtail_StorageAPI, ProgressAPIProxy, StorageAPI, StoreIndex, VersionDiff, VersionIndex,
};
use std::{
    ops::{Deref, DerefMut},
    ptr::null_mut,
};

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
    pub fn new_fs(
        jobs: &BikeshedJobAPI,
        storage_api: &StorageAPI,
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
                jobs.job_api,
                storage_api.storage_api,
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

    pub fn new_cached(
        jobs: &BikeshedJobAPI,
        cache_blockstore: &BlockstoreAPI,
        persistent_blockstore: &BlockstoreAPI,
    ) -> BlockstoreAPI {
        let blockstore_api = unsafe {
            Longtail_CreateCacheBlockStoreAPI(
                jobs.job_api,
                **cache_blockstore,
                **persistent_blockstore,
            )
        };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_compressed(
        backing_blockstore: &BlockstoreAPI,
        compression_api: &CompressionRegistry,
    ) -> BlockstoreAPI {
        let blockstore_api = unsafe {
            Longtail_CreateCompressBlockStoreAPI(**backing_blockstore, **compression_api)
        };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_share(backing_blockstore: &BlockstoreAPI) -> BlockstoreAPI {
        let blockstore_api = unsafe { Longtail_CreateShareBlockStoreAPI(**backing_blockstore) };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_lru(backing_blockstore: &BlockstoreAPI, max_cache_size: u32) -> BlockstoreAPI {
        let blockstore_api =
            unsafe { Longtail_CreateLRUBlockStoreAPI(**backing_blockstore, max_cache_size) };
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

    pub fn new_block_store(
        hash_api: &HashAPI,
        job_api: &BikeshedJobAPI,
        block_store: &BlockstoreAPI,
        store_index: &StoreIndex,
        version_index: &VersionIndex,
    ) -> StorageAPI {
        let blockstore_api = unsafe {
            Longtail_CreateBlockStoreStorageAPI(
                **hash_api,
                **job_api,
                **block_store,
                **store_index,
                **version_index,
            )
        };
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

    // TODO: All of these functions that take many arguments would probably benefit from a builder
    // or something else to make them easier to use.
    #[allow(clippy::too_many_arguments)]
    pub fn change_version(
        &self,
        version_storage_api: &StorageAPI,
        concurrent_chunk_write_api: &ConcurrentChunkWriteAPI,
        hash_api: &HashAPI,
        job_api: &BikeshedJobAPI,
        progress_api: &ProgressAPIProxy,
        store_index: &StoreIndex,
        source_version_index: &VersionIndex,
        target_version_index: &VersionIndex,
        version_diff: &VersionDiff,
        version_path: &str,
        retain_permissions: bool,
    ) -> Result<(), i32> {
        let version_path = std::ffi::CString::new(version_path).unwrap();
        let result = unsafe {
            Longtail_ChangeVersion2(
                self.blockstore_api,
                **version_storage_api,
                **concurrent_chunk_write_api,
                **hash_api,
                **job_api,
                progress_api as *const ProgressAPIProxy as *mut Longtail_ProgressAPI,
                null_mut::<Longtail_CancelAPI>(),
                null_mut::<Longtail_CancelAPI_CancelToken>(),
                **store_index,
                **source_version_index,
                **target_version_index,
                **version_diff,
                version_path.as_ptr(),
                retain_permissions as i32,
            )
        };
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }
}
