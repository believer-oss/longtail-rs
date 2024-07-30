use crate::*;
use std::{
    ffi::c_char,
    ops::{
        Deref,
        DerefMut,
    },
};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct CFullPath {
    pub full_path: *const c_char,
}

impl Drop for CFullPath {
    fn drop(&mut self) {
        unsafe { Longtail_Free((self.full_path as *mut c_char) as *mut std::ffi::c_void) };
    }
}

impl Deref for CFullPath {
    type Target = *const c_char;
    fn deref(&self) -> &Self::Target {
        &self.full_path
    }
}

impl DerefMut for CFullPath {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.full_path
    }
}

impl CFullPath {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new(
        storage_api: *mut Longtail_StorageAPI,
        root_path: &str,
        path: &str,
    ) -> CFullPath {
        let c_root_path = std::ffi::CString::new(root_path).unwrap();
        let c_path = std::ffi::CString::new(path).unwrap();
        let c_full_path = unsafe {
            Longtail_Storage_ConcatPath(storage_api, c_root_path.as_ptr(), c_path.as_ptr())
        };
        CFullPath {
            full_path: c_full_path,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct COpenFile {
    pub storage_api: *mut Longtail_StorageAPI,
    pub open_file: Longtail_StorageAPI_HOpenFile,
}

impl Drop for COpenFile {
    fn drop(&mut self) {
        println!("Dropping {:?}", self);
        unsafe { Longtail_Storage_CloseFile(self.storage_api, self.open_file) };
        let open_file = unsafe { Box::from_raw(self.open_file) };
        drop(open_file)
    }
}

impl Deref for COpenFile {
    type Target = *mut Longtail_StorageAPI_OpenFile;
    fn deref(&self) -> &Self::Target {
        &self.open_file
    }
}

impl DerefMut for COpenFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.open_file
    }
}

impl COpenFile {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_for_write(
        storage_api: *mut Longtail_StorageAPI,
        c_full_path: *const c_char,
        block_size: u64,
    ) -> Result<COpenFile, i32> {
        let open_file_h = Box::into_raw(Box::new(0 as Longtail_StorageAPI_HOpenFile));
        let result = unsafe {
            Longtail_Storage_OpenWriteFile(
                storage_api,
                c_full_path,
                block_size,
                open_file_h as *mut Longtail_StorageAPI_HOpenFile,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(COpenFile {
            storage_api,
            open_file: *open_file_h,
        })
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_for_read(
        storage_api: *mut Longtail_StorageAPI,
        c_full_path: *const c_char,
    ) -> Result<COpenFile, i32> {
        let open_file_h = Box::into_raw(Box::new(0 as Longtail_StorageAPI_HOpenFile));
        let result = unsafe {
            Longtail_Storage_OpenReadFile(
                storage_api,
                c_full_path,
                open_file_h as *mut Longtail_StorageAPI_HOpenFile,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(COpenFile {
            storage_api,
            open_file: *open_file_h,
        })
    }

    pub fn read(&self, offset: u64, size: u64) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; size as usize];
        let result = unsafe {
            Longtail_Storage_Read(
                self.storage_api,
                self.open_file,
                offset,
                size,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(buffer)
    }
}

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

impl Deref for StorageAPI {
    type Target = *mut Longtail_StorageAPI;
    fn deref(&self) -> &Self::Target {
        &self.storage_api
    }
}

impl DerefMut for StorageAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage_api
    }
}

impl StorageAPI {
    pub fn new_from_api(storage_api: *mut Longtail_StorageAPI) -> StorageAPI {
        StorageAPI {
            storage_api,
            _pin: std::marker::PhantomPinned,
        }
    }

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

    pub fn read_from_storage(&self, root_path: &str, path: &str) -> Result<Vec<u8>, i32> {
        let c_full_path = unsafe { CFullPath::new(self.storage_api, root_path, path) };
        let c_open_file = unsafe { COpenFile::new_for_read(self.storage_api, *c_full_path)? };
        let mut file_size = 0;
        let result = unsafe {
            Longtail_Storage_GetSize(self.storage_api, *c_open_file, &mut file_size as *mut u64)
        };
        if result != 0 {
            return Err(result);
        }
        let buffer = c_open_file.read(0, file_size)?;
        Ok(buffer)
    }

    pub fn write_to_storage(&self, root_path: &str, path: &str, buffer: &[u8]) -> Result<(), i32> {
        let c_full_path = unsafe { CFullPath::new(self.storage_api, root_path, path) };
        let result = unsafe { EnsureParentPathExists(self.storage_api, *c_full_path) };
        if result != 0 {
            return Err(result);
        }
        let block_size = buffer.len() as u64;

        let c_open_file =
            unsafe { COpenFile::new_for_write(self.storage_api, *c_full_path, block_size)? };
        if block_size > 0 {
            let result = unsafe {
                Longtail_Storage_Write(
                    self.storage_api,
                    *c_open_file,
                    0,
                    block_size,
                    buffer.as_ptr() as *const std::ffi::c_void,
                )
            };
            if result != 0 {
                return Err(result);
            }
        }
        Ok(())
    }

    pub fn read_version_index(&self, path: &str) -> Result<VersionIndex, i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let mut version_index = std::ptr::null_mut::<Longtail_VersionIndex>();
        let result = unsafe {
            Longtail_ReadVersionIndex(
                self.storage_api,
                c_path.as_ptr(),
                &mut version_index as *mut *mut Longtail_VersionIndex,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(VersionIndex::from_longtail_versionindex(version_index))
    }

    pub fn file_exists(&self, path: &str) -> bool {
        let c_path = std::ffi::CString::new(path).unwrap();
        unsafe { Longtail_Storage_IsFile(self.storage_api, c_path.as_ptr()) == 1 }
    }

    pub fn delete_file(&self, path: &str) -> Result<(), i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let result = unsafe { Longtail_Storage_RemoveFile(self.storage_api, c_path.as_ptr()) };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }

    // TODO: Not sure this belongs here, because we have COpenFile, but modeling
    // golongtail for now.
    pub fn get_size(&self, f: COpenFile) -> Result<u64, i32> {
        let mut size = 0;
        let result = unsafe { Longtail_Storage_GetSize(self.storage_api, f.open_file, &mut size) };
        if result != 0 {
            return Err(result);
        }
        Ok(size)
    }

    pub fn read(&self, f: COpenFile, offset: u64, size: u64) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; size as usize];
        let result = unsafe {
            Longtail_Storage_Read(
                self.storage_api,
                f.open_file,
                offset,
                size,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(buffer)
    }

    // TODO: This shouldn't be needed, since RAII on COpenFile should handle this.
    pub fn close_file(&self, f: COpenFile) {
        unsafe { Longtail_Storage_CloseFile(self.storage_api, f.open_file) };
    }

    pub fn start_find(&self, path: &str) -> Result<*mut Longtail_StorageAPI_Iterator, i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let mut iterator = std::ptr::null_mut::<Longtail_StorageAPI_Iterator>();
        let result =
            unsafe { Longtail_Storage_StartFind(self.storage_api, c_path.as_ptr(), &mut iterator) };
        if result != 0 {
            return Err(result);
        }
        Ok(iterator)
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn find_next(
        &self,
        iterator: *mut Longtail_StorageAPI_HIterator,
    ) -> Result<(), i32> {
        let result = unsafe { Longtail_Storage_FindNext(self.storage_api, *iterator) };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn close_find(&self, iterator: *mut Longtail_StorageAPI_HIterator) {
        unsafe { Longtail_Storage_CloseFind(self.storage_api, *iterator) };
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn get_entry_properties(
        &self,
        iterator: *mut Longtail_StorageAPI_HIterator,
    ) -> Result<Longtail_StorageAPI_EntryProperties, i32> {
        let mut properties = Longtail_StorageAPI_EntryProperties {
            m_Name: std::ptr::null(),
            m_IsDir: 0,
            m_Size: 0,
            m_Permissions: 0,
        };
        let result = unsafe {
            Longtail_Storage_GetEntryProperties(
                self.storage_api,
                *iterator,
                &mut properties as *mut Longtail_StorageAPI_EntryProperties,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(properties)
    }

    pub fn write_version_index(&self, version_index: &VersionIndex, path: &str) -> Result<(), i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let result = unsafe {
            Longtail_WriteVersionIndex(
                self.storage_api,
                version_index.version_index,
                c_path.as_ptr(),
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_in_mem_storage() {
        let _guard = crate::init_logging().unwrap();
        let storage_api = crate::StorageAPI::new_inmem();
        let buffer = vec![1; 1024 * 1024];
        let result = storage_api.write_to_storage("folder", "file", buffer.as_slice());
        if result.is_err() {
            panic!("Failed to write to storage {:?}", result)
        }
        let read_buffer = storage_api.read_from_storage("folder", "file").unwrap();
        assert_eq!(buffer, read_buffer);
    }
}
