#[rustfmt::skip]
// Chunker API
// pub fn Longtail_GetChunkerAPISize() -> u64;
// pub fn Longtail_MakeChunkerAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, get_min_chunk_size_func: Longtail_Chunker_GetMinChunkSizeFunc, create_chunker_func: Longtail_Chunker_CreateChunkerFunc, next_chunk_func: Longtail_Chunker_NextChunkFunc, dispose_chunker_func: Longtail_Chunker_DisposeChunkerFunc, next_chunk_from_buffer: Longtail_Chunker_NextChunkFromBufferFunc,) -> *mut Longtail_ChunkerAPI;
// pub fn Longtail_Chunker_GetMinChunkSize( chunker_api: *mut Longtail_ChunkerAPI, out_min_chunk_size: *mut u32,) -> ::std::os::raw::c_int;
// pub fn Longtail_Chunker_CreateChunker( chunker_api: *mut Longtail_ChunkerAPI, min_chunk_size: u32, avg_chunk_size: u32, max_chunk_size: u32, out_chunker: *mut Longtail_ChunkerAPI_HChunker,) -> ::std::os::raw::c_int;
// pub fn Longtail_Chunker_NextChunk( chunker_api: *mut Longtail_ChunkerAPI, chunker: Longtail_ChunkerAPI_HChunker, feeder: Longtail_Chunker_Feeder, feeder_context: *mut ::std::os::raw::c_void, out_chunk_range: *mut Longtail_Chunker_ChunkRange,) -> ::std::os::raw::c_int;
// pub fn Longtail_Chunker_DisposeChunker( chunker_api: *mut Longtail_ChunkerAPI, chunker: Longtail_ChunkerAPI_HChunker,) -> ::std::os::raw::c_int;
// pub fn Longtail_Chunker_NextChunkFromBuffer( chunker_api: *mut Longtail_ChunkerAPI, chunker: Longtail_ChunkerAPI_HChunker, buffer: *const ::std::os::raw::c_void, buffer_size: u64, out_next_chunk_start: *mut *const ::std::os::raw::c_void,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateHPCDCChunkerAPI() -> *mut Longtail_ChunkerAPI;
//
// struct Longtail_ChunkerAPI
// {
//     struct Longtail_API m_API;
//     Longtail_Chunker_GetMinChunkSizeFunc GetMinChunkSize;
//     Longtail_Chunker_CreateChunkerFunc CreateChunker;
//     Longtail_Chunker_NextChunkFunc NextChunk;
//     Longtail_Chunker_DisposeChunkerFunc DisposeChunker;
//     Longtail_Chunker_NextChunkFromBufferFunc NextChunkFromBuffer;
// };

use std::ops::{Deref, DerefMut};

use crate::{
    Longtail_API, Longtail_ChunkerAPI, Longtail_CreateHPCDCChunkerAPI, Longtail_DisposeAPI,
};

/// The Chunker API provides functions for chunking data into smaller pieces.
/// This is implemented in Longtail using the algorithm described on this site:
/// [HDCDC](https://moinakg.wordpress.com/2013/06/22/high-performance-content-defined-chunking/)
///
/// This is currently the only chunker algorithm implemented in Longtail.
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
