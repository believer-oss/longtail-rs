#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Path Filter API
// pub fn Longtail_GetPathFilterAPISize() -> u64;
// pub fn Longtail_MakePathFilterAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, include_filter_func: Longtail_PathFilter_IncludeFunc,) -> *mut Longtail_PathFilterAPI;
// pub fn Longtail_PathFilter_Include( path_filter_api: *mut Longtail_PathFilterAPI, root_path: *const ::std::os::raw::c_char, asset_path: *const ::std::os::raw::c_char, asset_name: *const ::std::os::raw::c_char, is_dir: ::std::os::raw::c_int, size: u64, permissions: u16,) -> ::std::os::raw::c_int;

use std::ops::{Deref, DerefMut};

#[allow(unused_imports)]
use crate::{
    Longtail_API, Longtail_Alloc, Longtail_Free, Longtail_MakePathFilterAPI, Longtail_PathFilterAPI,
};

/// Trait for testing the metadata of a file or directory against a filter
pub trait PathFilterAPI: std::fmt::Debug {
    fn include(
        &self,
        root_path: &str,
        asset_path: &str,
        asset_name: &str,
        is_dir: bool,
        size: u64,
        permissions: u16,
    ) -> bool;
}

/// Proxy struct to hold the PathFilterAPI trait object for use in C, and a
/// context pointer to a Box<dyn PathFilterAPI> we create in rust.
#[repr(C)]
pub struct PathFilterAPIProxy {
    pub api: Longtail_PathFilterAPI,
    pub context: *mut std::ffi::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Deref for PathFilterAPIProxy {
    type Target = Longtail_PathFilterAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for PathFilterAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl Drop for PathFilterAPIProxy {
    fn drop(&mut self) {
        // Take ownership of the Box<dyn PathFilterAPI> and drop it
        let _ = unsafe { Box::from_raw(self.context as *mut Box<dyn PathFilterAPI>) };
        // Free the C struct
        // unsafe {
        //     let api = self.api;
        //     Longtail_Free(api as *mut std::ffi::c_void)
        // }
    }
}

impl PathFilterAPIProxy {
    // TODO: Does this need to have it's memory managed by C? It doesn't seem
    // necessary? implemented using rust to manage this in ProgressAPIProxy.
    pub fn new_proxy_ptr(path_filter: Box<dyn PathFilterAPI>) -> *mut Self {
        let context = Box::into_raw(Box::new(path_filter)) as *mut std::ffi::c_void;
        let api_mem = unsafe {
            let fn_context: longtail_sys::Longtail_Context = "CreatePathFilterProxyAPI".into();
            Longtail_Alloc(
                (&fn_context).into(),
                std::mem::size_of::<PathFilterAPIProxy>(),
            )
        };
        let api = unsafe {
            Longtail_MakePathFilterAPI(
                api_mem,
                Some(path_filter_dispose),
                Some(path_filter_include),
            )
        };
        let proxy = api as *mut PathFilterAPIProxy;
        unsafe {
            (*proxy).context = context;
        }
        assert_eq!(api as *mut std::ffi::c_void, api_mem);
        proxy
    }
    pub fn new(path_filter: Box<dyn PathFilterAPI>) -> Self {
        PathFilterAPIProxy {
            api: Longtail_PathFilterAPI {
                m_API: Longtail_API {
                    Dispose: Some(path_filter_dispose),
                },
                Include: Some(path_filter_include),
            },
            context: Box::into_raw(Box::new(path_filter)) as *mut std::ffi::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }
    pub fn get_context(&self) -> *mut std::ffi::c_void {
        self.context
    }
    pub fn as_ptr(&self) -> *mut Longtail_PathFilterAPI {
        &self.api as *const Longtail_PathFilterAPI as *mut Longtail_PathFilterAPI
    }
}

/// # Safety
/// This function is unsafe because it dereferences `path_filter` and `context`.
#[no_mangle]
pub unsafe extern "C" fn path_filter_include(
    path_filter: *mut Longtail_PathFilterAPI,
    root_path: *const std::os::raw::c_char,
    asset_path: *const std::os::raw::c_char,
    asset_name: *const std::os::raw::c_char,
    is_dir: std::os::raw::c_int,
    size: u64,
    permissions: u16,
) -> std::os::raw::c_int {
    let proxy = path_filter as *mut PathFilterAPIProxy;
    let context = unsafe { (*proxy).context };
    let path_filter = Box::from_raw(context as *mut Box<dyn PathFilterAPI>);
    let root_path = unsafe {
        std::ffi::CStr::from_ptr(root_path)
            .to_str()
            .expect("root_path is not valid utf-8")
    };
    let asset_path = unsafe {
        std::ffi::CStr::from_ptr(asset_path)
            .to_str()
            .expect("asset_path is not valid utf-8")
    };
    let asset_name = unsafe {
        std::ffi::CStr::from_ptr(asset_name)
            .to_str()
            .expect("asset_name is not valid utf-8")
    };
    let is_dir = is_dir != 0;
    let result =
        path_filter.include(root_path, asset_path, asset_name, is_dir, size, permissions) as i32;
    let _ = Box::into_raw(path_filter);
    result
}

#[no_mangle]
pub extern "C" fn path_filter_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut PathFilterAPIProxy)).context };
    let _path_filter = unsafe { Box::from_raw(context as *mut Box<dyn PathFilterAPI>) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    #[derive(Debug)]
    struct TestPathFilterAPI {}
    impl PathFilterAPI for TestPathFilterAPI {
        fn include(
            &self,
            _root_path: &str,
            _asset_path: &str,
            _asset_name: &str,
            _is_dir: bool,
            _size: u64,
            _permissions: u16,
        ) -> bool {
            if _root_path == "root" {
                return true;
            }
            false
        }
    }
    #[test]
    fn test_path_filter_include() {
        let _guard = crate::init_logging().unwrap();
        let path_filter = Box::new(TestPathFilterAPI {});
        let path_filter_proxy = PathFilterAPIProxy::new_proxy_ptr(path_filter);
        let asset_path = CString::new("asset").unwrap();
        let asset_name = CString::new("name").unwrap();
        let is_dir = 1;
        let size = 1024;
        let permissions = 0;
        let root_path = CString::new("root").unwrap();
        let result = unsafe {
            path_filter_include(
                path_filter_proxy as *mut Longtail_PathFilterAPI,
                root_path.as_ptr(),
                asset_path.as_ptr(),
                asset_name.as_ptr(),
                is_dir,
                size,
                permissions,
            )
        };
        assert_eq!(result, 1);
        let root_path = CString::new("not_root").unwrap();
        let result = unsafe {
            path_filter_include(
                path_filter_proxy as *mut Longtail_PathFilterAPI,
                root_path.as_ptr(),
                asset_path.as_ptr(),
                asset_name.as_ptr(),
                is_dir,
                size,
                permissions,
            )
        };
        assert_eq!(result, 0);
    }

    #[test]
    fn test_path_filter_dispose() {
        let _guard = crate::init_logging().unwrap();
        let path_filter = Box::new(TestPathFilterAPI {});
        let path_filter_proxy = PathFilterAPIProxy::new_proxy_ptr(path_filter);
        path_filter_dispose(path_filter_proxy as *mut Longtail_API);
    }
}
