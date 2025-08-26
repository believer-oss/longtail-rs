#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Store Index API
// pub fn Longtail_StoreIndex_GetVersion(store_index: *const Longtail_StoreIndex) -> u32;
// pub fn Longtail_StoreIndex_GetHashIdentifier(store_index: *const Longtail_StoreIndex) -> u32;
// pub fn Longtail_StoreIndex_GetBlockCount(store_index: *const Longtail_StoreIndex) -> u32;
// pub fn Longtail_StoreIndex_GetChunkCount(store_index: *const Longtail_StoreIndex) -> u32;
// pub fn Longtail_StoreIndex_GetBlockHashes( store_index: *const Longtail_StoreIndex,) -> *const TLongtail_Hash;
// pub fn Longtail_StoreIndex_GetChunkHashes( store_index: *const Longtail_StoreIndex,) -> *const TLongtail_Hash;
// pub fn Longtail_StoreIndex_GetBlockChunksOffsets( store_index: *const Longtail_StoreIndex,) -> *const u32;
// pub fn Longtail_StoreIndex_GetBlockChunkCounts( store_index: *const Longtail_StoreIndex,) -> *const u32;
// pub fn Longtail_StoreIndex_GetBlockTags(store_index: *const Longtail_StoreIndex) -> *const u32;
// pub fn Longtail_StoreIndex_GetChunkSizes(store_index: *const Longtail_StoreIndex) -> *const u32;
// pub fn Longtail_GetStoreIndexSize(block_count: u32, chunk_count: u32) -> usize;
// pub fn Longtail_CreateStoreIndex( hash_api: *mut Longtail_HashAPI, chunk_count: u32, chunk_hashes: *const TLongtail_Hash, chunk_sizes: *const u32, optional_chunk_tags: *const u32, max_block_size: u32, max_chunks_per_block: u32, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateStoreIndexFromBlocks( block_count: u32, block_indexes: *mut *const Longtail_BlockIndex, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_MergeStoreIndex( local_store_index: *const Longtail_StoreIndex, remote_store_index: *const Longtail_StoreIndex, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_PruneStoreIndex( source_store_index: *const Longtail_StoreIndex, keep_block_count: u32, keep_block_hashes: *const TLongtail_Hash, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_GetExistingStoreIndex( store_index: *const Longtail_StoreIndex, chunk_count: u32, chunks: *const TLongtail_Hash, min_block_usage_percent: u32, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_ValidateStore( store_index: *const Longtail_StoreIndex, version_index: *const Longtail_VersionIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_CopyStoreIndex( store_index: *const Longtail_StoreIndex,) -> *mut Longtail_StoreIndex;
// pub fn Longtail_SplitStoreIndex( store_index: *mut Longtail_StoreIndex, split_size: usize, out_store_indexes: *mut *mut *mut Longtail_StoreIndex, out_count: *mut u64,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteStoreIndexToBuffer( store_index: *const Longtail_StoreIndex, out_buffer: *mut *mut ::std::os::raw::c_void, out_size: *mut usize,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadStoreIndexFromBuffer( buffer: *const ::std::os::raw::c_void, size: usize, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteStoreIndex( storage_api: *mut Longtail_StorageAPI, store_index: *mut Longtail_StoreIndex, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadStoreIndex( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_store_index: *mut *mut Longtail_StoreIndex,) -> ::std::os::raw::c_int;
//
// struct Longtail_StoreIndex
// {
//     uint32_t* m_Version;
//     uint32_t* m_HashIdentifier;
//     uint32_t* m_BlockCount;             // Total number of blocks
//     uint32_t* m_ChunkCount;             // Total number of chunks across all blocks - chunk hashes may occur more than once
//     TLongtail_Hash* m_BlockHashes;      // [] m_BlockHashes is the hash of each block
//     TLongtail_Hash* m_ChunkHashes;      // [] For each m_BlockChunkCount[n] there are n consecutive chunk hashes in m_ChunkHashes[]
//     uint32_t* m_BlockChunksOffsets;     // [] m_BlockChunksOffsets[n] is the offset in m_ChunkSizes[] and m_ChunkHashes[]
//     uint32_t* m_BlockChunkCounts;       // [] m_BlockChunkCounts[n] is number of chunks in block m_BlockHash[n]
//     uint32_t* m_BlockTags;              // [] m_BlockTags is the tag for each block
//     uint32_t* m_ChunkSizes;             // [] m_ChunkSizes is the size of each chunk
// };

