use crate::*;
use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

#[repr(C)]
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
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new(
        hash_api: *mut Longtail_HashAPI,
        version_index: *const VersionIndex,
        max_block_size: u32,
        max_chunks_per_block: u32,
    ) -> Result<StoreIndex, i32> {
        let mut store_index = std::ptr::null_mut::<Longtail_StoreIndex>();
        let version_index = &*version_index;
        let result = unsafe {
            Longtail_CreateStoreIndex(
                hash_api,
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

    pub fn copy() {
        todo!()
    }
    pub fn is_valid() {
        todo!()
    }
    pub fn get_version() {
        todo!()
    }
    pub fn get_hash_identifier() {
        todo!()
    }
    pub fn get_block_count() {
        todo!()
    }
    pub fn get_chunk_count() {
        todo!()
    }
    pub fn get_block_hashes() {
        todo!()
    }
    pub fn get_chunk_hashes() {
        todo!()
    }
    pub fn get_chunk_sizes() {
        todo!()
    }
}
