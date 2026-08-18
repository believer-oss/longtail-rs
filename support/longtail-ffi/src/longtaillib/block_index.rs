#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Block Index API
// pub fn Longtail_MakeBlockIndex( store_index: *const Longtail_StoreIndex, block_index: u32, out_block_index: *mut Longtail_BlockIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_GetBlockIndexDataSize(chunk_count: u32) -> usize;
// pub fn Longtail_GetBlockIndexSize(chunk_count: u32) -> usize;
// pub fn Longtail_InitBlockIndex( mem: *mut ::std::os::raw::c_void, chunk_count: u32,) -> *mut Longtail_BlockIndex;
// pub fn Longtail_CopyBlockIndex( block_index: *mut Longtail_BlockIndex,) -> *mut Longtail_BlockIndex;
// pub fn Longtail_InitBlockIndexFromData( block_index: *mut Longtail_BlockIndex, data: *mut ::std::os::raw::c_void, data_size: u64,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateBlockIndex( hash_api: *mut Longtail_HashAPI, tag: u32, chunk_count: u32, chunk_indexes: *const u32, chunk_hashes: *const TLongtail_Hash, chunk_sizes: *const u32, out_block_index: *mut *mut Longtail_BlockIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteBlockIndexToBuffer( block_index: *const Longtail_BlockIndex, out_buffer: *mut *mut ::std::os::raw::c_void, out_size: *mut usize,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadBlockIndexFromBuffer( buffer: *const ::std::os::raw::c_void, size: usize, out_block_index: *mut *mut Longtail_BlockIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteBlockIndex( storage_api: *mut Longtail_StorageAPI, block_index: *mut Longtail_BlockIndex, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadBlockIndex( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_block_index: *mut *mut Longtail_BlockIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_BlockIndex_GetChunkCount(block_index: *const Longtail_BlockIndex) -> u32;
// pub fn Longtail_BlockIndex_GetChunkTag(block_index: *const Longtail_BlockIndex) -> *const u32;
// pub fn Longtail_BlockIndex_GetChunkHashes( block_index: *const Longtail_BlockIndex,) -> *const TLongtail_Hash;
// pub fn Longtail_BlockIndex_GetChunkSizes(block_index: *const Longtail_BlockIndex) -> *const u32;
// pub fn Longtail_BlockIndex_BlockData( stored_block: *mut Longtail_StoredBlock,) -> *mut ::std::os::raw::c_void;
// pub fn Longtail_BlockIndex_GetBlockChunksDataSize( stored_block: *mut Longtail_StoredBlock,) -> u32;
//
// struct Longtail_BlockIndex
// {
//     TLongtail_Hash* m_BlockHash;
//     uint32_t* m_HashIdentifier;
//     uint32_t* m_ChunkCount;
//     uint32_t* m_Tag;
//     TLongtail_Hash* m_ChunkHashes; // []
//     uint32_t* m_ChunkSizes; // []
// };

use crate::{
    Longtail_BlockIndex, Longtail_CopyBlockIndex, Longtail_ReadBlockIndexFromBuffer, StoredBlock,
};
use std::{
    ops::{Deref, DerefMut},
    path::Path,
};

/// A block index in the Longtail API consists of pointers to the block hash,
/// hash identifier, chunk count, tag, chunk hashes, and chunk sizes. The block
/// index is used to describe the contents of a block.
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
    /// Create a new `BlockIndex` from a `Longtail_BlockIndex` pointer.
    pub fn new_from_lt(block_index: *mut Longtail_BlockIndex) -> BlockIndex {
        BlockIndex {
            block_index,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// Deserialize a `BlockIndex` from a buffer
    pub fn new_from_buffer(buffer: &mut [u8]) -> Result<Self, i32> {
        //! Safety: The buffer is a not zero length
        assert!(!buffer.is_empty());
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

    /// The block index is valid if it is not null
    /// Note: This is not a full check, as the block index could be invalid.
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
    pub fn get_chunk_hashes(&self) -> Vec<u64> {
        let count = self.get_chunk_count() as usize;

        // This prevents unaligned access to the chunk hashes
        let unaligned = unsafe { (*self.block_index).m_ChunkHashes };
        let mut hashes = Vec::with_capacity(count);
        for i in 0..count {
            let hash = unsafe { std::ptr::read_unaligned(unaligned.add(i)) };
            hashes.push(hash);
        }
        hashes
    }
    pub fn get_chunk_sizes(&self) -> &[u32] {
        unsafe {
            let chunk_sizes = (*self.block_index).m_ChunkSizes;
            std::slice::from_raw_parts(chunk_sizes, self.get_chunk_count() as usize)
        }
    }

    /// Find the expected file path for this block with a given base_path
    pub fn get_block_path(&self, base_path: &Path) -> String {
        let block_hash = self.get_block_hash();
        StoredBlock::get_block_path(base_path, block_hash)
    }
}
