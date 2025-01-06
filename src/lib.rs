//! # Longtail Bindings for Rust
//!
//! These bindings provide a safe interface to the Longtail C API. They are
//! currently incomplete, with the significant usage only on the ```longtail
//! get``` equivalent command.
//!
//! The bindings are generated using the [bindgen](https://github.com/rust-lang/rust-bindgen) tool,
//! and the bindings are in the ```longtail_sys``` module. The bindings are then
//! wrapped in a higher-level Rust API in the ```longtail``` module.
//!
//! Since this code was originally ported from
//! [golongtail](https://github.com/DanEngelbrecht/golongtail/), there are many places where the
//! code be more idiomatic Rust.

use longtail_sys::*;
use std::ffi::c_void;
use strum::{EnumString, FromRepr};

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

/// A native buffer that can be used to pass data to and from the Longtail C
/// API. This buffer is expected to be allocated by the C API and must be freed
/// by calling Longtail_Free(buffer).
pub(crate) struct NativeBuffer {
    pub buffer: *mut c_void,
    pub size: usize,
}

impl Drop for NativeBuffer {
    fn drop(&mut self) {
        unsafe { Longtail_Free(self.buffer) }
    }
}

impl NativeBuffer {
    /// Create a new empty NativeBuffer.
    pub(crate) fn new() -> Self {
        Self {
            buffer: std::ptr::null_mut(),
            size: 0,
        }
    }

    /// Get the buffer as a slice of bytes.
    pub(crate) fn as_slice(&self) -> &[u8] {
        //! Safety: The buffer is a valid pointer
        assert!(!self.buffer.is_null());
        unsafe { std::slice::from_raw_parts(self.buffer as *const u8, self.size) }
    }
}

#[cfg(test)]
mod tests {
    use super::NativeBuffer;
    use std::mem;
    use std::os::raw::c_void;

    #[test]
    fn test_new() {
        let buffer = NativeBuffer::new();
        assert!(buffer.buffer.is_null());
        assert_eq!(buffer.size, 0);
    }

    #[test]
    fn test_as_slice() {
        let mut buffer = NativeBuffer::new();
        let data = vec![1u8, 2, 3];
        let len = data.len();
        buffer.buffer = Box::into_raw(data.into_boxed_slice()) as *mut c_void;
        buffer.size = len;
        let slice = buffer.as_slice();
        assert_eq!(slice, &[1, 2, 3]);
    }

    #[test]
    #[should_panic]
    fn test_as_slice_panic() {
        let buffer = NativeBuffer::new();
        let _ = buffer.as_slice();
    }

    #[test]
    fn test_drop() {
        let mut buffer = NativeBuffer::new();
        let data = vec![1u8, 2, 3];
        buffer.size = data.len();
        buffer.buffer = Box::into_raw(data.into_boxed_slice()) as *mut c_void;
        mem::drop(buffer);
        // If the buffer was correctly dropped, we should be able to create a new one without any issues.
        let _new_buffer = NativeBuffer::new();
    }
}
