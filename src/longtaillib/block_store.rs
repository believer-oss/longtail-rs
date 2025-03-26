#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Blockstore API
// pub fn Longtail_GetBlockStoreAPISize() -> u64;
// pub fn Longtail_MakeBlockStoreAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, put_stored_block_func: Longtail_BlockStore_PutStoredBlockFunc, preflight_get_func: Longtail_BlockStore_PreflightGetFunc, get_stored_block_func: Longtail_BlockStore_GetStoredBlockFunc, get_existing_content_func: Longtail_BlockStore_GetExistingContentFunc, prune_blocks_func: Longtail_BlockStore_PruneBlocksFunc, get_stats_func: Longtail_BlockStore_GetStatsFunc, flush_func: Longtail_BlockStore_FlushFunc,) -> *mut Longtail_BlockStoreAPI;
// pub fn Longtail_BlockStore_PutStoredBlock( block_store_api: *mut Longtail_BlockStoreAPI, stored_block: *mut Longtail_StoredBlock, async_complete_api: *mut Longtail_AsyncPutStoredBlockAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockStore_PreflightGet( block_store_api: *mut Longtail_BlockStoreAPI, chunk_count: u32, chunk_hashes: *const TLongtail_Hash, optional_async_complete_api: *mut Longtail_AsyncPreflightStartedAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockStore_GetStoredBlock( block_store_api: *mut Longtail_BlockStoreAPI, block_hash: u64, async_complete_api: *mut Longtail_AsyncGetStoredBlockAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockStore_GetExistingContent( block_store_api: *mut Longtail_BlockStoreAPI, chunk_count: u32, chunk_hashes: *const TLongtail_Hash, min_block_usage_percent: u32, async_complete_api: *mut Longtail_AsyncGetExistingContentAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockStore_PruneBlocks( block_store_api: *mut Longtail_BlockStoreAPI, block_keep_count: u32, block_keep_hashes: *const TLongtail_Hash, async_complete_api: *mut Longtail_AsyncPruneBlocksAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockStore_GetStats( block_store_api: *mut Longtail_BlockStoreAPI, out_stats: *mut Longtail_BlockStore_Stats,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockStore_Flush( block_store_api: *mut Longtail_BlockStoreAPI, async_complete_api: *mut Longtail_AsyncFlushAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateArchiveBlockStore( storage_api: *mut Longtail_StorageAPI, archive_path: *const ::std::os::raw::c_char, archive_index: *mut Longtail_ArchiveIndex, enable_write: ::std::os::raw::c_int, enable_mmap_reading: ::std::os::raw::c_int,) -> *mut Longtail_BlockStoreAPI;
// pub fn Longtail_CreateCompressBlockStoreAPI( backing_block_store: *mut Longtail_BlockStoreAPI, compression_registry: *mut Longtail_CompressionRegistryAPI,) -> *mut Longtail_BlockStoreAPI;
// pub fn Longtail_CreateCacheBlockStoreAPI( job_api: *mut Longtail_JobAPI, local_block_store: *mut Longtail_BlockStoreAPI, remote_block_store: *mut Longtail_BlockStoreAPI,) -> *mut Longtail_BlockStoreAPI;
// pub fn Longtail_CreateFSBlockStoreAPI( job_api: *mut Longtail_JobAPI, storage_api: *mut Longtail_StorageAPI, content_path: *const ::std::os::raw::c_char, optional_extension: *const ::std::os::raw::c_char, enable_file_mapping: ::std::os::raw::c_int,) -> *mut Longtail_BlockStoreAPI;
// pub fn Longtail_CreateLRUBlockStoreAPI( backing_block_store: *mut Longtail_BlockStoreAPI, max_lru_count: u32,) -> *mut Longtail_BlockStoreAPI;
// pub fn Longtail_CreateShareBlockStoreAPI( backing_block_store: *mut Longtail_BlockStoreAPI,) -> *mut Longtail_BlockStoreAPI;
//
// struct Longtail_BlockStoreAPI
// {
//     struct Longtail_API m_API;
//     Longtail_BlockStore_PutStoredBlockFunc PutStoredBlock;
//     Longtail_BlockStore_PreflightGetFunc PreflightGet;
//     Longtail_BlockStore_GetStoredBlockFunc GetStoredBlock;
//     Longtail_BlockStore_GetExistingContentFunc GetExistingContent;
//     Longtail_BlockStore_PruneBlocksFunc PruneBlocks;
//     Longtail_BlockStore_GetStatsFunc GetStats;
//     Longtail_BlockStore_FlushFunc Flush;
// };

use tracing::debug;

use crate::{
    AsyncFlushAPIProxy, AsyncGetExistingContentAPIProxy, AsyncGetStoredBlockAPIProxy,
    AsyncPreflightStartedAPIProxy, AsyncPruneBlocksAPIProxy, AsyncPutStoredBlockAPIProxy,
    BikeshedJobAPI, CompressionRegistry, ConcurrentChunkWriteAPI, HashAPI, Longtail_API,
    Longtail_ArchiveIndex, Longtail_AsyncFlushAPI, Longtail_AsyncGetExistingContentAPI,
    Longtail_AsyncGetStoredBlockAPI, Longtail_AsyncPreflightStartedAPI,
    Longtail_AsyncPruneBlocksAPI, Longtail_AsyncPutStoredBlockAPI, Longtail_BlockStoreAPI,
    Longtail_BlockStore_Flush, Longtail_BlockStore_GetExistingContent,
    Longtail_BlockStore_GetStats, Longtail_BlockStore_GetStoredBlock,
    Longtail_BlockStore_PreflightGet, Longtail_BlockStore_PruneBlocks,
    Longtail_BlockStore_PutStoredBlock, Longtail_BlockStore_Stats, Longtail_CancelAPI,
    Longtail_CancelAPI_CancelToken, Longtail_ChangeVersion2, Longtail_CreateArchiveBlockStore,
    Longtail_CreateCacheBlockStoreAPI, Longtail_CreateCompressBlockStoreAPI,
    Longtail_CreateFSBlockStoreAPI, Longtail_CreateLRUBlockStoreAPI,
    Longtail_CreateShareBlockStoreAPI, Longtail_ProgressAPI, Longtail_StorageAPI,
    Longtail_StoredBlock, ProgressAPIProxy, StorageAPI, StoreIndex, StoredBlock, VersionDiff,
    VersionIndex,
};
use std::{
    ops::{Deref, DerefMut},
    ptr::null_mut,
};

