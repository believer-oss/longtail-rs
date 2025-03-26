#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Stored Block API
// pub fn Longtail_StoredBlock_Dispose(stored_block: *mut Longtail_StoredBlock);
// pub fn Longtail_GetStoredBlockSize(block_data_size: usize) -> usize;
// pub fn Longtail_InitStoredBlockFromData( stored_block: *mut Longtail_StoredBlock, block_data: *mut ::std::os::raw::c_void, block_data_size: usize,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateStoredBlock( block_hash: TLongtail_Hash, hash_identifier: u32, chunk_count: u32, tag: u32, chunk_hashes: *mut TLongtail_Hash, chunk_sizes: *mut u32, block_data_size: u32, out_stored_block: *mut *mut Longtail_StoredBlock,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteStoredBlockToBuffer( stored_block: *const Longtail_StoredBlock, out_buffer: *mut *mut ::std::os::raw::c_void, out_size: *mut usize,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadStoredBlockFromBuffer( buffer: *const ::std::os::raw::c_void, size: usize, out_stored_block: *mut *mut Longtail_StoredBlock,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteStoredBlock( storage_api: *mut Longtail_StorageAPI, stored_block: *mut Longtail_StoredBlock, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadStoredBlock( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_stored_block: *mut *mut Longtail_StoredBlock,) -> ::std::os::raw::c_int;
// pub fn Longtail_StoredBlock_GetBlockIndex( stored_block: *mut Longtail_StoredBlock,) -> *mut Longtail_BlockIndex;
//
// struct Longtail_StoredBlock
// {
//     Longtail_StoredBlock_DisposeFunc Dispose;
//     struct Longtail_BlockIndex* m_BlockIndex;
//     void* m_BlockData;
//     uint32_t m_BlockChunksDataSize;
// };
//
// On disk representation of a stored block:
// [BlockIndex][BlockData]

use crate::{
    BlockIndex, Longtail_GetBlockIndexSize, Longtail_ReadStoredBlockFromBuffer,
    Longtail_StoredBlock, Longtail_WriteStoredBlockToBuffer, NativeBuffer,
};
use std::{
    ops::{Deref, DerefMut},
    path::Path,
};

/// A stored block in the Longtail API consists of pointers to a block index and
/// the associated block data.
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

impl Drop for StoredBlock {
    fn drop(&mut self) {
        // unsafe { Longtail_StoredBlock_Dispose(self.stored_block) }
    }
}

impl StoredBlock {
    /// Create a new `StoredBlock` from a `Longtail_StoredBlock` pointer.
    pub fn new_from_lt(stored_block: *mut Longtail_StoredBlock) -> StoredBlock {
        assert!(!stored_block.is_null());
        StoredBlock {
            stored_block,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// Deserialize a `StoredBlock` from a buffer
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

    /// Write the `StoredBlock` to a buffer
    pub(crate) fn write_to_buffer(&self) -> Result<NativeBuffer, i32> {
        let mut buf = NativeBuffer::new();
        let result = unsafe {
            Longtail_WriteStoredBlockToBuffer(self.stored_block, &mut buf.buffer, &mut buf.size)
        };
        if result != 0 {
            return Err(result);
        }
        Ok(buf)
    }

    /// The stored block is valid if it is not null
    /// Note: This is not a full check, as the stored block could be invalid.
    pub fn is_valid(&self) -> bool {
        !self.stored_block.is_null()
    }

    /// Return a `BlockIndex` for the `StoredBlock`
    // Longtail_StoredBlock_GetBlockIndex?
    pub fn get_block_index(&self) -> BlockIndex {
        BlockIndex::new_from_lt(unsafe { (*self.stored_block).m_BlockIndex })
    }

    /// Return the size of the `StoredBlock`, including the block index and
    /// block data
    // Longtail_GetStoredBlockSize?
    pub fn get_block_size(&self) -> usize {
        let block_index = self.get_block_index();
        let chunk_count = block_index.get_chunk_count();
        let block_index_size = unsafe { Longtail_GetBlockIndexSize(chunk_count) };
        let block_data_size = unsafe { (*self.stored_block).m_BlockChunksDataSize as usize };
        block_index_size + block_data_size
    }

    /// Find the expected file path for a block with the given hash
    pub fn get_block_path(base_path: &Path, block_hash: u64) -> String {
        let file_name = format!("0x{:016x}.lsb", block_hash);
        let dir = base_path.join(&file_name[2..6]);
        let block_path = dir.join(file_name);
        block_path.to_string_lossy().into_owned().replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use longtail_sys::Longtail_CreateStoredBlock;

    #[test]
    fn test_stored_block() {
        let block_data = vec![0u8; 1024];
        let block_hash = 0x123456789abcdef0;
        let hash_identifier = 0;
        let chunk_count = 1;
        let tag = 0;
        let chunk_hashes = vec![block_hash];
        let chunk_sizes = vec![1024];
        let mut stored_block = std::ptr::null_mut::<Longtail_StoredBlock>();
        let result = unsafe {
            Longtail_CreateStoredBlock(
                block_hash,
                hash_identifier,
                chunk_count,
                tag,
                chunk_hashes.as_ptr() as *mut u64,
                chunk_sizes.as_ptr() as *mut u32,
                block_data.len() as u32,
                &mut stored_block,
            )
        };
        assert_eq!(result, 0);
        let stored_block = StoredBlock::new_from_lt(stored_block);
        assert!(stored_block.is_valid());
        assert_eq!(stored_block.get_block_size(), 1104);
        let buf = stored_block.write_to_buffer().unwrap();
        assert_eq!(buf.size, 1056);
        let block_index = stored_block.get_block_index();
        assert_eq!(block_index.get_block_hash(), block_hash);
        assert_eq!(block_index.get_chunk_count(), chunk_count);
        assert_eq!(block_index.get_chunk_hashes(), chunk_hashes.as_slice());
        assert_eq!(block_index.get_chunk_sizes(), chunk_sizes.as_slice());
    }

    #[test]
    fn test_get_block_path() {
        let base_path = Path::new("test");
        let block_hash = 0x123456789abcdef0;
        let block_path = StoredBlock::get_block_path(base_path, block_hash);
        assert_eq!(block_path, "test/1234/0x123456789abcdef0.lsb");
    }
}
