#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::{
    ffi::CString,
    fmt::{Display, Formatter},
};

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

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
        let name = unsafe { std::ffi::CStr::from_ptr(self.m_Name) };
        let safename = String::from_utf8_lossy(name.to_bytes());
        let size = self.m_Size;
        let dirbit = if self.m_IsDir == 1 { 'd' } else { '-' };
        let mode = permissions_to_string(self.m_Permissions);

        write!(f, "{dirbit}{mode} {size:16} {safename}")
    }
}

#[repr(transparent)]
pub struct Longtail_Context(CString);

impl Longtail_Context {
    pub fn new(name: &str) -> Self {
        Self(CString::new(name).expect("Longtail_Context::new cannot have null bytes"))
    }
}

impl From<&str> for Longtail_Context {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<&Longtail_Context> for *const std::os::raw::c_char {
    fn from(context: &Longtail_Context) -> *const std::os::raw::c_char {
        context.0.as_ptr()
    }
}
