use std::ops::{Deref, DerefMut};

use crate::{
    Longtail_API, Longtail_ConcurrentChunkWriteAPI, Longtail_CreateConcurrentChunkWriteAPI,
    Longtail_DisposeAPI, StorageAPI, VersionDiff, VersionIndex,
};

#[rustfmt::skip]
// Concurrent Write API
// pub fn Longtail_GetConcurrentChunkWriteAPISize() -> u64;
// pub fn Longtail_MakeConcurrentChunkWriteAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, create_dir_func: Longtail_ConcurrentChunkWrite_CreateDirFunc, open_func: Longtail_ConcurrentChunkWrite_OpenFunc, close_func: Longtail_ConcurrentChunkWrite_CloseFunc, write_func: Longtail_ConcurrentChunkWrite_WriteFunc, flush_func: Longtail_ConcurrentChunkWrite_FlushFunc,) -> *mut Longtail_ConcurrentChunkWriteAPI;
// pub fn Longtail_ConcurrentChunkWrite_CreateDir( concurrent_file_write_api: *mut Longtail_ConcurrentChunkWriteAPI, asset_index: u32,) -> ::std::os::raw::c_int;
// pub fn Longtail_ConcurrentChunkWrite_Open( concurrent_file_write_api: *mut Longtail_ConcurrentChunkWriteAPI, asset_index: u32,) -> ::std::os::raw::c_int;
// pub fn Longtail_ConcurrentChunkWrite_Close( concurrent_file_write_api: *mut Longtail_ConcurrentChunkWriteAPI, asset_index: u32,);
// pub fn Longtail_ConcurrentChunkWrite_Write( concurrent_file_write_api: *mut Longtail_ConcurrentChunkWriteAPI, asset_index: u32, offset: u64, size: u32, input: *const ::std::os::raw::c_void,) -> ::std::os::raw::c_int;
// pub fn Longtail_ConcurrentChunkWrite_Flush( concurrent_file_write_api: *mut Longtail_ConcurrentChunkWriteAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateConcurrentChunkWriteAPI( storageAPI: *mut Longtail_StorageAPI, version_index: *mut Longtail_VersionIndex, version_diff: *mut Longtail_VersionDiff, base_path: *const ::std::os::raw::c_char,) -> *mut Longtail_ConcurrentChunkWriteAPI;
//
// struct Longtail_ConcurrentChunkWriteAPI
// {
//     struct Longtail_API m_API;
//     Longtail_ConcurrentChunkWrite_CreateDirFunc CreateDir;
//     Longtail_ConcurrentChunkWrite_OpenFunc Open;
//     Longtail_ConcurrentChunkWrite_CloseFunc Close;
//     Longtail_ConcurrentChunkWrite_WriteFunc Write;
//     Longtail_ConcurrentChunkWrite_FlushFunc Flush;
// };

/// The Concurrent Chunk Write API provides functions for high performance writing of chunks to
/// storage.
#[repr(C)]
pub struct ConcurrentChunkWriteAPI {
    concurrent_chunk_write_api: *mut Longtail_ConcurrentChunkWriteAPI,
}

impl ConcurrentChunkWriteAPI {
    pub fn new(
        storage_api: &StorageAPI,
        version_index: &VersionIndex,
        version_diff: &VersionDiff,
        base_path: &str,
    ) -> Self {
        let base_path = std::ffi::CString::new(base_path).unwrap();
        let storage_api = **storage_api;
        let version_index = **version_index;
        let version_diff = **version_diff;
        let concurrent_chunk_write_api = unsafe {
            Longtail_CreateConcurrentChunkWriteAPI(
                storage_api,
                version_index,
                version_diff,
                base_path.as_ptr(),
            )
        };

        Self {
            concurrent_chunk_write_api,
        }
    }
}

impl Drop for ConcurrentChunkWriteAPI {
    fn drop(&mut self) {
        unsafe {
            Longtail_DisposeAPI(&mut (*self.concurrent_chunk_write_api).m_API as *mut Longtail_API)
        };
    }
}

impl Deref for ConcurrentChunkWriteAPI {
    type Target = *mut Longtail_ConcurrentChunkWriteAPI;
    fn deref(&self) -> &Self::Target {
        &self.concurrent_chunk_write_api
    }
}

impl DerefMut for ConcurrentChunkWriteAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.concurrent_chunk_write_api
    }
}
