#![allow(clippy::empty_line_after_outer_attr)]
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
use std::os::raw::{c_char, c_int, c_void};

use crate::{
    Longtail_API, Longtail_Chunker_ChunkRange, Longtail_Chunker_CreateChunker,
    Longtail_Chunker_DisposeChunker, Longtail_Chunker_NextChunk,
    Longtail_Chunker_NextChunkFromBuffer, Longtail_ChunkerAPI, Longtail_ChunkerAPI_HChunker,
    Longtail_CreateHPCDCChunkerAPI, Longtail_DisposeAPI,
};

/// A single chunk boundary as produced by the HPCDC chunker: the absolute byte
/// offset of the chunk within the input, and its length in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSpan {
    pub offset: u64,
    pub size: u32,
}

/// Feeder context used to drive the streaming chunker entry point
/// (`Longtail_Chunker_NextChunk`) from an in-memory buffer, exactly the way the
/// C library drives it from a file handle during `CreateVersionIndex`.
struct FeederContext {
    data: *const u8,
    len: usize,
    pos: usize,
}

/// C-ABI feeder callback: copies up to `requested_size` bytes from the in-memory
/// input into the chunker's feed buffer, mirroring the file-read feeder used by
/// `Longtail_CreateVersionIndex` (longtail.c). Returns 0 (never fails on an
/// in-memory buffer); `out_size == 0` signals end of stream.
unsafe extern "C" fn buffer_feeder(
    context: *mut c_void,
    _chunker: Longtail_ChunkerAPI_HChunker,
    requested_size: u32,
    buffer: *mut c_char,
    out_size: *mut u32,
) -> c_int {
    let ctx = unsafe { &mut *(context as *mut FeederContext) };
    let remaining = ctx.len - ctx.pos;
    let n = remaining.min(requested_size as usize);
    if n > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(ctx.data.add(ctx.pos), buffer as *mut u8, n);
        }
    }
    ctx.pos += n;
    unsafe { *out_size = n as u32 };
    0
}

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

    /// Drive the **streaming** chunker entry point (`Longtail_HPCDCNextChunk` via
    /// `Longtail_Chunker_NextChunk` + a feeder) over an in-memory buffer,
    /// returning the ordered chunk boundaries. This is the canonical path used
    /// by golongtail when `--enable-file-mapping` is false (its default), so the
    /// boundaries produced here match production `.lvi` chunking. See
    /// `docs/format-spec.md` §6.
    pub fn chunk_streaming(
        &self,
        data: &[u8],
        min: u32,
        avg: u32,
        max: u32,
    ) -> Result<Vec<ChunkSpan>, i32> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let mut chunker: Longtail_ChunkerAPI_HChunker = std::ptr::null_mut();
        let r = unsafe {
            Longtail_Chunker_CreateChunker(self.chunker_api, min, avg, max, &mut chunker)
        };
        if r != 0 {
            return Err(r);
        }
        let mut ctx = FeederContext {
            data: data.as_ptr(),
            len: data.len(),
            pos: 0,
        };
        let mut out = Vec::new();
        loop {
            let mut range = Longtail_Chunker_ChunkRange {
                buf: std::ptr::null(),
                offset: 0,
                len: 0,
            };
            let r = unsafe {
                Longtail_Chunker_NextChunk(
                    self.chunker_api,
                    chunker,
                    Some(buffer_feeder),
                    &mut ctx as *mut _ as *mut c_void,
                    &mut range,
                )
            };
            // `Longtail_Chunker_NextChunk` returns `ESPIPE` as the exhaustion
            // sentinel (hpcdcchunker.c:420-423): it maps a zero-length chunk
            // range to `ESPIPE` and everything else to `0`. Match `ESPIPE`
            // explicitly so any OTHER non-zero return becomes a real error
            // instead of a silently truncated boundary table. (Upstream conflates
            // feeder errors into `ESPIPE` too, so this mainly hardens Rust-side
            // error paths — a healthy in-memory feed never errors.)
            if r == libc::ESPIPE {
                break;
            }
            if r != 0 {
                unsafe { Longtail_Chunker_DisposeChunker(self.chunker_api, chunker) };
                return Err(r);
            }
            if range.len == 0 {
                // Defensive: a 0-return with an empty range should not occur, but
                // treat it as end-of-stream rather than pushing a zero chunk.
                break;
            }
            out.push(ChunkSpan {
                offset: range.offset,
                size: range.len,
            });
        }
        unsafe { Longtail_Chunker_DisposeChunker(self.chunker_api, chunker) };
        Ok(out)
    }

    /// Drive the **buffer/mmap** chunker entry point
    /// (`HPCDCChunker_NextChunkFromBuffer`) over an in-memory buffer. This seeds
    /// the rolling hash over the first 48 bytes of each scope rather than the 48
    /// bytes preceding `min`, so its boundaries can DIVERGE from the streaming
    /// path when `min > 48` (see `docs/format-spec.md` §6). Provided only for
    /// labeled differential goldens against C-with-file-mapping.
    pub fn chunk_from_buffer(
        &self,
        data: &[u8],
        min: u32,
        avg: u32,
        max: u32,
    ) -> Result<Vec<ChunkSpan>, i32> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        let mut chunker: Longtail_ChunkerAPI_HChunker = std::ptr::null_mut();
        let r = unsafe {
            Longtail_Chunker_CreateChunker(self.chunker_api, min, avg, max, &mut chunker)
        };
        if r != 0 {
            return Err(r);
        }
        let base = data.as_ptr();
        let total = data.len();
        let mut cur = 0usize;
        let mut out = Vec::new();
        while cur < total {
            let remaining = total - cur;
            let mut next: *const c_void = std::ptr::null();
            let r = unsafe {
                Longtail_Chunker_NextChunkFromBuffer(
                    self.chunker_api,
                    chunker,
                    base.add(cur) as *const c_void,
                    remaining as u64,
                    &mut next,
                )
            };
            if r != 0 {
                unsafe { Longtail_Chunker_DisposeChunker(self.chunker_api, chunker) };
                return Err(r);
            }
            let next_off = next as usize - base as usize;
            let len = (next_off - cur) as u32;
            out.push(ChunkSpan {
                offset: cur as u64,
                size: len,
            });
            cur = next_off;
        }
        unsafe { Longtail_Chunker_DisposeChunker(self.chunker_api, chunker) };
        Ok(out)
    }
}
