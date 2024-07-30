use tracing::debug;

#[allow(unused_imports)]
use crate::{
    AsyncFlushAPIProxy,
    AsyncGetExistingContentAPIProxy,
    AsyncGetStoredBlockAPIProxy,
    AsyncPreflightStartedAPIProxy,
    AsyncPruneBlocksAPIProxy,
    AsyncPutStoredBlockAPIProxy,
    BikeshedJobAPI,
    CompressionRegistry,
    ConcurrentChunkWriteAPI,
    HashAPI,
    Longtail_API,
    Longtail_Alloc,
    Longtail_ArchiveIndex,
    Longtail_AsyncFlushAPI,
    Longtail_AsyncGetExistingContentAPI,
    Longtail_AsyncGetStoredBlockAPI,
    Longtail_AsyncPreflightStartedAPI,
    Longtail_AsyncPruneBlocksAPI,
    Longtail_AsyncPutStoredBlockAPI,
    Longtail_BlockIndex,
    Longtail_BlockStoreAPI,
    Longtail_BlockStore_Flush,
    Longtail_BlockStore_GetExistingContent,
    Longtail_BlockStore_GetStats,
    Longtail_BlockStore_GetStoredBlock,
    Longtail_BlockStore_PreflightGet,
    Longtail_BlockStore_PruneBlocks,
    Longtail_BlockStore_PutStoredBlock,
    Longtail_BlockStore_Stats,
    Longtail_CancelAPI,
    Longtail_CancelAPI_CancelToken,
    Longtail_ChangeVersion,
    Longtail_ChangeVersion2,
    Longtail_CopyBlockIndex,
    Longtail_CreateArchiveBlockStore,
    Longtail_CreateBlockStoreStorageAPI,
    Longtail_CreateCacheBlockStoreAPI,
    Longtail_CreateCompressBlockStoreAPI,
    Longtail_CreateFSBlockStoreAPI,
    Longtail_CreateLRUBlockStoreAPI,
    Longtail_CreateShareBlockStoreAPI,
    Longtail_DisposeAPI,
    Longtail_GetBlockIndexSize,
    Longtail_InitStoredBlockFromData,
    Longtail_ProgressAPI,
    Longtail_ReadBlockIndexFromBuffer,
    Longtail_ReadStoreIndexFromBuffer,
    Longtail_ReadStoredBlockFromBuffer,
    Longtail_StorageAPI,
    Longtail_StoredBlock,
    Longtail_WriteStoredBlockToBuffer,
    NativeBuffer,
    ProgressAPIProxy,
    StorageAPI,
    StoreIndex,
    VersionDiff,
    VersionIndex,
};
use std::{
    ops::{
        Deref,
        DerefMut,
    },
    path::Path,
    ptr::null_mut,
};

#[repr(C)]
pub struct BlockIndex {
    pub block_index: *mut Longtail_BlockIndex,
    _pin: std::marker::PhantomPinned,
}

impl Deref for BlockIndex {
    type Target = *mut Longtail_BlockIndex;
    fn deref(&self) -> &Self::Target {
        &self.block_index
    }
}

impl DerefMut for BlockIndex {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.block_index
    }
}

impl Clone for BlockIndex {
    fn clone(&self) -> Self {
        let block_index = unsafe { Longtail_CopyBlockIndex(self.block_index) };
        BlockIndex {
            block_index,
            _pin: std::marker::PhantomPinned,
        }
    }
}

impl BlockIndex {
    pub fn new_from_buffer(buffer: &mut [u8]) -> Result<Self, i32> {
        let size = buffer.len();
        let buffer = buffer.as_mut_ptr() as *mut std::ffi::c_void;
        let mut block_index = std::ptr::null_mut::<Longtail_BlockIndex>();
        let result = unsafe { Longtail_ReadBlockIndexFromBuffer(buffer, size, &mut block_index) };
        if result != 0 {
            return Err(result);
        }
        Ok(BlockIndex {
            block_index,
            _pin: std::marker::PhantomPinned,
        })
    }
    pub fn is_valid(&self) -> bool {
        !self.block_index.is_null()
    }
    pub fn get_block_hash(&self) -> u64 {
        unsafe { *(*self.block_index).m_BlockHash }
    }
    pub fn get_hash_identifier(&self) -> u32 {
        unsafe { *(*self.block_index).m_HashIdentifier }
    }
    pub fn get_chunk_count(&self) -> u32 {
        unsafe { *(*self.block_index).m_ChunkCount }
    }
    pub fn get_tag(&self) -> u32 {
        unsafe { *(*self.block_index).m_Tag }
    }
    pub fn get_chunk_hashes(&self) -> &[u64] {
        unsafe {
            let chunk_hashes = (*self.block_index).m_ChunkHashes;
            std::slice::from_raw_parts(chunk_hashes, self.get_chunk_count() as usize)
        }
    }
    pub fn get_chunk_sizes(&self) -> &[u32] {
        unsafe {
            let chunk_sizes = (*self.block_index).m_ChunkSizes;
            std::slice::from_raw_parts(chunk_sizes, self.get_chunk_count() as usize)
        }
    }
    pub fn get_block_path(&self, base_path: &Path) -> String {
        let block_hash = self.get_block_hash();
        StoredBlock::get_block_path(base_path, block_hash)
    }
}

