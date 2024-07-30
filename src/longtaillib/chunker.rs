use std::ops::{Deref, DerefMut};

use crate::{
    Longtail_API, Longtail_ChunkerAPI, Longtail_ConcurrentChunkWriteAPI,
    Longtail_CreateConcurrentChunkWriteAPI, Longtail_CreateHPCDCChunkerAPI, Longtail_DisposeAPI,
    StorageAPI, VersionDiff, VersionIndex,
};

#[repr(C)]
pub struct ChunkerAPI {
    chunker_api: *mut Longtail_ChunkerAPI,
}

impl Drop for ChunkerAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.chunker_api).m_API as *mut Longtail_API) };
    }
}

impl Default for ChunkerAPI {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for ChunkerAPI {
    type Target = *mut Longtail_ChunkerAPI;
    fn deref(&self) -> &Self::Target {
        &self.chunker_api
    }
}

impl DerefMut for ChunkerAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.chunker_api
    }
}

impl ChunkerAPI {
    pub fn new() -> ChunkerAPI {
        ChunkerAPI {
            chunker_api: unsafe { Longtail_CreateHPCDCChunkerAPI() },
        }
    }
    pub fn get_chunker_api(&self) -> *mut Longtail_ChunkerAPI {
        self.chunker_api
    }
}

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
