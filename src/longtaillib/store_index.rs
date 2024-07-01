use crate::*;
use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct StoreIndex {
    pub store_index: *mut Longtail_StoreIndex,
    _pin: std::marker::PhantomPinned,
}

impl Drop for StoreIndex {
    fn drop(&mut self) {
        unsafe { Longtail_Free((self.store_index as *mut c_char) as *mut std::ffi::c_void) };
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
    pub fn new(store_index: *mut Longtail_StoreIndex) -> StoreIndex {
        StoreIndex {
            store_index,
            _pin: std::marker::PhantomPinned,
        }
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

    // TODO: Add async...
    pub fn get_existing_store_index(
        index_store: &BlockstoreAPI,
        chunk_hashes: Vec<u64>,
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, i32> {
        let api = Box::<GetExistingContentCompletion>::default();
        let completion = AsyncGetExistingContentAPIProxy::new(api);
        unsafe {
            index_store.get_existing_content(
                chunk_hashes,
                min_block_usage_percent,
                &completion as *const _ as *mut Longtail_AsyncGetExistingContentAPI,
            )?
        };
        // TODO: This is terrible
        loop {
            let store_index = unsafe { completion.get_store_index() };
            match store_index {
                Ok(Some(store_index)) => return Ok(store_index),
                Err(err) => return Err(err),
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
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
}