#[repr(C)]
// TODO: This Clone is sus... it's not a deep copy. The trait should be implemented properly.
#[derive(Debug, Clone)]
pub struct StoredBlock {
    pub stored_block: *mut Longtail_StoredBlock,
    _pin: std::marker::PhantomPinned,
}

impl Deref for StoredBlock {
    type Target = *mut Longtail_StoredBlock;
    fn deref(&self) -> &Self::Target {
        &self.stored_block
    }
}

impl DerefMut for StoredBlock {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stored_block
    }
}

impl StoredBlock {
    pub fn new_from_lt(stored_block: *mut Longtail_StoredBlock) -> StoredBlock {
        StoredBlock {
            stored_block,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_buffer(block_data: &mut [u8]) -> Result<Self, i32> {
        let buffer = block_data.as_mut_ptr() as *mut std::ffi::c_void;
        let size = block_data.len();
        let mut stored_block = std::ptr::null_mut::<Longtail_StoredBlock>();
        let result = unsafe { Longtail_ReadStoredBlockFromBuffer(buffer, size, &mut stored_block) };
        if result != 0 {
            return Err(result);
        }
        Ok(StoredBlock {
            stored_block,
            _pin: std::marker::PhantomPinned,
        })
    }

    pub fn is_valid(&self) -> bool {
        !self.stored_block.is_null()
    }

    pub fn get_block_index(&self) -> BlockIndex {
        BlockIndex {
            block_index: unsafe { (*self.stored_block).m_BlockIndex },
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn get_block_size(&self) -> usize {
        let block_index = self.get_block_index();
        let chunk_count = block_index.get_chunk_count();
        let block_index_size = unsafe { Longtail_GetBlockIndexSize(chunk_count) };
        let block_data_size = unsafe { (*self.stored_block).m_BlockChunksDataSize as usize };
        block_index_size + block_data_size
    }

    pub fn write_to_buffer(&self) -> Result<NativeBuffer, i32> {
        let buffer = std::ptr::null_mut();
        let mut size = 0usize;
        let result =
            unsafe { Longtail_WriteStoredBlockToBuffer(self.stored_block, buffer, &mut size) };
        if result != 0 {
            return Err(result);
        }
        Ok(NativeBuffer {
            buffer: unsafe { *buffer },
            size,
        })
    }

    pub fn get_block_path(base_path: &Path, block_hash: u64) -> String {
        let file_name = format!("0x{:016x}.lsb", block_hash);
        let dir = base_path.join(&file_name[2..6]);
        let block_path = dir.join(file_name);
        block_path.to_string_lossy().into_owned().replace('\\', "/")
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct BlockstoreAPI {
    pub blockstore_api: *mut Longtail_BlockStoreAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for BlockstoreAPI {
    fn drop(&mut self) {
        debug!(
            "(Would be) Dropping BlockstoreAPI {:p}",
            self.blockstore_api
        );
        // unsafe { Longtail_DisposeAPI(&mut (*self.blockstore_api).m_API as
        // *mut Longtail_API) };
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
        content_path: &str,
        block_extension: Option<&str>,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_content_path = std::ffi::CString::new(content_path).unwrap();
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
        backing_blockstore: Box<BlockstoreAPI>,
        compression_api: &CompressionRegistry,
    ) -> BlockstoreAPI {
        tracing::info!("Compressed blockstore: {:p}", backing_blockstore);
        tracing::info!(
            "Compressed blockstore blockstore_api: {:p}",
            backing_blockstore.blockstore_api
        );
        tracing::info!("Dispose API: {:p}", unsafe {
            (*(backing_blockstore.blockstore_api))
                .m_API
                .Dispose
                .unwrap()
        });
        tracing::info!(
            "blockstore_api_dispose: {:p}",
            blockstore_api_dispose as *const ()
        );
        let backing_blockstore = Box::into_raw(backing_blockstore);
        let longtail_blockstore = unsafe { *(*backing_blockstore) };
        let blockstore_api =
            unsafe { Longtail_CreateCompressBlockStoreAPI(longtail_blockstore, **compression_api) };
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

    // max_cache_size == max number of blocks to keep in the cache
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

    pub fn new_from_proxy(proxy: Box<BlockstoreAPIProxy>) -> BlockstoreAPI {
        tracing::info!("Creating new blockstore from proxy: {:p}", proxy);
        let proxy = Box::into_raw(proxy);
        BlockstoreAPI {
            blockstore_api: proxy as *mut Longtail_BlockStoreAPI,
            _pin: std::marker::PhantomPinned,
        }
    }

    // TODO: All of these functions that take many arguments would probably benefit
    // from a builder or something else to make them easier to use.
    #[allow(clippy::too_many_arguments)]
    pub fn change_version(
        &self,
        version_storage_api: &StorageAPI,
        _concurrent_chunk_write_api: &ConcurrentChunkWriteAPI,
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
        let store_index = store_index.store_index;
        debug!("store_index: {:?}", store_index);
        debug!("store_index: {:?}", unsafe { *store_index });
        let result = unsafe {
            Longtail_ChangeVersion(
                self.blockstore_api,
                **version_storage_api,
                // **concurrent_chunk_write_api,
                **hash_api,
                **job_api,
                progress_api as *const ProgressAPIProxy as *mut Longtail_ProgressAPI,
                null_mut::<Longtail_CancelAPI>(),
                null_mut::<Longtail_CancelAPI_CancelToken>(),
                store_index,
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

impl Blockstore for BlockstoreAPI {
    fn get_existing_content(
        &self,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
        mut async_complete_api: AsyncGetExistingContentAPIProxy,
    ) -> Result<(), i32> {
        debug!("blockstore async_complete_api: {:?}", async_complete_api);
        let result = unsafe {
            Longtail_BlockStore_GetExistingContent(
                self.blockstore_api,
                chunk_hashes.len() as u32,
                chunk_hashes.as_ptr(),
                min_block_usage_percent,
                // &async_complete_api as *const _ as *mut Longtail_AsyncGetExistingContentAPI,
                &mut *async_complete_api,
            )
        };
        debug!("blockstore async_complete_api2: {:?}", async_complete_api);
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }

    fn put_stored_block(
        &self,
        stored_block: &StoredBlock,
        async_complete_api: AsyncPutStoredBlockAPIProxy,
    ) -> Result<(), i32> {
        let result = unsafe {
            Longtail_BlockStore_PutStoredBlock(
                self.blockstore_api,
                **stored_block,
                &async_complete_api as *const _ as *mut Longtail_AsyncPutStoredBlockAPI,
            )
        };
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }

    fn preflight_get(
        &self,
        chunk_hashes: Vec<u64>,
        optional_async_complete_api: Option<AsyncPreflightStartedAPIProxy>,
    ) -> Result<(), i32> {
        let async_complete_api = if let Some(async_complete_api) = optional_async_complete_api {
            &async_complete_api as *const _ as *mut Longtail_AsyncPreflightStartedAPI
        } else {
            null_mut::<Longtail_AsyncPreflightStartedAPI>()
        };
        let result = unsafe {
            Longtail_BlockStore_PreflightGet(
                self.blockstore_api,
                chunk_hashes.len() as u32,
                chunk_hashes.as_ptr(),
                async_complete_api,
            )
        };
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }

    // #[tracing::instrument(skip(self))]
    fn get_stored_block(
        &self,
        block_hash: u64,
        async_complete_api: *mut AsyncGetStoredBlockAPIProxy,
    ) -> Result<(), i32> {
        let result = unsafe {
            Longtail_BlockStore_GetStoredBlock(
                self.blockstore_api,
                block_hash,
                async_complete_api as *const _ as *mut Longtail_AsyncGetStoredBlockAPI,
            )
        };
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }

    fn prune_blocks(
        &self,
        block_keep_hashes: Vec<u64>,
        async_complete_api: AsyncPruneBlocksAPIProxy,
    ) -> Result<(), i32> {
        let block_count = block_keep_hashes.len() as u32;
        let result = unsafe {
            Longtail_BlockStore_PruneBlocks(
                self.blockstore_api,
                block_count,
                block_keep_hashes.as_ptr(),
                &async_complete_api as *const _ as *mut Longtail_AsyncPruneBlocksAPI,
            )
        };
        if result != 0 {
            return Err(result);
        };
        Ok(())
    }

    fn get_stats(&self) -> Result<Longtail_BlockStore_Stats, i32> {
        let c_stats = std::ptr::null_mut::<Longtail_BlockStore_Stats>();
        let result = unsafe { Longtail_BlockStore_GetStats(self.blockstore_api, c_stats) };
        if result != 0 {
            return Err(result);
        }
        let stats = unsafe { *c_stats };
        let mut out_stats = Longtail_BlockStore_Stats { m_StatU64: [0; 22] };
        out_stats.m_StatU64.copy_from_slice(&stats.m_StatU64);
        Ok(out_stats)
    }

    fn flush(&self, async_complete_api: AsyncFlushAPIProxy) -> Result<(), i32> {
        let result = unsafe {
            Longtail_BlockStore_Flush(
                self.blockstore_api,
                &async_complete_api as *const _ as *mut Longtail_AsyncFlushAPI,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }
}

pub trait Blockstore {
    fn put_stored_block(
        &self,
        stored_block: &StoredBlock,
        async_complete_api: AsyncPutStoredBlockAPIProxy,
    ) -> Result<(), i32>;
    fn preflight_get(
        &self,
        block_hashes: Vec<u64>,
        async_complete_api: Option<AsyncPreflightStartedAPIProxy>,
    ) -> Result<(), i32>;
    fn get_stored_block(
        &self,
        block_hash: u64,
        async_complete_api: *mut AsyncGetStoredBlockAPIProxy,
    ) -> Result<(), i32>;
    fn get_existing_content(
        &self,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
        async_complete_api: AsyncGetExistingContentAPIProxy,
    ) -> Result<(), i32>;
    fn prune_blocks(
        &self,
        block_keep_hashes: Vec<u64>,
        async_complete_api: AsyncPruneBlocksAPIProxy,
    ) -> Result<(), i32>;
    fn get_stats(&self) -> Result<Longtail_BlockStore_Stats, i32>;
    fn flush(&self, async_complete_api: AsyncFlushAPIProxy) -> Result<(), i32>;
}

// This proxies a blockstore implementation to the Longtail_BlockStoreAPI C
// interface.
#[repr(C)]
#[derive(Debug)]
pub struct BlockstoreAPIProxy {
    pub api: Longtail_BlockStoreAPI,
    pub context: *mut std::ffi::c_void,
}

impl Deref for BlockstoreAPIProxy {
    type Target = Longtail_BlockStoreAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for BlockstoreAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl Drop for BlockstoreAPIProxy {
    fn drop(&mut self) {
        debug!("(Would be) Dropping BlockstoreAPIProxy {:p}", self.context);
        // let _ = unsafe { Box::from_raw(self.context as *mut Box<dyn
        // Blockstore>) };
    }
}

impl BlockstoreAPIProxy {
    pub fn new(blockstore: Box<dyn Blockstore>) -> BlockstoreAPIProxy {
        let blockstore_ptr = Box::into_raw(Box::new(blockstore));
        debug!("BlockstoreAPIProxy: {:p}", blockstore_ptr);
        BlockstoreAPIProxy {
            api: Longtail_BlockStoreAPI {
                m_API: Longtail_API {
                    Dispose: Some(blockstore_api_dispose),
                },
                GetExistingContent: Some(blockstore_api_get_existing_content),
                PutStoredBlock: Some(blockstore_api_put_stored_block),
                PreflightGet: Some(blockstore_api_preflight_get),
                GetStoredBlock: Some(blockstore_api_get_stored_block),
                PruneBlocks: Some(blockstore_api_prune_blocks),
                GetStats: Some(blockstore_api_get_stats),
                Flush: Some(blockstore_api_flush),
            },
            context: blockstore_ptr as *mut std::ffi::c_void,
        }
    }
}

pub extern "C" fn blockstore_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut BlockstoreAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_get_existing_content(
    context: *mut Longtail_BlockStoreAPI,
    chunk_count: u32,
    chunk_hashes: *const u64,
    min_block_usage_percent: u32,
    async_complete_api: *mut Longtail_AsyncGetExistingContentAPI,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };

    let chunk_hashes = unsafe { std::slice::from_raw_parts(chunk_hashes, chunk_count as usize) };
    let async_complete_api = AsyncGetExistingContentAPIProxy::new_from_api(async_complete_api);
    let result = blockstore.get_existing_content(
        chunk_hashes.to_vec(),
        min_block_usage_percent,
        async_complete_api,
    );
    Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_put_stored_block(
    context: *mut Longtail_BlockStoreAPI,
    stored_block: *mut Longtail_StoredBlock,
    async_complete_api: *mut Longtail_AsyncPutStoredBlockAPI,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
    let stored_block = StoredBlock::new_from_lt(stored_block);
    let async_complete_api = AsyncPutStoredBlockAPIProxy::new_from_api(*async_complete_api);
    let result = blockstore.put_stored_block(&stored_block, async_complete_api);
    Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_preflight_get(
    context: *mut Longtail_BlockStoreAPI,
    chunk_count: u32,
    chunk_hashes: *const u64,
    async_complete_api: *mut Longtail_AsyncPreflightStartedAPI,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
    let chunk_hashes = unsafe { std::slice::from_raw_parts(chunk_hashes, chunk_count as usize) };
    let async_complete_api = if !async_complete_api.is_null() {
        Some(AsyncPreflightStartedAPIProxy::new_from_api(
            *async_complete_api,
        ))
    } else {
        None
    };
    let result = blockstore.preflight_get(chunk_hashes.to_vec(), async_complete_api);
    Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_get_stored_block(
    context: *mut Longtail_BlockStoreAPI,
    block_hash: u64,
    async_complete_api: *mut Longtail_AsyncGetStoredBlockAPI,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
    debug!("AsyncCompleteAPI: {:p}", async_complete_api);
    debug!("AsyncCompleteAPI: {:?}", async_complete_api);
    let async_complete_api = AsyncGetStoredBlockAPIProxy::new_from_api(async_complete_api);
    debug!("Getting stored block: {}", block_hash);
    debug!("Blockstore: {:p}", blockstore);
    debug!("AsyncCompleteAPI: {:?}", async_complete_api);
    let result = blockstore.get_stored_block(block_hash, async_complete_api);
    Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_prune_blocks(
    context: *mut Longtail_BlockStoreAPI,
    block_count: u32,
    block_keep_hashes: *const u64,
    async_complete_api: *mut Longtail_AsyncPruneBlocksAPI,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
    let block_keep_hashes =
        unsafe { std::slice::from_raw_parts(block_keep_hashes, block_count as usize) };
    let async_complete_api = AsyncPruneBlocksAPIProxy::new_from_api(*async_complete_api);
    let result = blockstore.prune_blocks(block_keep_hashes.to_vec(), async_complete_api);
    Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_get_stats(
    context: *mut Longtail_BlockStoreAPI,
    out_stats: *mut Longtail_BlockStore_Stats,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
    let result = blockstore.get_stats();
    Box::into_raw(blockstore);
    match result {
        Ok(stats) => {
            unsafe {
                *out_stats = stats;
            }
            0
        }
        Err(err) => err,
    }
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn blockstore_api_flush(
    context: *mut Longtail_BlockStoreAPI,
    async_complete_api: *mut Longtail_AsyncFlushAPI,
) -> i32 {
    let proxy = context as *mut BlockstoreAPIProxy;
    let context = unsafe { (*proxy).context };
    let blockstore = unsafe { Box::from_raw(context as *mut Box<dyn Blockstore>) };
    let async_complete_api = unsafe { AsyncFlushAPIProxy::new_from_api(*async_complete_api) };
    let result = blockstore.flush(async_complete_api);
    Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}

// #[cfg(test)]
// mod tests {
//     use crate::Longtail_StoredBlock;
//
//     #[test]
//     fn test_blockstore_api() {
//         let jobs = crate::BikeshedJobAPI::new(1, 1);
//         let storage_api = crate::StorageAPI::new_inmem();
//         let blockstore_api =
//             crate::BlockstoreAPI::new_fs(&jobs, &storage_api, "content",
// None, false);         assert!(!blockstore_api.is_null());
//
//         let result = unsafe {
//             let async_complete_api = std::ptr::null_mut();
//             blockstore_api.put_stored_block(&mut stored_block,
// async_complete_api)         };
//         assert_eq!(result, Ok(()));
//     }
// }