use crate::*;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Condvar, Mutex};

use longtail_sys::Longtail_StoreIndex;

/// A store index in the Longtail API consists of pointers to block hashes and
/// their constituent chunk hashes. The store index is used to describe a subset
/// of the store.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct StoreIndex {
    pub store_index: *mut Longtail_StoreIndex,
    _pin: std::marker::PhantomPinned,
}

// Make StoreIndex Send so it can be used in async/threaded contexts
unsafe impl Send for StoreIndex {}

impl Drop for StoreIndex {
    fn drop(&mut self) {
        // unsafe { Longtail_Free((self.store_index as *mut c_char) as *mut
        // std::ffi::c_void) };
    }
}

impl Deref for StoreIndex {
    type Target = *mut Longtail_StoreIndex;
    fn deref(&self) -> &Self::Target {
        &self.store_index
    }
}

impl DerefMut for StoreIndex {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store_index
    }
}

impl StoreIndex {
    // TODO: This creates a null pointer, so it should ideally be a StoreIndexNull
    // type.
    pub(crate) fn new_null_index() -> StoreIndex {
        StoreIndex {
            store_index: std::ptr::null_mut::<Longtail_StoreIndex>(),
            _pin: std::marker::PhantomPinned,
        }
    }

    pub(crate) fn new_from_lt(store_index: *mut Longtail_StoreIndex) -> StoreIndex {
        assert!(!store_index.is_null());
        StoreIndex {
            store_index,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// Deserialize a `StoreIndex` from a buffer
    pub fn new_from_buffer(buffer: &[u8]) -> Result<StoreIndex, i32> {
        assert!(!buffer.is_empty());
        let mut store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let result = unsafe {
            Longtail_ReadStoreIndexFromBuffer(
                buffer.as_ptr() as *const std::ffi::c_void,
                buffer.len(),
                &mut store_index,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(StoreIndex {
            store_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    /// Create a new `StoreIndex` from a set of BlockIndex structs
    pub fn new_from_blocks(block_indexes: Vec<BlockIndex>) -> Result<StoreIndex, i32> {
        let mut store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let result = unsafe {
            Longtail_CreateStoreIndexFromBlocks(
                block_indexes.len() as u32,
                block_indexes.as_ptr() as *mut *const Longtail_BlockIndex,
                &mut store_index,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(StoreIndex {
            store_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    /// Create a new `StoreIndex` from a `VersionIndex`
    pub fn new_from_version_index(
        hash_api: &HashAPI,
        version_index: &VersionIndex,
        max_block_size: u32,
        max_chunks_per_block: u32,
    ) -> Result<StoreIndex, i32> {
        let mut store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let result = unsafe {
            Longtail_CreateStoreIndex(
                **hash_api,
                version_index.get_chunk_count(),
                version_index.get_chunk_hashes().as_ptr(),
                version_index.get_chunk_sizes().as_ptr(),
                version_index.get_chunk_tags().as_ptr(),
                max_block_size,
                max_chunks_per_block,
                &mut store_index,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(StoreIndex {
            store_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    /// Create a new `StoreIndex` from a union of the current index and a set of
    /// block indexes
    pub fn add_blocks(&self, block_indexes: Vec<BlockIndex>) -> Result<StoreIndex, i32> {
        let added_store_index = Self::new_from_blocks(block_indexes)?;
        self.merge_store_index(&added_store_index)
    }

    /// Get the hashes contained in the store index
    pub fn get_block_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.store_index).m_BlockCount } as usize;
        let indexes =
            unsafe { std::slice::from_raw_parts((*self.store_index).m_BlockHashes, count) };
        indexes.to_vec()
    }

    pub fn get_existing_store_index_sync(
        index_store: &BlockstoreAPI,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, i32> {
        type CompletionResult = Arc<Mutex<Option<Result<usize, i32>>>>;

        let result: CompletionResult = Arc::new(Mutex::new(None));
        let condvar = Arc::new(Condvar::new());

        #[derive(Debug)]
        struct CompletionWrapper {
            result: CompletionResult,
            condvar: Arc<Condvar>,
        }

        impl AsyncGetExistingContentAPI for CompletionWrapper {
            unsafe fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
                tracing::debug!(
                    "CompletionWrapper::on_complete store_index={:p} err={}",
                    store_index,
                    err
                );

                let completion_result = if err != 0 {
                    Err(err)
                } else {
                    Ok(store_index as usize) // Convert pointer to usize for Send safety
                };

                // Set result and notify waiting thread
                if let Ok(mut guard) = self.result.lock() {
                    *guard = Some(completion_result);
                    self.condvar.notify_one();
                } else {
                    tracing::warn!("CompletionWrapper::on_complete failed to acquire lock");
                }
            }
        }

        let completion = AsyncGetExistingContentAPIProxy::new(Box::new(CompletionWrapper {
            result: result.clone(),
            condvar: condvar.clone(),
        }));
        tracing::debug!(
            "Getting existing store index, completion: {:p}",
            &completion,
        );

        index_store.get_existing_content(chunk_hashes, min_block_usage_percent, completion)?;

        // Wait for completion using condvar (efficient, no polling!)
        let mut guard = result.lock().map_err(|_| -1)?;
        while guard.is_none() {
            guard = condvar.wait(guard).map_err(|_| -1)?;
        }

        match guard.take().unwrap() {
            Ok(store_index_addr) => {
                let store_index_ptr = store_index_addr as *mut Longtail_StoreIndex;
                tracing::info!(
                    "Taking ownership of store index pointer: {:p}",
                    store_index_ptr
                );
                Ok(StoreIndex::new_from_lt(store_index_ptr))
            }
            Err(err) => Err(err),
        }
    }

    /// Creates a store index from a given set of chunk hashes, while keeping
    /// the existing store index blocks in use as long as the block usage is
    /// above the given minimum block usage threshold.
    pub fn get_existing_store_index(
        &self,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, i32> {
        let chunk_count = chunk_hashes.len();
        let mut store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let result = unsafe {
            Longtail_GetExistingStoreIndex(
                self.store_index,
                chunk_count as u32,
                chunk_hashes.as_ptr(),
                min_block_usage_percent,
                &mut store_index,
            )
        };
        if result != 0 {
            return Err(result);
        } else {
            tracing::debug!("Got existing store index");
        }
        Ok(StoreIndex::new_from_lt(store_index))
    }

    /// Remove blocks from the store index that are not in the given list of
    /// block hashes
    pub fn prune_store_index(
        store_index: &StoreIndex,
        keep_block_hashes: Vec<u64>,
    ) -> Result<StoreIndex, i32> {
        let mut pruned_store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let result = unsafe {
            Longtail_PruneStoreIndex(
                **store_index,
                keep_block_hashes.len() as u32,
                keep_block_hashes.as_ptr(),
                &mut pruned_store_index,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(StoreIndex {
            store_index: pruned_store_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    /// Merge the current store index with another
    pub fn merge_store_index(&self, other: &StoreIndex) -> Result<StoreIndex, i32> {
        let mut merged_store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let result = unsafe { Longtail_MergeStoreIndex(**self, **other, &mut merged_store_index) };
        if result != 0 {
            return Err(result);
        }
        Ok(StoreIndex {
            store_index: merged_store_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    /// The store index is valid if it is not null
    /// Note: This is not a full check, as the store index could be invalid.
    pub fn is_valid(&self) -> bool {
        !self.store_index.is_null()
    }
}
