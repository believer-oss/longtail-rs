use longtail_sys::*;

use std::ffi::c_void;

use strum::{
    EnumString,
    FromRepr,
};

pub mod longtaillib;

pub use longtaillib::*;

pub mod longtailstorelib;

pub use longtailstorelib::*;

pub mod remotestore;

pub use remotestore::*;

pub mod path_filter;

pub use path_filter::RegexPathFilter;

pub mod async_apis;

pub use async_apis::*;

pub mod commands;
mod error;

pub use commands::*;

pub struct NativeBuffer {
    pub buffer: *mut c_void,
    pub size: usize,
}

impl NativeBuffer {
    pub fn new(buffer: *mut c_void, size: usize) -> Self {
        Self { buffer, size }
    }
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.buffer as *const u8, self.size) }
    }
}
