use crate::*;

#[repr(C)]
pub struct StorageAPI {
    pub storage_api: *mut Longtail_StorageAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for StorageAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.storage_api).m_API as *mut Longtail_API) };
    }
}

impl StorageAPI {
    pub fn new_fs() -> StorageAPI {
        let storage_api = unsafe { Longtail_CreateFSStorageAPI() };
        StorageAPI {
            storage_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_inmem() -> StorageAPI {
        let storage_api = unsafe { Longtail_CreateInMemStorageAPI() };
        StorageAPI {
            storage_api,
            _pin: std::marker::PhantomPinned,
        }
    }
}
