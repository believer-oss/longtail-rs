#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
// FIXME: This is pretty bad...
#![allow(clippy::missing_safety_doc)]

use std::{
    ffi::c_void,
    fmt::{Display, Formatter},
};

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

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

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

impl Display for Longtail_LogField {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let name = unsafe { std::ffi::CStr::from_ptr(self.name) }
            .to_str()
            .unwrap_or("INVALID NAME");
        let value = unsafe { std::ffi::CStr::from_ptr(self.value) }
            .to_str()
            .unwrap_or("INVALID VALUE");
        write!(f, "{}={}", name, value)
    }
}

pub fn permissions_to_string(permissions: u16) -> String {
    let mut mode = String::new();
    mode.push(if permissions & 0o400 != 0 { 'r' } else { '-' });
    mode.push(if permissions & 0o200 != 0 { 'w' } else { '-' });
    mode.push(if permissions & 0o100 != 0 { 'x' } else { '-' });
    mode.push(if permissions & 0o040 != 0 { 'r' } else { '-' });
    mode.push(if permissions & 0o020 != 0 { 'w' } else { '-' });
    mode.push(if permissions & 0o010 != 0 { 'x' } else { '-' });
    mode.push(if permissions & 0o004 != 0 { 'r' } else { '-' });
    mode.push(if permissions & 0o002 != 0 { 'w' } else { '-' });
    mode.push(if permissions & 0o001 != 0 { 'x' } else { '-' });
    mode
}

impl Display for Longtail_StorageAPI_EntryProperties {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let name = unsafe { std::ffi::CStr::from_ptr(self.m_Name) }
            .to_str()
            .unwrap();
        let size = self.m_Size;
        let dirbit = if self.m_IsDir == 1 { 'd' } else { '-' };
        let mode = permissions_to_string(self.m_Permissions);

        write!(f, "{dirbit}{mode} {size:16} {name}")
    }
}