/// A block store in the Longtail API consists of pointers to functions that
/// implement a backing block store.
#[repr(C)]
#[derive(Debug)]
pub struct BlockstoreAPI {
    pub blockstore_api: *mut Longtail_BlockStoreAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for BlockstoreAPI {
    fn drop(&mut self) {
        unsafe {
            longtail_sys::Longtail_DisposeAPI(
                &mut (*self.blockstore_api).m_API as *mut Longtail_API,
            )
        };
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
    /// Create a Longtail C File System block store. If no block extension is
    /// provided, defaults to '.lrb' in the C implementation. If
    /// enable_file_mapping is true, the C implementation will use memory
    /// mapping to read files.
    pub fn new_fs(
        jobs: &BikeshedJobAPI,
        storage_api: &StorageAPI,
        content_path: &str,
        block_extension: &str,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_content_path =
            std::ffi::CString::new(content_path).expect("content_path contains null bytes");
        let c_block_extension =
            std::ffi::CString::new(block_extension).expect("block_extension contains null bytes");
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

    /// Create a local caching block store that uses a remote block store as the
    /// backing store.
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

    /// Create a compressed block store that wraps a backing block store.
    pub fn new_compressed(
        backing_blockstore: Box<BlockstoreAPI>,
        compression_api: &CompressionRegistry,
    ) -> BlockstoreAPI {
        tracing::debug!("Compressed blockstore: {:p}", backing_blockstore);
        // TODO: Why is this a Box, and new_cached is not?
        let backing_blockstore = Box::into_raw(backing_blockstore);
        let longtail_blockstore = unsafe { *(*backing_blockstore) };
        let blockstore_api =
            unsafe { Longtail_CreateCompressBlockStoreAPI(longtail_blockstore, **compression_api) };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// Create a shared block store that aggregates multiple gets for the same
    /// block into a single get. The async on_completion callback will be
    /// called once for each get.
    pub fn new_share(backing_blockstore: &BlockstoreAPI) -> BlockstoreAPI {
        let blockstore_api = unsafe { Longtail_CreateShareBlockStoreAPI(**backing_blockstore) };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// Creates an LRU in-memory cache block store that wraps a backing block
    /// store. The cache will evict blocks based on the least recently used
    /// policy. The max_cache_size is the maximum number of blocks to keep
    /// in the cache.
    pub fn new_lru(backing_blockstore: &BlockstoreAPI, max_cache_size: u32) -> BlockstoreAPI {
        let blockstore_api =
            unsafe { Longtail_CreateLRUBlockStoreAPI(**backing_blockstore, max_cache_size) };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    // TODO: This function is unsafe because we haven't wrapped the ArchiveIndex
    // into a safe Rust struct yet.
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_archive(
        storage_api: *mut Longtail_StorageAPI,
        archive_path: &str,
        archive_index: *mut Longtail_ArchiveIndex,
        enable_write: bool,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_archive_path =
            std::ffi::CString::new(archive_path).expect("archive_path contains null bytes");
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

    /// Create a new block store from a BlockstoreAPIProxy. This allows us to
    /// create a new block store from a Rust implementation of the block
    /// store trait.
    pub fn new_from_proxy(proxy: Box<BlockstoreAPIProxy>) -> BlockstoreAPI {
        tracing::debug!("Creating new blockstore from proxy: {:p}", proxy);
        let proxy = Box::into_raw(proxy);
        BlockstoreAPI {
            blockstore_api: proxy as *mut Longtail_BlockStoreAPI,
            _pin: std::marker::PhantomPinned,
        }
    }

    // TODO: Is this the appropriate place for this function?
    // TODO: All of these functions that take many arguments would probably benefit
    // from a builder or something else to make them easier to use.
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
        let version_path =
            std::ffi::CString::new(version_path).expect("version_path contains null bytes");
        let store_index = store_index.store_index;
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

// These implementations dispatch the blockstore API calls to the Longtail C
// API.
impl Blockstore for BlockstoreAPI {
    fn get_existing_content(
        &self,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
        mut async_complete_api: AsyncGetExistingContentAPIProxy,
    ) -> Result<(), i32> {
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

// This is a trait for a block store. The Longtail_BlockStoreAPI C interface is
// implemented by a block store.
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

// This proxies between the Rust block store trait and the Longtail C API.
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
        // unsafe { Box::from_raw(self.context as *mut Box<dyn Blockstore>) };
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
    let _ = Box::into_raw(blockstore);
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
    let _ = Box::into_raw(blockstore);
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
    let _ = Box::into_raw(blockstore);
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
    let async_complete_api = AsyncGetStoredBlockAPIProxy::new_from_api(async_complete_api);
    let result = blockstore.get_stored_block(block_hash, async_complete_api);
    let _ = Box::into_raw(blockstore);
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
    let _ = Box::into_raw(blockstore);
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
    let _ = Box::into_raw(blockstore);
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
    let _ = Box::into_raw(blockstore);
    result.and(Ok(0)).unwrap_or_else(|err| err)
}
