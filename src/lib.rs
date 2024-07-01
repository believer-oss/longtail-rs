#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::fmt::{Display, Formatter};

use strum::{EnumString, FromRepr};

pub mod longtaillib;
pub use longtaillib::*;

pub mod longtailstorelib;
pub use longtailstorelib::*;

pub mod remotestore;
pub use remotestore::*;

pub mod path_filter;
pub use path_filter::RegexPathFilter;

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
        let name = unsafe { std::ffi::CStr::from_ptr(self.m_Name) }
            .to_str()
            .unwrap();
        let size = self.m_Size;
        let dirbit = if self.m_IsDir == 1 { 'd' } else { '-' };
        let mode = permissions_to_string(self.m_Permissions);

        write!(f, "{dirbit}{mode} {size:16} {name}")
    }
}

// AsyncGetExistingContentAPI
// TODO: This needs to be a macro
pub trait AsyncGetExistingContentAPI: std::fmt::Debug {
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32);
    fn get_store_index(&self) -> Result<Option<StoreIndex>, i32>;
}
#[repr(C)]
pub struct AsyncGetExistingContentAPIProxy {
    pub api: Longtail_AsyncGetExistingContentAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

// TODO: Unused, since we're relying on the dispose function to handle it?
impl Drop for AsyncGetExistingContentAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
    }
}

impl AsyncGetExistingContentAPIProxy {
    pub fn new(
        async_get_existing_content_api: Box<dyn AsyncGetExistingContentAPI>,
    ) -> AsyncGetExistingContentAPIProxy {
        AsyncGetExistingContentAPIProxy {
            api: Longtail_AsyncGetExistingContentAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_get_existing_content_api_dispose),
                },
                OnComplete: Some(async_get_existing_content_api_on_complete),
            },
            context: Box::into_raw(Box::new(async_get_existing_content_api))
                as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }
    /// # Safety
    /// This function is unsafe because it dereferences `context`.
    pub unsafe fn get_store_index(&self) -> Result<Option<StoreIndex>, i32> {
        let context = self.context as *mut Box<dyn AsyncGetExistingContentAPI>;

        (*context).get_store_index()
    }
}

pub extern "C" fn async_get_existing_content_api_on_complete(
    context: *mut Longtail_AsyncGetExistingContentAPI,
    store_index: *mut Longtail_StoreIndex,
    err: i32,
) {
    let proxy = context as *mut AsyncGetExistingContentAPIProxy;
    let context = unsafe { (*proxy).context };
    let mut async_get_existing_content_api =
        unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetExistingContentAPI>) };
    async_get_existing_content_api.on_complete(store_index, err);
    Box::into_raw(async_get_existing_content_api);
}

pub extern "C" fn async_get_existing_content_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncGetExistingContentAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetExistingContentAPI>) };
}

#[derive(Debug, Default)]
// TODO: Does this need locking?
pub struct GetExistingContentCompletion {
    pub store_index: Option<StoreIndex>,
    pub err: Option<i32>,
}

impl AsyncGetExistingContentAPI for GetExistingContentCompletion {
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
        self.store_index = Some(StoreIndex::new(store_index));
        self.err = Some(err);
    }
    fn get_store_index(&self) -> Result<Option<StoreIndex>, i32> {
        match self.err {
            // TODO: This is a clone, should it be?
            Some(0) => Ok(Some(self.store_index.clone().unwrap())),
            Some(err) => Err(err),
            None => Ok(None),
        }
    }
}
