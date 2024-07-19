use crate::*;
use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct StoreIndex {
    pub store_index: *mut Longtail_StoreIndex,
    _pin: std::marker::PhantomPinned,
}

impl Drop for StoreIndex {
    fn drop(&mut self) {
        tracing::debug!("Dropping StoreIndex");
        // unsafe { Longtail_Free((self.store_index as *mut c_char) as *mut std::ffi::c_void) };
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
    pub fn new() -> StoreIndex {
        StoreIndex {
            store_index: std::ptr::null_mut::<Longtail_StoreIndex>(),
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_lt(store_index: *mut Longtail_StoreIndex) -> StoreIndex {
        StoreIndex {
            store_index,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_buffer(buffer: &[u8]) -> Result<StoreIndex, i32> {
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

    pub fn add_blocks(&self, block_indexes: Vec<BlockIndex>) -> Result<StoreIndex, i32> {
        let added_store_index = Self::new_from_blocks(block_indexes)?;
        self.merge_store_index(&added_store_index)
    }

    pub fn get_block_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.store_index).m_BlockCount } as usize;
        let indexes =
            unsafe { std::slice::from_raw_parts((*self.store_index).m_BlockHashes, count) };
        indexes.to_vec()
    }

    // TODO: Add async...
    pub fn get_existing_store_index_sync(
        index_store: &BlockstoreAPI,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, i32> {
        tracing::info!("Getting existing store index");
        #[derive(Debug, Clone, Default)]
        struct GetExistingContentCompletion {
            store_index: Arc<Mutex<Option<Result<StoreIndex, i32>>>>,
        }
        impl AsyncGetExistingContentAPI for GetExistingContentCompletion {
            fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
                tracing::info!("GetExistingContentCompletion::on_complete");
                let out = match err {
                    0 => Ok(StoreIndex::new_from_lt(store_index)),
                    _ => Err(err),
                };
                let mut store_index = self.store_index.lock().unwrap();
                store_index.replace(out);
            }
        }

        let x = GetExistingContentCompletion::default();
        let api = Box::new(x.clone());
        let completion = AsyncGetExistingContentAPIProxy::new(api);
        tracing::debug!(
            "Getting existing store index, completion: {:p}",
            &completion
        );
        index_store.get_existing_content(
            chunk_hashes,
            min_block_usage_percent,
            completion.clone(),
        )?;
        // TODO: This is terrible
        loop {
            if let Some(store_index) = x.store_index.lock().unwrap().clone() {
                return store_index;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            tracing::warn!("Waiting for store index");
        }
    }

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

    // TODO: Need BlockIndex struct
    // pub fn new_from_blocks(block_indexes: Vec<BlockIndex>) -> Result<StoreIndex, i32> {
    //     let mut store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
    //     let result = unsafe { Longtail_CreateStoreIndexFromBlocks() };
    //     if result != 0 {
    //         return Err(result);
    //     }
    //     Ok(StoreIndex {
    //         store_index,
    //         _pin: std::marker::PhantomPinned,
    //     })
    // }

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

    pub fn is_valid(&self) -> bool {
        !self.store_index.is_null()
    }
}

// FIXME: This generates an invalid StoreIndex by default, which doesn't seem right, but is used in
// the golang code. This should be fixed.
impl Default for StoreIndex {
    fn default() -> Self {
        Self::new()
    }
}
